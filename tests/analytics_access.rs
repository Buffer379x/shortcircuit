//! Authorization matrix for analytics reports and the dedicated analytics key scope.

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
    response::Response,
};
use serde_json::{Value, json};
use shortcircuit::{
    AdminConfig, Analytics, Auth, BootstrapKind, PublicHosts, admin_router_with_analytics,
    open_database,
};
use sqlx::SqlitePool;
use tower::ServiceExt;

const ORIGIN: &str = "https://admin.example";
const PASSWORD: &str = "synthetic-test-password";

struct Harness {
    _directory: tempfile::TempDir,
    admin: Router,
    pool: SqlitePool,
    cookie: String,
    csrf: String,
}

async fn send(
    router: &Router,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<Value>,
) -> Response {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("host", "admin.example");
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let body = match body {
        Some(value) => {
            request = request.header("content-type", "application/json");
            Body::from(value.to_string())
        }
        None => Body::empty(),
    };
    router
        .clone()
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap()
}

async fn json_body(response: Response) -> Value {
    serde_json::from_slice(&to_bytes(response.into_body(), 200_000).await.unwrap()).unwrap()
}

impl Harness {
    async fn start() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let pool = open_database(&directory.path().join("reports.db"))
            .await
            .unwrap();
        let auth = Auth::new(pool.clone());
        let setup = auth.issue_bootstrap(BootstrapKind::Setup).await.unwrap();
        let session = auth.setup(&setup, "Operator", PASSWORD).await.unwrap();
        let (analytics, _writer) = Analytics::start(pool.clone()).await.unwrap();
        let admin = admin_router_with_analytics(
            pool.clone(),
            PublicHosts::parse("go.example,s.example").unwrap(),
            AdminConfig::new(ORIGIN, 8080).unwrap(),
            analytics,
        );
        sqlx::raw_sql(
            "INSERT INTO links (id, host, slug, target, title) VALUES
                (1, 'go.example', 'alpha', 'https://example.test/a', 'Alpha'),
                (2, 's.example', 'beta', 'https://example.test/b', 'Beta');
             INSERT INTO link_tags (link_id, tag) VALUES (1, 'go-only'), (2, 's-only');
             INSERT INTO analytics_daily (day, host, link_id, kind, visit_class, browser, total) VALUES
                (unixepoch() / 86400, 'go.example', 1, 'redirect', 'human', 'firefox', 7),
                (unixepoch() / 86400 - 3, 'go.example', 1, 'redirect', 'human', 'firefox', 70),
                (unixepoch() / 86400, 'go.example', 0, 'missing', 'unknown', 'unknown', 5),
                (unixepoch() / 86400, 's.example', 2, 'redirect', 'human', 'safari', 2),
                (unixepoch() / 86400 - 3, 's.example', 2, 'redirect', 'human', 'safari', 20),
                (unixepoch() / 86400, 's.example', 0, 'blocked', 'unknown', 'unknown', 1);",
        )
        .execute(&pool)
        .await
        .unwrap();
        Self {
            _directory: directory,
            admin,
            pool,
            cookie: format!("__Host-shortcircuit_session={}", session.token),
            csrf: session.session.csrf,
        }
    }

    fn browser(&self) -> [(&str, &str); 3] {
        [
            ("cookie", self.cookie.as_str()),
            ("origin", ORIGIN),
            ("x-csrf-token", self.csrf.as_str()),
        ]
    }

    async fn key(&self, scopes: &[&str], host: Option<&str>) -> (i64, String) {
        let mut input = json!({"name": "Report key", "scopes": scopes});
        if let Some(host) = host {
            input["host"] = json!(host);
        }
        let response = send(
            &self.admin,
            "POST",
            "/api/v1/keys",
            &self.browser(),
            Some(input),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CREATED);
        let created = json_body(response).await;
        (
            created["key"]["id"].as_i64().unwrap(),
            format!("Bearer {}", created["token"].as_str().unwrap()),
        )
    }
}

#[tokio::test]
async fn analytics_scope_reads_reports_only_within_its_host() {
    let app = Harness::start().await;
    let summary = "/api/v1/analytics/summary?days=7";
    let export = "/api/v1/analytics/export?days=1";

    // Browser sessions keep the instance-wide report.
    let response = send(&app.admin, "GET", summary, &[("cookie", &app.cookie)], None).await;
    assert_eq!(response.status(), StatusCode::OK);
    let instance = json_body(response).await;
    assert_eq!(instance["totals"]["redirect"], 99);
    assert_eq!(instance["dropped"], 0);

    // An ordinary link key cannot read analytics, even for its own host.
    let (_, reader) = app.key(&["read", "write", "delete"], None).await;
    for path in [summary, export] {
        let response = send(&app.admin, "GET", path, &[("authorization", &reader)], None).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{path}");
        assert_eq!(json_body(response).await["code"], "insufficient_scope");
    }

    // An unrestricted analytics key reads the same instance-wide report.
    let (_, unrestricted) = app.key(&["analytics"], None).await;
    let response = send(
        &app.admin,
        "GET",
        summary,
        &[("authorization", &unrestricted)],
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await, instance);

    // A host-bound analytics key sees only its own host in every section.
    let (bound_id, bound) = app.key(&["analytics"], Some("s.example")).await;
    let headers = [("authorization", bound.as_str())];
    let response = send(&app.admin, "GET", summary, &headers, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    let scoped = json_body(response).await;
    assert_eq!(scoped["host"], "s.example");
    assert_eq!(scoped["totals"]["redirect"], 22);
    assert_eq!(scoped["totals"]["blocked"], 1);
    assert_eq!(scoped["totals"]["missing"], 0);
    assert_eq!(scoped["dropped"], Value::Null);
    assert_eq!(
        scoped["top_domains"],
        json!([{"host": "s.example", "total": 22}])
    );
    assert_eq!(scoped["top_tags"], json!([{"tag": "s-only", "total": 22}]));
    assert_eq!(scoped["top_links"].as_array().unwrap().len(), 1);
    assert_eq!(scoped["top_links"][0]["slug"], "beta");
    assert_eq!(
        scoped["breakdowns"]["browser"],
        json!([{"category": "safari", "total": 22}])
    );
    let text = scoped.to_string();
    for private in ["go.example", "alpha", "Alpha", "go-only", "firefox"] {
        assert!(!text.contains(private), "{private}");
    }
    let previous = send(
        &app.admin,
        "GET",
        "/api/v1/analytics/summary?days=2",
        &headers,
        None,
    )
    .await;
    assert_eq!(json_body(previous).await["previous_redirects"], 20);

    let same_host = send(
        &app.admin,
        "GET",
        "/api/v1/analytics/summary?days=7&host=S.EXAMPLE",
        &headers,
        None,
    )
    .await;
    assert_eq!(same_host.status(), StatusCode::OK);
    for path in [
        "/api/v1/analytics/summary?days=7&host=go.example",
        "/api/v1/analytics/export?days=7&host=go.example",
    ] {
        let response = send(&app.admin, "GET", path, &headers, None).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{path}");
    }
    let response = send(&app.admin, "GET", export, &headers, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    let exported = json_body(response).await;
    assert_eq!(exported["report"]["host"], "s.example");
    assert_eq!(exported["report"]["totals"]["redirect"], 2);
    assert_eq!(exported["report"]["dropped"], Value::Null);

    let traffic = send(
        &app.admin,
        "GET",
        "/api/v1/analytics/traffic",
        &headers,
        None,
    )
    .await;
    assert_eq!(traffic.status(), StatusCode::OK);
    assert_eq!(json_body(traffic).await["host"], "s.example");
    let traffic = send(
        &app.admin,
        "GET",
        "/api/v1/analytics/traffic?host=go.example",
        &headers,
        None,
    )
    .await;
    assert_eq!(traffic.status(), StatusCode::FORBIDDEN);
    let traffic = send(
        &app.admin,
        "GET",
        "/api/v1/analytics/traffic",
        &[("authorization", &reader)],
        None,
    )
    .await;
    assert_eq!(traffic.status(), StatusCode::FORBIDDEN);

    // The analytics scope does not grant link access, settings, visit details, or deletion.
    for (method, path) in [("GET", "/api/v1/links"), ("GET", "/api/v1/domains")] {
        let response = send(&app.admin, method, path, &headers, None).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{path}");
    }
    for (method, path) in [
        ("GET", "/api/v1/analytics/settings"),
        ("PATCH", "/api/v1/analytics/settings"),
        ("GET", "/api/v1/analytics/events"),
        ("DELETE", "/api/v1/analytics/events/1"),
        ("DELETE", "/api/v1/analytics/data"),
        ("GET", "/api/v1/keys"),
    ] {
        let response = send(&app.admin, method, path, &headers, None).await;
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "{method} {path}"
        );
    }
    // Mixed credentials are ambiguous and rejected.
    let mixed = send(
        &app.admin,
        "GET",
        summary,
        &[("authorization", &bound), ("cookie", &app.cookie)],
        None,
    )
    .await;
    assert_eq!(mixed.status(), StatusCode::UNAUTHORIZED);

    // Rotation keeps the scope and host binding.
    let response = send(
        &app.admin,
        "POST",
        &format!("/api/v1/keys/{bound_id}/rotate"),
        &app.browser(),
        Some(json!({"current_password": PASSWORD})),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let rotated = json_body(response).await;
    assert_eq!(rotated["key"]["scopes"], json!(["analytics"]));
    assert_eq!(rotated["key"]["host"], "s.example");
    let replacement = format!("Bearer {}", rotated["token"].as_str().unwrap());
    assert_eq!(
        send(&app.admin, "GET", summary, &headers, None)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let response = send(
        &app.admin,
        "GET",
        summary,
        &[("authorization", &replacement)],
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["host"], "s.example");
}

#[tokio::test]
async fn expired_and_revoked_analytics_keys_are_rejected() {
    let app = Harness::start().await;
    let summary = "/api/v1/analytics/summary";
    let (expired_id, expired) = app.key(&["analytics"], None).await;
    let (revoked_id, revoked) = app.key(&["analytics"], None).await;
    for token in [&expired, &revoked] {
        let response = send(
            &app.admin,
            "GET",
            summary,
            &[("authorization", token)],
            None,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
    }
    sqlx::query("UPDATE admin_api_keys SET expires_at = unixepoch() WHERE id = ?")
        .bind(expired_id)
        .execute(&app.pool)
        .await
        .unwrap();
    let response = send(
        &app.admin,
        "DELETE",
        &format!("/api/v1/keys/{revoked_id}"),
        &app.browser(),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    for token in [&expired, &revoked] {
        for path in [summary, "/api/v1/analytics/export"] {
            let response = send(&app.admin, "GET", path, &[("authorization", token)], None).await;
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
        }
    }
    let invalid = send(
        &app.admin,
        "GET",
        summary,
        &[(
            "authorization",
            "Bearer 1.0000000000000000000000000000000000000000000000000000000000000000",
        )],
        None,
    )
    .await;
    assert_eq!(invalid.status(), StatusCode::UNAUTHORIZED);
    let response = send(&app.admin, "POST", summary, &app.browser(), None).await;
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
}
