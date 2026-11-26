use axum::{
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    middleware::Next,
    response::Response,
};
use subtle::ConstantTimeEq;
use url::{Host, Url};

use crate::{
    history::Actor,
    input::{normalize_host, request_host_port},
    rate_limit::peer,
};

use super::{AppState, Problem};

const HTTPS_SESSION_COOKIE: &str = "__Host-shortcircuit_session";
const HTTP_SESSION_COOKIE: &str = "shortcircuit_session";
const CSRF_HEADER: &str = "x-csrf-token";

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
        })
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
}

pub(super) async fn admin_host(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, Problem> {
    let matches =
        request_host_port(request.uri(), request.headers()).is_some_and(|(host, port)| {
            host == state.config.host
                && port.unwrap_or(state.config.default_port) == state.config.port
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
    state.attempts.allow(peer(&request)).map_err(rate_error)?;
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
    if request.uri().path() == "/api/v1/auth/password" {
        state.attempts.allow(peer(&request)).map_err(rate_error)?;
    }
    request.extensions_mut().insert(BrowserToken(token));
    request.extensions_mut().insert(Actor::Admin(session.username));
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
    let (host, actor) = match (cookie, bearer) {
        (Some(token), None) => {
            let session = state.auth.session(&token).await?;
            require_csrf(&request, &state.config, &session.csrf)?;
            (None, Actor::Admin(session.username))
        }
        (None, Some(value)) => {
            let Some((scheme, token)) = value.split_once(' ') else {
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
            (key.host, Actor::ApiKey(key.id))
        }
        _ => return Err(Problem::unauthorized()),
    };
    request.extensions_mut().insert(LinkAccess { host, actor });
    Ok(next.run(request).await)
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
