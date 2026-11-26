use std::{collections::BTreeMap, time::Duration};

use axum::{
    Extension, Json, Router,
    body::Bytes,
    extract::{
        DefaultBodyLimit, FromRequest, Path, Query, Request, State,
        rejection::{JsonRejection, PathRejection, QueryRejection},
    },
    http::{StatusCode, header},
    routing::{get, patch, post},
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::{
    auth::{Auth, AuthError},
    domains,
    history::{self, LinkHistory, RestoreError},
    input::{link_description, link_tags, link_title, normalize_host, valid_slug, valid_target},
    protection::{self, PasswordChange, ProtectionDetails, ProtectionDraft, ProtectionError},
    qr::{self, Format},
    redirect::{self, Policy, PolicyDetails, PolicyError},
    store::{self, Link, LinkDraft, LinkUpdate, PAGE_SIZE},
};

use super::{
    AppState, Problem,
    access::{LinkAccess, MAX_SHORTCODE_LENGTH},
    json_payload, resource_id,
};

const SHORTCODE_ALPHABET: &[u8; 64] =
    b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz-_";
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
        .route(
            "/api/v1/links/{id}/protection",
            get(read_protection).patch(replace_protection),
        )
        .route(
            "/api/v1/links/{id}/history",
            get(read_history).delete(purge_history),
        )
        .route(
            "/api/v1/links/{id}/history/{revision}/restore",
            post(restore_history),
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
    description: Option<String>,
    enabled: Option<bool>,
    analytics_enabled: Option<bool>,
    tags: Option<Vec<String>>,
    protection: Option<NewProtection>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewProtection {
    one_time: bool,
    password: Option<String>,
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
    if input.slug.as_deref().is_some_and(|slug| !valid_slug(slug)) {
        return Err(Problem::invalid("invalid_slug"));
    }
    let mut draft = LinkDraft {
        host,
        slug: String::new(),
        title: title(input.title.unwrap_or_default())?,
        description: description(input.description.unwrap_or_default())?,
        enabled: input.enabled.unwrap_or(true),
        analytics_enabled: input.analytics_enabled.unwrap_or(true),
        tags: tags(input.tags.unwrap_or_default())?,
        target: input.target,
    };
    let protection = match input.protection {
        Some(options) => Some(ProtectionDraft {
            one_time: options.one_time,
            password_hash: match options.password {
                Some(password) => Some(link_password_hash(&state.app.auth, &password).await?),
                None => None,
            },
        }),
        None => None,
    };
    if let Some(slug) = input.slug {
        draft.slug = slug;
        let link = store::insert_link(&state.pool, &draft, protection.as_ref())
            .await
            .map_err(|_| Problem::unavailable())?
            .ok_or(Problem(StatusCode::CONFLICT, "slug_in_use"))?;
        return Ok((StatusCode::CREATED, Json(link)));
    }
    for _ in 0..MAX_CODE_ATTEMPTS {
        draft.slug = random_slug(state.app.config.shortcode_length)?;
        if let Some(link) = store::insert_link(&state.pool, &draft, protection.as_ref())
            .await
            .map_err(|_| Problem::unavailable())?
        {
            return Ok((StatusCode::CREATED, Json(link)));
        }
    }
    Err(Problem::unavailable())
}

async fn link_password_hash(auth: &Auth, password: &str) -> Result<String, Problem> {
    auth.hash_password(password)
        .await
        .map_err(|error| match error {
            AuthError::Invalid => Problem::invalid("invalid_link_password"),
            other => other.into(),
        })
}

fn forbidden() -> Problem {
    Problem(StatusCode::FORBIDDEN, "insufficient_scope")
}

fn title(value: String) -> Result<String, Problem> {
    link_title(&value)
        .map(str::to_owned)
        .ok_or_else(|| Problem::invalid("invalid_title"))
}

fn description(value: String) -> Result<String, Problem> {
    link_description(&value)
        .map(str::to_owned)
        .ok_or_else(|| Problem::invalid("invalid_description"))
}

fn tags(values: Vec<String>) -> Result<Vec<String>, Problem> {
    link_tags(values).ok_or_else(|| Problem::invalid("invalid_tags"))
}

fn random_slug(length: usize) -> Result<String, Problem> {
    let mut bytes = [0; MAX_SHORTCODE_LENGTH];
    let bytes = &mut bytes[..length];
    getrandom::fill(bytes).map_err(|_| Problem::unavailable())?;
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
    description: Option<String>,
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
        && input.description.is_none()
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
        description: input.description.map(description).transpose()?,
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
        &access.actor,
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
    redirect::replace_policy(&state.pool, &hosts, resource_id(id)?, policy, &access.actor)
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

async fn read_protection(
    State(state): State<LinkState>,
    Extension(access): Extension<LinkAccess>,
    id: Result<Path<i64>, PathRejection>,
) -> Result<Json<ProtectionDetails>, Problem> {
    let id = resource_id(id)?;
    let hosts = access.allowed_hosts(&state, false).await?;
    protection::read(&state.pool, &hosts, id)
        .await
        .map_err(|_| Problem::unavailable())?
        .map(Json)
        .ok_or(Problem(StatusCode::NOT_FOUND, "not_found"))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PatchProtection {
    one_time: bool,
    password: Option<String>,
    #[serde(default)]
    remove_password: bool,
}

async fn replace_protection(
    State(state): State<LinkState>,
    Extension(access): Extension<LinkAccess>,
    id: Result<Path<i64>, PathRejection>,
    body: Result<Json<PatchProtection>, JsonRejection>,
) -> Result<Json<ProtectionDetails>, Problem> {
    let id = resource_id(id)?;
    let input = json_payload(body)?;
    let hosts = access.allowed_hosts(&state, false).await?;
    let password = match (input.password, input.remove_password) {
        (Some(_), true) => return Err(Problem::invalid("invalid_link_protection")),
        (Some(password), false) => {
            if protection::read(&state.pool, &hosts, id)
                .await
                .map_err(|_| Problem::unavailable())?
                .is_none()
            {
                return Err(Problem(StatusCode::NOT_FOUND, "not_found"));
            }
            PasswordChange::Set(link_password_hash(&state.app.auth, &password).await?)
        }
        (None, true) => PasswordChange::Remove,
        (None, false) => PasswordChange::Unchanged,
    };
    protection::replace(
        &state.pool,
        &hosts,
        id,
        input.one_time,
        password,
        &access.actor,
    )
    .await
    .map_err(|error| match error {
        ProtectionError::Database => Problem::unavailable(),
        ProtectionError::RequiresTemporaryRedirect => Problem(
            StatusCode::CONFLICT,
            "protection_requires_temporary_redirect",
        ),
        ProtectionError::Consumed => Problem(StatusCode::CONFLICT, "one_time_consumed"),
    })?
    .map(Json)
    .ok_or(Problem(StatusCode::NOT_FOUND, "not_found"))
}

async fn read_history(
    State(state): State<LinkState>,
    Extension(access): Extension<LinkAccess>,
    id: Result<Path<i64>, PathRejection>,
) -> Result<Json<LinkHistory>, Problem> {
    if !access.actor.is_admin() {
        return Err(forbidden());
    }
    let id = resource_id(id)?;
    let hosts = access.allowed_hosts(&state, false).await?;
    history::read(&state.pool, &hosts, id)
        .await
        .map_err(|_| Problem::unavailable())?
        .map(Json)
        .ok_or(Problem(StatusCode::NOT_FOUND, "not_found"))
}

async fn purge_history(
    State(state): State<LinkState>,
    Extension(access): Extension<LinkAccess>,
    id: Result<Path<i64>, PathRejection>,
) -> Result<StatusCode, Problem> {
    if !access.actor.is_admin() {
        return Err(forbidden());
    }
    let id = resource_id(id)?;
    let hosts = access.allowed_hosts(&state, false).await?;
    if !history::purge(&state.pool, &hosts, id)
        .await
        .map_err(|_| Problem::unavailable())?
    {
        return Err(Problem(StatusCode::NOT_FOUND, "not_found"));
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn restore_history(
    State(state): State<LinkState>,
    Extension(access): Extension<LinkAccess>,
    path: Result<Path<(i64, i64)>, PathRejection>,
) -> Result<StatusCode, Problem> {
    if !access.actor.is_admin() {
        return Err(forbidden());
    }
    let Path((id, revision)) = path.map_err(|_| Problem(StatusCode::NOT_FOUND, "not_found"))?;
    if id <= 0 || revision <= 0 {
        return Err(Problem(StatusCode::NOT_FOUND, "not_found"));
    }
    let hosts = access.allowed_hosts(&state, false).await?;
    history::restore(&state.pool, &hosts, id, revision, &access.actor)
        .await
        .map_err(|error| match error {
            RestoreError::NotFound => Problem(StatusCode::NOT_FOUND, "not_found"),
            RestoreError::NotRestorable => Problem(StatusCode::CONFLICT, "history_not_restorable"),
            RestoreError::Invalid(code) => Problem::invalid(code),
            RestoreError::Database => Problem::unavailable(),
        })?;
    Ok(StatusCode::NO_CONTENT)
}

static QR_WORK: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);
static QR_LOGO_UPLOADS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);
const QR_LOGO_TIMEOUT: Duration = Duration::from_secs(8);
type QrResponse = ([(header::HeaderName, &'static str); 1], Bytes);

pub(super) fn qr_logo_routes() -> Router<AppState> {
    Router::new().route(
        "/api/v1/links/{id}/qr/logo",
        post(link_qr_logo).layer(DefaultBodyLimit::max(qr::MAX_LOGO_BYTES)),
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QrQuery {
    format: Option<String>,
    size: Option<u16>,
    foreground: Option<String>,
    background: Option<String>,
    transparent: Option<bool>,
    correction: Option<String>,
    quiet_zone: Option<i64>,
    frame: Option<String>,
}

async fn link_qr(
    State(state): State<LinkState>,
    Extension(access): Extension<LinkAccess>,
    id: Result<Path<i64>, PathRejection>,
    query: Result<Query<QrQuery>, QueryRejection>,
) -> Result<QrResponse, Problem> {
    render_qr(&state.app, Some(&access), id, query, None).await
}

async fn link_qr_logo(
    State(state): State<AppState>,
    id: Result<Path<i64>, PathRejection>,
    query: Result<Query<QrQuery>, QueryRejection>,
    request: Request,
) -> Result<QrResponse, Problem> {
    let content_types = request.headers().get_all(header::CONTENT_TYPE);
    let mut content_types = content_types.iter();
    if content_types
        .next()
        .is_none_or(|value| value != "image/png")
        || content_types.next().is_some()
    {
        return Err(Problem(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "invalid_qr_logo_type",
        ));
    }
    let _slot = QR_LOGO_UPLOADS.try_acquire().map_err(|_| qr_busy())?;
    let image = tokio::time::timeout(QR_LOGO_TIMEOUT, Bytes::from_request(request, &state))
        .await
        .map_err(|_| Problem(StatusCode::REQUEST_TIMEOUT, "qr_logo_timeout"))?
        .map_err(|error| Problem(error.status(), "invalid_qr_logo_body"))?;
    render_qr(&state, None, id, query, Some(image)).await
}

fn qr_busy() -> Problem {
    Problem(StatusCode::TOO_MANY_REQUESTS, "try_again_later")
}

async fn render_qr(
    state: &AppState,
    access: Option<&LinkAccess>,
    id: Result<Path<i64>, PathRejection>,
    query: Result<Query<QrQuery>, QueryRejection>,
    logo: Option<Bytes>,
) -> Result<QrResponse, Problem> {
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
            .filter(|(host, _)| access.is_none_or(|access| access.permits(host)))
            .ok_or(Problem(StatusCode::NOT_FOUND, "not_found"))?;
    let domain = domains::lookup(&state.pool, &state.hosts, &host, state.config.local_port)
        .await
        .map_err(|_| Problem::unavailable())?
        .ok_or(Problem(StatusCode::NOT_FOUND, "not_found"))?;
    let value = format!("{}/{}", domain.origin, slug);
    let render_options = qr::Options {
        size: options.size,
        foreground: options.foreground,
        background: options.background,
        transparent: options.transparent,
        correction: options.correction,
        quiet_zone: options.quiet_zone,
        frame: options.frame,
    };
    let permit = QR_WORK.try_acquire().map_err(|_| qr_busy())?;
    let bytes = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        match logo {
            Some(image) => qr::generate_with_logo(&value, format, &render_options, &image),
            None => qr::generate(&value, format, &render_options),
        }
    })
    .await
    .map_err(|_| Problem::unavailable())??;
    Ok(([(header::CONTENT_TYPE, content_type)], Bytes::from(bytes)))
}
