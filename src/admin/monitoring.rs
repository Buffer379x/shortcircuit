use axum::{
    Extension, Json, Router,
    extract::{
        Path, Query, State,
        rejection::{JsonRejection, PathRejection, QueryRejection},
    },
    http::StatusCode,
    routing::{get, patch, post},
};
use serde::{Deserialize, Serialize};

use crate::{
    domains,
    input::normalize_host,
    monitor::{CheckError, LinkCheck, Monitor},
    outbound::{Failure, Probe},
};

use super::{
    AppState, Problem,
    admin_security::{self, AuditAction, AuditActor, AuditOutcome},
    json_payload, resource_id,
};

pub(super) fn routes(monitor: Monitor) -> Router<AppState> {
    Router::new()
        .route("/api/v1/monitoring", get(list))
        .route("/api/v1/monitoring/check", post(queue_checks))
        .route("/api/v1/monitoring/links/{id}", patch(enroll))
        .route("/api/v1/monitoring/links/{id}/check", post(check))
        .route("/api/v1/monitoring/domain", post(domain))
        .layer(Extension(monitor))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Page {
    after: Option<i64>,
}

#[derive(Serialize)]
struct Report {
    enabled: bool,
    items: Vec<LinkCheck>,
    next_after: Option<i64>,
}

async fn list(
    State(state): State<AppState>,
    Extension(monitor): Extension<Monitor>,
    query: Result<Query<Page>, QueryRejection>,
) -> Result<Json<Report>, Problem> {
    let Query(query) = query.map_err(|_| Problem::invalid("invalid_query"))?;
    let after = query.after.unwrap_or_default();
    if after < 0 {
        return Err(Problem::invalid("invalid_query"));
    }
    let mut items = monitor
        .list(&state.pool, after, 51)
        .await
        .map_err(|_| Problem::unavailable())?;
    let next_after = if items.len() > 50 {
        items.truncate(50);
        items.last().map(|item| item.id)
    } else {
        None
    };
    Ok(Json(Report {
        enabled: monitor.enabled(),
        items,
        next_after,
    }))
}

#[derive(Serialize)]
struct Queued {
    queued: u64,
}

async fn queue_checks(
    State(state): State<AppState>,
    Extension(monitor): Extension<Monitor>,
) -> Result<(StatusCode, Json<Queued>), Problem> {
    let queued = monitor.queue_checks(&state.pool).await?;
    Ok((StatusCode::ACCEPTED, Json(Queued { queued })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Enrollment {
    enabled: bool,
    #[serde(default)]
    priority: bool,
}

async fn enroll(
    State(state): State<AppState>,
    Extension(monitor): Extension<Monitor>,
    id: Result<Path<i64>, PathRejection>,
    body: Result<Json<Enrollment>, JsonRejection>,
) -> Result<StatusCode, Problem> {
    let id = resource_id(id)?;
    let input = json_payload(body)?;
    if input.enabled && !monitor.enabled() {
        return Err(CheckError::Disabled.into());
    }
    let mut transaction = state
        .pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(|_| Problem::unavailable())?;
    let exists: i64 = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM links WHERE id = ?)")
        .bind(id)
        .fetch_one(&mut *transaction)
        .await
        .map_err(|_| Problem::unavailable())?;
    if exists == 0 {
        return Err(CheckError::Missing.into());
    }
    if input.enabled {
        sqlx::query(
            "INSERT INTO link_monitor(link_id, priority) VALUES (?, ?)
                     ON CONFLICT(link_id) DO UPDATE SET priority = excluded.priority",
        )
        .bind(id)
        .bind(input.priority)
        .execute(&mut *transaction)
        .await
        .map_err(|_| Problem::unavailable())?;
    } else {
        sqlx::query("DELETE FROM link_monitor WHERE link_id = ?")
            .bind(id)
            .execute(&mut *transaction)
            .await
            .map_err(|_| Problem::unavailable())?;
    }
    admin_security::record_audit(
        &mut transaction,
        AuditAction::MonitoringUpdate,
        AuditActor::Admin,
        Some(id),
        AuditOutcome::Success,
    )
    .await
    .map_err(|_| Problem::unavailable())?;
    transaction
        .commit()
        .await
        .map_err(|_| Problem::unavailable())?;
    monitor.changed();
    Ok(StatusCode::NO_CONTENT)
}

async fn check(
    State(state): State<AppState>,
    Extension(monitor): Extension<Monitor>,
    id: Result<Path<i64>, PathRejection>,
) -> Result<Json<LinkCheck>, Problem> {
    Ok(Json(monitor.check(&state.pool, resource_id(id)?).await?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DomainInput {
    host: String,
}

#[derive(Serialize)]
struct DomainCheck {
    probe: Option<Probe>,
    failure: Option<Failure>,
}

async fn domain(
    State(state): State<AppState>,
    Extension(monitor): Extension<Monitor>,
    body: Result<Json<DomainInput>, JsonRejection>,
) -> Result<Json<DomainCheck>, Problem> {
    let input = json_payload(body)?;
    let host = normalize_host(&input.host).ok_or_else(|| Problem::invalid("invalid_host"))?;
    let domain = domains::lookup(&state.pool, &state.hosts, &host, state.config.local_port)
        .await
        .map_err(|_| Problem::unavailable())?
        .filter(|domain| domain.enabled)
        .ok_or(CheckError::Missing)?;
    let result = monitor.check_domain(&domain.origin).await?;
    let (probe, failure) = match result {
        Ok(probe) => (Some(probe), None),
        Err(failure) => (None, Some(failure)),
    };
    Ok(Json(DomainCheck { probe, failure }))
}

impl From<CheckError> for Problem {
    fn from(error: CheckError) -> Self {
        match error {
            CheckError::Disabled => Self(StatusCode::CONFLICT, "monitoring_disabled"),
            CheckError::Busy => Self(StatusCode::TOO_MANY_REQUESTS, "monitoring_busy"),
            CheckError::Inactive => Self(StatusCode::CONFLICT, "monitoring_link_inactive"),
            CheckError::Missing => Self(StatusCode::NOT_FOUND, "not_found"),
            CheckError::Database => Self::unavailable(),
        }
    }
}
