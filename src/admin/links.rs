use std::collections::BTreeMap;

use axum::{
    Extension, Json, Router,
    body::Bytes,
    extract::{
        Path, Query, State,
        rejection::{JsonRejection, PathRejection, QueryRejection},
    },
    http::{StatusCode, header},
    routing::{get, patch},
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::{
    domains,
    input::{normalize_host, valid_slug, valid_target},
    qr::{self, Format, QrError},
    redirect::{self, Policy, PolicyDetails, PolicyError},
    store::{self, Link, LinkDraft, LinkUpdate, PAGE_SIZE},
};

use super::{AppState, Problem, access::LinkAccess, catalog::tag_name, json_payload, resource_id};

const SHORTCODE_ALPHABET: &[u8; 64] =
    b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz-_";
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
        .route(
            "/api/v1/links/{id}/policy",
            get(read_policy).patch(replace_policy),
        )
        .route("/api/v1/links/{id}/qr", get(link_qr))
        .with_state(LinkState { pool, app })
}

impl LinkAccess {
    async fn allowed_hosts(&self, state: &LinkState, active: bool) -> Result<Vec<String>, Problem> {
        let hosts = if active {
            domains::active_hosts(&state.pool, &state.app.hosts).await
        } else {
            domains::list(&state.pool, &state.app.hosts, state.app.config.local_port)
                .await
                .map(|items| items.into_iter().map(|domain| domain.host).collect())
        }
        .map_err(|_| Problem::unavailable())?;
        Ok(hosts
            .into_iter()
            .filter(|host| self.permits(host))
            .collect())
    }

    fn permits(&self, host: &str) -> bool {
        self.host.as_deref().is_none_or(|scope| scope == host)
    }
}

#[derive(Serialize)]
struct Domains {
    hosts: Vec<String>,
    origins: BTreeMap<String, String>,
    local_port: u16,
}

async fn domains(
    State(state): State<LinkState>,
    Extension(access): Extension<LinkAccess>,
) -> Result<Json<Domains>, Problem> {
    let items = domains::list(&state.pool, &state.app.hosts, state.app.config.local_port)
        .await
        .map_err(|_| Problem::unavailable())?;
    let mut response = Domains {
        hosts: Vec::new(),
        origins: BTreeMap::new(),
        local_port: state.app.config.local_port,
    };
    for domain in items.into_iter().filter(|d| access.permits(&d.host)) {
        if domain.enabled {
            response.hosts.push(domain.host.clone());
        }
        response.origins.insert(domain.host, domain.origin);
    }
    Ok(Json(response))
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
    let hosts = access.allowed_hosts(&state, true).await?;
    let host = match input.host {
        Some(host) => normalize_host(&host).ok_or_else(|| Problem::invalid("invalid_host"))?,
        None => hosts.first().ok_or_else(forbidden)?.to_owned(),
    };
    if !access.permits(&host) {
        return Err(forbidden());
    }
    if !hosts.contains(&host) {
        return Err(Problem::invalid("invalid_host"));
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
            .await
            .map_err(|_| Problem::unavailable())?
            .ok_or(Problem(StatusCode::CONFLICT, "slug_in_use"))?;
        return Ok((StatusCode::CREATED, Json(link)));
    }
    for _ in 0..MAX_CODE_ATTEMPTS {
        draft.slug = random_slug()?;
        if let Some(link) = store::insert_link(&state.pool, &draft)
            .await
            .map_err(|_| Problem::unavailable())?
        {
            return Ok((StatusCode::CREATED, Json(link)));
        }
    }
    Err(Problem::unavailable())
}

fn forbidden() -> Problem {
    Problem(StatusCode::FORBIDDEN, "insufficient_scope")
}

fn title(value: String) -> Result<String, Problem> {
    let value = value.trim();
    if value.chars().count() > 160 || value.chars().any(char::is_control) {
        return Err(Problem::invalid("invalid_title"));
    }
    Ok(value.to_owned())
}

fn tags(values: Vec<String>) -> Result<Vec<String>, Problem> {
    if values.len() > 20 {
        return Err(Problem::invalid("invalid_tags"));
    }
    let mut checked = Vec::with_capacity(values.len());
    for tag in values {
        let tag = tag_name(&tag).ok_or_else(|| Problem::invalid("invalid_tags"))?;
        checked.push(tag.to_owned());
    }
    checked.sort_unstable();
    checked.dedup();
    Ok(checked)
}

fn random_slug() -> Result<String, Problem> {
    let mut bytes = [0; SHORTCODE_LEN];
    getrandom::fill(&mut bytes).map_err(|_| Problem::unavailable())?;
    Ok(bytes
        .iter()
        .map(|byte| char::from(SHORTCODE_ALPHABET[usize::from(*byte & 63)]))
        .collect())
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
    let mut items = store::list(
        &state.pool,
        &access.allowed_hosts(&state, false).await?,
        after_id,
    )
    .await
    .map_err(|_| Problem::unavailable())?;
    let has_more = items.len() > PAGE_SIZE;
    items.truncate(PAGE_SIZE);
    let next_after_id = if has_more {
        items.last().map(|link| link.id)
    } else {
        None
    };
    Ok(Json(LinkPage {
        items,
        next_after_id,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PatchLink {
    target: Option<String>,
    title: Option<String>,
    enabled: Option<bool>,
    analytics_enabled: Option<bool>,
    tags: Option<Vec<String>>,
}

async fn update_link(
    State(state): State<LinkState>,
    Extension(access): Extension<LinkAccess>,
    id: Result<Path<i64>, PathRejection>,
    body: Result<Json<PatchLink>, JsonRejection>,
) -> Result<Json<Link>, Problem> {
    let input = json_payload(body)?;
    if input.target.is_none()
        && input.title.is_none()
        && input.enabled.is_none()
        && input.analytics_enabled.is_none()
        && input.tags.is_none()
    {
        return Err(Problem::invalid("empty_update"));
    }
    if input
        .target
        .as_deref()
        .is_some_and(|target| !valid_target(target))
    {
        return Err(Problem::invalid("invalid_target"));
    }
    let update = LinkUpdate {
        target: input.target,
        title: input.title.map(title).transpose()?,
        enabled: input.enabled,
        analytics_enabled: input.analytics_enabled,
        tags: input.tags.map(tags).transpose()?,
    };
    let id = resource_id(id)?;
    store::update_link(
        &state.pool,
        &access.allowed_hosts(&state, false).await?,
        id,
        update,
    )
    .await
    .map_err(|_| Problem::unavailable())?
    .map(Json)
    .ok_or(Problem(StatusCode::NOT_FOUND, "not_found"))
}

async fn remove_link(
    State(state): State<LinkState>,
    Extension(access): Extension<LinkAccess>,
    id: Result<Path<i64>, PathRejection>,
) -> Result<StatusCode, Problem> {
    let removed = store::remove_link(
        &state.pool,
        &access.allowed_hosts(&state, false).await?,
        resource_id(id)?,
    )
    .await
    .map_err(|_| Problem::unavailable())?;
    if !removed {
        return Err(Problem(StatusCode::NOT_FOUND, "not_found"));
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn read_policy(
    State(state): State<LinkState>,
    Extension(access): Extension<LinkAccess>,
    id: Result<Path<i64>, PathRejection>,
) -> Result<Json<PolicyDetails>, Problem> {
    let hosts = access.allowed_hosts(&state, false).await?;
    redirect::read_policy(&state.pool, &hosts, resource_id(id)?)
        .await
        .map_err(|_| Problem::unavailable())?
        .map(Json)
        .ok_or(Problem(StatusCode::NOT_FOUND, "not_found"))
}

async fn replace_policy(
    State(state): State<LinkState>,
    Extension(access): Extension<LinkAccess>,
    id: Result<Path<i64>, PathRejection>,
    body: Result<Json<Policy>, JsonRejection>,
) -> Result<Json<PolicyDetails>, Problem> {
    let policy = json_payload(body)?;
    let hosts = access.allowed_hosts(&state, false).await?;
    redirect::replace_policy(&state.pool, &hosts, resource_id(id)?, policy)
        .await
        .map_err(|error| match error {
            PolicyError::Invalid(code) => Problem::invalid(code),
            PolicyError::Database(error) => {
                tracing::debug!(
                    code = ?error.as_database_error().and_then(|error| error.code()),
                    "redirect policy write failed"
                );
                Problem::unavailable()
            }
        })?
        .map(Json)
        .ok_or(Problem(StatusCode::NOT_FOUND, "not_found"))
}

static QR_WORK: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QrQuery {
    format: Option<String>,
    size: Option<u16>,
}

async fn link_qr(
    State(state): State<LinkState>,
    Extension(access): Extension<LinkAccess>,
    id: Result<Path<i64>, PathRejection>,
    query: Result<Query<QrQuery>, QueryRejection>,
) -> Result<([(header::HeaderName, &'static str); 1], Bytes), Problem> {
    let Query(options) = query.map_err(|error| Problem(error.status(), "invalid_query"))?;
    let (format, content_type) = match options.format.as_deref().unwrap_or("svg") {
        "svg" => (Format::Svg, "image/svg+xml"),
        "png" => (Format::Png, "image/png"),
        _ => return Err(Problem::invalid("invalid_qr_format")),
    };
    let (host, slug) =
        sqlx::query_as::<_, (String, String)>("SELECT host, slug FROM links WHERE id = ?")
            .bind(resource_id(id)?)
            .fetch_optional(&state.pool)
            .await
            .map_err(|_| Problem::unavailable())?
            .filter(|(host, _)| access.permits(host))
            .ok_or(Problem(StatusCode::NOT_FOUND, "not_found"))?;
    let domain = domains::lookup(
        &state.pool,
        &state.app.hosts,
        &host,
        state.app.config.local_port,
    )
    .await
    .map_err(|_| Problem::unavailable())?
    .ok_or(Problem(StatusCode::NOT_FOUND, "not_found"))?;
    let value = format!("{}/{}", domain.origin, slug);
    let permit = QR_WORK
        .try_acquire()
        .map_err(|_| Problem(StatusCode::TOO_MANY_REQUESTS, "try_again_later"))?;
    let bytes = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        qr::generate(&value, format, options.size)
    })
    .await
    .map_err(|_| Problem::unavailable())?
    .map_err(|error| match error {
        QrError::InvalidSize => Problem::invalid("invalid_qr_size"),
        QrError::DataTooLong => Problem::invalid("qr_data_too_long"),
        QrError::EncodingFailed => Problem::unavailable(),
    })?;
    Ok(([(header::CONTENT_TYPE, content_type)], Bytes::from(bytes)))
}
