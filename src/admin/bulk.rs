use std::collections::HashSet;

use axum::{
    Extension, Json, Router,
    extract::{State, rejection::JsonRejection},
    http::StatusCode,
    response::Response,
    routing::post,
};
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;

use crate::{
    domains,
    history::{Action, Actor},
    input::link_tags,
    store::{self, Link, LinkUpdate, bulk::{self, LinkSelection}},
};

use super::{AppState, Problem, exchange, json_payload};

const MAX_SELECTION: usize = 50;

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/links/bulk", post(change))
        .route("/api/v1/links/bulk/export", post(export))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Update {
    links: Vec<LinkSelection>,
    change: Change,
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Change {
    SetEnabled { enabled: bool },
    AddTags { tags: Vec<String> },
    RemoveTags { tags: Vec<String> },
    Delete { confirmed: bool },
}

impl Change {
    fn validate(&mut self) -> Result<(), Problem> {
        match self {
            Self::AddTags { tags } | Self::RemoveTags { tags } => {
                *tags = link_tags(std::mem::take(tags))
                    .filter(|tags| !tags.is_empty())
                    .ok_or_else(|| Problem::invalid("invalid_tags"))?;
            }
            Self::Delete { confirmed: false } => {
                return Err(Problem::invalid("bulk_delete_confirmation"));
            }
            _ => {}
        }
        Ok(())
    }

    fn link_update(&self, link: &Link) -> Result<LinkUpdate, Problem> {
        let mut update = LinkUpdate::default();
        match self {
            Self::SetEnabled { enabled } => update.enabled = Some(*enabled),
            Self::AddTags { tags } => {
                let mut merged = link.tags.clone();
                merged.extend_from_slice(tags);
                merged.sort_unstable();
                merged.dedup();
                update.tags = Some(link_tags(merged).ok_or_else(|| Problem::invalid("bulk_tag_limit"))?);
            }
            Self::RemoveTags { tags } => {
                update.tags = Some(link.tags.iter().filter(|tag| !tags.contains(tag)).cloned().collect());
            }
            Self::Delete { .. } => return Err(Problem::invalid("invalid_bulk_selection")),
        }
        Ok(update)
    }
}

#[derive(Serialize)]
struct UpdateResult {
    items: Vec<Link>,
    deleted_ids: Vec<i64>,
}

async fn change(
    State(state): State<AppState>,
    Extension(actor): Extension<Actor>,
    body: Result<Json<Update>, JsonRejection>,
) -> Result<Json<UpdateResult>, Problem> {
    let mut input = json_payload(body)?;
    check_selection(&input.links)?;
    input.change.validate()?;
    let hosts = hosts(&state).await?;
    let mut transaction = state.pool.begin_with("BEGIN IMMEDIATE").await.map_err(|_| Problem::unavailable())?;
    let links = selected(&mut transaction, &hosts, &input.links).await?;
    if matches!(input.change, Change::Delete { .. }) {
        let count = bulk::delete_selected(&mut transaction, &input.links)
            .await
            .map_err(|_| Problem::unavailable())?;
        if count != input.links.len() as u64 {
            return Err(selection_changed());
        }
        transaction.commit().await.map_err(|_| Problem::unavailable())?;
        tracing::info!(links_deleted = count, "bulk link deletion committed");
        let deleted_ids = links.into_iter().map(|(link, _)| link.id).collect();
        return Ok(Json(UpdateResult { items: Vec::new(), deleted_ids }));
    }
    let mut items = Vec::with_capacity(links.len());
    for (link, _) in links {
        let update = input.change.link_update(&link)?;
        let item = store::write_link_in(&mut transaction, link, update, &actor, Action::Metadata)
            .await
            .map_err(|_| Problem::unavailable())?;
        items.push(item);
    }
    transaction.commit().await.map_err(|_| Problem::unavailable())?;
    Ok(Json(UpdateResult { items, deleted_ids: Vec::new() }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Export {
    links: Vec<LinkSelection>,
    format: exchange::ExportFormat,
}

async fn export(
    State(state): State<AppState>,
    body: Result<Json<Export>, JsonRejection>,
) -> Result<Response, Problem> {
    let input = json_payload(body)?;
    check_selection(&input.links)?;
    let hosts = hosts(&state).await?;
    let mut connection = state.pool.acquire().await.map_err(|_| Problem::unavailable())?;
    let links = selected(&mut connection, &hosts, &input.links).await?;
    drop(connection);
    exchange::export_selected(links, input.format)
}

fn check_selection(items: &[LinkSelection]) -> Result<(), Problem> {
    let mut seen = HashSet::with_capacity(items.len().min(MAX_SELECTION));
    if !(1..=MAX_SELECTION).contains(&items.len()) || items.iter().any(|item| {
        item.id <= 0 || !seen.insert(item.id) || item.key.len() != 32
            || !item.key.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    }) {
        return Err(Problem::invalid("invalid_bulk_selection"));
    }
    Ok(())
}

async fn hosts(state: &AppState) -> Result<Vec<String>, Problem> {
    Ok(domains::list(&state.pool, &state.hosts, state.config.local_port)
        .await
        .map_err(|_| Problem::unavailable())?
        .into_iter()
        .map(|domain| domain.host)
        .collect())
}

async fn selected(
    connection: &mut SqliteConnection,
    hosts: &[String],
    selection: &[LinkSelection],
) -> Result<Vec<(Link, bool)>, Problem> {
    let items = bulk::read_selected(connection, hosts, selection)
        .await
        .map_err(|_| Problem::unavailable())?;
    if items.len() != selection.len() {
        return Err(selection_changed());
    }
    Ok(items)
}

fn selection_changed() -> Problem {
    Problem(StatusCode::CONFLICT, "bulk_selection_changed")
}
