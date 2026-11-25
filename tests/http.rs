use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
    response::Response,
};
use serde_json::{Value, json};
use shortcircuit::{AdminKey, PublicHosts, admin_router, open_database, public_router};
use tower::ServiceExt;

const TOKEN: &str = "512f19995ff89f5fb6f8a792773dd8ff2e81d67fe754fdc20378198589c87ac5";
const TARGET: &str = "https://example.test/file?sig=XL%2f&key=A+B&key=C#original";

struct App {
    _temp: tempfile::TempDir,
    database: std::path::PathBuf,
    pool: sqlx::SqlitePool,
    public: Router,
    admin: Router,
}

impl App {
    async fn start() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let database = temp.path().join("test.sqlite3");
        let pool = open_database(&database).await.unwrap();
        let hosts = PublicHosts::parse("go.example,s.example").unwrap();
        let public = public_router(pool.clone(), hosts.clone());
        let admin = admin_router(pool.clone(), hosts, Some(AdminKey::parse(TOKEN).unwrap()));
        Self {
            _temp: temp,
            database,
            pool,
            public,
            admin,
        }
    }

    async fn create(&self, body: Value, authenticated: bool) -> Response {
        let mut request = Request::post("/api/v1/links").header(header::CONTENT_TYPE, "application/json");
        if authenticated {
            request = request.header(header::AUTHORIZATION, format!("Bearer {TOKEN}"));
        }
        self.admin
            .clone()
            .oneshot(request.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap()
    }
}

async fn response_json(response: Response) -> Value {
    let body = to_bytes(response.into_body(), 32_768).await.unwrap();
    serde_json::from_slice(&body).unwrap()
}

async fn redirect(router: &Router, method: &str, url: &str, host: &str) -> Response {
    router
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(url)
                .header(header::HOST, host)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn admin_creation_requires_a_key_and_public_cannot_create() {
    let app = App::start().await;
    let body = json!({"target": TARGET});
    assert_eq!(app.create(body.clone(), false).await.status(), StatusCode::UNAUTHORIZED);
    let created = app.create(body, true).await;
    assert_eq!(created.status(), StatusCode::CREATED);
    assert_eq!(created.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(response_json(created).await["slug"].as_str().unwrap().len(), 10);

    let response = redirect(&app.public, "POST", "/api/v1/links", "go.example").await;
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(response.headers()[header::ALLOW], "GET, HEAD");
    let disabled_admin = admin_router(app.pool.clone(), PublicHosts::parse("go.example").unwrap(), None);
    assert_eq!(
        redirect(&disabled_admin, "GET", "/api/v1/links", "go.example").await.status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn public_redirect_preserves_the_stored_location_across_restarts() {
    let app = App::start().await;
    assert_eq!(
        app.create(json!({"slug":"Team/Docs","target":TARGET}), true).await.status(),
        StatusCode::CREATED
    );
    app.pool.close().await;
    let fresh_pool = open_database(&app.database).await.unwrap();
    let public = public_router(fresh_pool, PublicHosts::parse("go.example,s.example").unwrap());

    for method in ["GET", "HEAD"] {
        let result = redirect(&public, method, "/Team/Docs?target=https://wrong.example", "GO.EXAMPLE:8080").await;
        assert_eq!(result.status(), StatusCode::FOUND);
        assert_eq!(result.headers()[header::LOCATION], TARGET);
        assert_eq!(result.headers()[header::CACHE_CONTROL], "no-store");
        assert!(to_bytes(result.into_body(), 1024).await.unwrap().is_empty());
    }
    for (url, host) in [
        ("/team/docs", "go.example"),
        ("/Team/Docs/extra", "go.example"),
        ("/Team%2FDocs", "go.example"),
        ("/Team/Docs", "s.example"),
        ("/Team/Docs", "other.example"),
        ("http://other.example/Team/Docs", "go.example"),
    ] {
        let result = redirect(&public, "GET", url, host).await;
        assert_eq!(result.status(), StatusCode::NOT_FOUND);
        assert!(!result.headers().contains_key(header::LOCATION));
    }
}

#[tokio::test]
async fn collisions_are_scoped_to_a_host_and_invalid_input_is_rejected() {
    let app = App::start().await;
    let input = json!({"slug": "docs", "target": TARGET});
    assert_eq!(app.create(input.clone(), true).await.status(), StatusCode::CREATED);
    assert_eq!(app.create(input, true).await.status(), StatusCode::CONFLICT);
    assert_eq!(
        app.create(json!({"host":"s.example","slug":"docs","target":"http://home.internal/"}), true).await.status(),
        StatusCode::CREATED
    );
    for input in [
        json!({"slug":"a//b","target":TARGET}),
        json!({"target":"ftp://example.test/file"}),
        json!({"target":"https://user:pass@example.test/file"}),
        json!({"target":"https://example.test/\r\nLocation: https://wrong.example"}),
        json!({"target":"https://example.test/%zz"}),
        json!({"host":"wrong.example","target":TARGET}),
        json!({"target":TARGET,"status":302}),
    ] {
        assert_eq!(app.create(input, true).await.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }
}

#[tokio::test]
async fn diagnostics_are_admin_only_and_readiness_checks_the_store() {
    let app = App::start().await;
    assert_eq!(redirect(&app.admin, "GET", "/healthz", "localhost").await.status(), StatusCode::OK);
    assert_eq!(redirect(&app.admin, "GET", "/readyz", "localhost").await.status(), StatusCode::OK);
    assert_eq!(redirect(&app.public, "GET", "/healthz", "go.example").await.status(), StatusCode::NOT_FOUND);
    app.pool.close().await;
    assert_eq!(redirect(&app.admin, "GET", "/readyz", "localhost").await.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn admin_list_is_authenticated_and_respects_configured_hosts() {
    let app = App::start().await;
    assert_eq!(redirect(&app.admin, "GET", "/api/v1/links", "localhost").await.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(app.create(json!({"slug":"one","target":TARGET}), true).await.status(), StatusCode::CREATED);
    sqlx::raw_sql(
        "INSERT INTO links(host, slug, target)
         VALUES ('retired.example', 'hidden', 'https://example.test/');
         WITH RECURSIVE numbers(n) AS (VALUES (1) UNION ALL SELECT n + 1 FROM numbers WHERE n < 100)
         INSERT INTO links(host, slug, target)
         SELECT 'go.example', 'link-' || n, 'https://example.test/' FROM numbers;",
    )
    .execute(&app.pool)
    .await
    .unwrap();

    let list = authenticated_list(&app.admin, "/api/v1/links").await;
    assert_eq!(list["items"][0]["slug"], "one");
    assert_eq!(list["items"][0]["target"], TARGET);
    let items = list["items"].as_array().unwrap();
    assert_eq!(items.len(), 100);
    assert!(items.iter().all(|item| item["host"] == "go.example"));
    let cursor = list["next_after_id"].as_i64().unwrap();
    let remainder = authenticated_list(&app.admin, &format!("/api/v1/links?after_id={cursor}")).await;
    assert_eq!(remainder["items"].as_array().unwrap().len(), 1);
    assert_eq!(remainder["items"][0]["slug"], "link-100");
    assert!(remainder["next_after_id"].is_null());
}

async fn authenticated_list(admin: &Router, path: &str) -> Value {
    let request = Request::get(path)
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let response = admin.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    response_json(response).await
}

#[tokio::test]
async fn a_commit_failure_does_not_report_a_created_link() {
    let app = App::start().await;
    sqlx::raw_sql(
        "CREATE TABLE parents(id INTEGER PRIMARY KEY);
         CREATE TABLE children(link_id INTEGER REFERENCES parents(id) DEFERRABLE INITIALLY DEFERRED);
         CREATE TRIGGER insert_child AFTER INSERT ON links
         BEGIN INSERT INTO children(link_id) VALUES (NEW.id); END;",
    )
    .execute(&app.pool)
    .await
    .unwrap();
    let result = app.create(json!({"slug":"test","target":TARGET}), true).await;
    assert_eq!(result.status(), StatusCode::SERVICE_UNAVAILABLE);
    let stored: i64 = sqlx::query_scalar("SELECT count(*) FROM links")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(stored, 0);
}

#[tokio::test]
async fn oversized_body_and_uri_are_rejected_and_forwarded_host_is_ignored() {
    let app = App::start().await;
    let response = app.create(json!({"target": "a".repeat(33_000)}), true).await;
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);

    let oversized = format!("/docs?value={}", "a".repeat(8200));
    let response = redirect(&app.public, "GET", &oversized, "go.example").await;
    assert_eq!(response.status(), StatusCode::URI_TOO_LONG);

    let result = app.create(json!({"slug":"docs","target":TARGET}), true).await;
    assert_eq!(result.status(), StatusCode::CREATED);
    let response = app.public.oneshot(
        Request::get("/docs")
            .header(header::HOST, "outside.example")
            .header("x-forwarded-host", "go.example")
            .body(Body::empty())
            .unwrap(),
    ).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
