use std::{
    io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[cfg(target_os = "linux")]
use std::io::Read;

use axum::{
    Extension, Json, Router,
    body::{Body, Bytes},
    extract::{Request, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures_util::{StreamExt, stream};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncWriteExt},
    sync::{OwnedSemaphorePermit, Semaphore},
    time::{Instant, timeout, timeout_at},
};

use crate::{
    auth::AuthError,
    backup::{BackupStatus, MAX_SNAPSHOT_BYTES, StagedSnapshot, backup_health, verified_snapshot},
    restore::{PendingUpload, RestoreStatus, cancel_pending_restore, restore_status},
};

use super::{
    AppState, Problem,
    access::BrowserToken,
    admin_security::{AuditAction, AuditActor, AuditOutcome, audit_event},
    json_payload,
};

const AUTHORIZATION_TTL: Duration = Duration::from_secs(120);
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(1800);
const SNAPSHOT_MEDIA_TYPE: &str = "application/octet-stream";

#[derive(Clone)]
pub struct Operations {
    shared: Arc<OperationsState>,
    monitoring_enabled: bool,
}

struct OperationsState {
    database: PathBuf,
    scheduled_backups: bool,
    started_at: Option<u64>,
    started: Instant,
    grants: Mutex<[Option<Grant>; 2]>,
    transfer: Arc<Semaphore>,
}

struct Grant {
    session: [u8; 32],
    expires_at: Instant,
}

#[derive(Clone, Copy)]
enum Action {
    Backup,
    Restore,
}

impl Operations {
    pub fn new(database: PathBuf, scheduled_backups: bool) -> Self {
        Self {
            shared: Arc::new(OperationsState {
                database,
                scheduled_backups,
                started_at: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .ok()
                    .map(|duration| duration.as_secs()),
                started: Instant::now(),
                grants: Mutex::new([None, None]),
                transfer: Arc::new(Semaphore::new(1)),
            }),
            monitoring_enabled: false,
        }
    }

    pub fn with_monitoring(mut self, enabled: bool) -> Self {
        self.monitoring_enabled = enabled;
        self
    }

    pub async fn record_backup_result(database: &Path, succeeded: bool) {
        crate::backup::record_backup_result(&database.with_file_name("backups"), succeeded).await;
    }

    pub async fn record_startup_restore(pool: &sqlx::SqlitePool, applied: bool) {
        let outcome = if applied {
            AuditOutcome::Success
        } else {
            AuditOutcome::Failed
        };
        if audit_event(
            pool,
            AuditAction::RestoreApply,
            AuditActor::System,
            None,
            outcome,
        )
        .await
        .is_err()
        {
            tracing::warn!("could not record startup restore audit event");
        }
    }

    fn grant(&self, action: Action, token: &str) -> Result<(), Problem> {
        let mut grants = self
            .shared
            .grants
            .lock()
            .map_err(|_| Problem::unavailable())?;
        grants[action as usize] = Some(Grant {
            session: Sha256::digest(token.as_bytes()).into(),
            expires_at: Instant::now() + AUTHORIZATION_TTL,
        });
        Ok(())
    }

    fn consume(&self, action: Action, token: &str) -> Result<(), Problem> {
        let mut grants = self
            .shared
            .grants
            .lock()
            .map_err(|_| Problem::unavailable())?;
        let session: [u8; 32] = Sha256::digest(token.as_bytes()).into();
        grants[action as usize]
            .take_if(|grant| {
                Instant::now() < grant.expires_at && bool::from(grant.session.ct_eq(&session))
            })
            .map(|_| ())
            .ok_or(Problem(
                StatusCode::FORBIDDEN,
                "operation_authorization_required",
            ))
    }

    fn start_transfer(&self) -> Result<OwnedSemaphorePermit, Problem> {
        self.shared
            .transfer
            .clone()
            .try_acquire_owned()
            .map_err(|_| Problem(StatusCode::CONFLICT, "system_operation_in_progress"))
    }
}

pub(super) fn routes(operations: Operations) -> Router<AppState> {
    Router::new()
        .route("/api/v1/system", get(diagnostics))
        .route("/api/v1/system/backup/authorize", post(authorize_backup))
        .route("/api/v1/system/restore/authorize", post(authorize_restore))
        .route("/api/v1/system/backup", get(download))
        .route("/api/v1/system/restore", post(upload).delete(cancel))
        .layer(Extension(operations))
}

#[derive(Serialize)]
struct Diagnostics {
    version: &'static str,
    build_revision: Option<&'static str>,
    started_at: Option<u64>,
    uptime_seconds: u64,
    process_rss_bytes: Option<u64>,
    sqlite_version: String,
    schema_version: i64,
    database_bytes: i64,
    link_count: i64,
    raw_event_count: i64,
    analytics_enabled: bool,
    scheduled_backups: bool,
    last_backup_status: Option<BackupStatus>,
    last_verified_backup_at: Option<u64>,
    monitoring_enabled: bool,
    last_monitor_check_at: Option<i64>,
    system_operation_in_progress: bool,
    restore_status: RestoreStatus,
    max_snapshot_bytes: u64,
}

#[derive(sqlx::FromRow)]
struct DatabaseDiagnostics {
    sqlite_version: String,
    schema_version: i64,
    database_bytes: i64,
    link_count: i64,
    raw_event_count: i64,
    analytics_enabled: bool,
    last_monitor_check_at: Option<i64>,
}

async fn diagnostics(
    State(state): State<AppState>,
    Extension(operations): Extension<Operations>,
) -> Result<Json<Diagnostics>, Problem> {
    let metrics: DatabaseDiagnostics = sqlx::query_as(
        "SELECT sqlite_version() AS sqlite_version,
             (SELECT max(version) FROM _sqlx_migrations WHERE success = 1) AS schema_version,
             (SELECT page_count FROM pragma_page_count()) *
                 (SELECT page_size FROM pragma_page_size()) AS database_bytes,
             (SELECT count(*) FROM links) AS link_count,
             (SELECT count(*) FROM analytics_events) AS raw_event_count,
             (SELECT enabled FROM analytics_settings WHERE id = 1) AS analytics_enabled,
             (SELECT max(last_checked) FROM link_monitor WHERE last_checked >= 0)
                 AS last_monitor_check_at",
    )
    .fetch_one(&state.pool)
    .await
    .map_err(|_| Problem::unavailable())?;
    let database = operations.shared.database.clone();
    let (backup, restore_status, process_rss_bytes) = tokio::task::spawn_blocking(move || {
        (
            backup_health(&database.with_file_name("backups")),
            restore_status(&database),
            read_process_rss(),
        )
    })
    .await
    .map_err(|_| Problem::unavailable())?;
    Ok(Json(Diagnostics {
        version: env!("CARGO_PKG_VERSION"),
        build_revision: [
            option_env!("SHORTCIRCUIT_BUILD_REVISION"),
            option_env!("GITHUB_SHA"),
        ]
        .into_iter()
        .flatten()
        .find(|revision| {
            (7..=64).contains(&revision.len())
                && revision.bytes().all(|byte| byte.is_ascii_hexdigit())
        }),
        started_at: operations.shared.started_at,
        uptime_seconds: operations.shared.started.elapsed().as_secs(),
        process_rss_bytes,
        sqlite_version: metrics.sqlite_version,
        schema_version: metrics.schema_version,
        database_bytes: metrics.database_bytes,
        link_count: metrics.link_count,
        raw_event_count: metrics.raw_event_count,
        analytics_enabled: metrics.analytics_enabled,
        scheduled_backups: operations.shared.scheduled_backups,
        last_backup_status: backup.status,
        last_verified_backup_at: backup.verified_at,
        monitoring_enabled: operations.monitoring_enabled,
        last_monitor_check_at: metrics.last_monitor_check_at,
        system_operation_in_progress: operations.shared.transfer.available_permits() == 0,
        restore_status,
        max_snapshot_bytes: MAX_SNAPSHOT_BYTES,
    }))
}

#[cfg(target_os = "linux")]
fn read_process_rss() -> Option<u64> {
    let mut status = String::new();
    std::fs::File::open("/proc/self/status")
        .ok()?
        .take(16 * 1024)
        .read_to_string(&mut status)
        .ok()?;
    parse_rss(&status)
}

#[cfg(not(target_os = "linux"))]
fn read_process_rss() -> Option<u64> {
    None
}

#[cfg(any(test, target_os = "linux"))]
fn parse_rss(status: &str) -> Option<u64> {
    let mut value = status
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))?
        .split_ascii_whitespace();
    let kilobytes: u64 = value.next()?.parse().ok()?;
    if value.next()? != "kB" || value.next().is_some() {
        return None;
    }
    kilobytes.checked_mul(1024)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reauthentication {
    password: String,
}

async fn authorize_backup(
    State(state): State<AppState>,
    Extension(operations): Extension<Operations>,
    Extension(token): Extension<BrowserToken>,
    input: Result<Json<Reauthentication>, JsonRejection>,
) -> Result<StatusCode, Problem> {
    authorize(
        state,
        operations,
        token,
        json_payload(input)?,
        Action::Backup,
    )
    .await
}

async fn authorize_restore(
    State(state): State<AppState>,
    Extension(operations): Extension<Operations>,
    Extension(token): Extension<BrowserToken>,
    input: Result<Json<Reauthentication>, JsonRejection>,
) -> Result<StatusCode, Problem> {
    authorize(
        state,
        operations,
        token,
        json_payload(input)?,
        Action::Restore,
    )
    .await
}

async fn authorize(
    state: AppState,
    operations: Operations,
    token: BrowserToken,
    input: Reauthentication,
    action: Action,
) -> Result<StatusCode, Problem> {
    let audit_action = match action {
        Action::Backup => AuditAction::BackupAuthorize,
        Action::Restore => AuditAction::RestoreAuthorize,
    };
    match state.auth.reauthenticate(&token.0, &input.password).await {
        Ok(()) => {
            audit_event(
                &state.pool,
                audit_action,
                AuditActor::Admin,
                None,
                AuditOutcome::Success,
            )
            .await
            .map_err(|_| Problem::unavailable())?;
            operations.grant(action, &token.0)?;
            Ok(StatusCode::NO_CONTENT)
        }
        Err(AuthError::Unauthorized | AuthError::Invalid) => {
            audit(&state, audit_action, AuditOutcome::Rejected).await;
            Err(Problem(StatusCode::FORBIDDEN, "invalid_current_password"))
        }
        Err(error) => Err(error.into()),
    }
}

async fn download(
    State(state): State<AppState>,
    Extension(operations): Extension<Operations>,
    Extension(token): Extension<BrowserToken>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, Problem> {
    if method != Method::GET {
        return Err(Problem(
            StatusCode::METHOD_NOT_ALLOWED,
            "method_not_allowed",
        ));
    }
    // Native same-origin downloads can stream without holding the full file in browser memory.
    if headers
        .get("sec-fetch-site")
        .is_some_and(|site| site != "same-origin")
    {
        return Err(Problem(StatusCode::FORBIDDEN, "invalid_origin"));
    }
    let permit = operations.start_transfer()?;
    operations.consume(Action::Backup, &token.0)?;
    let directory = operations.shared.database.with_file_name("backups");
    let result = verified_snapshot(&state.pool, &directory)
        .await
        .and_then(|snapshot| {
            let (file, length) = snapshot.open()?;
            Ok((snapshot, file, length))
        });
    crate::backup::record_backup_result(&directory, result.is_ok()).await;
    let (snapshot, file, length) = match result {
        Ok(transfer) => transfer,
        Err(_) => {
            audit(&state, AuditAction::BackupDownload, AuditOutcome::Failed).await;
            return Err(Problem(StatusCode::SERVICE_UNAVAILABLE, "backup_failed"));
        }
    };
    audit_event(
        &state.pool,
        AuditAction::BackupDownload,
        AuditActor::Admin,
        None,
        AuditOutcome::Success,
    )
    .await
    .map_err(|_| Problem::unavailable())?;

    let body = Body::from_stream(stream::try_unfold(
        Download {
            _snapshot: snapshot,
            _permit: permit,
            file: File::from_std(file),
            remaining: length,
            deadline: Instant::now() + TRANSFER_TIMEOUT,
        },
        download_chunk,
    ));
    let mut response = (
        [
            (header::CONTENT_TYPE, SNAPSHOT_MEDIA_TYPE),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"shortcircuit-backup.sqlite\"",
            ),
        ],
        body,
    )
        .into_response();
    response
        .headers_mut()
        .insert(header::CONTENT_LENGTH, HeaderValue::from(length));
    Ok(response)
}

struct Download {
    _snapshot: StagedSnapshot,
    _permit: OwnedSemaphorePermit,
    file: File,
    remaining: u64,
    deadline: Instant,
}

async fn download_chunk(mut transfer: Download) -> io::Result<Option<(Bytes, Download)>> {
    if transfer.remaining == 0 {
        return Ok(None);
    }
    let mut bytes = vec![0; transfer.remaining.min(64 * 1024) as usize];
    timeout_at(transfer.deadline, transfer.file.read_exact(&mut bytes))
        .await
        .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))??;
    transfer.remaining -= bytes.len() as u64;
    Ok(Some((Bytes::from(bytes), transfer)))
}

#[derive(Serialize)]
struct StagedRestore {
    schema_version: i64,
    created_at: i64,
}

async fn upload(
    State(state): State<AppState>,
    Extension(operations): Extension<Operations>,
    Extension(token): Extension<BrowserToken>,
    request: Request,
) -> Result<(StatusCode, Json<StagedRestore>), Problem> {
    let permit = operations.start_transfer()?;
    operations.consume(Action::Restore, &token.0)?;
    let (parts, body) = request.into_parts();
    let result = receive_snapshot(&operations, &parts.headers, body).await;
    let pending = match result {
        Ok(pending) => pending,
        Err(error) => {
            audit(&state, AuditAction::RestoreValidate, error_outcome(&error)).await;
            return Err(error);
        }
    };
    let validated = match pending.validate().await {
        Ok(validated) => validated,
        Err(_) => {
            audit(&state, AuditAction::RestoreValidate, AuditOutcome::Rejected).await;
            return Err(Problem::invalid("invalid_snapshot"));
        }
    };
    let report = StagedRestore {
        schema_version: validated.schema_version,
        created_at: validated.created_at,
    };
    audit(&state, AuditAction::RestoreValidate, AuditOutcome::Success).await;
    // Revoking the session during a slow upload prevents publishing the pending restore.
    state.auth.session(&token.0).await?;
    let result = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        validated.publish()
    })
    .await
    .map_err(|_| Problem::unavailable())?;
    if let Err(error) = result {
        let problem = stage_problem(error);
        audit(&state, AuditAction::RestoreStage, error_outcome(&problem)).await;
        return Err(problem);
    }
    audit(&state, AuditAction::RestoreStage, AuditOutcome::Success).await;
    Ok((StatusCode::CREATED, Json(report)))
}

async fn receive_snapshot(
    operations: &Operations,
    headers: &HeaderMap,
    body: Body,
) -> Result<PendingUpload, Problem> {
    if headers
        .get(header::CONTENT_TYPE)
        .is_none_or(|value| value != SNAPSHOT_MEDIA_TYPE)
        || headers.contains_key(header::CONTENT_ENCODING)
    {
        return Err(Problem(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported_snapshot_media_type",
        ));
    }
    if headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .is_some_and(|size| size > MAX_SNAPSHOT_BYTES)
    {
        return Err(snapshot_too_large());
    }
    let database = operations.shared.database.clone();
    let pending = tokio::task::spawn_blocking(move || PendingUpload::reserve(&database))
        .await
        .map_err(|_| Problem::unavailable())?
        .map_err(stage_problem)?;
    let mut file = File::options()
        .write(true)
        .open(pending.path())
        .await
        .map_err(|_| Problem::unavailable())?;
    timeout(TRANSFER_TIMEOUT, write_snapshot(&mut file, body))
        .await
        .map_err(|_| Problem(StatusCode::REQUEST_TIMEOUT, "snapshot_upload_timed_out"))??;
    Ok(pending)
}

async fn write_snapshot(file: &mut File, body: Body) -> Result<(), Problem> {
    let mut size = 0_u64;
    let mut stream = body.into_data_stream();
    while let Some(item) = stream.next().await {
        let bytes =
            item.map_err(|_| Problem(StatusCode::BAD_REQUEST, "invalid_snapshot_upload"))?;
        size = size.saturating_add(bytes.len() as u64);
        if size > MAX_SNAPSHOT_BYTES {
            return Err(snapshot_too_large());
        }
        file.write_all(&bytes)
            .await
            .map_err(|_| Problem::unavailable())?;
    }
    file.flush().await.map_err(|_| Problem::unavailable())
}

async fn cancel(
    State(state): State<AppState>,
    Extension(operations): Extension<Operations>,
    Extension(token): Extension<BrowserToken>,
) -> Result<StatusCode, Problem> {
    let permit = operations.start_transfer()?;
    operations.consume(Action::Restore, &token.0)?;
    let database = operations.shared.database.clone();
    let result = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        cancel_pending_restore(&database)
    })
    .await
    .map_err(|_| Problem::unavailable())?;
    if let Err(error) = result {
        let problem = if error.kind() == io::ErrorKind::NotFound {
            Problem(StatusCode::NOT_FOUND, "no_pending_restore")
        } else {
            Problem::unavailable()
        };
        audit(&state, AuditAction::RestoreCancel, error_outcome(&problem)).await;
        return Err(problem);
    }
    audit(&state, AuditAction::RestoreCancel, AuditOutcome::Success).await;
    Ok(StatusCode::NO_CONTENT)
}

fn stage_problem(error: io::Error) -> Problem {
    if error.kind() == io::ErrorKind::AlreadyExists {
        Problem(StatusCode::CONFLICT, "restore_already_pending")
    } else {
        Problem::unavailable()
    }
}

fn snapshot_too_large() -> Problem {
    Problem(StatusCode::PAYLOAD_TOO_LARGE, "snapshot_too_large")
}

fn error_outcome(error: &Problem) -> AuditOutcome {
    if error.0.is_client_error() {
        AuditOutcome::Rejected
    } else {
        AuditOutcome::Failed
    }
}

async fn audit(state: &AppState, action: AuditAction, outcome: AuditOutcome) {
    if audit_event(&state.pool, action, AuditActor::Admin, None, outcome)
        .await
        .is_err()
    {
        tracing::warn!("could not record system operation audit event");
    }
}

#[cfg(test)]
mod tests {
    use axum::{body::to_bytes, http::Request};
    use serde_json::{Value, json};
    use tower::ServiceExt;

    use crate::{AdminConfig, Auth, BootstrapKind, PublicHosts, open_database};

    use super::*;

    #[test]
    fn authorization_is_one_use_and_bound_to_the_browser_session_and_action() {
        let operations = Operations::new(PathBuf::from("unused.sqlite"), false);
        assert!(
            operations
                .grant(Action::Backup, "synthetic-session-a")
                .is_ok()
        );
        assert!(
            operations
                .consume(Action::Backup, "synthetic-session-b")
                .is_err()
        );
        assert!(
            operations
                .consume(Action::Restore, "synthetic-session-a")
                .is_err()
        );
        assert!(
            operations
                .consume(Action::Backup, "synthetic-session-a")
                .is_ok()
        );
        assert!(
            operations
                .consume(Action::Backup, "synthetic-session-a")
                .is_err()
        );
    }

    #[test]
    fn expired_grants_cannot_be_consumed() {
        let operations = Operations::new(PathBuf::from("unused.sqlite"), false);
        operations.shared.grants.lock().unwrap()[Action::Backup as usize] = Some(Grant {
            session: Sha256::digest("synthetic-session").into(),
            expires_at: Instant::now(),
        });
        assert!(
            operations
                .consume(Action::Backup, "synthetic-session")
                .is_err()
        );
    }

    #[test]
    fn reads_only_a_valid_linux_resident_memory_value() {
        assert_eq!(
            parse_rss("Name:\tshortcircuit\nVmRSS:\t1042 kB\n"),
            Some(1_067_008)
        );
        for invalid in [
            "VmHWM: 1042 kB",
            "VmRSS: two kB",
            "VmRSS: 1042 MB",
            "VmRSS: 18446744073709551615 kB",
        ] {
            assert_eq!(parse_rss(invalid), None, "{invalid}");
        }
    }

    #[tokio::test]
    async fn browser_backup_round_trips_through_checked_staging_and_discard() {
        let work = tempfile::tempdir().unwrap();
        let database = work.path().join("live.sqlite");
        let pool = open_database(&database).await.unwrap();
        let auth = Auth::new(pool.clone());
        let setup = auth.issue_bootstrap(BootstrapKind::Setup).await.unwrap();
        let password = "synthetic-test-password";
        let account = auth.setup(&setup, "owner", password).await.unwrap();
        let cookie = format!("__Host-shortcircuit_session={}", account.token);
        let router = super::super::build_admin_router(
            pool,
            PublicHosts::parse("go.example").unwrap(),
            AdminConfig::new("https://admin.example", 8080).unwrap(),
            None,
            Some(Operations::new(database.clone(), false)),
            None,
        );

        let request = |method: Method, path: &str| {
            Request::builder()
                .method(method)
                .uri(path)
                .header(header::HOST, "admin.example")
                .header(header::ORIGIN, "https://admin.example")
                .header(header::COOKIE, &cookie)
                .header("x-csrf-token", &account.session.csrf)
        };
        let send = |request| router.clone().oneshot(request);
        let authorizing = |path| {
            request(Method::POST, path)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"password": password}).to_string()))
                .unwrap()
        };

        let backup_path = "/api/v1/system/backup";
        let restore_path = "/api/v1/system/restore";
        let before = send(
            request(Method::GET, backup_path)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(before.status(), StatusCode::FORBIDDEN);
        assert_eq!(
            send(authorizing("/api/v1/system/backup/authorize"))
                .await
                .unwrap()
                .status(),
            StatusCode::NO_CONTENT
        );
        let head = send(
            request(Method::HEAD, backup_path)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(head.status(), StatusCode::METHOD_NOT_ALLOWED);
        let response = send(
            request(Method::GET, backup_path)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert!(
            response.headers()[header::CONTENT_DISPOSITION]
                .to_str()
                .unwrap()
                .contains("attachment")
        );
        let snapshot = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        assert!(snapshot.starts_with(b"SQLite format 3\0"));
        assert!(
            std::fs::read_dir(database.with_file_name("backups"))
                .unwrap()
                .all(|entry| entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".backup-status"))
        );

        assert_eq!(
            send(authorizing("/api/v1/system/restore/authorize"))
                .await
                .unwrap()
                .status(),
            StatusCode::NO_CONTENT
        );
        let uploaded = send(
            request(Method::POST, restore_path)
                .header(header::CONTENT_TYPE, SNAPSHOT_MEDIA_TYPE)
                .body(Body::from(snapshot))
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(uploaded.status(), StatusCode::CREATED);
        assert_eq!(restore_status(&database), RestoreStatus::PendingRestart);
        let diagnostics = send(
            request(Method::GET, "/api/v1/system")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(diagnostics.status(), StatusCode::OK);
        let contents = to_bytes(diagnostics.into_body(), 4096).await.unwrap();
        let report: Value = serde_json::from_slice(&contents).unwrap();
        assert_eq!(report["restore_status"], "pending_restart");
        assert_eq!(report["link_count"], 0);
        assert_eq!(report["raw_event_count"], 0);
        assert_eq!(report["last_backup_status"], "verified");
        assert!(report["last_verified_backup_at"].as_u64().unwrap() > 0);
        assert!(report["started_at"].as_u64().unwrap() > 0);
        assert_eq!(report["monitoring_enabled"], false);
        assert_eq!(report["last_monitor_check_at"], Value::Null);
        assert!(!String::from_utf8_lossy(&contents).contains("live.sqlite"));

        assert_eq!(
            send(authorizing("/api/v1/system/restore/authorize"))
                .await
                .unwrap()
                .status(),
            StatusCode::NO_CONTENT
        );
        let discarded = send(
            request(Method::DELETE, restore_path)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(discarded.status(), StatusCode::NO_CONTENT);
        assert_eq!(restore_status(&database), RestoreStatus::None);
    }
}
