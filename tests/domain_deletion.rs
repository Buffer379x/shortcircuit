//! Domain deletion: preview, moving or deleting the domain's links in one transaction, typed
//! confirmation, stale previews, environment hosts, API key refusal, and the audit entry.

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

/// Tables that hold per-link state and are cleaned up when a link row is deleted.
const LINK_TABLES: [&str; 8] = [
    "link_tags",
    "link_protection",
    "link_redirect_policies",
    "link_history",
    "link_monitor",
    "link_title_lookups",
    "analytics_events",
    "analytics_daily",
];

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
    /// `go.example` and `env.example` come from `PUBLIC_HOSTS`. `old.example` and `new.example`
    /// are saved domains.
    async fn start() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let pool = open_database(&directory.path().join("domains.db"))
            .await
            .unwrap();
        let auth = Auth::new(pool.clone());
        let setup = auth.issue_bootstrap(BootstrapKind::Setup).await.unwrap();
        let session = auth
            .setup(&setup, "Operator", "synthetic-test-password")
            .await
            .unwrap();
        let hosts = PublicHosts::parse("go.example,env.example").unwrap();
        let app = Self {
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
        };
        for host in ["old.example", "new.example"] {
            app.save_domain(host, true).await;
        }
        app
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

    async fn save_domain(&self, host: &str, enabled: bool) {
        let response = self
            .admin(
                "POST",
                "/api/v1/catalog/domains",
                Some(
                    json!({"host": host, "origin": format!("https://{host}"), "enabled": enabled}),
                ),
            )
            .await;
        assert_eq!(response.status(), StatusCode::OK, "{host}");
    }

    async fn create(&self, host: &str, slug: &str) -> i64 {
        let response = self
            .admin(
                "POST",
                "/api/v1/links",
                Some(json!({"host": host, "slug": slug, "target": format!("https://example.test/{slug}")})),
            )
            .await;
        assert_eq!(response.status(), StatusCode::CREATED, "{host}/{slug}");
        json_body(response).await["id"].as_i64().unwrap()
    }

    async fn preview(&self, query: &str) -> (StatusCode, Value) {
        let response = self
            .admin(
                "GET",
                &format!("/api/v1/catalog/domains/deletion?{query}"),
                None,
            )
            .await;
        let status = response.status();
        (status, json_body(response).await)
    }

    async fn delete(&self, body: Value) -> (StatusCode, Value) {
        let response = self
            .admin("DELETE", "/api/v1/catalog/domains", Some(body))
            .await;
        let status = response.status();
        (status, json_body(response).await)
    }

    async fn hosts(&self) -> Vec<String> {
        let response = self.admin("GET", "/api/v1/catalog/domains", None).await;
        json_body(response).await["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["host"].as_str().unwrap().to_owned())
            .collect()
    }

    async fn scalar(&self, query: &'static str, value: &str) -> i64 {
        sqlx::query_scalar(query)
            .bind(value)
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }

    async fn links_on(&self, host: &str) -> i64 {
        self.scalar("SELECT count(*) FROM links WHERE host = ?", host)
            .await
    }

    /// Adds history, tags, protection, a redirect policy, monitoring, a settled title lookup,
    /// and analytics to one link.
    async fn enrich(&self, id: i64, host: &str) {
        let response = self
            .admin(
                "PATCH",
                &format!("/api/v1/links/{id}"),
                Some(json!({"title": "Synthetic title", "tags": ["team"]})),
            )
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        sqlx::query(
            "INSERT INTO link_protection (link_id, one_time) VALUES (?1, 1);
             INSERT INTO link_redirect_policies (link_id, status, expires_at, forward_query, forward_path, rules)
             VALUES (?1, 302, unixepoch() + 3600, 0, 0, '[]');
             INSERT INTO link_monitor (link_id, priority) VALUES (?1, 1);
             INSERT INTO link_title_lookups (link_id, host, target, requested_by, status, completed_at)
             VALUES (?1, ?2, 'https://example.test/', 'admin', 'no_title', unixepoch());
             INSERT INTO analytics_events (occurred_at, link_id, host, kind, visit_class, source)
             VALUES (unixepoch(), ?1, ?2, 'redirect', 'human', 'direct');
             INSERT INTO analytics_daily (day, host, link_id, kind, visit_class, total)
             VALUES (20000, ?2, ?1, 'redirect', 'human', 3);",
        )
        .bind(id)
        .bind(host)
        .execute(&self.pool)
        .await
        .unwrap();
    }

    async fn per_link_rows(&self, id: i64) -> Vec<i64> {
        let mut counts = Vec::new();
        for table in LINK_TABLES {
            counts.push(
                sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
                    "SELECT count(*) FROM {table} WHERE link_id = ?"
                )))
                .bind(id)
                .fetch_one(&self.pool)
                .await
                .unwrap(),
            );
        }
        counts
    }
}

#[tokio::test]
async fn a_domain_without_links_is_deleted_and_audited() {
    let app = App::start().await;
    let (status, preview) = app.preview("host=OLD.example").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(preview["host"], "old.example");
    assert_eq!(preview["links"], 0);
    assert_eq!(preview["managed"], true);
    assert_eq!(preview["environment"], false);
    assert_eq!(preview["api_keys"], 0);
    assert_eq!(
        preview["targets"],
        json!(["go.example", "env.example", "new.example"])
    );
    assert!(preview.get("move").is_none());

    let (status, _) = app
        .delete(json!({"host": "old.example", "links": "none", "expected_links": 1}))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, removed) = app
        .delete(json!({"host": "old.example", "links": "none", "expected_links": 0}))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        removed,
        json!({"host": "old.example", "links": "none", "moved": 0, "deleted": 0, "environment": false})
    );
    assert!(!app.hosts().await.contains(&"old.example".to_owned()));
    let (status, _) = app.preview("host=old.example").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = app
        .delete(json!({"host": "old.example", "links": "none", "expected_links": 0}))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let audit = json_body(app.admin("GET", "/api/v1/audit", None).await).await;
    let entry = &audit["items"][0];
    assert_eq!(entry["action"], "domain_delete");
    assert_eq!(entry["actor"], "admin");
    assert_eq!(entry["outcome"], "success");
    assert_eq!(
        entry["details"],
        json!({"host": "old.example", "links": "none", "target": null, "count": 0, "environment": false})
    );
    // Entries without details keep their previous shape.
    assert!(audit["items"][1].get("details").is_none());
}

#[tokio::test]
async fn moving_links_keeps_their_identity_history_and_state() {
    let app = App::start().await;
    let first = app.create("old.example", "a").await;
    let second = app.create("old.example", "b").await;
    app.create("new.example", "c").await;
    app.enrich(first, "old.example").await;
    sqlx::query("UPDATE links SET archived = 1, enabled = 0 WHERE id = ?")
        .bind(second)
        .execute(&app.pool)
        .await
        .unwrap();
    let before = app.per_link_rows(first).await;
    assert!(before.iter().all(|count| *count > 0), "{before:?}");

    let (status, preview) = app.preview("host=old.example&target=new.example").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(preview["links"], 2, "archived links count");
    assert_eq!(
        preview["move"],
        json!({"target": "new.example", "conflicts": 0, "conflicting_slugs": []})
    );
    for target in ["old.example", "unknown.example", "bad%20host"] {
        let (status, body) = app
            .preview(&format!("host=old.example&target={target}"))
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{target}");
        assert_eq!(body["code"], "invalid_move_target");
    }

    // A move needs a target, and a disabled domain cannot receive links.
    let (status, _) = app
        .delete(json!({"host": "old.example", "links": "move", "expected_links": 2}))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    app.save_domain("new.example", false).await;
    let (status, body) = app
        .delete(json!({"host": "old.example", "links": "move", "target": "new.example", "expected_links": 2}))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["code"], "invalid_move_target");
    app.save_domain("new.example", true).await;

    let (status, removed) = app
        .delete(json!({"host": "old.example", "links": "move", "target": "NEW.example", "expected_links": 2}))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(removed["moved"], 2);
    assert_eq!(removed["target"], "new.example");
    assert_eq!(app.links_on("old.example").await, 0);
    let moved: Vec<(i64, String)> =
        sqlx::query_as("SELECT id, slug FROM links WHERE host = 'new.example' ORDER BY id")
            .fetch_all(&app.pool)
            .await
            .unwrap();
    assert_eq!(
        moved,
        [
            (first, "a".to_owned()),
            (second, "b".to_owned()),
            (first + 2, "c".to_owned())
        ]
    );
    // Everything stays attached to the link. The title lookup belongs to the old host and
    // destination and is cleared, as with any host change.
    let after = app.per_link_rows(first).await;
    for (index, table) in LINK_TABLES.iter().enumerate() {
        let expected = if *table == "link_title_lookups" {
            0
        } else {
            before[index]
        };
        assert_eq!(after[index], expected, "{table}");
    }
    // Recorded analytics keep the host they were recorded under.
    assert_eq!(
        app.scalar(
            "SELECT count(*) FROM analytics_daily WHERE host = ?",
            "old.example"
        )
        .await,
        1
    );
    assert!(!app.hosts().await.contains(&"old.example".to_owned()));

    let redirect = app
        .public
        .clone()
        .oneshot(
            Request::builder()
                .uri("/c")
                .header("host", "new.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(redirect.status(), StatusCode::FOUND);
    let audit = json_body(app.admin("GET", "/api/v1/audit", None).await).await;
    assert_eq!(
        audit["items"][0]["details"],
        json!({"host": "old.example", "links": "move", "target": "new.example", "count": 2, "environment": false})
    );
}

#[tokio::test]
async fn a_move_with_slug_conflicts_is_refused_without_changes() {
    let app = App::start().await;
    for slug in ["x", "y", "z"] {
        app.create("old.example", slug).await;
    }
    for slug in ["z", "x"] {
        app.create("new.example", slug).await;
    }
    let (status, preview) = app.preview("host=old.example&target=new.example").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        preview["move"],
        json!({"target": "new.example", "conflicts": 2, "conflicting_slugs": ["x", "z"]})
    );
    let (status, body) = app
        .delete(json!({"host": "old.example", "links": "move", "target": "new.example", "expected_links": 3}))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "domain_move_conflict");
    assert_eq!(app.links_on("old.example").await, 3);
    assert_eq!(app.links_on("new.example").await, 2);
    assert!(app.hosts().await.contains(&"old.example".to_owned()));
}

#[tokio::test]
async fn deleting_links_cleans_up_exactly_like_bulk_delete() {
    let app = App::start().await;
    let bulk = app.create("go.example", "same").await;
    let domain = app.create("old.example", "same").await;
    app.create("old.example", "other").await;
    for (id, host) in [(bulk, "go.example"), (domain, "old.example")] {
        app.enrich(id, host).await;
    }
    sqlx::query(
        "INSERT INTO analytics_daily (day, host, link_id, kind, visit_class, total)
         VALUES (20000, 'old.example', 0, 'missing', 'human', 2);
         INSERT INTO notification_outbox (event_key, event, payload, occurred_at)
         VALUES ('link_health:synthetic', 'link_health', '{}', unixepoch());",
    )
    .execute(&app.pool)
    .await
    .unwrap();
    let seeded = app.per_link_rows(bulk).await;
    assert!(seeded.iter().all(|count| *count > 0), "{seeded:?}");
    assert_eq!(seeded, app.per_link_rows(domain).await);

    let key: String = sqlx::query_scalar("SELECT selection_key FROM links WHERE id = ?")
        .bind(bulk)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    let response = app
        .admin(
            "POST",
            "/api/v1/links/bulk",
            Some(json!({"links": [{"id": bulk, "key": key}], "change": {"action": "delete", "confirmed": true}})),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);

    // Deleting links requires the typed domain name.
    for confirm in [None, Some("go.example"), Some("old")] {
        let mut body = json!({"host": "old.example", "links": "delete", "expected_links": 2});
        if let Some(value) = confirm {
            body["confirm"] = json!(value);
        }
        let (status, problem) = app.delete(body).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{confirm:?}");
        assert_eq!(problem["code"], "domain_delete_confirmation");
    }
    assert_eq!(app.links_on("old.example").await, 2);

    let (status, removed) = app
        .delete(json!({"host": "old.example", "links": "delete", "confirm": " Old.Example ", "expected_links": 2}))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(removed["deleted"], 2);
    assert_eq!(app.links_on("old.example").await, 0);
    let cleared = vec![0; LINK_TABLES.len()];
    assert_eq!(app.per_link_rows(bulk).await, cleared);
    assert_eq!(app.per_link_rows(domain).await, cleared);
    // Domain-level analytics and settled notifications are not link rows and stay, as with
    // bulk delete.
    assert_eq!(
        app.scalar(
            "SELECT count(*) FROM analytics_daily WHERE host = ?",
            "old.example"
        )
        .await,
        1
    );
    assert_eq!(
        app.scalar(
            "SELECT count(*) FROM notification_outbox WHERE event = ?",
            "link_health"
        )
        .await,
        1
    );
    let orphans: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM links WHERE host NOT IN ('go.example', 'env.example')
         AND host NOT IN (SELECT host FROM domains)",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(orphans, 0);
}

#[tokio::test]
async fn a_stale_preview_and_links_without_a_choice_are_refused() {
    let app = App::start().await;
    app.create("old.example", "first").await;
    let (_, preview) = app.preview("host=old.example").await;
    assert_eq!(preview["links"], 1);
    app.create("old.example", "second").await;
    for body in [
        json!({"host": "old.example", "links": "delete", "confirm": "old.example", "expected_links": 1}),
        json!({"host": "old.example", "links": "move", "target": "new.example", "expected_links": 1}),
        json!({"host": "old.example", "links": "none", "expected_links": 0}),
    ] {
        let (status, problem) = app.delete(body).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(problem["code"], "domain_deletion_changed");
    }
    for body in [
        json!({"host": "old.example", "links": "none", "expected_links": 2}),
        json!({"host": "old.example", "links": "delete", "target": "new.example", "confirm": "old.example", "expected_links": 2}),
        json!({"host": "old.example", "links": "delete", "confirm": "old.example", "expected_links": -1}),
        json!({"host": "old.example", "links": "archive", "expected_links": 2}),
        json!({"host": "old.example", "links": "none", "expected_links": 0, "force": true}),
    ] {
        let (status, _) = app.delete(body.clone()).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    }
    assert_eq!(app.links_on("old.example").await, 2);
    assert!(app.hosts().await.contains(&"old.example".to_owned()));
}

#[tokio::test]
async fn api_keys_cannot_preview_or_delete_and_bound_keys_block_deletion() {
    let app = App::start().await;
    let response = app
        .admin(
            "POST",
            "/api/v1/keys",
            Some(json!({"name": "Bound key", "scopes": ["read", "write", "delete"], "host": "old.example"})),
        )
        .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let created = json_body(response).await;
    let token = created["token"].as_str().unwrap().to_owned();
    let key_id = created["key"]["id"].as_i64().unwrap();
    for (method, path, body) in [
        (
            "GET",
            "/api/v1/catalog/domains/deletion?host=old.example",
            Body::empty(),
        ),
        (
            "DELETE",
            "/api/v1/catalog/domains",
            Body::from(
                json!({"host": "old.example", "links": "none", "expected_links": 0}).to_string(),
            ),
        ),
    ] {
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header("host", "admin.example")
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/json")
            .body(body)
            .unwrap();
        let response = app.admin.clone().oneshot(request).await.unwrap();
        assert!(
            matches!(
                response.status(),
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
            ),
            "{method} {}",
            response.status()
        );
    }

    let (_, preview) = app.preview("host=old.example").await;
    assert_eq!(preview["api_keys"], 1);
    let (status, problem) = app
        .delete(json!({"host": "old.example", "links": "none", "expected_links": 0}))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(problem["code"], "domain_has_api_keys");
    assert!(app.hosts().await.contains(&"old.example".to_owned()));

    let response = app
        .admin("DELETE", &format!("/api/v1/keys/{key_id}"), None)
        .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let (status, _) = app
        .delete(json!({"host": "old.example", "links": "none", "expected_links": 0}))
        .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn an_environment_host_loses_its_settings_and_links_but_stays_served() {
    let app = App::start().await;
    app.create("env.example", "moving").await;
    let response = app
        .admin(
            "POST",
            "/api/v1/catalog/domains",
            Some(
                json!({"host": "env.example", "origin": "https://env.example:8443", "enabled": true,
                "missing_target": "https://example.test/help"}),
            ),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    // A key bound to an environment host keeps a served host, so it does not block deletion.
    let response = app
        .admin(
            "POST",
            "/api/v1/keys",
            Some(json!({"name": "Env key", "scopes": ["read"], "host": "env.example"})),
        )
        .await;
    assert_eq!(response.status(), StatusCode::CREATED);

    let (_, preview) = app.preview("host=env.example&target=go.example").await;
    assert_eq!(preview["environment"], true);
    assert_eq!(preview["managed"], true);
    assert_eq!(preview["api_keys"], 1);
    let (status, removed) = app
        .delete(json!({"host": "env.example", "links": "move", "target": "go.example", "expected_links": 1}))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(removed["environment"], true);
    assert_eq!(app.links_on("go.example").await, 1);

    let domains = json_body(app.admin("GET", "/api/v1/catalog/domains", None).await).await;
    let env = domains["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["host"] == "env.example")
        .unwrap()
        .clone();
    assert_eq!(
        env,
        json!({"host": "env.example", "origin": "https://env.example", "enabled": true,
            "root_target": null, "missing_target": null, "block_crawlers": false})
    );
    // Deleting it again without saved settings is allowed and changes nothing else.
    let (_, preview) = app.preview("host=env.example").await;
    assert_eq!(preview["managed"], false);
    let (status, _) = app
        .delete(json!({"host": "env.example", "links": "none", "expected_links": 0}))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(app.hosts().await.contains(&"env.example".to_owned()));
}
