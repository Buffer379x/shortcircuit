use axum::{
    Json, Router,
    extract::{
        Path, State,
        rejection::{JsonRejection, PathRejection},
    },
    http::StatusCode,
    routing::{patch, post},
};
use serde::Deserialize;

use crate::{
    domains,
    input::{link_tags, normalize_host},
    store::catalog::{self, Organization, Search, SearchPage},
};

use super::{AppState, Problem, json_payload, resource_id};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/links/search", post(search_links))
        .route("/api/v1/links/{id}/organization", patch(organize_link))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OrganizationPatch {
    key: String,
    favorite: Option<bool>,
    archived: Option<bool>,
}

async fn organize_link(
    State(state): State<AppState>,
    id: Result<Path<i64>, PathRejection>,
    body: Result<Json<OrganizationPatch>, JsonRejection>,
) -> Result<Json<Organization>, Problem> {
    let id = resource_id(id)?;
    let input = json_payload(body)?;
    if (input.favorite.is_none() && input.archived.is_none())
        || input.key.len() != 32
        || !input
            .key
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(Problem::invalid("invalid_link_organization"));
    }
    let domains = domains::list(&state.pool, &state.hosts, state.config.local_port)
        .await
        .map_err(|_| Problem::unavailable())?;
    let hosts = domains
        .into_iter()
        .map(|domain| domain.host)
        .collect::<Vec<_>>();
    catalog::organize(
        &state.pool,
        &hosts,
        id,
        &input.key,
        input.favorite,
        input.archived,
    )
    .await
    .map_err(|_| Problem::unavailable())?
    .map(Json)
    .ok_or(Problem(StatusCode::CONFLICT, "link_organization_changed"))
}

async fn search_links(
    State(state): State<AppState>,
    body: Result<Json<Search>, JsonRejection>,
) -> Result<Json<SearchPage>, Problem> {
    let mut filter = json_payload(body)?;
    if let Some(query) = &filter.query {
        let text = query.trim();
        if text.len() > 4096 || text.chars().any(char::is_control) {
            return Err(Problem::invalid("invalid_link_search"));
        }
        filter.query = (!text.is_empty()).then(|| text.to_lowercase());
    }
    if let Some(host) = &filter.host {
        filter.host = Some(normalize_host(host).ok_or_else(|| Problem::invalid("invalid_host"))?);
    }
    if let Some(tag) = filter.tag.take() {
        if !filter.tags.is_empty() {
            return Err(Problem::invalid("invalid_tags"));
        }
        filter.tags.push(tag);
    }
    filter.tags = link_tags(filter.tags).ok_or_else(|| Problem::invalid("invalid_tags"))?;
    if filter
        .after
        .as_ref()
        .is_some_and(|after| after.id <= 0 || after.key.len() > 4096 || after.sort != filter.sort)
    {
        return Err(Problem::invalid("invalid_link_cursor"));
    }
    let domains = domains::list(&state.pool, &state.hosts, state.config.local_port)
        .await
        .map_err(|_| Problem::unavailable())?;
    catalog::search(&state.pool, &domains, &filter)
        .await
        .map(Json)
        .map_err(|_| Problem::unavailable())
}
