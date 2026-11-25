mod access;
mod assets;
mod catalog;
mod exchange;
mod links;
mod metrics;

use std::sync::Arc;

use axum::{
    Extension, Json, Router,
    extract::{
        DefaultBodyLimit, Path, State,
        rejection::{JsonRejection, PathRejection},
    },
    http::{HeaderValue, StatusCode, header},
    middleware,
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::{
    analytics::Analytics,
    auth::{Auth, AuthError, IssuedSession, KeyMeta, Session},
    domains,
    http::response_headers,
    input::{PublicHosts, normalize_host},
};

pub use access::AdminConfig;

const BODY_LIMIT: usize = 32_768;

#[derive(Clone)]
struct AppState {
    pool: SqlitePool,
    auth: Auth,
    hosts: PublicHosts,
    config: Arc<AdminConfig>,
    attempts: Arc<access::AttemptLimiter>,
}

pub fn admin_router(pool: SqlitePool, hosts: PublicHosts, config: AdminConfig) -> Router {
    build_admin_router(pool, hosts, config, None)
}

pub fn admin_router_with_analytics(
    pool: SqlitePool,
    hosts: PublicHosts,
    config: AdminConfig,
    analytics: Analytics,
) -> Router {
    build_admin_router(pool, hosts, config, Some(analytics))
}

fn build_admin_router(
    pool: SqlitePool,
    hosts: PublicHosts,
    config: AdminConfig,
    analytics: Option<Analytics>,
) -> Router {
    let state = AppState {
        pool: pool.clone(),
        auth: Auth::new(pool.clone()),
        hosts,
        config: Arc::new(config),
        attempts: Arc::new(access::AttemptLimiter::new()),
    };

    let sign_in = Router::new()
        .route("/api/v1/auth/setup", post(setup))
        .route("/api/v1/auth/login", post(login))
        .route("/api/v1/auth/recover", post(recover))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            access::entry_access,
        ));
    let session_routes = Router::new()
        .route("/api/v1/auth/logout", post(logout))
        .route("/api/v1/auth/password", post(change_password))
        .route("/api/v1/keys", get(list_keys).post(create_key))
        .route("/api/v1/keys/{id}", delete(revoke_key))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            access::browser_access,
        ));
    let link_routes = links::routes(pool.clone(), state.clone()).route_layer(
        middleware::from_fn_with_state(state.clone(), access::link_access),
    );
    let catalog_routes =
        catalog::routes(pool.clone(), state.hosts.clone(), state.config.local_port).route_layer(
            middleware::from_fn_with_state(state.clone(), access::browser_access),
        );
    let analytics_routes = analytics.map_or_else(Router::new, |service| {
        metrics::routes(pool.clone(), service).route_layer(middleware::from_fn_with_state(
            state.clone(),
            access::browser_access,
        ))
    });
    let exchange_routes = exchange::routes(pool.clone(), state.hosts.clone()).route_layer(
        middleware::from_fn_with_state(state.clone(), access::browser_access),
    );
    let application = Router::new()
        .route("/api/v1/auth/session", get(session_status))
        .merge(sign_in)
        .merge(session_routes)
        .with_state(state.clone())
        .merge(link_routes)
        .merge(catalog_routes)
        .merge(analytics_routes)
        .merge(exchange_routes)
        .merge(assets::routes())
        .route_layer(middleware::from_fn_with_state(state, access::admin_host));

    Router::new()
        .route("/healthz", get(|| async { StatusCode::OK }))
        .route("/readyz", get(move || readiness(pool)))
        .merge(application)
        .fallback(|| async { StatusCode::NOT_FOUND })
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .layer(middleware::from_fn(assets::security_headers))
        .layer(middleware::from_fn(response_headers))
}

async fn readiness(pool: SqlitePool) -> StatusCode {
    match sqlx::query_scalar::<_, i64>("SELECT 1 FROM links LIMIT 1")
        .fetch_optional(&pool)
        .await
    {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::SERVICE_UNAVAILABLE,
    }
}

#[derive(Serialize)]
struct SessionStatus {
    setup_required: bool,
    user: Option<Session>,
}

async fn session_status(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<SessionStatus>, Problem> {
    let setup_required = !state.auth.has_account().await?;
    let user = if let Some(token) = access::session_token(&headers, &state.config)? {
        match state.auth.session(&token).await {
            Ok(session) => Some(session),
            Err(AuthError::Unauthorized) => None,
            Err(error) => return Err(error.into()),
        }
    } else {
        None
    };
    Ok(Json(SessionStatus {
        setup_required,
        user,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Setup {
    username: String,
    password: String,
    token: String,
}

async fn setup(
    State(state): State<AppState>,
    body: Result<Json<Setup>, JsonRejection>,
) -> Result<Response, Problem> {
    let input = json_payload(body)?;
    let issued = state
        .auth
        .setup(&input.token, &input.username, &input.password)
        .await?;
    Ok(session_response(&state.config, issued, StatusCode::CREATED))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Login {
    username: String,
    password: String,
}

async fn login(
    State(state): State<AppState>,
    body: Result<Json<Login>, JsonRejection>,
) -> Result<Response, Problem> {
    let input = json_payload(body)?;
    let issued = state.auth.login(&input.username, &input.password).await?;
    Ok(session_response(&state.config, issued, StatusCode::OK))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Recovery {
    token: String,
    password: String,
}

async fn recover(
    State(state): State<AppState>,
    body: Result<Json<Recovery>, JsonRejection>,
) -> Result<Response, Problem> {
    let input = json_payload(body)?;
    let issued = state.auth.recover(&input.token, &input.password).await?;
    Ok(session_response(&state.config, issued, StatusCode::OK))
}

async fn logout(
    State(state): State<AppState>,
    Extension(token): Extension<access::BrowserToken>,
) -> Result<Response, Problem> {
    state.auth.logout(&token.0).await?;
    Ok((
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, state.config.cookie("", 0))],
    )
        .into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PasswordChange {
    current_password: String,
    password: String,
}

async fn change_password(
    State(state): State<AppState>,
    Extension(token): Extension<access::BrowserToken>,
    body: Result<Json<PasswordChange>, JsonRejection>,
) -> Result<Response, Problem> {
    let input = json_payload(body)?;
    let updated = state
        .auth
        .change_password(&token.0, &input.current_password, &input.password)
        .await?;
    Ok(session_response(&state.config, updated, StatusCode::OK))
}

fn session_response(config: &AdminConfig, input: IssuedSession, status: StatusCode) -> Response {
    let cookie = config.cookie(&input.token, 43_200);
    (status, [(header::SET_COOKIE, cookie)], Json(input.session)).into_response()
}

#[derive(Serialize)]
struct KeyList {
    items: Vec<KeyMeta>,
}

async fn list_keys(State(state): State<AppState>) -> Result<Json<KeyList>, Problem> {
    Ok(Json(KeyList {
        items: state.auth.list_keys().await?,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyInput {
    name: String,
    scopes: Vec<String>,
    host: Option<String>,
    expires_at: Option<i64>,
}

#[derive(Serialize)]
struct KeyCreated {
    key: KeyMeta,
    token: String,
}

async fn create_key(
    State(state): State<AppState>,
    body: Result<Json<KeyInput>, JsonRejection>,
) -> Result<(StatusCode, Json<KeyCreated>), Problem> {
    let input = json_payload(body)?;
    let host = if let Some(raw) = input.host {
        let host = normalize_host(&raw).ok_or_else(|| Problem::invalid("invalid_host"))?;
        let domain = domains::lookup(&state.pool, &state.hosts, &host, state.config.local_port)
            .await
            .map_err(|_| Problem::unavailable())?;
        if domain.is_none() {
            return Err(Problem::invalid("invalid_host"));
        }
        Some(host)
    } else {
        None
    };
    let issued = state
        .auth
        .create_key(&input.name, input.scopes, host, input.expires_at)
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(KeyCreated {
            key: issued.key,
            token: issued.token,
        }),
    ))
}

async fn revoke_key(
    State(state): State<AppState>,
    id: Result<Path<i64>, PathRejection>,
) -> Result<StatusCode, Problem> {
    state.auth.revoke_key(resource_id(id)?).await?;
    Ok(StatusCode::NO_CONTENT)
}

fn json_payload<T>(value: Result<Json<T>, JsonRejection>) -> Result<T, Problem> {
    value
        .map(|Json(body)| body)
        .map_err(|error| Problem(error.status(), "invalid_request"))
}

fn resource_id(value: Result<Path<i64>, PathRejection>) -> Result<i64, Problem> {
    value
        .ok()
        .map(|Path(id)| id)
        .filter(|id| *id > 0)
        .ok_or(Problem(StatusCode::NOT_FOUND, "not_found"))
}

#[derive(Serialize)]
struct ErrorBody {
    status: u16,
    title: &'static str,
    code: &'static str,
}

struct Problem(StatusCode, &'static str);

impl Problem {
    fn invalid(code: &'static str) -> Self {
        Self(StatusCode::UNPROCESSABLE_ENTITY, code)
    }

    fn unavailable() -> Self {
        Self(StatusCode::SERVICE_UNAVAILABLE, "temporarily_unavailable")
    }

    fn unauthorized() -> Self {
        Self(StatusCode::UNAUTHORIZED, "authentication_required")
    }
}

impl From<AuthError> for Problem {
    fn from(error: AuthError) -> Self {
        match error {
            AuthError::Invalid => Self::invalid("invalid_auth_input"),
            AuthError::Unauthorized => Self::unauthorized(),
            AuthError::NotFound => Self(StatusCode::NOT_FOUND, "not_found"),
            AuthError::Conflict => Self(StatusCode::CONFLICT, "auth_conflict"),
            AuthError::Busy => Self(StatusCode::TOO_MANY_REQUESTS, "try_again_later"),
            AuthError::Unavailable => Self::unavailable(),
        }
    }
}

impl IntoResponse for Problem {
    fn into_response(self) -> Response {
        let mut response = (
            self.0,
            Json(ErrorBody {
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
        if self.0 == StatusCode::TOO_MANY_REQUESTS {
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from_static("60"));
        }
        response
    }
}
