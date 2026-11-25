use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State, rejection::{JsonRejection, PathRejection, QueryRejection}},
    http::StatusCode,
    routing::{get, patch},
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::{
    input::{PublicHosts, normalize_host, valid_slug, valid_target},
    store::{self, Link, LinkDraft, LinkUpdate, PAGE_SIZE},
};

use super::{AppState, Problem, access::LinkAccess, json_payload, resource_id};

const SHORTCODE_ALPHABET: &[u8; 64] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz-_";
const SHORTCODE_LEN: usize = 10;
const MAX_CODE_ATTEMPTS: usize = 8;

#[derive(Clone)]
pub(super) struct LinkState {
    pool: SqlitePool,
    app: AppState,
}

pub(super) fn routes(pool: SqlitePool, app: AppState) -> Router {
    Router::new()
        .route("/api/v1/domains", get(domains))
        .route("/api/v1/links", get(list_links).post(create_link))
        .route("/api/v1/links/{id}", patch(update_link).delete(remove_link))
        .with_state(LinkState { pool, app })
}

impl LinkAccess {
    fn allowed_hosts<'a>(&'a self, hosts: &'a PublicHosts) -> &'a [String] {
        match &self.host {
            Some(host) if hosts.contains(host) => std::slice::from_ref(host),
            Some(_) => &[],
            None => hosts.all(),
        }
    }
}

#[derive(Serialize)]
struct Domains {
    hosts: Vec<String>,
    local_port: u16,
}

async fn domains(
    State(state): State<LinkState>,
    Extension(access): Extension<LinkAccess>,
) -> Json<Domains> {
    Json(Domains {
        hosts: access.allowed_hosts(&state.app.hosts).to_vec(),
        local_port: state.app.config.local_port,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewLink {
    host: Option<String>,
    slug: Option<String>,
    target: String,
    title: Option<String>,
    tags: Option<Vec<String>>,
}

async fn create_link(
    State(state): State<LinkState>,
    Extension(access): Extension<LinkAccess>,
    body: Result<Json<NewLink>, JsonRejection>,
) -> Result<(StatusCode, Json<Link>), Problem> {
    let input = json_payload(body)?;
    let hosts = access.allowed_hosts(&state.app.hosts);
    let host = match input.host {
        Some(host) => normalize_host(&host)
            .filter(|host| state.app.hosts.contains(host))
            .ok_or_else(|| Problem::invalid("invalid_host"))?,
        None => hosts.first().ok_or_else(forbidden)?.to_owned(),
    };
    if !hosts.contains(&host) {
        return Err(forbidden());
    }
    if !valid_target(&input.target) {
        return Err(Problem::invalid("invalid_target"));
    }
    let mut draft = LinkDraft {
        host,
        slug: String::new(),
        title: title(input.title.unwrap_or_default())?,
        tags: tags(input.tags.unwrap_or_default())?,
        target: input.target,
    };
    if let Some(slug) = input.slug {
        if !valid_slug(&slug) {
            return Err(Problem::invalid("invalid_slug"));
        }
        draft.slug = slug;
        let link = store::insert_link(&state.pool, &draft)
            .await.map_err(|_| Problem::unavailable())?
            .ok_or(Problem(StatusCode::CONFLICT,"slug_in_use"))?;
        return Ok((StatusCode::CREATED, Json(link)));
    }
    for _ in 0..MAX_CODE_ATTEMPTS {
        draft.slug = random_slug()?;
        if let Some(link) = store::insert_link(&state.pool, &draft)
            .await.map_err(|_| Problem::unavailable())?
        {
            return Ok((StatusCode::CREATED, Json(link)));
        }
    }
    Err(Problem::unavailable())
}

fn forbidden() -> Problem {
    Problem(StatusCode::FORBIDDEN, "insufficient_scope")
}

fn title(value: String) -> Result<String,Problem> {
    let value = value.trim();
    if value.chars().count() > 160 || value.chars().any(char::is_control) {
        return Err(Problem::invalid("invalid_title"));
    }
    Ok(value.to_owned())
}

fn tags(values: Vec<String>) -> Result<Vec<String>,Problem> {
    if values.len() > 20 {
        return Err(Problem::invalid("invalid_tags"));
    }
    let mut checked = Vec::with_capacity(values.len());
    for tag in values {
        let tag = tag.trim();
        if tag.is_empty() || tag.chars().count() > 40 || tag.contains(',') || tag.chars().any(char::is_control) {
            return Err(Problem::invalid("invalid_tags"));
        }
        checked.push(tag.to_owned());
    }
    checked.sort_unstable();
    checked.dedup();
    Ok(checked)
}

fn random_slug() -> Result<String,Problem> {
    let mut bytes = [0; SHORTCODE_LEN];
    getrandom::fill(&mut bytes).map_err(|_| Problem::unavailable())?;
    Ok(bytes.iter().map(|byte| char::from(SHORTCODE_ALPHABET[usize::from(*byte & 63)])).collect())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PageQuery {
    after_id: Option<i64>,
}

#[derive(Serialize)]
struct LinkPage {
    items: Vec<Link>,
    next_after_id: Option<i64>,
}

async fn list_links(
    State(state): State<LinkState>,
    Extension(access): Extension<LinkAccess>,
    query: Result<Query<PageQuery>, QueryRejection>,
) -> Result<Json<LinkPage>, Problem> {
    let Query(query) = query.map_err(|error| Problem(error.status(), "invalid_query"))?;
    let after_id = query.after_id.unwrap_or(0);
    if after_id < 0 {
        return Err(Problem::invalid("invalid_after_id"));
    }
    let mut items = store::list(&state.pool, access.allowed_hosts(&state.app.hosts), after_id)
        .await.map_err(|_| Problem::unavailable())?;
    let has_more = items.len() > PAGE_SIZE;
    items.truncate(PAGE_SIZE);
    let next_after_id = if has_more { items.last().map(|link| link.id) } else { None };
    Ok(Json(LinkPage { items, next_after_id }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PatchLink {
    target: Option<String>,
    title: Option<String>,
    enabled: Option<bool>,
    tags: Option<Vec<String>>,
}

async fn update_link(
    State(state): State<LinkState>,
    Extension(access): Extension<LinkAccess>,
    id: Result<Path<i64>,PathRejection>,
    body: Result<Json<PatchLink>,JsonRejection>,
) -> Result<Json<Link>,Problem> {
    let input = json_payload(body)?;
    if input.target.is_none() && input.title.is_none() && input.enabled.is_none() && input.tags.is_none() {
        return Err(Problem::invalid("empty_update"));
    }
    if input.target.as_deref().is_some_and(|target| !valid_target(target)) {
        return Err(Problem::invalid("invalid_target"));
    }
    let update = LinkUpdate {
        target: input.target,
        title: input.title.map(title).transpose()?,
        enabled: input.enabled,
        tags: input.tags.map(tags).transpose()?,
    };
    let id = resource_id(id)?;
    store::update_link(&state.pool, access.allowed_hosts(&state.app.hosts), id, update)
        .await
        .map_err(|_| Problem::unavailable())?
        .map(Json)
        .ok_or(Problem(StatusCode::NOT_FOUND,"not_found"))
}

async fn remove_link(
    State(state): State<LinkState>,
    Extension(access): Extension<LinkAccess>,
    id: Result<Path<i64>,PathRejection>,
) -> Result<StatusCode,Problem> {
    let removed = store::remove_link(&state.pool, access.allowed_hosts(&state.app.hosts), resource_id(id)?)
        .await.map_err(|_| Problem::unavailable())?;
    if !removed {
        return Err(Problem(StatusCode::NOT_FOUND,"not_found"));
    }
    Ok(StatusCode::NO_CONTENT)
}
