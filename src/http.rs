use axum::{
    Router,
    extract::{Request, State},
    http::{HeaderValue, Method, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use sqlx::SqlitePool;

use crate::{
    input::{PublicHosts, request_host, valid_slug},
    store,
};

const MAX_REQUEST_URI: usize = 8192;

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
