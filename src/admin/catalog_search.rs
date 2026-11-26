use axum::{
    Json, Router,
    extract::{State, rejection::JsonRejection},
    routing::post,
};

use crate::{domains, input::{normalize_host, tag_name}, store::catalog::{self, Search, SearchPage}};

use super::{AppState, Problem, json_payload};

pub(super) fn routes() -> Router<AppState> {
    Router::new().route("/api/v1/links/search", post(search_links))
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
        filter.query = (!text.is_empty()).then(|| text.to_ascii_lowercase());
    }
    if let Some(host) = &filter.host {
        filter.host = Some(normalize_host(host).ok_or_else(|| Problem::invalid("invalid_host"))?);
    }
    if let Some(tag) = &filter.tag {
        filter.tag = Some(tag_name(tag).ok_or_else(|| Problem::invalid("invalid_tags"))?.to_owned());
    }
    if filter.after.as_ref().is_some_and(|after| {
        after.id <= 0 || after.key.len() > 4096 || after.sort != filter.sort
    }) {
        return Err(Problem::invalid("invalid_link_cursor"));
    }
    let hosts: Vec<String> = domains::list(&state.pool, &state.hosts, state.config.local_port)
        .await
        .map_err(|_| Problem::unavailable())?
        .into_iter()
        .map(|domain| domain.host)
        .collect();
    catalog::search(&state.pool, &hosts, &filter)
        .await
        .map(Json)
        .map_err(|_| Problem::unavailable())
}
