//! Settings > Operations: health checks, title lookup, scheduled backups, and notifications.
//! Browser sessions only. A change to the notification target or its authorization needs a
//! fresh password confirmation through `POST /api/v1/settings/operations/authorize`.

use axum::{
    Extension, Json, Router,
    extract::{State, rejection::JsonRejection},
    http::StatusCode,
    routing::{get, post},
};

use crate::runtime_settings::{Patch, RuntimeSettings, SaveError, View};

use super::{
    AppState, Problem,
    access::BrowserToken,
    admin_security::{AuditAction, AuditActor, AuditOutcome, audit_event},
    json_payload,
    operations::{self, Action, Operations, Reauthentication},
};

pub(super) fn routes(operations: Operations, settings: RuntimeSettings) -> Router<AppState> {
    Router::new()
        .route("/api/v1/settings/operations", get(read).patch(update))
        .route("/api/v1/settings/operations/authorize", post(authorize))
        .layer(Extension(operations))
        .layer(Extension(settings))
}

async fn read(Extension(settings): Extension<RuntimeSettings>) -> Json<View> {
    Json(settings.view())
}

async fn authorize(
    State(state): State<AppState>,
    Extension(operations): Extension<Operations>,
    Extension(token): Extension<BrowserToken>,
    input: Result<Json<Reauthentication>, JsonRejection>,
) -> Result<StatusCode, Problem> {
    operations::authorize(
        state,
        operations,
        token,
        json_payload(input)?,
        Action::Settings,
    )
    .await
}

async fn update(
    State(state): State<AppState>,
    Extension(operations): Extension<Operations>,
    Extension(settings): Extension<RuntimeSettings>,
    Extension(token): Extension<BrowserToken>,
    input: Result<Json<Patch>, JsonRejection>,
) -> Result<Json<View>, Problem> {
    let patch = json_payload(input)?;
    let confirm = || operations.consume(Action::Settings, &token.0).is_ok();
    match settings.save(&state.pool, &patch, confirm).await {
        Ok(()) => Ok(Json(settings.view())),
        Err(SaveError::Invalid(code)) => Err(Problem::invalid(code)),
        Err(SaveError::Unauthorized) => {
            if audit_event(
                &state.pool,
                AuditAction::OperationsSettingsUpdate,
                AuditActor::Admin,
                None,
                AuditOutcome::Rejected,
            )
            .await
            .is_err()
            {
                tracing::warn!("could not record operations settings audit event");
            }
            Err(Problem(
                StatusCode::FORBIDDEN,
                "operation_authorization_required",
            ))
        }
        Err(SaveError::Database) => Err(Problem::unavailable()),
    }
}
