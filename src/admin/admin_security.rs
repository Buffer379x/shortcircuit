use axum::{
    Extension, Json,
    extract::{
        Path, Query, State,
        rejection::{JsonRejection, PathRejection, QueryRejection},
    },
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use sqlx::{SqliteConnection, SqlitePool};

use crate::auth::AuthError;

use super::{AppState, KeyCreated, Problem, access::BrowserToken, json_payload, resource_id};

#[derive(Clone, Copy)]
pub(crate) enum AuditAction {
    SetupTokenIssued,
    RecoveryTokenIssued,
    AccountSetup,
    Login,
    AccountRecovery,
    PasswordChange,
    KeyCreate,
    KeyRotate,
    KeyRevoke,
    BackupAuthorize,
    BackupDownload,
    RestoreAuthorize,
    RestoreValidate,
    RestoreStage,
    RestoreCancel,
    RestoreApply,
    MonitoringUpdate,
    DestinationPolicyUpdate,
    DomainDelete,
    OperationsSettingsAuthorize,
    OperationsSettingsUpdate,
    PublicPagesUpdate,
    PublicPagesReset,
    PublicPagesImport,
}

impl AuditAction {
    fn label(self) -> &'static str {
        match self {
            Self::SetupTokenIssued => "setup_token_issued",
            Self::RecoveryTokenIssued => "recovery_token_issued",
            Self::AccountSetup => "account_setup",
            Self::Login => "login",
            Self::AccountRecovery => "account_recovery",
            Self::PasswordChange => "password_change",
            Self::KeyCreate => "key_create",
            Self::KeyRotate => "key_rotate",
            Self::KeyRevoke => "key_revoke",
            Self::BackupAuthorize => "backup_authorize",
            Self::BackupDownload => "backup_download",
            Self::RestoreAuthorize => "restore_authorize",
            Self::RestoreValidate => "restore_validate",
            Self::RestoreStage => "restore_stage",
            Self::RestoreCancel => "restore_cancel",
            Self::RestoreApply => "restore_apply",
            Self::MonitoringUpdate => "monitoring_update",
            Self::DestinationPolicyUpdate => "destination_policy_update",
            Self::DomainDelete => "domain_delete",
            Self::OperationsSettingsAuthorize => "operations_settings_authorize",
            Self::OperationsSettingsUpdate => "operations_settings_update",
            Self::PublicPagesUpdate => "public_pages_update",
            Self::PublicPagesReset => "public_pages_reset",
            Self::PublicPagesImport => "public_pages_import",
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum AuditActor {
    Admin,
    System,
}

#[derive(Clone, Copy)]
pub(crate) enum AuditOutcome {
    Success,
    Rejected,
    Failed,
}

impl AuditOutcome {
    fn label(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Rejected => "rejected",
            Self::Failed => "failed",
        }
    }
}

pub(crate) async fn record_audit(
    connection: &mut SqliteConnection,
    action: AuditAction,
    actor: AuditActor,
    object_id: Option<i64>,
    outcome: AuditOutcome,
) -> Result<(), sqlx::Error> {
    record_audit_details(connection, action, actor, object_id, outcome, None).await
}

/// Records an audit entry with a small JSON object of non-secret details, such as a host name
/// and counts. The schema bounds the object to 1,024 bytes.
pub(crate) async fn record_audit_details(
    connection: &mut SqliteConnection,
    action: AuditAction,
    actor: AuditActor,
    object_id: Option<i64>,
    outcome: AuditOutcome,
    details: Option<&serde_json::Value>,
) -> Result<(), sqlx::Error> {
    let (actor, actor_id) = match actor {
        AuditActor::Admin => ("admin", Some(1_i64)),
        AuditActor::System => ("system", None),
    };
    // Entries without details name only the columns that every schema since 17 has, so
    // code that runs against an older schema, such as an upgrade test, keeps working.
    let statement = if details.is_some() {
        "INSERT INTO admin_audit (action, actor, actor_id, object_id, outcome, details)
         VALUES (?, ?, ?, ?, ?, ?)"
    } else {
        "INSERT INTO admin_audit (action, actor, actor_id, object_id, outcome)
         VALUES (?, ?, ?, ?, ?)"
    };
    let mut query = sqlx::query(statement)
        .bind(action.label())
        .bind(actor)
        .bind(actor_id)
        .bind(object_id)
        .bind(outcome.label());
    if let Some(details) = details {
        query = query.bind(details.to_string());
    }
    query.execute(connection).await?;
    Ok(())
}

pub(crate) async fn audit_event(
    pool: &SqlitePool,
    action: AuditAction,
    actor: AuditActor,
    object_id: Option<i64>,
    outcome: AuditOutcome,
) -> Result<(), sqlx::Error> {
    let mut connection = pool.acquire().await?;
    record_audit(&mut connection, action, actor, object_id, outcome).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RotateInput {
    current_password: String,
}

pub(super) async fn rotate_key(
    State(state): State<AppState>,
    Extension(token): Extension<BrowserToken>,
    id: Result<Path<i64>, PathRejection>,
    body: Result<Json<RotateInput>, JsonRejection>,
) -> Result<(StatusCode, Json<KeyCreated>), Problem> {
    let id = resource_id(id)?;
    let input = json_payload(body)?;
    let issued = match state
        .auth
        .rotate_key(id, &token.0, &input.current_password)
        .await
    {
        Ok(issued) => issued,
        Err(error) => {
            let outcome = match error {
                AuthError::Unauthorized | AuthError::NotFound | AuthError::Conflict => {
                    Some(AuditOutcome::Rejected)
                }
                AuthError::Unavailable => Some(AuditOutcome::Failed),
                _ => None,
            };
            if let Some(outcome) = outcome {
                audit_event(
                    &state.pool,
                    AuditAction::KeyRotate,
                    AuditActor::Admin,
                    Some(id),
                    outcome,
                )
                .await
                .map_err(|_| Problem::unavailable())?;
            }
            return Err(match error {
                AuthError::Unauthorized => Problem(StatusCode::UNAUTHORIZED, "invalid_credentials"),
                AuthError::Conflict => Problem(StatusCode::CONFLICT, "expired_key"),
                _ => error.into(),
            });
        }
    };
    Ok((
        StatusCode::CREATED,
        Json(KeyCreated {
            key: issued.key,
            token: issued.token,
        }),
    ))
}

const AUDIT_PAGE: i64 = 100;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AuditQuery {
    before: Option<i64>,
    limit: Option<i64>,
}

#[derive(Serialize, sqlx::FromRow)]
struct AuditRow {
    id: i64,
    created_at: i64,
    action: String,
    actor: String,
    actor_id: Option<i64>,
    object_id: Option<i64>,
    outcome: String,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "json_object"
    )]
    details: Option<String>,
}

/// Writes the stored details as the JSON object they hold. The schema accepts only valid JSON
/// objects, so a parse failure cannot occur for stored rows.
fn json_object<S: serde::Serializer>(
    value: &Option<String>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let parsed = value
        .as_deref()
        .map(serde_json::from_str::<serde_json::Value>)
        .transpose()
        .map_err(serde::ser::Error::custom)?;
    serde::Serialize::serialize(&parsed, serializer)
}

#[derive(Serialize)]
pub(super) struct AuditPage {
    items: Vec<AuditRow>,
    next_before: Option<i64>,
}

pub(super) async fn list_audit(
    State(state): State<AppState>,
    query: Result<Query<AuditQuery>, QueryRejection>,
) -> Result<Json<AuditPage>, Problem> {
    let Query(query) = query.map_err(|_| Problem::invalid("invalid_audit_query"))?;
    let limit = query.limit.unwrap_or(AUDIT_PAGE);
    if !(1..=AUDIT_PAGE).contains(&limit) || query.before.is_some_and(|before| before < 1) {
        return Err(Problem::invalid("invalid_audit_query"));
    }
    let mut items = sqlx::query_as::<_, AuditRow>(
        "SELECT id, created_at, action, actor, actor_id, object_id, outcome, details FROM admin_audit
         WHERE id <= ? ORDER BY id DESC LIMIT ?",
    )
    .bind(query.before.map_or(i64::MAX, |id| id - 1))
    .bind(limit + 1)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| Problem::unavailable())?;
    let next_before = if items.len() > limit as usize {
        items.pop();
        items.last().map(|row| row.id)
    } else {
        None
    };
    Ok(Json(AuditPage { items, next_before }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every action, in declaration order. A new variant fails to compile in the match below
    /// until it is listed here too, so the schema check covers it.
    fn every_action() -> Vec<AuditAction> {
        use AuditAction::*;
        let listed = vec![
            SetupTokenIssued,
            RecoveryTokenIssued,
            AccountSetup,
            Login,
            AccountRecovery,
            PasswordChange,
            KeyCreate,
            KeyRotate,
            KeyRevoke,
            BackupAuthorize,
            BackupDownload,
            RestoreAuthorize,
            RestoreValidate,
            RestoreStage,
            RestoreCancel,
            RestoreApply,
            MonitoringUpdate,
            DestinationPolicyUpdate,
            DomainDelete,
            OperationsSettingsAuthorize,
            OperationsSettingsUpdate,
            PublicPagesUpdate,
            PublicPagesReset,
            PublicPagesImport,
        ];
        for action in &listed {
            match action {
                SetupTokenIssued
                | RecoveryTokenIssued
                | AccountSetup
                | Login
                | AccountRecovery
                | PasswordChange
                | KeyCreate
                | KeyRotate
                | KeyRevoke
                | BackupAuthorize
                | BackupDownload
                | RestoreAuthorize
                | RestoreValidate
                | RestoreStage
                | RestoreCancel
                | RestoreApply
                | MonitoringUpdate
                | DestinationPolicyUpdate
                | DomainDelete
                | OperationsSettingsAuthorize
                | OperationsSettingsUpdate
                | PublicPagesUpdate
                | PublicPagesReset
                | PublicPagesImport => {}
            }
        }
        listed
    }

    #[tokio::test]
    async fn the_schema_accepts_every_audit_action_and_bounds_details() {
        let directory = tempfile::tempdir().unwrap();
        let pool = crate::open_database(&directory.path().join("audit.sqlite"))
            .await
            .unwrap();
        let mut connection = pool.acquire().await.unwrap();
        for action in every_action() {
            record_audit(
                &mut connection,
                action,
                AuditActor::Admin,
                None,
                AuditOutcome::Success,
            )
            .await
            .unwrap_or_else(|error| panic!("{}: {error}", action.label()));
        }
        let details = serde_json::json!({"host": "old.example", "links": 2});
        record_audit_details(
            &mut connection,
            AuditAction::DomainDelete,
            AuditActor::Admin,
            None,
            AuditOutcome::Success,
            Some(&details),
        )
        .await
        .unwrap();
        for invalid in [
            "[]",
            "\"text\"",
            "{",
            &format!("{{\"x\":\"{}\"}}", "a".repeat(1024)),
        ] {
            let rejected = sqlx::query(
                "INSERT INTO admin_audit (action, actor, actor_id, outcome, details)
                 VALUES ('domain_delete', 'admin', 1, 'success', ?)",
            )
            .bind(invalid)
            .execute(&mut *connection)
            .await;
            assert!(rejected.is_err(), "{invalid}");
        }
    }
}
