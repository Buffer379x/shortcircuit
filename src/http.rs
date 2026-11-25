use axum::{
    Router,
    extract::{Request, State},
    http::{HeaderValue, Method, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use sqlx::SqlitePool;

use crate::{
    analytics::{Analytics, Kind},
    domains,
    input::{PublicHosts, request_host, valid_slug},
    redirect::{self, Outcome, RedirectRequest},
};

const MAX_REQUEST_URI: usize = 8192;

#[derive(Clone)]
struct AppState {
    pool: SqlitePool,
    hosts: PublicHosts,
    analytics: Option<Analytics>,
}

pub fn public_router(pool: SqlitePool, hosts: PublicHosts) -> Router {
    build_public_router(pool, hosts, None)
}

pub fn public_router_with_analytics(
    pool: SqlitePool,
    hosts: PublicHosts,
    analytics: Analytics,
) -> Router {
    build_public_router(pool, hosts, Some(analytics))
}

fn build_public_router(
    pool: SqlitePool,
    hosts: PublicHosts,
    analytics: Option<Analytics>,
) -> Router {
    Router::new()
        .fallback(resolve)
        .with_state(AppState {
            pool,
            hosts,
            analytics,
        })
        .layer(middleware::from_fn(response_headers))
}

pub(crate) async fn response_headers(request: Request, next: Next) -> Response {
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
    let (parts, _) = request.into_parts();
    if !matches!(parts.method, Method::GET | Method::HEAD) {
        return (
            StatusCode::METHOD_NOT_ALLOWED,
            [(header::ALLOW, "GET, HEAD")],
        )
            .into_response();
    }
    let uri = &parts.uri;
    if uri
        .path_and_query()
        .is_some_and(|value| value.as_str().len() > MAX_REQUEST_URI)
    {
        return StatusCode::URI_TOO_LONG.into_response();
    }
    let Some(host) = request_host(uri, &parts.headers) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let domain = match domains::lookup(&state.pool, &state.hosts, &host, 8080).await {
        Ok(Some(domain)) if domain.enabled => domain,
        Ok(_) => return StatusCode::NOT_FOUND.into_response(),
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    let Some(slug) = uri.path().strip_prefix('/') else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if slug.is_empty() {
        return fallback(&host, "/", domain.root_target.as_deref());
    }
    if !valid_slug(slug) {
        if parts.method == Method::GET
            && let Some(analytics) = &state.analytics
        {
            analytics.record(&host, None, Kind::Missing, &parts.headers);
        }
        return StatusCode::NOT_FOUND.into_response();
    }
    let value = |name| {
        parts
            .headers
            .get(name)
            .and_then(|value| value.to_str().ok())
    };
    let lookup = RedirectRequest {
        host: &host,
        slug,
        query: uri.query(),
        user_agent: value(header::USER_AGENT),
        language: value(header::ACCEPT_LANGUAGE),
        head: parts.method == Method::HEAD,
    };
    let Ok(result) = redirect::resolve_with_subject(&state.pool, lookup).await else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let (kind, response) = match result.outcome {
        Outcome::Redirect { target, status } => (Kind::Redirect, respond(&target, status)),
        Outcome::Missing => (
            Kind::Missing,
            fallback(&host, uri.path(), domain.missing_target.as_deref()),
        ),
        Outcome::Blocked => (Kind::Blocked, StatusCode::NOT_FOUND.into_response()),
    };
    if !lookup.head
        && !response.status().is_server_error()
        && result
            .subject
            .as_ref()
            .is_none_or(|subject| subject.analytics_enabled)
        && let Some(analytics) = &state.analytics
    {
        analytics.record(
            &host,
            result.subject.map(|subject| subject.id),
            kind,
            &parts.headers,
        );
    }
    response
}

fn fallback(host: &str, path: &str, target: Option<&str>) -> Response {
    match target {
        Some(target) if !domains::fallback_loops(host, path, target) => respond(target, 302),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

fn respond(target: &str, status: u16) -> Response {
    match (StatusCode::from_u16(status), HeaderValue::from_str(target)) {
        (Ok(status), Ok(location)) => (status, [(header::LOCATION, location)]).into_response(),
        _ => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}
