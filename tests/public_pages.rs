//! Settings > Public pages: administrator-defined visitor pages on the public listener.

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Method, Request, StatusCode, header},
    response::Response,
};
use serde_json::{Value, json};
use shortcircuit::{
    AdminConfig, Auth, BootstrapKind, PublicHosts, admin_router, open_database, public_router,
};
use sqlx::SqlitePool;
use tower::ServiceExt;

const ORIGIN: &str = "https://admin.example";
const ADMIN_HOST: &str = "admin.example";
const PAGES: &str = "/api/v1/settings/public-pages";
const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

struct App {
    _temp: tempfile::TempDir,
    admin: Router,
    public: Router,
    pool: SqlitePool,
    cookie: String,
    csrf: String,
}

async fn send(
    router: &Router,
    method: Method,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<Value>,
) -> Response {
    let mut builder = Request::builder().method(method).uri(path);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    let body = match body {
        Some(body) => {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            Body::from(body.to_string())
        }
        None => Body::empty(),
    };
    router
        .clone()
        .oneshot(builder.body(body).unwrap())
        .await
        .unwrap()
}

async fn bytes(response: Response) -> Vec<u8> {
    to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap()
        .to_vec()
}

async fn json_body(response: Response) -> Value {
    serde_json::from_slice(&bytes(response).await).unwrap()
}

async fn text(response: Response) -> String {
    String::from_utf8(bytes(response).await).unwrap()
}

impl App {
    async fn start() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let pool = open_database(&temp.path().join("pages.db")).await.unwrap();
        let hosts = PublicHosts::parse("go.example,localhost").unwrap();
        let admin = admin_router(
            pool.clone(),
            hosts.clone(),
            AdminConfig::new(ORIGIN, 8080).unwrap(),
        );
        let public = public_router(pool.clone(), hosts);
        let token = Auth::new(pool.clone())
            .issue_bootstrap(BootstrapKind::Setup)
            .await
            .unwrap();
        let response = send(
            &admin,
            Method::POST,
            "/api/v1/auth/setup",
            &[("host", ADMIN_HOST), ("origin", ORIGIN)],
            Some(json!({"token": token, "username": "Owner", "password": "synthetic-admin-password"})),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CREATED);
        let cookie = response.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        let csrf = json_body(response).await["csrf"]
            .as_str()
            .unwrap()
            .to_owned();
        sqlx::raw_sql(
            "INSERT INTO links (id, host, slug, target, enabled) VALUES
                (1, 'go.example', 'paused', 'https://example.test/paused', 0),
                (2, 'go.example', 'live', 'https://example.test/live', 1),
                (3, 'localhost', 'once', 'https://example.test/once', 1),
                (4, 'localhost', 'secret', 'https://example.test/secret', 1);
             INSERT INTO link_protection (link_id, one_time) VALUES (3, 1);",
        )
        .execute(&pool)
        .await
        .unwrap();
        Self {
            _temp: temp,
            admin,
            public,
            pool,
            cookie,
            csrf,
        }
    }

    async fn api(&self, method: Method, path: &str, body: Option<Value>) -> Response {
        let headers = [
            ("host", ADMIN_HOST),
            ("cookie", self.cookie.as_str()),
            ("origin", ORIGIN),
            ("x-csrf-token", self.csrf.as_str()),
        ];
        send(&self.admin, method, path, &headers, body).await
    }

    async fn visit(
        &self,
        method: Method,
        host: &str,
        path: &str,
        language: Option<&str>,
    ) -> Response {
        let mut headers = vec![("host", host)];
        if let Some(language) = language {
            headers.push(("accept-language", language));
        }
        send(&self.public, method, path, &headers, None).await
    }

    async fn audit(&self, action: &str) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM admin_audit WHERE action = ?")
            .bind(action)
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }
}

fn policy(headers: &HeaderMap) -> String {
    headers[header::CONTENT_SECURITY_POLICY]
        .to_str()
        .unwrap()
        .to_owned()
}

#[tokio::test]
async fn structured_pages_replace_the_copy_and_keep_missing_and_inactive_identical() {
    let app = App::start().await;
    let builtin = app.visit(Method::GET, "go.example", "/nope", None).await;
    let builtin_policy = policy(builtin.headers());
    assert!(
        text(builtin)
            .await
            .contains("This short link doesn’t exist")
    );

    let saved = app
        .api(
            Method::PATCH,
            PAGES,
            Some(json!({
                "page": "missing", "lang": "en", "mode": "structured",
                "title": "Gone <for> now", "message": "Line one\nLine two", "accent": "#3366CC",
                "logo": {"type": "image/png", "data": PNG}
            })),
        )
        .await;
    assert_eq!(saved.status(), StatusCode::OK);
    assert_eq!(json_body(saved).await["page"]["accent"], "#3366cc");
    assert_eq!(app.audit("public_pages_update").await, 1);

    let mut bodies = Vec::new();
    for path in ["/nope", "/paused", "/also/missing"] {
        let response = app.visit(Method::GET, "go.example", path, None).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
        let headers = response.headers().clone();
        let csp = policy(&headers);
        assert!(
            csp.starts_with("default-src 'none'; style-src 'sha256-"),
            "{csp}"
        );
        assert!(csp.contains(
            "img-src data:; form-action 'self'; base-uri 'none'; frame-ancestors 'none'"
        ));
        assert!(!csp.contains("script-src") && !csp.contains("unsafe"));
        assert_ne!(csp, builtin_policy);
        assert_eq!(headers[header::X_FRAME_OPTIONS], "DENY");
        assert_eq!(headers[header::CACHE_CONTROL], "no-store");
        bodies.push((headers, bytes(response).await));
    }
    // Missing and inactive links answer byte for byte the same.
    assert_eq!(bodies[0].1, bodies[1].1);
    assert_eq!(bodies[0].0, bodies[1].0);
    let page = String::from_utf8(bodies[0].1.clone()).unwrap();
    assert!(page.contains("<h1>Gone &lt;for&gt; now</h1>"));
    assert!(page.contains("Line one<br>Line two"));
    assert!(page.contains("<img class=\"logo\" src=\"data:image/png;base64,"));
    assert!(page.contains("--accent:#3366cc"));
    assert!(!page.contains("paused") && !page.contains("nope"));

    // German keeps the built-in page, HEAD stays bodiless, and redirects are unchanged.
    let german = app
        .visit(Method::GET, "go.example", "/paused", Some("de-DE,de;q=0.9"))
        .await;
    assert_eq!(policy(german.headers()), builtin_policy);
    assert!(text(german).await.contains("Diesen Kurzlink gibt es nicht"));
    let head = app.visit(Method::HEAD, "go.example", "/paused", None).await;
    assert_eq!(head.status(), StatusCode::NOT_FOUND);
    assert!(
        head.headers()
            .get(header::CONTENT_SECURITY_POLICY)
            .is_none()
    );
    assert!(bytes(head).await.is_empty());
    let live = app.visit(Method::GET, "go.example", "/live", None).await;
    assert_eq!(live.status(), StatusCode::FOUND);

    // Reset returns the built-in page.
    let reset = app
        .api(
            Method::POST,
            &format!("{PAGES}/reset"),
            Some(json!({"page": "missing", "lang": "en"})),
        )
        .await;
    assert_eq!(reset.status(), StatusCode::OK);
    assert_eq!(app.audit("public_pages_reset").await, 1);
    let restored = app.visit(Method::GET, "go.example", "/paused", None).await;
    assert_eq!(policy(restored.headers()), builtin_policy);
    assert!(
        text(restored)
            .await
            .contains("This short link doesn’t exist")
    );
}

#[tokio::test]
async fn custom_pages_are_sanitized_escaped_and_never_reflect_the_path() {
    let app = App::start().await;
    let html = "<title>{{host}} missing</title><style>h1{color:#123456}</style>\
        <h1 onclick=\"steal()\">Nothing at {{host}}</h1><script>alert(1)</script>\
        <a href=\"javascript:alert(1)\">bad</a><iframe src=\"https://evil.example\"></iframe>\
        <meta http-equiv=\"refresh\" content=\"0;url=https://evil.example\">\
        <p lang=\"{{lang}}\">{{root_link}}</p>";
    let saved = app
        .api(
            Method::PATCH,
            PAGES,
            Some(json!({"page": "unknown_host", "lang": "en", "mode": "custom", "html": html})),
        )
        .await;
    assert_eq!(saved.status(), StatusCode::OK);
    let saved = json_body(saved).await;
    let removed: Vec<_> = saved["removed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    for category in ["event_handler", "frame", "meta", "script", "unsafe_url"] {
        assert!(removed.contains(&category), "{category}: {removed:?}");
    }
    let stored = saved["page"]["html"].as_str().unwrap();
    assert!(
        !stored.contains("script") && !stored.contains("onclick") && !stored.contains("iframe")
    );

    // The Host header is untrusted on the unknown-host page and is escaped.
    let response = app
        .visit(Method::GET, "evil\"<b>.example", "/x/secret", None)
        .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let csp = policy(response.headers());
    let page = text(response).await;
    assert!(!page.contains("<b>") && !page.contains("secret"));
    let response = app
        .visit(Method::GET, "unknown.example", "/secret-slug", None)
        .await;
    let page = text(response).await;
    assert!(
        page.contains("<title>unknown.example missing</title>"),
        "{page}"
    );
    assert!(page.contains("<h1>Nothing at unknown.example</h1>"));
    assert!(
        page.contains("<p lang=\"en\"></p>"),
        "the root link renders only on the missing page: {page}"
    );
    assert!(page.contains("<style>h1{color:#123456}</style>"));
    assert!(
        !page.contains("secret-slug") && !page.contains("<script") && !page.contains("http-equiv")
    );
    assert_eq!(csp.matches("'sha256-").count(), 1);
    assert!(csp.ends_with("frame-ancestors 'none'"));

    // The root link placeholder renders only while the root destination redirects.
    let missing = "<h1>Gone</h1>{{root_link}}";
    let saved = app
        .api(
            Method::PATCH,
            PAGES,
            Some(json!({"page": "missing", "lang": "en", "mode": "custom", "html": missing})),
        )
        .await;
    assert_eq!(saved.status(), StatusCode::OK);
    let page = text(app.visit(Method::GET, "go.example", "/nope", None).await).await;
    assert!(
        page.ends_with("<body><h1>Gone</h1></body></html>"),
        "{page}"
    );
    sqlx::query("INSERT INTO domains (host, origin, enabled, root_target) VALUES ('go.example', 'https://go.example', 1, 'https://root.example/')")
        .execute(&app.pool)
        .await
        .unwrap();
    let nope = text(app.visit(Method::GET, "go.example", "/nope", None).await).await;
    let paused = text(app.visit(Method::GET, "go.example", "/paused", None).await).await;
    assert_eq!(nope, paused);
    assert!(
        nope.contains("<a class=\"btn\" href=\"https://go.example/\">Go to root.example</a>"),
        "{nope}"
    );
}

#[tokio::test]
async fn challenge_pages_require_the_server_form_and_keep_its_mechanics() {
    let app = App::start().await;
    for (page, html) in [
        ("once", "<h1>Once</h1>"),
        ("password", "<h1>Pw</h1>{{form}}{{form}}"),
    ] {
        let refused = app
            .api(
                Method::PATCH,
                PAGES,
                Some(json!({"page": page, "lang": "en", "mode": "custom", "html": html})),
            )
            .await;
        assert_eq!(refused.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }
    let refused = app
        .api(
            Method::PATCH,
            PAGES,
            Some(json!({"page": "missing", "lang": "en", "mode": "custom", "html": "{{form}}"})),
        )
        .await;
    assert_eq!(json_body(refused).await["code"], "form_not_supported");
    let refused = app
        .api(
            Method::PATCH,
            PAGES,
            Some(json!({"page": "once", "lang": "en", "mode": "custom", "html": "<h1>x</h1>"})),
        )
        .await;
    assert_eq!(json_body(refused).await["code"], "form_required");
    assert_eq!(app.audit("public_pages_update").await, 0);

    let html =
        "<style>form{display:grid}</style><main><h1>One visit at {{host}}</h1>{{form}}</main>";
    let saved = app
        .api(
            Method::PATCH,
            PAGES,
            Some(json!({"page": "once", "lang": "en", "mode": "custom", "html": html})),
        )
        .await;
    assert_eq!(saved.status(), StatusCode::OK);
    let response = app.visit(Method::GET, "localhost", "/once", None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::REFERRER_POLICY], "strict-origin");
    assert!(policy(response.headers()).contains("form-action 'self'"));
    let page = text(response).await;
    assert!(page.contains("<h1>One visit at localhost</h1><form method=\"post\" action=\"http://localhost:8080/once\">"), "{page}");
    let challenge = page
        .split("name=\"challenge\" value=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .to_owned();
    // The signed challenge from the custom page confirms the visit exactly like the built-in one.
    let confirm = Request::builder()
        .method(Method::POST)
        .uri("/once")
        .header("host", "localhost")
        .header("origin", "http://localhost:8080")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(format!("challenge={challenge}")))
        .unwrap();
    let mut confirm = confirm;
    confirm
        .extensions_mut()
        .insert(axum::extract::ConnectInfo(std::net::SocketAddr::from((
            [127, 0, 0, 1],
            4000,
        ))));
    let confirmed = app.public.clone().oneshot(confirm).await.unwrap();
    assert_eq!(confirmed.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        confirmed.headers()[header::LOCATION],
        "https://example.test/once"
    );
    // Used up, the link now answers like any missing link.
    let used = app.visit(Method::GET, "localhost", "/once", None).await;
    assert_eq!(used.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_broken_stored_template_falls_back_to_the_built_in_page() {
    let app = App::start().await;
    // Rows written around the API: a password page without its form and a missing page whose
    // markup a newer sanitizer refuses. Neither may reach visitors.
    sqlx::raw_sql(
        "INSERT INTO public_pages (page, lang, mode, html) VALUES
            ('password', 'en', 'custom', '<h1>No form</h1>'),
            ('missing', 'en', 'custom', '<style>a{background:url(https://evil.example/x)}</style><h1>x</h1>'),
            ('unknown_host', 'en', 'custom', '<h1 onclick=\"x()\">Kept</h1><script>alert(1)</script>');",
    )
    .execute(&app.pool)
    .await
    .unwrap();
    let missing = app.visit(Method::GET, "go.example", "/nope", None).await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    let csp = policy(missing.headers());
    assert!(
        !csp.contains("img-src"),
        "the built-in policy applies: {csp}"
    );
    assert!(
        text(missing)
            .await
            .contains("This short link doesn’t exist")
    );
    // Stored markup is sanitized again before it is served.
    let unknown = text(app.visit(Method::GET, "other.example", "/", None).await).await;
    assert!(
        unknown.contains("<h1>Kept</h1>")
            && !unknown.contains("script")
            && !unknown.contains("onclick")
    );
}

#[tokio::test]
async fn api_keys_cannot_read_or_change_public_pages() {
    let app = App::start().await;
    let key = app
        .api(
            Method::POST,
            "/api/v1/keys",
            Some(json!({"name": "Writer", "scopes": ["read", "write", "delete"]})),
        )
        .await;
    assert_eq!(key.status(), StatusCode::CREATED);
    let bearer = format!("Bearer {}", json_body(key).await["token"].as_str().unwrap());
    let draft = json!({"page": "missing", "lang": "en", "mode": "structured", "title": "x"});
    let bundle = json!({"format": "shortcircuit-public-pages", "version": 1, "pages": [draft]});
    for (method, path, body) in [
        (Method::GET, PAGES.to_owned(), None),
        (Method::PATCH, PAGES.to_owned(), Some(draft.clone())),
        (
            Method::POST,
            format!("{PAGES}/preview"),
            Some(draft.clone()),
        ),
        (
            Method::POST,
            format!("{PAGES}/reset"),
            Some(json!({"all": true})),
        ),
        (
            Method::POST,
            format!("{PAGES}/import"),
            Some(bundle.clone()),
        ),
        (
            Method::POST,
            format!("{PAGES}/import/validate"),
            Some(bundle),
        ),
        (Method::GET, format!("{PAGES}/export"), None),
    ] {
        let response = send(
            &app.admin,
            method.clone(),
            &path,
            &[("host", ADMIN_HOST), ("authorization", bearer.as_str())],
            body,
        )
        .await;
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "{method} {path}"
        );
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM public_pages")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn imports_are_validated_previewed_and_applied_as_a_whole() {
    let app = App::start().await;
    for (body, code) in [
        (
            json!({"format": "other", "version": 1, "pages": []}),
            "invalid_format",
        ),
        (
            json!({"format": "shortcircuit-public-pages", "version": 2, "pages": []}),
            "unsupported_version",
        ),
        (
            json!({"format": "shortcircuit-public-pages", "version": 1}),
            "invalid_format",
        ),
        (
            json!({"format": "shortcircuit-public-pages", "version": 1, "pages": [], "extra": 1}),
            "invalid_format",
        ),
    ] {
        let response = app
            .api(
                Method::POST,
                &format!("{PAGES}/import/validate"),
                Some(body),
            )
            .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(json_body(response).await["code"], code);
    }
    let large = "a".repeat(64 * 1024 + 1);
    let oversized = json!({"format": "shortcircuit-public-pages", "version": 1, "pages": [
        {"page": "missing", "lang": "en", "mode": "structured", "title": "Fine"},
        {"page": "missing", "lang": "de", "mode": "custom", "html": large}
    ]});
    let response = app
        .api(
            Method::POST,
            &format!("{PAGES}/import/validate"),
            Some(oversized),
        )
        .await;
    let problem = json_body(response).await;
    assert_eq!(
        (
            problem["code"].as_str(),
            problem["row"].as_u64(),
            problem["field"].as_str()
        ),
        (Some("page_too_large"), Some(2), Some("html"))
    );

    let presets = json_body(
        app.api(Method::GET, &format!("{PAGES}/presets"), None)
            .await,
    )
    .await;
    let minimal = presets["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == "minimal")
        .unwrap()["bundle"]
        .clone();
    let report = app
        .api(
            Method::POST,
            &format!("{PAGES}/import/validate"),
            Some(minimal.clone()),
        )
        .await;
    assert_eq!(report.status(), StatusCode::OK);
    let report = json_body(report).await;
    assert_eq!(report["pages"].as_array().unwrap().len(), 8);
    // Validation changes nothing.
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM public_pages")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    let preview_path = report["pages"][0]["preview"].as_str().unwrap().to_owned();
    let preview = app.api(Method::GET, &preview_path, None).await;
    assert_eq!(
        preview.status(),
        StatusCode::NOT_FOUND,
        "the missing page keeps its status"
    );
    let csp = policy(preview.headers());
    assert!(csp.ends_with("frame-ancestors 'self'"), "{csp}");
    assert!(csp.contains("default-src 'none'") && !csp.contains("script-src"));
    assert_eq!(preview.headers()[header::X_FRAME_OPTIONS], "SAMEORIGIN");
    // A preview is not reachable without the session.
    let anonymous = send(
        &app.admin,
        Method::GET,
        &preview_path,
        &[("host", ADMIN_HOST)],
        None,
    )
    .await;
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);

    let applied = app
        .api(Method::POST, &format!("{PAGES}/import"), Some(minimal))
        .await;
    assert_eq!(applied.status(), StatusCode::OK);
    assert_eq!(
        json_body(applied).await["pages"].as_array().unwrap().len(),
        8
    );
    assert_eq!(app.audit("public_pages_import").await, 1);
    let page = text(
        app.visit(Method::GET, "go.example", "/nope", Some("de"))
            .await,
    )
    .await;
    assert!(page.contains("Link nicht gefunden"));

    let export = app.api(Method::GET, &format!("{PAGES}/export"), None).await;
    assert!(
        export.headers()[header::CONTENT_DISPOSITION]
            .to_str()
            .unwrap()
            .starts_with("attachment")
    );
    let export = json_body(export).await;
    assert_eq!(export["format"], "shortcircuit-public-pages");
    assert_eq!(export["pages"].as_array().unwrap().len(), 8);

    let reset = app
        .api(
            Method::POST,
            &format!("{PAGES}/reset"),
            Some(json!({"all": true})),
        )
        .await;
    assert_eq!(json_body(reset).await["pages"], json!([]));
    let page = text(
        app.visit(Method::GET, "go.example", "/nope", Some("de"))
            .await,
    )
    .await;
    assert!(page.contains("Diesen Kurzlink gibt es nicht"));
}

#[tokio::test]
async fn the_admin_shell_may_frame_only_its_own_previews() {
    let app = App::start().await;
    let index = send(&app.admin, Method::GET, "/", &[("host", ADMIN_HOST)], None).await;
    let csp = policy(index.headers());
    assert!(csp.contains("frame-src 'self'") && csp.contains("frame-ancestors 'none'"));
    assert_eq!(index.headers()[header::X_FRAME_OPTIONS], "DENY");
    let created = app
        .api(
            Method::POST,
            &format!("{PAGES}/preview"),
            Some(
                json!({"page": "password", "lang": "de", "mode": "structured", "button": "Öffnen"}),
            ),
        )
        .await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let url = json_body(created).await["url"].as_str().unwrap().to_owned();
    let preview = app.api(Method::GET, &url, None).await;
    assert_eq!(preview.status(), StatusCode::OK);
    assert_eq!(preview.headers()[header::X_FRAME_OPTIONS], "SAMEORIGIN");
    let page = text(preview).await;
    assert!(page.contains(">Öffnen</button>") && page.contains("lang=\"de\""));
    let unknown = app
        .api(
            Method::GET,
            &format!("{PAGES}/preview/0123456789abcdef0123456789abcdef"),
            None,
        )
        .await;
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
    // A preview never changes what visitors see.
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM public_pages")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}
