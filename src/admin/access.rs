use axum::{
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use subtle::ConstantTimeEq;
use url::{Host, Url};

use crate::{
    confirmation::{Notice, notice},
    history::Actor,
    input::{normalize_host, request_host_port},
    rate_limit::{RateLimit, TrustedProxies},
};

use super::{AppState, Problem};

const HTTPS_SESSION_COOKIE: &str = "__Host-shortcircuit_session";
const HTTP_SESSION_COOKIE: &str = "shortcircuit_session";
const CSRF_HEADER: &str = "x-csrf-token";
const DEFAULT_SHORTCODE_LENGTH: usize = 10;
pub(super) const MAX_SHORTCODE_LENGTH: usize = 32;

pub struct AdminConfig {
    origin: String,
    host: String,
    port: u16,
    default_port: u16,
    secure: bool,
    pub(super) local_port: u16,
    pub(super) shortcode_length: usize,
    proxies: TrustedProxies,
    pub(super) entry_limit: RateLimit,
    pub(super) sensitive_limit: RateLimit,
    pub(super) api_limit: RateLimit,
}

impl AdminConfig {
    pub fn new(origin: &str, local_port: u16) -> Result<Self, &'static str> {
        let url = Url::parse(origin).map_err(|_| "invalid ADMIN_ORIGIN")?;
        if url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err("ADMIN_ORIGIN must be a bare HTTP or HTTPS origin");
        }
        let secure = match url.scheme() {
            "https" => true,
            "http" if is_loopback(&url) => false,
            _ => return Err("ADMIN_ORIGIN requires HTTPS outside loopback"),
        };
        let host = url
            .host_str()
            .and_then(normalize_host)
            .ok_or("invalid admin host")?;
        let default_port = if secure { 443 } else { 80 };
        Ok(Self {
            origin: url.origin().ascii_serialization(),
            host,
            port: url.port().unwrap_or(default_port),
            default_port,
            secure,
            local_port,
            shortcode_length: DEFAULT_SHORTCODE_LENGTH,
            proxies: TrustedProxies::default(),
            entry_limit: RateLimit::PASSWORD_DEFAULT,
            sensitive_limit: RateLimit::SENSITIVE_DEFAULT,
            api_limit: RateLimit::ADMIN_DEFAULT,
        })
    }

    pub fn with_access_controls(
        mut self,
        proxies: TrustedProxies,
        entry_limit: RateLimit,
        sensitive_limit: RateLimit,
        api_limit: RateLimit,
    ) -> Self {
        self.proxies = proxies;
        self.entry_limit = entry_limit;
        self.sensitive_limit = sensitive_limit;
        self.api_limit = api_limit;
        self
    }

    pub fn with_shortcode_length(mut self, length: usize) -> Result<Self, &'static str> {
        if !(8..=MAX_SHORTCODE_LENGTH).contains(&length) {
            return Err("SHORTCODE_LENGTH must be an integer from 8 to 32");
        }
        self.shortcode_length = length;
        Ok(self)
    }

    fn cookie_name(&self) -> &'static str {
        if self.secure {
            HTTPS_SESSION_COOKIE
        } else {
            HTTP_SESSION_COOKIE
        }
    }

    pub(super) fn cookie(&self, token: &str, ttl: u32) -> HeaderValue {
        let secure = if self.secure { "; Secure" } else { "" };
        HeaderValue::from_str(&format!(
            "{}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={ttl}{secure}",
            self.cookie_name()
        ))
        .expect("session tokens contain ASCII hexadecimal characters")
    }
}

fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain("localhost")) => true,
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        _ => false,
    }
}

#[derive(Clone)]
pub(super) struct BrowserToken(pub(super) String);

#[derive(Clone)]
pub(super) struct LinkAccess {
    pub(super) host: Option<String>,
    pub(super) actor: Actor,
    /// Browser sessions and keys with the analytics scope may read link traffic.
    pub(super) analytics: bool,
}

pub(super) async fn admin_host(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, Problem> {
    if !is_admin_host(&state, &request) {
        report_other_address(&state);
        // API clients keep the JSON problem; browsers get a page that explains the 404.
        if is_api_path(&request) {
            return Err(Problem(StatusCode::NOT_FOUND, "not_found"));
        }
        return Ok(other_address(&request));
    }
    Ok(next.run(request).await)
}

/// Unrouted paths stay a bare `404` on the configured address and for API paths. Other paths
/// at another address get the same explanation as routed ones.
pub(super) async fn unrouted(state: AppState, request: Request) -> Response {
    if is_admin_host(&state, &request) || is_api_path(&request) {
        return StatusCode::NOT_FOUND.into_response();
    }
    report_other_address(&state);
    other_address(&request)
}

fn is_admin_host(state: &AppState, request: &Request) -> bool {
    request_host_port(request.uri(), request.headers()).is_some_and(|(host, port)| {
        host == state.config.host && port.unwrap_or(state.config.default_port) == state.config.port
    })
}

fn is_api_path(request: &Request) -> bool {
    let path = request.uri().path();
    path == "/api" || path.starts_with("/api/")
}

/// Explains the most common installation mistake once per process. Only the log names the
/// expected origin; the response never does.
fn report_other_address(state: &AppState) {
    static REPORTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !REPORTED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        tracing::info!(
            expected = %state.config.origin,
            "a request used an address other than ADMIN_ORIGIN; open the configured address"
        );
    }
}

/// A static page without scripts that names no address, host, or version. HEAD stays bodiless.
fn other_address(request: &Request) -> Response {
    let language = request
        .headers()
        .get(header::ACCEPT_LANGUAGE)
        .and_then(|value| value.to_str().ok());
    notice(
        request.method(),
        StatusCode::NOT_FOUND,
        Notice::AdminAddress,
        language,
    )
}

pub(super) async fn entry_access(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, Problem> {
    require_origin(request.headers(), &state.config)?;
    state
        .attempts
        .allow(state.config.proxies.client(&request))
        .map_err(rate_error)?;
    Ok(next.run(request).await)
}

pub(super) async fn api_limit(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, Problem> {
    if request.uri().path().starts_with("/api/v1/") {
        state
            .api_attempts
            .allow(state.config.proxies.client(&request))
            .map_err(rate_error)?;
    }
    Ok(next.run(request).await)
}

pub(super) async fn browser_access(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, Problem> {
    if request.headers().contains_key(header::AUTHORIZATION) {
        return Err(Problem::unauthorized());
    }
    let token =
        session_token(request.headers(), &state.config)?.ok_or_else(Problem::unauthorized)?;
    let session = state.auth.session(&token).await?;
    require_csrf(&request, &state.config, &session.csrf)?;
    let path = request.uri().path();
    if request.method() == Method::POST
        && (matches!(
            path,
            "/api/v1/auth/password"
                | "/api/v1/system/backup/authorize"
                | "/api/v1/system/restore/authorize"
        ) || (path.starts_with("/api/v1/keys/") && path.ends_with("/rotate")))
    {
        state
            .sensitive_attempts
            .allow(state.config.proxies.client(&request))
            .map_err(rate_error)?;
    }
    request.extensions_mut().insert(BrowserToken(token));
    request
        .extensions_mut()
        .insert(Actor::Admin(session.username));
    Ok(next.run(request).await)
}

pub(super) async fn link_access(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, Problem> {
    let cookie = session_token(request.headers(), &state.config)?;
    let bearer =
        single_header(request.headers(), header::AUTHORIZATION.as_str())?.map(str::to_owned);
    let (host, actor, analytics) = match (cookie, bearer) {
        (Some(token), None) => {
            let session = state.auth.session(&token).await?;
            require_csrf(&request, &state.config, &session.csrf)?;
            (None, Actor::Admin(session.username), true)
        }
        (None, Some(value)) => {
            let key = bearer_key(&state, &value).await?;
            let scope = match *request.method() {
                Method::GET | Method::HEAD => "read",
                Method::POST | Method::PATCH => "write",
                Method::DELETE => "delete",
                _ => {
                    return Err(Problem(
                        StatusCode::METHOD_NOT_ALLOWED,
                        "method_not_allowed",
                    ));
                }
            };
            if !key.allows(scope) {
                return Err(Problem(StatusCode::FORBIDDEN, "insufficient_scope"));
            }
            let analytics = key.allows("analytics");
            (key.host, Actor::ApiKey(key.id), analytics)
        }
        _ => return Err(Problem::unauthorized()),
    };
    request.extensions_mut().insert(LinkAccess {
        host,
        actor,
        analytics,
    });
    Ok(next.run(request).await)
}

/// Read access to analytics reports for a browser session or a key with the analytics scope.
#[derive(Clone)]
pub(super) struct ReportAccess {
    /// A key's host restriction. Browser sessions and unrestricted keys report on every host.
    pub(super) host: Option<String>,
}

pub(super) async fn report_access(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, Problem> {
    if !matches!(*request.method(), Method::GET | Method::HEAD) {
        return Err(Problem(
            StatusCode::METHOD_NOT_ALLOWED,
            "method_not_allowed",
        ));
    }
    let cookie = session_token(request.headers(), &state.config)?;
    let bearer =
        single_header(request.headers(), header::AUTHORIZATION.as_str())?.map(str::to_owned);
    let access = match (cookie, bearer) {
        (Some(token), None) => {
            state.auth.session(&token).await?;
            ReportAccess { host: None }
        }
        (None, Some(value)) => {
            let key = bearer_key(&state, &value).await?;
            if !key.allows("analytics") {
                return Err(Problem(StatusCode::FORBIDDEN, "insufficient_scope"));
            }
            ReportAccess { host: key.host }
        }
        _ => return Err(Problem::unauthorized()),
    };
    request.extensions_mut().insert(access);
    Ok(next.run(request).await)
}

async fn bearer_key(state: &AppState, value: &str) -> Result<crate::auth::KeyAccess, Problem> {
    let Some((scheme, token)) = value.split_once(' ') else {
        return Err(Problem::unauthorized());
    };
    if !scheme.eq_ignore_ascii_case("bearer") {
        return Err(Problem::unauthorized());
    }
    Ok(state.auth.key(token).await?)
}

fn require_origin(headers: &HeaderMap, config: &AdminConfig) -> Result<(), Problem> {
    if single_header(headers, header::ORIGIN.as_str())
        .ok()
        .flatten()
        != Some(config.origin.as_str())
    {
        return Err(Problem(StatusCode::FORBIDDEN, "invalid_origin"));
    }
    Ok(())
}

fn require_csrf(request: &Request, config: &AdminConfig, csrf: &str) -> Result<(), Problem> {
    if matches!(*request.method(), Method::GET | Method::HEAD) {
        return Ok(());
    }
    require_origin(request.headers(), config)?;
    let valid = single_header(request.headers(), CSRF_HEADER)
        .ok()
        .flatten()
        .is_some_and(|header| bool::from(header.as_bytes().ct_eq(csrf.as_bytes())));
    if !valid {
        return Err(Problem(StatusCode::FORBIDDEN, "invalid_csrf"));
    }
    Ok(())
}

fn single_header<'a>(headers: &'a HeaderMap, name: &str) -> Result<Option<&'a str>, Problem> {
    let values = headers.get_all(name);
    let mut fields = values.iter();
    let first = fields.next();
    if fields.next().is_some() {
        return Err(Problem::unauthorized());
    }
    first
        .map(|value| value.to_str().map_err(|_| Problem::unauthorized()))
        .transpose()
}

pub(super) fn session_token(
    headers: &HeaderMap,
    config: &AdminConfig,
) -> Result<Option<String>, Problem> {
    let mut token = None;
    for field in headers.get_all(header::COOKIE) {
        let value = field.to_str().map_err(|_| Problem::unauthorized())?;
        for item in value.split(';') {
            let Some((name, secret)) = item.trim().split_once('=') else {
                continue;
            };
            if name != config.cookie_name() {
                continue;
            }
            if token.is_some()
                || secret.len() != 64
                || !secret.bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                return Err(Problem::unauthorized());
            }
            token = Some(secret.to_owned());
        }
    }
    Ok(token)
}

fn rate_error(status: StatusCode) -> Problem {
    if status == StatusCode::TOO_MANY_REQUESTS {
        Problem(status, "try_again_later")
    } else {
        Problem::unavailable()
    }
}
