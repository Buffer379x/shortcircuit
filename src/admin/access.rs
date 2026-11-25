use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Mutex,
    time::{Duration, Instant},
};

use axum::{
    extract::{ConnectInfo, Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    middleware::Next,
    response::Response,
};
use subtle::ConstantTimeEq;
use url::{Host, Url};

use crate::input::{normalize_host, request_host_port};

use super::{AppState, Problem};

const HTTPS_SESSION_COOKIE: &str = "__Host-shortcircuit_session";
const HTTP_SESSION_COOKIE: &str = "shortcircuit_session";
const CSRF_HEADER: &str = "x-csrf-token";
const WINDOW: Duration = Duration::from_secs(60);
const PEER_ATTEMPTS: u8 = 8;
const GLOBAL_ATTEMPTS: u8 = 60;

pub struct AdminConfig {
    origin: String,
    host: String,
    port: u16,
    default_port: u16,
    secure: bool,
    pub(super) local_port: u16,
}

impl AdminConfig {
    pub fn new(origin: &str, local_port: u16) -> Result<Self, &'static str> {
        let url = Url::parse(origin).map_err(|_| "invalid ADMIN_ORIGIN")?;
        if url.path() != "/" || url.query().is_some() || url.fragment().is_some()
            || !url.username().is_empty() || url.password().is_some()
        {
            return Err("ADMIN_ORIGIN must be a bare HTTP or HTTPS origin");
        }
        let secure = match url.scheme() {
            "https" => true,
            "http" if is_loopback(&url) => false,
            _ => return Err("ADMIN_ORIGIN requires HTTPS outside loopback"),
        };
        let host = url.host_str().and_then(normalize_host).ok_or("invalid admin host")?;
        let default_port = if secure { 443 } else { 80 };
        Ok(Self {
            origin: url.origin().ascii_serialization(),
            host,
            port: url.port().unwrap_or(default_port),
            default_port,
            secure,
            local_port,
        })
    }

    fn cookie_name(&self) -> &'static str {
        if self.secure { HTTPS_SESSION_COOKIE } else { HTTP_SESSION_COOKIE }
    }

    pub(super) fn cookie(&self, token: &str, ttl: u32) -> HeaderValue {
        let secure = if self.secure { "; Secure" } else { "" };
        HeaderValue::from_str(&format!(
            "{}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={ttl}{secure}",
            self.cookie_name()
        )).expect("session tokens contain ASCII hexadecimal characters")
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
}

pub(super) async fn admin_host(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, Problem> {
    let matches = request_host_port(request.uri(), request.headers())
        .is_some_and(|(host,port)| {
            host == state.config.host && port.unwrap_or(state.config.default_port) == state.config.port
        });
    if !matches {
        return Err(Problem(StatusCode::NOT_FOUND, "not_found"));
    }
    Ok(next.run(request).await)
}

pub(super) async fn entry_access(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, Problem> {
    require_origin(request.headers(), &state.config)?;
    state.attempts.allow(peer(&request))?;
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
    let token = session_token(request.headers(), &state.config)?.ok_or_else(Problem::unauthorized)?;
    let session = state.auth.session(&token).await?;
    require_csrf(&request, &state.config, &session.csrf)?;
    if request.uri().path() == "/api/v1/auth/password" {
        state.attempts.allow(peer(&request))?;
    }
    request.extensions_mut().insert(BrowserToken(token));
    Ok(next.run(request).await)
}

pub(super) async fn link_access(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, Problem> {
    let cookie = session_token(request.headers(), &state.config)?;
    let bearer = single_header(request.headers(), header::AUTHORIZATION.as_str())?.map(str::to_owned);
    let host = match (cookie, bearer) {
        (Some(token), None) => {
            let session = state.auth.session(&token).await?;
            require_csrf(&request, &state.config, &session.csrf)?;
            None
        }
        (None, Some(value)) => {
            let Some((scheme,token)) = value.split_once(' ') else {
                return Err(Problem::unauthorized());
            };
            if !scheme.eq_ignore_ascii_case("bearer") {
                return Err(Problem::unauthorized());
            }
            let key = state.auth.key(token).await?;
            let scope = match *request.method() {
                Method::GET | Method::HEAD => "read",
                Method::POST | Method::PATCH => "write",
                Method::DELETE => "delete",
                _ => return Err(Problem(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed")),
            };
            if !key.allows(scope) {
                return Err(Problem(StatusCode::FORBIDDEN, "insufficient_scope"));
            }
            key.host
        }
        _ => return Err(Problem::unauthorized()),
    };
    request.extensions_mut().insert(LinkAccess { host });
    Ok(next.run(request).await)
}

fn require_origin(headers: &HeaderMap, config: &AdminConfig) -> Result<(), Problem> {
    if single_header(headers, header::ORIGIN.as_str()).ok().flatten() != Some(config.origin.as_str()) {
        return Err(Problem(StatusCode::FORBIDDEN, "invalid_origin"));
    }
    Ok(())
}

fn require_csrf(request: &Request, config: &AdminConfig, csrf: &str) -> Result<(), Problem> {
    if matches!(*request.method(), Method::GET | Method::HEAD) {
        return Ok(());
    }
    require_origin(request.headers(), config)?;
    let valid = single_header(request.headers(), CSRF_HEADER).ok().flatten()
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
    first.map(|value| value.to_str().map_err(|_| Problem::unauthorized())).transpose()
}

pub(super) fn session_token(headers: &HeaderMap, config: &AdminConfig) -> Result<Option<String>, Problem> {
    let mut token = None;
    for field in headers.get_all(header::COOKIE) {
        let value = field.to_str().map_err(|_| Problem::unauthorized())?;
        for item in value.split(';') {
            let Some((name,secret)) = item.trim().split_once('=') else { continue };
            if name != config.cookie_name() {
                continue;
            }
            if token.is_some() || secret.len() != 64 || !secret.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(Problem::unauthorized());
            }
            token = Some(secret.to_owned());
        }
    }
    Ok(token)
}

fn peer(request: &Request) -> IpAddr {
    request.extensions().get::<ConnectInfo<SocketAddr>>()
        .map_or(IpAddr::V4(Ipv4Addr::LOCALHOST), |info| info.0.ip())
}

pub(super) struct AttemptLimiter(Mutex<Attempts>);

struct Attempts {
    since: Instant,
    per_peer: HashMap<IpAddr,u8>,
    total: u8,
}

impl AttemptLimiter {
    pub(super) fn new() -> Self {
        Self(Mutex::new(Attempts {
            since: Instant::now(),
            per_peer: HashMap::new(),
            total: 0,
        }))
    }

    fn allow(&self, peer: IpAddr) -> Result<(),Problem> {
        let mut window = self.0.lock().map_err(|_| Problem::unavailable())?;
        if window.since.elapsed() >= WINDOW {
            window.since = Instant::now();
            window.per_peer.clear();
            window.total = 0;
        }
        let limited = Problem(StatusCode::TOO_MANY_REQUESTS, "try_again_later");
        if window.total >= GLOBAL_ATTEMPTS {
            return Err(limited);
        }
        let count = window.per_peer.entry(peer).or_default();
        if *count >= PEER_ATTEMPTS {
            return Err(limited);
        }
        *count += 1;
        window.total += 1;
        Ok(())
    }
}
