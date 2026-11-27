use axum::{
    Extension, Json, Router,
    extract::{Path, State, rejection::PathRejection},
    http::StatusCode,
    routing::{get, post},
};
use serde::Serialize;

use crate::{
    history::Actor,
    previews,
    titles::{LookupEntry, RequestError, TitleLookup},
};

use super::{AppState, Problem, resource_id};

pub(super) fn routes(titles: TitleLookup) -> Router<AppState> {
    Router::new()
        .route("/api/v1/title-lookups", get(list).post(queue_untitled))
        .route("/api/v1/links/{id}/title-lookup", post(queue_link))
        // Browser sessions only, like title lookups: it starts outgoing requests.
        .route("/api/v1/links/{id}/preview/refresh", post(refresh_preview))
        .layer(Extension(titles))
}

#[derive(Serialize)]
struct LookupList {
    enabled: bool,
    pending: i64,
    items: Vec<LookupEntry>,
}

#[derive(Serialize)]
struct Queued {
    queued: u64,
}

fn problem(error: RequestError) -> Problem {
    match error {
        RequestError::Disabled => Problem(StatusCode::CONFLICT, "title_lookup_disabled"),
        RequestError::Missing => Problem(StatusCode::NOT_FOUND, "not_found"),
        RequestError::TitlePresent => Problem(StatusCode::CONFLICT, "title_present"),
        RequestError::Protected => Problem(StatusCode::CONFLICT, "title_lookup_protected"),
        RequestError::Database => Problem::unavailable(),
    }
}

fn requester(actor: &Actor) -> String {
    match actor {
        Actor::Admin(name) => name.clone(),
        Actor::ApiKey(id) => format!("API key {id}"),
    }
}

async fn list(
    State(state): State<AppState>,
    Extension(titles): Extension<TitleLookup>,
) -> Result<Json<LookupList>, Problem> {
    let (pending, items) = titles
        .list(&state.pool)
        .await
        .map_err(|_| Problem::unavailable())?;
    Ok(Json(LookupList {
        enabled: titles.enabled(),
        pending,
        items,
    }))
}

async fn queue_untitled(
    State(state): State<AppState>,
    Extension(titles): Extension<TitleLookup>,
    Extension(actor): Extension<Actor>,
) -> Result<(StatusCode, Json<Queued>), Problem> {
    let queued = titles
        .request_untitled(&state.pool, &requester(&actor))
        .await
        .map_err(problem)?;
    Ok((StatusCode::ACCEPTED, Json(Queued { queued })))
}

async fn queue_link(
    State(state): State<AppState>,
    Extension(titles): Extension<TitleLookup>,
    Extension(actor): Extension<Actor>,
    id: Result<Path<i64>, PathRejection>,
) -> Result<StatusCode, Problem> {
    titles
        .request(&state.pool, resource_id(id)?, &requester(&actor))
        .await
        .map_err(problem)?;
    Ok(StatusCode::ACCEPTED)
}

fn preview_problem(error: previews::RequestError) -> Problem {
    match error {
        previews::RequestError::Disabled => {
            Problem(StatusCode::CONFLICT, "preview_images_disabled")
        }
        previews::RequestError::TitleLookupRequired => {
            Problem(StatusCode::CONFLICT, "title_lookup_required")
        }
        previews::RequestError::Missing => Problem(StatusCode::NOT_FOUND, "not_found"),
        previews::RequestError::Protected => Problem(StatusCode::CONFLICT, "preview_protected"),
        previews::RequestError::Database => Problem::unavailable(),
    }
}

/// Clears the link's preview and queues a new fetch from its destination page.
async fn refresh_preview(
    State(state): State<AppState>,
    Extension(titles): Extension<TitleLookup>,
    id: Result<Path<i64>, PathRejection>,
) -> Result<StatusCode, Problem> {
    titles
        .request_preview(&state.pool, resource_id(id)?)
        .await
        .map_err(preview_problem)?;
    Ok(StatusCode::ACCEPTED)
}
