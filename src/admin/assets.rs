use axum::{
    Router,
    body::Bytes,
    extract::Request,
    http::{HeaderValue, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::get,
};

const INDEX: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/admin.html"));
const JS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/admin.js"));
const CSS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/admin.css"));
const ICON: &[u8] = include_bytes!("../../assets/branding/favicon.svg");

pub(super) fn routes() -> Router {
    Router::new()
        .route(
            "/",
            get(|| async { asset("text/html; charset=utf-8", INDEX) }),
        )
        .route(
            "/assets/admin.js",
            get(|| async { asset("text/javascript; charset=utf-8", JS) }),
        )
        .route(
            "/assets/admin.css",
            get(|| async { asset("text/css; charset=utf-8", CSS) }),
        )
        .route(
            "/favicon.svg",
            get(|| async { asset("image/svg+xml", ICON) }),
        )
}

fn asset(kind: &'static str, bytes: &'static [u8]) -> Response {
    if bytes.is_empty() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "Admin assets are unavailable. Build the frontend and rebuild ShortCircuit.",
        )
            .into_response();
    }
    ([(header::CONTENT_TYPE, kind)], Bytes::from_static(bytes)).into_response()
}

pub(super) async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self'; connect-src 'self'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'"),
    );
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(
        "permissions-policy",
        HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
    );
    response
}
