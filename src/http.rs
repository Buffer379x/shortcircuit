use axum::{
    Json, Router,
    extract::{
        DefaultBodyLimit, Query, Request, State,
        rejection::{JsonRejection, QueryRejection},
    },
    http::{HeaderValue, Method, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use subtle::ConstantTimeEq;

use crate::{
    input::{PublicHosts, normalize_host, request_host, valid_slug, valid_target},
    store::{self, Link, PAGE_SIZE},
};

const SHORTCODE_LEN: usize = 10;
const MAX_CODE_ATTEMPTS: usize = 8;
const MAX_REQUEST_URI: usize = 8192;
const MAX_CREATE_BODY: usize = 32_768;
const SHORTCODE_ALPHABET: &[u8; 64] =
    b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz-_";

#[derive(Clone)]
pub struct AdminKey([u8; 32]);

impl AdminKey {
    pub fn parse(value: &str) -> Result<Self, &'static str> {
        if !is_key_format(value) {
            return Err("admin token must contain exactly 64 hexadecimal characters");
        }
        Ok(Self(Sha256::digest(value.as_bytes()).into()))
    }

    fn matches(&self, value: &str) -> bool {
        is_key_format(value)
            && bool::from(
                self.0
                    .as_slice()
                    .ct_eq(Sha256::digest(value.as_bytes()).as_slice()),
            )
    }
}

fn is_key_format(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[derive(Clone)]
struct AppState {
    pool: SqlitePool,
    hosts: PublicHosts,
}

pub fn public_router(pool: SqlitePool, hosts: PublicHosts) -> Router {
    Router::new()
        .fallback(resolve)
        .with_state(AppState { pool, hosts })
        .layer(middleware::from_fn(response_headers))
}

pub fn admin_router(pool: SqlitePool, hosts: PublicHosts, key: Option<AdminKey>) -> Router {
    let mut router = Router::new()
        .route("/healthz", get(liveness))
        .route("/readyz", get(readiness));
    if let Some(key) = key {
        router = router.merge(
            Router::new()
                .route("/api/v1/links", get(list_links).post(create_link))
                .layer(DefaultBodyLimit::max(MAX_CREATE_BODY))
                .route_layer(middleware::from_fn_with_state(key, authorize)),
        );
    }
    router
        .fallback(|| async { StatusCode::NOT_FOUND })
        .with_state(AppState { pool, hosts })
        .layer(middleware::from_fn(response_headers))
}

async fn response_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
}

async fn resolve(State(state): State<AppState>, request: Request) -> Response {
    if !matches!(*request.method(), Method::GET | Method::HEAD) {
        return (
            StatusCode::METHOD_NOT_ALLOWED,
            [(header::ALLOW, "GET, HEAD")],
        )
            .into_response();
    }
    let uri = request.uri();
    if uri
        .path_and_query()
        .is_some_and(|value| value.as_str().len() > MAX_REQUEST_URI)
    {
        return StatusCode::URI_TOO_LONG.into_response();
    }
    let Some(host) = request_host(uri, request.headers()).filter(|host| state.hosts.contains(host))
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(slug) = uri.path().strip_prefix('/').filter(|slug| valid_slug(slug)) else {
        return StatusCode::NOT_FOUND.into_response();
    };

    match store::resolve(&state.pool, &host, slug).await {
        Ok(Some(target)) => match HeaderValue::from_str(&target) {
            Ok(location) => (StatusCode::FOUND, [(header::LOCATION, location)]).into_response(),
            Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
        },
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}

async fn liveness() -> StatusCode {
    StatusCode::OK
}

async fn readiness(State(state): State<AppState>) -> StatusCode {
    match sqlx::query_scalar::<_, i64>("SELECT 1 FROM links LIMIT 1")
        .fetch_optional(&state.pool)
        .await
    {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::SERVICE_UNAVAILABLE,
    }
}

async fn authorize(State(key): State<AdminKey>, request: Request, next: Next) -> Response {
    let mut values = request.headers().get_all(header::AUTHORIZATION).iter();
    let valid = values
        .next()
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split_once(' '))
        .is_some_and(|(scheme, value)| scheme.eq_ignore_ascii_case("bearer") && key.matches(value));
    if valid && values.next().is_none() {
        next.run(request).await
    } else {
        ApiError(StatusCode::UNAUTHORIZED, "authentication_required").into_response()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewLink {
    host: Option<String>,
    slug: Option<String>,
    target: String,
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

#[derive(Serialize)]
struct Problem {
    status: u16,
    title: &'static str,
    code: &'static str,
}

struct ApiError(StatusCode, &'static str);

impl ApiError {
    fn invalid(code: &'static str) -> Self {
        Self(StatusCode::UNPROCESSABLE_ENTITY, code)
    }

    fn unavailable() -> Self {
        Self(StatusCode::SERVICE_UNAVAILABLE, "temporarily_unavailable")
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut response = (
            self.0,
            Json(Problem {
                status: self.0.as_u16(),
                title: self.0.canonical_reason().unwrap_or("Request failed"),
                code: self.1,
            }),
        )
            .into_response();
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/problem+json"),
        );
        if self.0 == StatusCode::UNAUTHORIZED {
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        }
        response
    }
}

async fn create_link(
    State(state): State<AppState>,
    input: Result<Json<NewLink>, JsonRejection>,
) -> Result<(StatusCode, Json<Link>), ApiError> {
    let Json(input) = input.map_err(|error| ApiError(error.status(), "invalid_request"))?;
    let host = match input.host {
        Some(host) => normalize_host(&host)
            .filter(|host| state.hosts.contains(host))
            .ok_or_else(|| ApiError::invalid("invalid_host"))?,
        None => state.hosts.primary().to_owned(),
    };
    if !valid_target(&input.target) {
        return Err(ApiError::invalid("invalid_target"));
    }
    if let Some(slug) = input.slug {
        if !valid_slug(&slug) {
            return Err(ApiError::invalid("invalid_slug"));
        }
        let link = store::insert(&state.pool, &host, &slug, &input.target)
            .await
            .map_err(|_| ApiError::unavailable())?
            .ok_or(ApiError(StatusCode::CONFLICT, "slug_in_use"))?;
        return Ok((StatusCode::CREATED, Json(link)));
    }

    for _ in 0..MAX_CODE_ATTEMPTS {
        let slug = random_slug()?;
        if let Some(link) = store::insert(&state.pool, &host, &slug, &input.target)
            .await
            .map_err(|_| ApiError::unavailable())?
        {
            return Ok((StatusCode::CREATED, Json(link)));
        }
    }
    Err(ApiError::unavailable())
}

fn random_slug() -> Result<String, ApiError> {
    let mut bytes = [0; SHORTCODE_LEN];
    getrandom::fill(&mut bytes).map_err(|_| ApiError::unavailable())?;
    Ok(bytes
        .iter()
        .map(|byte| char::from(SHORTCODE_ALPHABET[usize::from(*byte & 63)]))
        .collect())
}

async fn list_links(
    State(state): State<AppState>,
    query: Result<Query<PageQuery>, QueryRejection>,
) -> Result<Json<LinkPage>, ApiError> {
    let Query(query) = query.map_err(|error| ApiError(error.status(), "invalid_query"))?;
    let after_id = query.after_id.unwrap_or(0);
    if after_id < 0 {
        return Err(ApiError::invalid("invalid_after_id"));
    }
    let mut items = store::list(&state.pool, state.hosts.all(), after_id)
        .await
        .map_err(|_| ApiError::unavailable())?;
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
