use axum::{
    Router,
    body::Bytes,
    extract::Request,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::get,
};

const INDEX: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/admin.html"));
const JS: Encoded = Encoded {
    identity: include_bytes!(concat!(env!("OUT_DIR"), "/admin.js")),
    gzip: include_bytes!(concat!(env!("OUT_DIR"), "/admin.js.gz")),
    brotli: include_bytes!(concat!(env!("OUT_DIR"), "/admin.js.br")),
};
const CSS: Encoded = Encoded {
    identity: include_bytes!(concat!(env!("OUT_DIR"), "/admin.css")),
    gzip: include_bytes!(concat!(env!("OUT_DIR"), "/admin.css.gz")),
    brotli: include_bytes!(concat!(env!("OUT_DIR"), "/admin.css.br")),
};

/// An asset in its original form and precompressed by the frontend build.
struct Encoded {
    identity: &'static [u8],
    gzip: &'static [u8],
    brotli: &'static [u8],
}
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
            get(|headers: HeaderMap| async move { encoded("text/javascript; charset=utf-8", &JS, &headers) }),
        )
        .route(
            "/assets/admin.css",
            get(|headers: HeaderMap| async move { encoded("text/css; charset=utf-8", &CSS, &headers) }),
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

/// Serves the smallest encoding the client accepts. A missing compressed copy (a frontend built
/// without the compression step) falls back to the original bytes.
fn encoded(kind: &'static str, asset_bytes: &Encoded, headers: &HeaderMap) -> Response {
    let accepted = headers
        .get_all(header::ACCEPT_ENCODING)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .filter_map(|item| {
            let mut parts = item.split(';');
            let name = parts.next()?.trim().to_ascii_lowercase();
            let refused = parts.any(|p| {
                matches!(
                    p.trim().replace(' ', "").as_str(),
                    "q=0" | "q=0.0" | "q=0.00" | "q=0.000"
                )
            });
            (!refused).then_some(name)
        })
        .collect::<Vec<_>>();
    let choice = if accepted.iter().any(|n| n == "br") && !asset_bytes.brotli.is_empty() {
        Some(("br", asset_bytes.brotli))
    } else if accepted.iter().any(|n| n == "gzip") && !asset_bytes.gzip.is_empty() {
        Some(("gzip", asset_bytes.gzip))
    } else {
        None
    };
    let mut response = match choice {
        Some((encoding, bytes)) => {
            let mut response = asset(kind, bytes);
            if response.status().is_success() {
                response
                    .headers_mut()
                    .insert(header::CONTENT_ENCODING, HeaderValue::from_static(encoding));
            }
            response
        }
        None => asset(kind, asset_bytes.identity),
    };
    response
        .headers_mut()
        .insert(header::VARY, HeaderValue::from_static("accept-encoding"));
    response
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
    // A response with its own, stricter policy keeps it: the address notice allows only its
    // inline stylesheet by hash and no scripts. The public page preview sets its own policy
    // and `SAMEORIGIN`, so only the admin interface can frame it. `frame-src 'self'` lets the
    // interface show that preview in a sandboxed frame.
    headers
        .entry(header::CONTENT_SECURITY_POLICY)
        .or_insert(HeaderValue::from_static("default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self'; font-src 'self'; manifest-src 'self'; connect-src 'self'; frame-src 'self'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'"));
    headers
        .entry(header::X_FRAME_OPTIONS)
        .or_insert(HeaderValue::from_static("DENY"));
    headers.insert(
        "permissions-policy",
        HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: Encoded = Encoded {
        identity: b"plain",
        gzip: b"gz",
        brotli: b"br",
    };

    fn served(accept: Option<&str>) -> (Option<String>, String) {
        let mut headers = HeaderMap::new();
        if let Some(value) = accept {
            headers.insert(
                header::ACCEPT_ENCODING,
                HeaderValue::from_str(value).unwrap(),
            );
        }
        let response = encoded("text/css; charset=utf-8", &SAMPLE, &headers);
        assert_eq!(response.headers()[header::VARY], "accept-encoding");
        let encoding = response
            .headers()
            .get(header::CONTENT_ENCODING)
            .map(|v| v.to_str().unwrap().to_owned());
        (
            encoding,
            response.headers()[header::CONTENT_TYPE]
                .to_str()
                .unwrap()
                .to_owned(),
        )
    }

    #[test]
    fn serves_the_best_accepted_encoding() {
        assert_eq!(served(Some("gzip, deflate, br")).0.as_deref(), Some("br"));
        assert_eq!(served(Some("gzip")).0.as_deref(), Some("gzip"));
        assert_eq!(served(Some("br;q=0, gzip")).0.as_deref(), Some("gzip"));
        assert_eq!(served(Some("identity")).0, None);
        assert_eq!(served(None).0, None);
        assert_eq!(served(Some("gzip")).1, "text/css; charset=utf-8");
    }

    #[test]
    fn falls_back_when_no_compressed_copy_exists() {
        let only_plain = Encoded {
            identity: b"plain",
            gzip: b"",
            brotli: b"",
        };
        let mut headers = HeaderMap::new();
        headers.insert(
            header::ACCEPT_ENCODING,
            HeaderValue::from_static("br, gzip"),
        );
        let response = encoded("text/javascript", &only_plain, &headers);
        assert!(response.headers().get(header::CONTENT_ENCODING).is_none());
    }
}
