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
    confirmation::{Action, Home, Notice, public_notice},
    destinations::Guard,
    domains,
    input::{PublicHosts, request_host, valid_slug},
    public_pages,
    rate_limit::{AttemptLimiter, RateLimit, TrustedProxies},
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
    public_limit: Option<Arc<AttemptLimiter>>,
    proxies: TrustedProxies,
    destinations: Guard,
    pages: public_pages::Cache,
}

pub struct PublicControls {
    pub proxies: TrustedProxies,
    pub password: RateLimit,
    /// Protected-link password attempts per link: per client key, and across all clients.
    pub password_link: RateLimit,
    pub public: Option<RateLimit>,
}

impl Default for PublicControls {
    fn default() -> Self {
        Self {
            proxies: TrustedProxies::default(),
            password: RateLimit::PASSWORD_DEFAULT,
            password_link: RateLimit::PASSWORD_LINK_DEFAULT,
            public: None,
        }
    }
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
    public_router_with_controls(
        pool,
        hosts,
        public_port,
        analytics,
        PublicControls::default(),
    )
}

pub fn public_router_with_controls(
    pool: SqlitePool,
    hosts: PublicHosts,
    public_port: u16,
    analytics: Option<Analytics>,
    controls: PublicControls,
) -> Router {
    Router::new()
        .fallback(resolve)
        .with_state(AppState {
            auth: Auth::new(pool.clone()),
            pool,
            hosts,
            public_port,
            analytics,
            attempts: Arc::new(AttemptLimiter::with_scope_limit(
                controls.password,
                controls.password_link,
            )),
            public_limit: controls
                .public
                .map(AttemptLimiter::with_limit)
                .map(Arc::new),
            proxies: controls.proxies,
            destinations: Guard::default(),
            pages: public_pages::Cache::default(),
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
    let peer = (post || state.public_limit.is_some()).then(|| state.proxies.client(&request));
    if let Some(limiter) = &state.public_limit
        && let Some(ip) = peer
        && let Err(status) = limiter.allow(ip)
    {
        return if status == StatusCode::TOO_MANY_REQUESTS {
            (status, [(header::RETRY_AFTER, "60")]).into_response()
        } else {
            status.into_response()
        };
    }
    let (parts, body) = request.into_parts();
    let uri = &parts.uri;
    let language = parts
        .headers
        .get(header::ACCEPT_LANGUAGE)
        .and_then(|value| value.to_str().ok());
    if uri
        .path_and_query()
        .is_some_and(|value| value.as_str().len() > MAX_REQUEST_URI)
    {
        return StatusCode::URI_TOO_LONG.into_response();
    }
    let Some(host) = request_host(uri, &parts.headers) else {
        return not_found(&state, &parts.method, Notice::UnknownHost, language, None).await;
    };
    let domain = match domains::lookup(&state.pool, &state.hosts, &host, state.public_port).await {
        Ok(Some(domain)) if domain.enabled => domain,
        Ok(_) => {
            return not_found(
                &state,
                &parts.method,
                Notice::UnknownHost,
                language,
                Some(&host),
            )
            .await;
        }
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
        return not_found(
            &state,
            &parts.method,
            Notice::Missing { home: None },
            language,
            Some(&host),
        )
        .await;
    };
    if slug.is_empty() {
        if post {
            return method_not_allowed();
        }
        let Ok(root_target) = permitted_fallback(&state, domain.root_target.as_deref()).await
        else {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        };
        let response = match fallback(&host, "/", root_target) {
            Some(response) => response,
            None => {
                not_found(
                    &state,
                    &parts.method,
                    Notice::Missing { home: None },
                    language,
                    Some(&host),
                )
                .await
            }
        };
        // Root requests use the same counting rule as unmatched slugs: GET only.
        if parts.method == Method::GET
            && !response.status().is_server_error()
            && let Some(analytics) = &state.analytics
        {
            let kind = if response.status().is_redirection() {
                Kind::RootFallback
            } else {
                Kind::RootMissing
            };
            analytics.record(&host, None, kind, &parts.headers);
        }
        return response;
    }
    if !valid_slug(slug) {
        if parts.method == Method::GET
            && let Some(analytics) = &state.analytics
        {
            analytics.record(&host, None, Kind::Missing, &parts.headers);
        }
        return missing(
            &state,
            &host,
            &domain.origin,
            domain.root_target.as_deref(),
            &parts.method,
            language,
        )
        .await;
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
    let Ok(mut result) =
        redirect::resolve_with_subject(&state.pool, lookup, &state.destinations).await
    else {
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
        if post && let Some(peer) = peer {
            result = match confirm(&state, action, lookup, &parts.headers, body, peer).await {
                Ok(confirmed) => confirmed,
                Err(StatusCode::UNAUTHORIZED) => {
                    let pages = state.pages.current(&state.pool).await;
                    return action.page(lookup.language, true, Some(&pages));
                }
                Err(StatusCode::TOO_MANY_REQUESTS) => {
                    return (StatusCode::TOO_MANY_REQUESTS, [(header::RETRY_AFTER, "60")])
                        .into_response();
                }
                Err(status) => return status.into_response(),
            };
        } else if parts.method == Method::HEAD {
            return StatusCode::OK.into_response();
        } else {
            let pages = state.pages.current(&state.pool).await;
            return action.page(lookup.language, false, Some(&pages));
        }
    } else if post {
        return match result.outcome {
            // A blocked link answers exactly like a missing one, so visitors cannot tell that
            // a disabled, scheduled, or used-up slug exists.
            Outcome::Blocked => {
                missing(
                    &state,
                    &host,
                    &domain.origin,
                    domain.root_target.as_deref(),
                    &parts.method,
                    language,
                )
                .await
            }
            _ => method_not_allowed(),
        };
    }
    let (kind, mut response) = match result.outcome {
        Outcome::Redirect { target, status } => (Kind::Redirect, respond(&target, status)),
        Outcome::Missing => {
            let Ok(missing_target) =
                permitted_fallback(&state, domain.missing_target.as_deref()).await
            else {
                return StatusCode::SERVICE_UNAVAILABLE.into_response();
            };
            let response = match fallback(&host, uri.path(), missing_target) {
                Some(response) => response,
                None => {
                    missing(
                        &state,
                        &host,
                        &domain.origin,
                        domain.root_target.as_deref(),
                        &parts.method,
                        language,
                    )
                    .await
                }
            };
            (Kind::Missing, response)
        }
        Outcome::Blocked => (
            Kind::Blocked,
            missing(
                &state,
                &host,
                &domain.origin,
                domain.root_target.as_deref(),
                &parts.method,
                language,
            )
            .await,
        ),
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
        state.attempts.allow_scoped(peer, action.link.id)?;
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
    redirect::confirm(
        &state.pool,
        lookup,
        action.link,
        action.origin,
        &state.destinations,
    )
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

/// A configured fallback that the destination policy refuses behaves like no fallback.
async fn permitted_fallback<'a>(
    state: &AppState,
    target: Option<&'a str>,
) -> Result<Option<&'a str>, sqlx::Error> {
    let Some(target) = target else {
        return Ok(None);
    };
    let policy = state.destinations.current_from(&state.pool).await?;
    Ok(policy.permits(target).then_some(target))
}

/// Redirects to a usable fallback, or returns `None` when the request ends with `404`.
fn fallback(host: &str, path: &str, target: Option<&str>) -> Option<Response> {
    match target {
        Some(target) if !domains::fallback_loops(host, path, target) => Some(respond(target, 302)),
        _ => None,
    }
}

/// The `404` page for an unmatched path. It links to the domain root only when the root path
/// redirects to a permitted destination; a policy lookup error omits the link.
async fn missing(
    state: &AppState,
    host: &str,
    origin: &str,
    root_target: Option<&str>,
    method: &Method,
    language: Option<&str>,
) -> Response {
    let home = if method == Method::HEAD {
        None
    } else {
        permitted_fallback(state, root_target)
            .await
            .ok()
            .flatten()
            .filter(|target| !domains::fallback_loops(host, "/", target))
            .and_then(|target| {
                let label = url::Url::parse(target).ok()?.host_str()?.to_owned();
                Some(Home {
                    href: format!("{origin}/"),
                    label,
                })
            })
    };
    not_found(
        state,
        method,
        Notice::Missing { home },
        language,
        Some(host),
    )
    .await
}

/// A public `404` page. HEAD keeps the bare status and never reads the page templates.
async fn not_found(
    state: &AppState,
    method: &Method,
    notice: Notice,
    language: Option<&str>,
    host: Option<&str>,
) -> Response {
    if method == Method::HEAD {
        return StatusCode::NOT_FOUND.into_response();
    }
    let pages = state.pages.current(&state.pool).await;
    public_notice(
        method,
        StatusCode::NOT_FOUND,
        notice,
        language,
        host,
        &pages,
    )
}

fn respond(target: &str, status: u16) -> Response {
    match (StatusCode::from_u16(status), HeaderValue::from_str(target)) {
        (Ok(status), Ok(location)) => (status, [(header::LOCATION, location)]).into_response(),
        _ => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}
