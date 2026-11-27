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
const FONT_SANS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/geist-latin.woff2"));
const FONT_SANS_EXT: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/geist-latin-ext.woff2"));
const FONT_MONO: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/geist-mono-latin.woff2"));
const ICON_PNG: &[u8] = include_bytes!("../../assets/branding/favicon.png");
const ICON_SVG: &[u8] = include_bytes!("../../assets/branding/favicon.svg");
const TOUCH_ICON: &[u8] = include_bytes!("../../assets/branding/apple-touch-icon-180.png");
const PWA_192: &[u8] = include_bytes!("../../assets/branding/pwa-192.png");
const PWA_512: &[u8] = include_bytes!("../../assets/branding/pwa-512.png");
const PWA_MASKABLE: &[u8] = include_bytes!("../../assets/branding/pwa-maskable-512.png");
const MANIFEST: &[u8] = include_bytes!("../../assets/branding/manifest.webmanifest");

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
            "/assets/geist-latin-wght-normal.woff2",
            get(|| async { asset("font/woff2", FONT_SANS) }),
        )
        .route(
            "/assets/geist-latin-ext-wght-normal.woff2",
            get(|| async { asset("font/woff2", FONT_SANS_EXT) }),
        )
        .route(
            "/assets/geist-mono-latin-wght-normal.woff2",
            get(|| async { asset("font/woff2", FONT_MONO) }),
        )
        .route(
            "/favicon.png",
            get(|| async { asset("image/png", ICON_PNG) }),
        )
        .route(
            "/apple-touch-icon.png",
            get(|| async { asset("image/png", TOUCH_ICON) }),
        )
        .route(
            "/pwa-192.png",
            get(|| async { asset("image/png", PWA_192) }),
        )
        .route(
            "/pwa-512.png",
            get(|| async { asset("image/png", PWA_512) }),
        )
        .route(
            "/pwa-maskable-512.png",
            get(|| async { asset("image/png", PWA_MASKABLE) }),
        )
        .route(
            "/manifest.webmanifest",
            get(|| async { asset("application/manifest+json", MANIFEST) }),
        )
        .route(
            "/favicon.svg",
            get(|| async { asset("image/svg+xml", ICON_SVG) }),
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
        HeaderValue::from_static("default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self'; font-src 'self'; manifest-src 'self'; connect-src 'self'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'"),
    );
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(
        "permissions-policy",
        HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
    );
    response
}
