//! Catalog link fields, single-link reads, per-link analytics summaries, and Overview attention.

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
const DAY: i64 = 86_400;

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
    serde_json::from_slice(&to_bytes(response.into_body(), 400_000).await.unwrap()).unwrap()
}

impl Harness {
    async fn start() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let pool = open_database(&directory.path().join("catalog.db"))
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

    async fn key(&self, scopes: &[&str], host: Option<&str>) -> String {
        let mut input = json!({"name": "Catalog key", "scopes": scopes});
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
        format!("Bearer {}", created["token"].as_str().unwrap())
    }

    async fn seed(&self, sql: impl sqlx::SqlSafeStr) {
        sqlx::raw_sql(sql).execute(&self.pool).await.unwrap();
    }

    async fn search(&self) -> Vec<Value> {
        let response = send(
            &self.admin,
            "POST",
            "/api/v1/links/search",
            &self.browser(),
            Some(json!({"sort": "oldest"})),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        json_body(response).await["items"]
            .as_array()
            .unwrap()
            .clone()
    }

    async fn get(&self, path: &str, auth: &str) -> Response {
        let header = if auth.starts_with("Bearer ") {
            ("authorization", auth)
        } else {
            ("cookie", auth)
        };
        send(&self.admin, "GET", path, &[header], None).await
    }
}

/// Links that cover each lifecycle state, protection kind, and traffic boundary.
const CATALOG: &str = "
    INSERT INTO links (id, host, slug, target, title, enabled, analytics_enabled, archived) VALUES
        (1, 'go.example', 'plain', 'https://example.test/1', 'Plain', 1, 1, 0),
        (2, 'go.example', 'limited', 'https://example.test/2', 'Limited', 1, 1, 0),
        (3, 'go.example', 'untracked', 'https://example.test/3', 'Untracked', 1, 0, 0),
        (4, 'go.example', 'once', 'https://example.test/4', 'Once', 1, 1, 0),
        (5, 's.example', 'later', 'https://example.test/5', 'Later', 1, 1, 0),
        (6, 'go.example', 'stored', 'https://example.test/6', 'Stored', 1, 1, 1),
        (7, 'go.example', 'paused', 'https://example.test/7', 'Paused', 0, 1, 0),
        (8, 'go.example', 'ended', 'https://example.test/8', 'Ended', 1, 1, 0);
    INSERT INTO link_redirect_policies
        (link_id, status, starts_at, expires_at, max_visits, visits_used, forward_query, forward_path, rules)
    VALUES
        (2, 302, NULL, unixepoch() + 3 * 86400, 10, 9, 0, 0,
         '[{\"target\":\"https://example.test/de\",\"conditions\":[{\"type\":\"language\",\"value\":\"de\"}]},
           {\"target\":\"https://example.test/m\",\"conditions\":[{\"type\":\"device\",\"value\":\"mobile\"}]}]'),
        (5, 302, unixepoch() + 86400, NULL, NULL, 0, 0, 0, '[]'),
        (6, 302, NULL, unixepoch() + 86400, NULL, 0, 0, 0, '[]'),
        (8, 302, NULL, unixepoch() - 1, NULL, 0, 0, 0, '[]');
    INSERT INTO link_protection (link_id, password_hash, one_time, consumed) VALUES
        (2, 'synthetic-hash', 0, 0),
        (4, NULL, 1, 1);
    INSERT INTO link_monitor (link_id, priority, status, last_checked, http_status, elapsed_ms, failure)
    VALUES (5, 1, 'unreachable', 1000, NULL, 812, 'timeout');
    INSERT INTO analytics_daily (day, host, link_id, kind, visit_class, total) VALUES
        (unixepoch() / 86400, 'go.example', 1, 'redirect', 'human', 3),
        (unixepoch() / 86400, 'go.example', 1, 'redirect', 'bot', 2),
        (unixepoch() / 86400, 'go.example', 1, 'blocked', 'human', 50),
        (unixepoch() / 86400 - 13, 'go.example', 1, 'redirect', 'unknown', 4),
        (unixepoch() / 86400 - 14, 'go.example', 1, 'redirect', 'human', 6),
        (unixepoch() / 86400 - 29, 'go.example', 1, 'redirect', 'human', 1),
        (unixepoch() / 86400 - 30, 'go.example', 1, 'redirect', 'human', 100),
        (unixepoch() / 86400, 'go.example', 3, 'redirect', 'human', 9),
        (unixepoch() / 86400 - 1, 's.example', 5, 'redirect', 'human', 2);";

fn by_slug<'a>(items: &'a [Value], slug: &str) -> &'a Value {
    items.iter().find(|item| item["slug"] == slug).unwrap()
}

#[tokio::test]
async fn search_items_report_state_limits_protection_monitor_and_traffic() {
    let app = Harness::start().await;
    app.seed(CATALOG).await;
    let items = app.search().await;
    assert_eq!(items.len(), 8);

    let plain = by_slug(&items, "plain");
    assert_eq!(plain["state"], "active");
    assert!(plain["starts_at"].is_null() && plain["expires_at"].is_null());
    assert!(plain["max_visits"].is_null());
    assert_eq!(plain["visits_used"], 0);
    assert!(plain["protection"].is_null());
    assert_eq!(plain["rule_count"], 0);
    assert!(plain["monitor"].is_null());
    // Every visit class counts. Blocked requests and days before the window do not.
    assert_eq!(plain["visits_30d"], 16);
    let mut trend = vec![0; 14];
    trend[0] = 4;
    trend[13] = 5;
    assert_eq!(plain["trend"], json!(trend));
    // Existing fields keep their meaning.
    assert_eq!(plain["recorded_redirects"], 116);
    assert_eq!(plain["archived"], false);

    let limited = by_slug(&items, "limited");
    assert_eq!(limited["state"], "active");
    assert_eq!(limited["max_visits"], 10);
    assert_eq!(limited["visits_used"], 9);
    assert!(limited["expires_at"].as_i64().unwrap() > 0);
    assert_eq!(limited["protection"], "password");
    assert_eq!(limited["rule_count"], 2);
    assert_eq!(limited["visits_30d"], 0);
    assert_eq!(limited["trend"], json!(vec![0; 14]));

    let untracked = by_slug(&items, "untracked");
    assert!(untracked["visits_30d"].is_null());
    assert_eq!(untracked["trend"], json!([]));
    assert_eq!(untracked["recorded_redirects"], 9);

    let once = by_slug(&items, "once");
    assert_eq!(once["state"], "exhausted");
    assert_eq!(once["protection"], "one_time");

    let later = by_slug(&items, "later");
    assert_eq!(later["state"], "scheduled");
    assert_eq!(
        later["monitor"],
        json!({"status": "unreachable", "priority": true, "last_checked": 1000,
               "http_status": null, "elapsed_ms": 812, "failure": "timeout"})
    );
    assert_eq!(later["trend"][12], 2);

    // Archiving is reported by its own flag. The state keeps the live redirect behavior.
    let stored = by_slug(&items, "stored");
    assert_eq!(
        (&stored["archived"], &stored["state"]),
        (&json!(true), &json!("active"))
    );
    assert_eq!(by_slug(&items, "paused")["state"], "disabled");
    assert_eq!(by_slug(&items, "ended")["state"], "expired");
}

#[tokio::test]
async fn traffic_is_null_while_analytics_is_off_for_the_installation() {
    let app = Harness::start().await;
    app.seed(CATALOG).await;
    app.seed("UPDATE analytics_settings SET enabled = 0").await;
    let items = app.search().await;
    let plain = by_slug(&items, "plain");
    assert!(plain["visits_30d"].is_null());
    assert_eq!(plain["trend"], json!([]));
    assert_eq!(plain["recorded_redirects"], 116);
}

#[tokio::test]
async fn single_link_read_matches_search_and_honors_scopes_and_hosts() {
    let app = Harness::start().await;
    app.seed(CATALOG).await;
    let items = app.search().await;

    for slug in ["plain", "limited", "untracked", "later", "stored"] {
        let item = by_slug(&items, slug);
        let path = format!("/api/v1/links/{}", item["id"]);
        let response = app.get(&path, &app.cookie).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(&json_body(response).await, item, "{slug}");
    }
    for path in [
        "/api/v1/links/999",
        "/api/v1/links/0",
        "/api/v1/links/-1",
        "/api/v1/links/text",
    ] {
        let response = app.get(path, &app.cookie).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
        assert_eq!(json_body(response).await["code"], "not_found");
    }
    let anonymous = send(&app.admin, "GET", "/api/v1/links/1", &[], None).await;
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);

    // A host-bound read key sees only its host and no traffic without the analytics scope.
    let bound = app.key(&["read"], Some("s.example")).await;
    let own = app.get("/api/v1/links/5", &bound).await;
    assert_eq!(own.status(), StatusCode::OK);
    let own = json_body(own).await;
    assert_eq!(own["state"], "scheduled");
    assert_eq!(own["monitor"]["status"], "unreachable");
    for field in [
        "visits_30d",
        "trend",
        "recorded_redirects",
        "last_redirect_date",
        "last_redirect_at",
    ] {
        assert!(own.get(field).is_none(), "{field}");
    }
    let other = app.get("/api/v1/links/1", &bound).await;
    assert_eq!(other.status(), StatusCode::NOT_FOUND);

    let reporting = app.key(&["read", "analytics"], Some("s.example")).await;
    let response = app.get("/api/v1/links/5", &reporting).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(&json_body(response).await, by_slug(&items, "later"));

    let write_only = app.key(&["write"], None).await;
    let denied = app.get("/api/v1/links/5", &write_only).await;
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    let analytics_only = app.key(&["analytics"], None).await;
    let denied = app.get("/api/v1/links/5", &analytics_only).await;
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn analytics_summary_restricts_to_one_visible_link() {
    let app = Harness::start().await;
    app.seed(CATALOG).await;
    app.seed(
        "INSERT INTO analytics_daily (day, host, link_id, kind, visit_class, total) VALUES
            (unixepoch() / 86400, 'go.example', 0, 'missing', 'unknown', 7);
         INSERT INTO link_tags (link_id, tag) VALUES (1, 'docs'), (3, 'other');",
    )
    .await;

    let response = app
        .get("/api/v1/analytics/summary?days=30&link=1", &app.cookie)
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let report = json_body(response).await;
    assert_eq!(report["link"], 1);
    assert_eq!(report["totals"]["redirect"], 16);
    assert_eq!(report["totals"]["blocked"], 50);
    assert_eq!(report["totals"]["missing"], 0);
    assert_eq!(report["totals"]["bot"], 2);
    assert_eq!(report["previous_redirects"], 100);
    assert_eq!(
        report["top_links"],
        json!([{"id": 1, "host": "go.example",
        "slug": "plain", "title": "Plain", "total": 16}])
    );
    assert_eq!(report["top_tags"], json!([{"tag": "docs", "total": 16}]));
    let daily = report["daily"].as_array().unwrap();
    assert_eq!(daily.len(), 30);
    assert_eq!(daily[29]["redirect"], 5);

    let unfiltered = json_body(
        app.get("/api/v1/analytics/summary?days=30", &app.cookie)
            .await,
    )
    .await;
    assert!(unfiltered["link"].is_null());
    assert_eq!(unfiltered["totals"]["redirect"], 27);
    assert_eq!(unfiltered["totals"]["missing"], 7);

    let export = app
        .get("/api/v1/analytics/export?days=1&link=3", &app.cookie)
        .await;
    assert_eq!(export.status(), StatusCode::OK);
    assert_eq!(json_body(export).await["report"]["totals"]["redirect"], 9);

    for path in [
        "/api/v1/analytics/summary?link=999",
        "/api/v1/analytics/summary?link=0",
        "/api/v1/analytics/summary?link=1&host=s.example",
    ] {
        let response = app.get(path, &app.cookie).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
        assert_eq!(json_body(response).await["code"], "not_found");
    }
    let malformed = app
        .get("/api/v1/analytics/summary?link=one", &app.cookie)
        .await;
    assert_eq!(malformed.status(), StatusCode::BAD_REQUEST);

    // A host-bound analytics key cannot reach a link on another host.
    let bound = app.key(&["analytics"], Some("s.example")).await;
    let hidden = app.get("/api/v1/analytics/summary?link=1", &bound).await;
    assert_eq!(hidden.status(), StatusCode::NOT_FOUND);
    let own = app.get("/api/v1/analytics/summary?link=5", &bound).await;
    assert_eq!(own.status(), StatusCode::OK);
    let own = json_body(own).await;
    assert_eq!(
        (&own["host"], &own["link"]),
        (&json!("s.example"), &json!(5))
    );
    assert_eq!(own["totals"]["redirect"], 2);
    let reader = app.key(&["read"], None).await;
    let denied = app.get("/api/v1/analytics/summary?link=1", &reader).await;
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn overview_lists_unarchived_links_needing_attention_in_priority_order() {
    let app = Harness::start().await;
    app.seed(CATALOG).await;
    app.seed(
        "INSERT INTO links (id, host, slug, target, archived) VALUES
            (20, 'go.example', 'near-limit', 'https://example.test/20', 0),
            (21, 'go.example', 'below-limit', 'https://example.test/21', 0),
            (22, 'go.example', 'soon', 'https://example.test/22', 0),
            (23, 'go.example', 'far', 'https://example.test/23', 0),
            (24, 'go.example', 'archived-limit', 'https://example.test/24', 1),
            (25, 'go.example', 'archived-down', 'https://example.test/25', 1),
            (26, 'go.example', 'used-up', 'https://example.test/26', 0);
         INSERT INTO link_redirect_policies
            (link_id, status, starts_at, expires_at, max_visits, visits_used, forward_query, forward_path, rules)
         VALUES (20, 302, NULL, NULL, 10, 8, 0, 0, '[]'),
                (21, 302, NULL, NULL, 100, 79, 0, 0, '[]'),
                (22, 302, NULL, unixepoch() + 86400, NULL, 0, 0, 0, '[]'),
                (23, 302, NULL, unixepoch() + 15 * 86400, NULL, 0, 0, 0, '[]'),
                (24, 302, NULL, NULL, 10, 10, 0, 0, '[]'),
                (26, 302, NULL, NULL, 4, 4, 0, 0, '[]');
         INSERT INTO link_monitor (link_id, priority, status) VALUES
            (1, 0, 'healthy'), (3, 0, 'broken'), (25, 0, 'unreachable'), (4, 0, 'warning');",
    )
    .await;
    let response = app.get("/api/v1/overview", &app.cookie).await;
    assert_eq!(response.status(), StatusCode::OK);
    let overview = json_body(response).await;
    let attention = overview["attention"].as_array().unwrap();
    let order: Vec<(i64, &str)> = attention
        .iter()
        .map(|item| {
            (
                item["id"].as_i64().unwrap(),
                item["reason"].as_str().unwrap(),
            )
        })
        .collect();
    // Unreachable first, then the nearest expiry, then the largest share of the visit limit.
    // Link 2 expires soon and is near its limit, so it appears once, as expiring.
    assert_eq!(
        order,
        [
            (5, "unreachable"),
            (3, "unreachable"),
            (22, "expiring"),
            (2, "expiring"),
            (26, "limit"),
            (20, "limit"),
        ]
    );
    assert_eq!(
        attention[3],
        json!({"id": 2, "host": "go.example", "slug": "limited", "title": "Limited",
               "reason": "expiring", "expires_at": attention[3]["expires_at"],
               "max_visits": 10, "visits_used": 9})
    );
    assert!(attention[3]["expires_at"].as_i64().unwrap() > 2 * DAY);
    assert!(attention[0]["expires_at"].is_null());
    assert_eq!(attention[0]["visits_used"], 0);
    // Existing overview fields are unchanged.
    assert_eq!(overview["links"]["archived"], 3);
    assert_eq!(overview["monitoring"]["attention"], 4);
}

#[tokio::test]
async fn overview_attention_is_capped_at_eight_links() {
    let app = Harness::start().await;
    let mut sql = String::new();
    for id in 1..=12 {
        sql.push_str(&format!(
            "INSERT INTO links (id, host, slug, target) VALUES
                ({id}, 'go.example', 'soon-{id}', 'https://example.test/{id}');
             INSERT INTO link_redirect_policies
                (link_id, status, expires_at, visits_used, forward_query, forward_path, rules)
             VALUES ({id}, 302, unixepoch() + {} , 0, 0, 0, '[]');",
            (20 - id) * 3_600
        ));
    }
    app.seed(sqlx::AssertSqlSafe(sql)).await;
    let overview = json_body(app.get("/api/v1/overview", &app.cookie).await).await;
    let ids: Vec<i64> = overview["attention"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_i64().unwrap())
        .collect();
    assert_eq!(ids, [12, 11, 10, 9, 8, 7, 6, 5]);
}
