use std::{net::IpAddr, sync::Arc};

use axum::{
    Router,
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use sqlx::SqlitePool;

use crate::{
    analytics::{Analytics, Kind},
    auth::{Auth, AuthError},
    confirmation::Action,
    domains,
    input::{PublicHosts, request_host, valid_slug},
    rate_limit::{AttemptLimiter, peer},
    redirect::{self, Outcome, RedirectRequest, Resolution},
};

const MAX_REQUEST_URI: usize = 8192;

#[derive(Clone)]
struct AppState {
    pool: SqlitePool,
    hosts: PublicHosts,
    public_port: u16,
    analytics: Option<Analytics>,
    auth: Auth,
    attempts: Arc<AttemptLimiter>,
}

pub fn public_router(pool: SqlitePool, hosts: PublicHosts) -> Router {
    public_router_on_port(pool, hosts, 8080, None)
}

pub fn public_router_with_analytics(
    pool: SqlitePool,
    hosts: PublicHosts,
    analytics: Analytics,
) -> Router {
    public_router_on_port(pool, hosts, 8080, Some(analytics))
}

pub fn public_router_on_port(
    pool: SqlitePool,
    hosts: PublicHosts,
    public_port: u16,
    analytics: Option<Analytics>,
) -> Router {
    Router::new()
        .fallback(resolve)
        .with_state(AppState {
            auth: Auth::new(pool.clone()),
            pool,
            hosts,
            public_port,
            analytics,
            attempts: Arc::new(AttemptLimiter::new()),
        })
        .layer(middleware::from_fn(response_headers))
}

pub(crate) async fn response_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers
        .entry(header::REFERRER_POLICY)
        .or_insert(HeaderValue::from_static("no-referrer"));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
}

async fn resolve(State(state): State<AppState>, request: Request) -> Response {
    let post = request.method() == Method::POST;
    if !matches!(*request.method(), Method::GET | Method::HEAD | Method::POST) {
        return method_not_allowed();
    }
    let peer = post.then(|| peer(&request));
    let (parts, body) = request.into_parts();
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
    let domain = match domains::lookup(&state.pool, &state.hosts, &host, state.public_port).await {
        Ok(Some(domain)) if domain.enabled => domain,
        Ok(_) => return StatusCode::NOT_FOUND.into_response(),
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    if uri.path() == "/robots.txt" {
        let body = if domain.block_crawlers {
            "User-agent: *\nDisallow: /\n"
        } else {
            "User-agent: *\nDisallow:\n"
        };
        return match parts.method {
            Method::GET => {
                ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], body).into_response()
            }
            Method::HEAD => (
                [
                    (
                        header::CONTENT_TYPE,
                        HeaderValue::from_static("text/plain; charset=utf-8"),
                    ),
                    (header::CONTENT_LENGTH, HeaderValue::from(body.len())),
                ],
                "",
            )
                .into_response(),
            _ => method_not_allowed(),
        };
    }
    let Some(slug) = uri.path().strip_prefix('/') else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if slug.is_empty() {
        if post {
            return method_not_allowed();
        }
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
        // Inspect the link without reserving any visits before checking the submitted proof.
        head: parts.method != Method::GET,
    };
    let Ok(mut result) = redirect::resolve_with_subject(&state.pool, lookup).await else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    if let Outcome::Challenge(link) = result.outcome {
        let loopback = host == "localhost"
            || host
                .trim_matches(['[', ']'])
                .parse::<IpAddr>()
                .is_ok_and(|ip| ip.is_loopback());
        if !domain.origin.starts_with("https://") && !loopback {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
        let action = Action {
            link,
            origin: &domain.origin,
            path_and_query: uri
                .path_and_query()
                .map_or(uri.path(), |path| path.as_str()),
        };
        if let Some(peer) = peer {
            result = match confirm(&state, action, lookup, &parts.headers, body, peer).await {
                Ok(confirmed) => confirmed,
                Err(StatusCode::UNAUTHORIZED) => return action.page(lookup.language, true),
                Err(status) => return status.into_response(),
            };
        } else if parts.method == Method::HEAD {
            return StatusCode::OK.into_response();
        } else {
            return action.page(lookup.language, false);
        }
    } else if post {
        return match result.outcome {
            Outcome::Blocked => StatusCode::NOT_FOUND.into_response(),
            _ => method_not_allowed(),
        };
    }
    let (kind, mut response) = match result.outcome {
        Outcome::Redirect { target, status } => (Kind::Redirect, respond(&target, status)),
        Outcome::Missing => (
            Kind::Missing,
            fallback(&host, uri.path(), domain.missing_target.as_deref()),
        ),
        Outcome::Blocked => (Kind::Blocked, StatusCode::NOT_FOUND.into_response()),
        Outcome::Challenge(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    if response.status().is_redirection()
        && result
            .subject
            .as_ref()
            .is_some_and(|subject| subject.noindex)
    {
        response.headers_mut().insert(
            "x-robots-tag",
            HeaderValue::from_static("noindex, nofollow"),
        );
    }
    if parts.method != Method::HEAD
        && (!post || response.status() == StatusCode::SEE_OTHER)
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

async fn confirm(
    state: &AppState,
    action: Action<'_>,
    lookup: RedirectRequest<'_>,
    headers: &HeaderMap,
    body: Body,
    peer: IpAddr,
) -> Result<Resolution, StatusCode> {
    let password = action.submit(headers, body).await?;
    if action.link.password_required {
        state.attempts.allow(peer)?;
        let hash = sqlx::query_scalar::<_, String>(
            "SELECT password_hash FROM link_protection
             WHERE link_id = ? AND revision = ? AND password_hash IS NOT NULL",
        )
        .bind(action.link.id)
        .bind(action.link.revision)
        .fetch_optional(&state.pool)
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
        .ok_or(StatusCode::FORBIDDEN)?;
        match state.auth.verify_password(&password, &hash, false).await {
            Ok(_) => {}
            Err(AuthError::Unauthorized) => return Err(StatusCode::UNAUTHORIZED),
            Err(AuthError::Busy) => return Err(StatusCode::TOO_MANY_REQUESTS),
            Err(_) => return Err(StatusCode::SERVICE_UNAVAILABLE),
        }
    }
    redirect::confirm(&state.pool, lookup, action.link, action.origin)
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)
}

fn method_not_allowed() -> Response {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        [(header::ALLOW, "GET, HEAD")],
    )
        .into_response()
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
