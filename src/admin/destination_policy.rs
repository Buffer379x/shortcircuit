use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State, rejection::JsonRejection},
    routing::{get, post},
};
use serde::Serialize;

use crate::destinations::{self, Impact, Policy};

use super::{
    AppState, Problem,
    admin_security::{AuditAction, AuditActor, AuditOutcome, record_audit},
    json_payload,
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/destination-policy",
            get(read_policy).patch(replace_policy),
        )
        .route("/api/v1/destination-policy/preview", post(preview_policy))
        // 200 rules with long IDNA names exceed the default 32 KiB body limit.
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
}

const BODY_LIMIT: usize = 64 * 1024;

#[derive(Serialize)]
struct PolicyReport {
    #[serde(flatten)]
    policy: Policy,
    /// Saved destinations that this policy refuses at redirect time.
    impact: Impact,
}

async fn report(state: &AppState, policy: Policy) -> Result<Json<PolicyReport>, Problem> {
    let impact = destinations::impact(&state.pool, &policy)
        .await
        .map_err(|_| Problem::unavailable())?;
    Ok(Json(PolicyReport { policy, impact }))
}

async fn read_policy(State(state): State<AppState>) -> Result<Json<PolicyReport>, Problem> {
    let policy = destinations::read(&state.pool)
        .await
        .map_err(|_| Problem::unavailable())?;
    report(&state, policy).await
}

fn validated(body: Result<Json<Policy>, JsonRejection>) -> Result<Policy, Problem> {
    let draft = json_payload(body)?;
    destinations::normalize_policy(&draft).map_err(Problem::invalid)
}

async fn preview_policy(
    State(state): State<AppState>,
    body: Result<Json<Policy>, JsonRejection>,
) -> Result<Json<PolicyReport>, Problem> {
    let policy = validated(body)?;
    report(&state, policy).await
}

async fn replace_policy(
    State(state): State<AppState>,
    body: Result<Json<Policy>, JsonRejection>,
) -> Result<Json<PolicyReport>, Problem> {
    let policy = validated(body)?;
    let mut transaction = state
        .pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(|_| Problem::unavailable())?;
    destinations::replace(&mut transaction, &policy)
        .await
        .map_err(|_| Problem::unavailable())?;
    record_audit(
        &mut transaction,
        AuditAction::DestinationPolicyUpdate,
        AuditActor::Admin,
        None,
        AuditOutcome::Success,
    )
    .await
    .map_err(|_| Problem::unavailable())?;
    transaction
        .commit()
        .await
        .map_err(|_| Problem::unavailable())?;
    report(&state, policy).await
}
