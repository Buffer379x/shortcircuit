use axum::{
    Extension, Json, Router,
    extract::State,
    http::StatusCode,
    routing::{get, post},
};
use serde::Serialize;

use crate::notifications::{self, Counts, Delivery, Event, EventKind, Notifier};

use super::{AppState, Problem};

pub(super) fn routes(notifier: Option<Notifier>) -> Router<AppState> {
    Router::new()
        .route("/api/v1/notifications", get(status))
        .route("/api/v1/notifications/test", post(send_test))
        .layer(Extension(notifier))
}

#[derive(Serialize)]
struct Status {
    enabled: bool,
    /// The webhook scheme, host, and port. The path, query, and credentials are not shown.
    origin: Option<String>,
    connector: bool,
    events: Vec<EventKind>,
    click_milestones: Vec<i64>,
    expiry_notice_hours: Option<i64>,
    counts: Counts,
    items: Vec<Delivery>,
}

async fn status(
    State(state): State<AppState>,
    Extension(notifier): Extension<Option<Notifier>>,
) -> Result<Json<Status>, Problem> {
    let (counts, items) = notifications::status(&state.pool)
        .await
        .map_err(|_| Problem::unavailable())?;
    Ok(Json(
        match notifier.and_then(|notifier| notifier.config()) {
            Some(notifier) => Status {
                enabled: true,
                origin: Some(notifier.origin()),
                connector: notifier.uses_connector(),
                events: notifier.events(),
                click_milestones: notifier.milestones().to_vec(),
                expiry_notice_hours: Some(notifier.expiry_notice_hours()),
                counts,
                items,
            },
            None => Status {
                enabled: false,
                origin: None,
                connector: false,
                events: Vec::new(),
                click_milestones: Vec::new(),
                expiry_notice_hours: None,
                counts,
                items,
            },
        },
    ))
}

async fn send_test(
    State(state): State<AppState>,
    Extension(notifier): Extension<Option<Notifier>>,
) -> Result<StatusCode, Problem> {
    let notifier = notifier
        .filter(|notifier| notifier.config().is_some())
        .ok_or(Problem(StatusCode::CONFLICT, "notifications_disabled"))?;
    let mut nonce = [0; 16];
    getrandom::fill(&mut nonce).map_err(|_| Problem::unavailable())?;
    notifier
        .record(
            &state.pool,
            Event::Test {
                nonce: u128::from_be_bytes(nonce),
            },
        )
        .await
        .map_err(|_| Problem::unavailable())?;
    Ok(StatusCode::ACCEPTED)
}
