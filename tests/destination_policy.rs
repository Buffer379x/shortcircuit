//! Destination policy: write checks, redirect-time enforcement, and preserved saved data.

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
    response::Response,
};
use serde_json::{Value, json};
use shortcircuit::{
    AdminConfig, Auth, BootstrapKind, PublicHosts, admin_router, open_database, public_router,
};
use sqlx::SqlitePool;
use tower::ServiceExt;

const ORIGIN: &str = "https://admin.example";

struct App {
    _directory: tempfile::TempDir,
    admin: Router,
    public: Router,
    pool: SqlitePool,
    cookie: String,
    csrf: String,
}

async fn json_body(response: Response) -> Value {
    serde_json::from_slice(&to_bytes(response.into_body(), 200_000).await.unwrap()).unwrap()
}

impl App {
    async fn start() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let pool = open_database(&directory.path().join("policy.db"))
            .await
            .unwrap();
        let auth = Auth::new(pool.clone());
        let setup = auth.issue_bootstrap(BootstrapKind::Setup).await.unwrap();
        let session = auth
            .setup(&setup, "Operator", "synthetic-test-password")
            .await
            .unwrap();
        let hosts = PublicHosts::parse("go.example,s.example").unwrap();
        Self {
            _directory: directory,
            admin: admin_router(
                pool.clone(),
                hosts.clone(),
                AdminConfig::new(ORIGIN, 8080).unwrap(),
            ),
            public: public_router(pool.clone(), hosts),
            pool,
            cookie: format!("__Host-shortcircuit_session={}", session.token),
            csrf: session.session.csrf,
        }
    }

    async fn admin(&self, method: &str, path: &str, body: Option<Value>) -> Response {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header("host", "admin.example")
            .header("cookie", &self.cookie)
            .header("origin", ORIGIN)
            .header("x-csrf-token", &self.csrf);
        let body = match body {
            Some(value) => {
                request = request.header("content-type", "application/json");
                Body::from(value.to_string())
            }
            None => Body::empty(),
        };
        self.admin
            .clone()
            .oneshot(request.body(body).unwrap())
            .await
            .unwrap()
    }

    async fn visit(&self, method: &str, path: &str) -> Response {
        self.public
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header("host", "go.example")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    async fn create(&self, slug: &str, target: &str) -> Response {
        self.admin(
            "POST",
            "/api/v1/links",
            Some(json!({"host": "go.example", "slug": slug, "target": target})),
        )
        .await
    }

    async fn set_policy(&self, policy: Value) -> Value {
        let response = self
            .admin("PATCH", "/api/v1/destination-policy", Some(policy))
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        json_body(response).await
    }
}

fn policy(enabled: bool) -> Value {
    json!({
        "enabled": enabled,
        "rules": [
            {"host": "Example.COM.", "subdomains": true, "action": "allow"},
            {"host": "private.example.com", "action": "block"},
            {"host": "Bücher.example", "subdomains": true, "action": "allow"},
            {"host": "shop.bücher.example", "action": "block"}
        ]
    })
}

#[tokio::test]
async fn the_policy_is_opt_in_and_checks_every_destination_write() {
    let app = App::start().await;
    let initial = json_body(app.admin("GET", "/api/v1/destination-policy", None).await).await;
    assert_eq!(initial["enabled"], false);
    assert_eq!(initial["rules"], json!([]));

    // Default behavior: any valid destination is accepted and redirects.
    let legacy = "https://Legacy.test/Path?Sig=A%2fB#Frag";
    assert_eq!(
        app.create("legacy", legacy).await.status(),
        StatusCode::CREATED
    );
    let response = app.visit("GET", "/legacy").await;
    assert_eq!(response.status(), StatusCode::FOUND);
    assert_eq!(response.headers()["location"], legacy);

    // Saved but disabled rules do not change behavior.
    let saved = app.set_policy(policy(false)).await;
    assert_eq!(saved["rules"][0]["host"], "private.example.com");
    assert_eq!(
        saved["rules"]
            .as_array()
            .unwrap()
            .iter()
            .map(|rule| rule["host"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "private.example.com",
            "shop.xn--bcher-kva.example",
            "example.com",
            "xn--bcher-kva.example"
        ]
    );
    assert_eq!(saved["impact"]["links"], 0);
    assert_eq!(
        app.visit("GET", "/legacy").await.status(),
        StatusCode::FOUND
    );

    // A preview reports the effect on saved destinations without saving.
    let preview = app
        .admin(
            "POST",
            "/api/v1/destination-policy/preview",
            Some(policy(true)),
        )
        .await;
    assert_eq!(preview.status(), StatusCode::OK);
    let preview = json_body(preview).await;
    assert_eq!(preview["impact"]["links"], 1);
    assert_eq!(preview["impact"]["examples"][0]["slug"], "legacy");
    assert_eq!(
        app.visit("GET", "/legacy").await.status(),
        StatusCode::FOUND
    );

    let enabled = app.set_policy(policy(true)).await;
    assert_eq!(enabled["enabled"], true);
    assert_eq!(enabled["impact"]["links"], 1);

    // New writes must comply. Case, trailing dots, and IDN spellings use the normalized host.
    for (slug, target) in [
        ("evil", "https://evil.test/"),
        ("private", "https://PRIVATE.example.com/"),
        ("lookalike", "https://example.com.evil.test/"),
        ("shop", "https://shop.xn--bcher-kva.example/"),
    ] {
        let response = app.create(slug, target).await;
        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "{target}"
        );
        assert_eq!(json_body(response).await["code"], "destination_not_allowed");
    }
    let allowed = "https://DOCS.Example.com./Read?x=1#top";
    let response = app.create("docs", allowed).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let docs = json_body(response).await;
    assert_eq!(docs["target"], allowed);
    let response = app.visit("GET", "/docs").await;
    assert_eq!(response.status(), StatusCode::FOUND);
    assert_eq!(response.headers()["location"], allowed);
    assert_eq!(
        app.create("idn", "https://news.xn--bcher-kva.example/")
            .await
            .status(),
        StatusCode::CREATED
    );

    // The saved legacy destination is kept exactly and its redirect is refused.
    assert_eq!(
        app.visit("GET", "/legacy").await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        app.visit("HEAD", "/legacy").await.status(),
        StatusCode::NOT_FOUND
    );
    let stored: String = sqlx::query_scalar("SELECT target FROM links WHERE slug = 'legacy'")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(stored, legacy);

    // Unrelated edits keep working; a changed destination must comply.
    let legacy_id: i64 = sqlx::query_scalar("SELECT id FROM links WHERE slug = 'legacy'")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    let edit = app
        .admin(
            "PATCH",
            &format!("/api/v1/links/{legacy_id}"),
            Some(json!({"title": "Kept", "target": legacy})),
        )
        .await;
    assert_eq!(edit.status(), StatusCode::OK);
    let edit = app
        .admin(
            "PATCH",
            &format!("/api/v1/links/{legacy_id}"),
            Some(json!({"target": "https://evil.test/"})),
        )
        .await;
    assert_eq!(edit.status(), StatusCode::UNPROCESSABLE_ENTITY);

    // Redirect rules, domain fallbacks, and imports use the same check.
    let docs_id = docs["id"].as_i64().unwrap();
    let rules = app
        .admin(
            "PATCH",
            &format!("/api/v1/links/{docs_id}/policy"),
            Some(json!({"rules": [{
                "target": "https://evil.test/",
                "conditions": [{"type": "device", "value": "mobile"}]
            }]})),
        )
        .await;
    assert_eq!(rules.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(json_body(rules).await["code"], "destination_not_allowed");
    let fallback = app
        .admin(
            "POST",
            "/api/v1/catalog/domains",
            Some(json!({
                "host": "go.example", "origin": "https://go.example", "enabled": true,
                "root_target": "https://evil.test/"
            })),
        )
        .await;
    assert_eq!(fallback.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let import = app
        .admin(
            "POST",
            "/api/v1/exchange/links",
            Some(json!({
                "version": 1, "preview": true,
                "items": [{
                    "host": "go.example", "slug": "imported", "target": "https://evil.test/",
                    "title": "", "tags": [], "enabled": true, "analytics_enabled": true
                }]
            })),
        )
        .await;
    assert_eq!(import.status(), StatusCode::UNPROCESSABLE_ENTITY);

    // The simulator reports whether the policy permits the selected destination.
    let simulated = app
        .admin(
            "POST",
            &format!("/api/v1/links/{legacy_id}/policy/simulate"),
            Some(json!({"policy": {}})),
        )
        .await;
    assert_eq!(simulated.status(), StatusCode::OK);
    assert_eq!(json_body(simulated).await["permitted"], false);

    // Disabling the policy restores the saved redirect unchanged.
    app.set_policy(policy(false)).await;
    let response = app.visit("GET", "/legacy").await;
    assert_eq!(response.status(), StatusCode::FOUND);
    assert_eq!(response.headers()["location"], legacy);

    let audit: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM admin_audit WHERE action = 'destination_policy_update'",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(audit, 3);
}

#[tokio::test]
async fn refused_destinations_keep_visit_limits_fallbacks_and_history_intact() {
    let app = App::start().await;
    let target = "https://outside.test/limited";
    let response = app.create("limited", target).await;
    let id = json_body(response).await["id"].as_i64().unwrap();
    let response = app
        .admin(
            "PATCH",
            &format!("/api/v1/links/{id}/policy"),
            Some(json!({"max_visits": 1})),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    // Configure fallbacks and a rule before the policy exists.
    let response = app
        .admin(
            "POST",
            "/api/v1/catalog/domains",
            Some(json!({
                "host": "go.example", "origin": "https://go.example", "enabled": true,
                "root_target": "https://outside.test/home",
                "missing_target": "https://example.com/help"
            })),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let response = app.create("ruled", "https://outside.test/default").await;
    let ruled = json_body(response).await["id"].as_i64().unwrap();
    let response = app
        .admin(
            "PATCH",
            &format!("/api/v1/links/{ruled}/policy"),
            Some(json!({"rules": [{
                "target": "https://outside.test/rule",
                "conditions": [{"type": "query", "key": "via", "value": "rule"}]
            }]})),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    // Change the destination so an older revision points outside the future allowlist.
    let response = app
        .admin(
            "PATCH",
            &format!("/api/v1/links/{ruled}"),
            Some(json!({"target": "https://example.com/current"})),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);

    app.set_policy(json!({
        "enabled": true,
        "rules": [{"host": "example.com", "action": "allow"}]
    }))
    .await;

    // The refused limited link consumes nothing.
    for method in ["GET", "GET", "HEAD"] {
        assert_eq!(
            app.visit(method, "/limited").await.status(),
            StatusCode::NOT_FOUND
        );
    }
    let used: i64 =
        sqlx::query_scalar("SELECT visits_used FROM link_redirect_policies WHERE link_id = ?")
            .bind(id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(used, 0);

    // A refused root destination behaves like no root destination. Permitted fallbacks work.
    assert_eq!(app.visit("GET", "/").await.status(), StatusCode::NOT_FOUND);
    let missing = app.visit("GET", "/unknown").await;
    assert_eq!(missing.status(), StatusCode::FOUND);
    assert_eq!(missing.headers()["location"], "https://example.com/help");

    // Only the selected rule destination is refused.
    let response = app.visit("GET", "/ruled").await;
    assert_eq!(response.status(), StatusCode::FOUND);
    assert_eq!(
        response.headers()["location"],
        "https://example.com/current"
    );
    assert_eq!(
        app.visit("GET", "/ruled?via=rule").await.status(),
        StatusCode::NOT_FOUND
    );

    // A metadata revision with a refused destination cannot be restored.
    let history = json_body(
        app.admin("GET", &format!("/api/v1/links/{ruled}/history"), None)
            .await,
    )
    .await;
    let revision = history["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["previous"]["kind"] == "metadata")
        .unwrap()["id"]
        .as_i64()
        .unwrap();
    let restore = app
        .admin(
            "POST",
            &format!("/api/v1/links/{ruled}/history/{revision}/restore"),
            None,
        )
        .await;
    assert_eq!(restore.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(json_body(restore).await["code"], "destination_not_allowed");

    // Once the policy permits the destination, the full allowance is still available.
    app.set_policy(json!({"enabled": false, "rules": []})).await;
    assert_eq!(
        app.visit("GET", "/limited").await.status(),
        StatusCode::FOUND
    );
    assert_eq!(
        app.visit("GET", "/limited").await.status(),
        StatusCode::NOT_FOUND
    );
    let root = app.visit("GET", "/").await;
    assert_eq!(root.headers()["location"], "https://outside.test/home");
}

#[tokio::test]
async fn invalid_rules_are_rejected_before_anything_is_saved() {
    let app = App::start().await;
    for (rules, code) in [
        (
            json!([{"host": "*.example.com", "action": "allow"}]),
            "invalid_destination_rule",
        ),
        (
            json!([{"host": "https://example.com", "action": "allow"}]),
            "invalid_destination_rule",
        ),
        (
            json!([{"host": "192.0.2.1", "subdomains": true, "action": "block"}]),
            "invalid_destination_rule",
        ),
        (
            json!([
                {"host": "example.com", "action": "allow"},
                {"host": "EXAMPLE.com.", "action": "block"}
            ]),
            "duplicate_destination_rule",
        ),
    ] {
        let response = app
            .admin(
                "PATCH",
                "/api/v1/destination-policy",
                Some(json!({"enabled": true, "rules": rules})),
            )
            .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(json_body(response).await["code"], code);
    }
    let too_many: Vec<Value> = (0..201)
        .map(|n| json!({"host": format!("host{n}.example"), "action": "block"}))
        .collect();
    let response = app
        .admin(
            "PATCH",
            "/api/v1/destination-policy",
            Some(json!({"enabled": true, "rules": too_many})),
        )
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let current = json_body(app.admin("GET", "/api/v1/destination-policy", None).await).await;
    assert_eq!(current["enabled"], false);
    assert_eq!(current["rules"], json!([]));
    // API keys cannot read or change the policy.
    let key = Request::builder()
        .uri("/api/v1/destination-policy")
        .header("host", "admin.example")
        .header(
            "authorization",
            "Bearer 1.0000000000000000000000000000000000000000000000000000000000000000",
        )
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.admin.clone().oneshot(key).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn a_host_bound_key_learns_nothing_about_links_on_other_hosts() {
    let app = App::start().await;
    let response = app.create("private", "https://outside.test/private").await;
    let id = json_body(response).await["id"].as_i64().unwrap();
    app.set_policy(
        json!({"enabled": true, "rules": [{"host": "outside.test", "action": "block"}]}),
    )
    .await;
    let response = app
        .admin(
            "POST",
            "/api/v1/keys",
            Some(json!({"name": "Other host", "scopes": ["write"], "host": "s.example"})),
        )
        .await;
    let token = json_body(response).await["token"]
        .as_str()
        .unwrap()
        .to_owned();
    for target in ["https://outside.test/private", "https://example.org/"] {
        let request = Request::builder()
            .method("PATCH")
            .uri(format!("/api/v1/links/{id}"))
            .header("host", "admin.example")
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/json")
            .body(Body::from(json!({"target": target}).to_string()))
            .unwrap();
        let response = app.admin.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{target}");
    }
}
