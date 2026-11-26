use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
    response::Response,
};
use serde_json::{Value, json};
use shortcircuit::{
    AdminConfig, Auth, BootstrapKind, PublicHosts, RateLimit, TrustedProxies, admin_router,
    open_database, public_router,
};
use sqlx::SqlitePool;
use tower::ServiceExt;

const ORIGIN: &str = "https://admin.example";
const HOST: &str = "admin.example";
const PASSWORD: &str = "synthetic-admin-password";

struct App {
    _temp: tempfile::TempDir,
    auth: Auth,
    admin: Router,
    public: Router,
    pool: SqlitePool,
}

impl App {
    async fn start() -> Self {
        Self::start_configured(AdminConfig::new(ORIGIN, 8080).unwrap()).await
    }

    async fn start_configured(config: AdminConfig) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let pool = open_database(&temp.path().join("admin.db")).await.unwrap();
        let hosts = PublicHosts::parse("go.example,s.example").unwrap();
        Self {
            _temp: temp,
            auth: Auth::new(pool.clone()),
            admin: admin_router(pool.clone(), hosts.clone(), config),
            public: public_router(pool.clone(), hosts),
            pool,
        }
    }

    async fn setup(&self) -> (String, String) {
        let token = self
            .auth
            .issue_bootstrap(BootstrapKind::Setup)
            .await
            .unwrap();
        let response = request(
            &self.admin,
            Method::POST,
            "/api/v1/auth/setup",
            &[(header::ORIGIN.as_str(), ORIGIN)],
            Some(json!({"token":token,"username":"Owner","password":PASSWORD})),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CREATED);
        let set_cookie = response.headers()[header::SET_COOKIE].to_str().unwrap();
        assert!(set_cookie.starts_with("__Host-shortcircuit_session="));
        for flag in ["; Path=/", "; HttpOnly", "; SameSite=Strict", "; Secure"] {
            assert!(set_cookie.contains(flag));
        }
        let cookie = set_cookie.split(';').next().unwrap().to_owned();
        let value = response_body(response).await;
        assert_eq!(value["username"], "Owner");
        (cookie, value["csrf"].as_str().unwrap().to_owned())
    }
}

async fn request(
    router: &Router,
    method: Method,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<Value>,
) -> Response {
    let mut builder = Request::builder().method(method).uri(path);
    if !headers.iter().any(|(name, _)| *name == "host") {
        builder = builder.header(header::HOST, HOST);
    }
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

async fn response_body(response: Response) -> Value {
    serde_json::from_slice(&to_bytes(response.into_body(), 64_000).await.unwrap()).unwrap()
}

#[tokio::test]
async fn configured_shortcode_length_applies_only_to_generated_slugs() {
    let config = AdminConfig::new(ORIGIN, 8080)
        .unwrap()
        .with_shortcode_length(16)
        .unwrap();
    let app = App::start_configured(config).await;
    let (cookie, csrf) = app.setup().await;
    let headers = [
        ("cookie", cookie.as_str()),
        ("origin", ORIGIN),
        ("x-csrf-token", csrf.as_str()),
    ];
    let generated = request(
        &app.admin,
        Method::POST,
        "/api/v1/links",
        &headers,
        Some(json!({"target":"https://example.test/generated"})),
    )
    .await;
    assert_eq!(generated.status(), StatusCode::CREATED);
    let generated = response_body(generated).await;
    let slug = generated["slug"].as_str().unwrap();
    assert_eq!(slug.len(), 16);
    assert!(
        slug.bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    );

    let custom = request(
        &app.admin,
        Method::POST,
        "/api/v1/links",
        &headers,
        Some(json!({"target":"https://example.test/custom", "slug":"Docs/ABC"})),
    )
    .await;
    assert_eq!(custom.status(), StatusCode::CREATED);
    assert_eq!(response_body(custom).await["slug"], "Docs/ABC");
}

#[test]
fn shortcode_length_rejects_values_outside_its_bounds() {
    for length in [0, 7, 33, usize::MAX] {
        assert!(
            AdminConfig::new(ORIGIN, 8080)
                .unwrap()
                .with_shortcode_length(length)
                .is_err()
        );
    }
}

#[tokio::test]
async fn catalog_search_spans_pages_and_a_deleted_cursor_anchor() {
    let app = App::start().await;
    let (cookie, csrf) = app.setup().await;
    let headers = [
        ("cookie", cookie.as_str()),
        ("origin", ORIGIN),
        ("x-csrf-token", csrf.as_str()),
    ];
    let endpoint = "/api/v1/links/search";
    assert_eq!(
        request(
            &app.admin,
            Method::POST,
            endpoint,
            &headers[..2],
            Some(json!({}))
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let other_origin = [
        ("cookie", cookie.as_str()),
        ("origin", "https://other.example"),
        ("x-csrf-token", csrf.as_str()),
    ];
    let response = request(
        &app.admin,
        Method::POST,
        endpoint,
        &other_origin,
        Some(json!({})),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(response_body(response).await["code"], "invalid_origin");
    sqlx::raw_sql(
        "WITH RECURSIVE counter(n) AS (VALUES (1) UNION ALL SELECT n + 1 FROM counter WHERE n < 102)
         INSERT INTO links(host, slug, target, title)
         SELECT 'go.example', printf('item-%03d', n), 'https://example.test/catalog', 'Same title' FROM counter;
         INSERT INTO links(host, slug, target, title, enabled)
         VALUES ('s.example', 'Release_Notes', 'https://example.test/?Signature=A%2b', 'Überblick', 0);
         INSERT INTO link_tags(link_id, tag) SELECT id, 'Änderungen' FROM links WHERE slug = 'Release_Notes';",
    )
    .execute(&app.pool)
    .await
    .unwrap();
    let search = request(
        &app.admin,
        Method::POST,
        endpoint,
        &headers,
        Some(json!({
            "query":"signature=a%2B", "host":"s.example", "enabled":false, "tag":"Änderungen"
        })),
    )
    .await;
    assert_eq!(search.status(), StatusCode::OK);
    assert_eq!(search.headers()[header::CACHE_CONTROL], "no-store");
    let result = response_body(search).await;
    assert_eq!(result["items"].as_array().unwrap().len(), 1);
    assert_eq!(result["items"][0]["slug"], "Release_Notes");
    assert!(result["next_cursor"].is_null());
    for query in ["überblick", "änderungen", "https://s.example/release"] {
        let response = request(
            &app.admin,
            Method::POST,
            endpoint,
            &headers,
            Some(json!({"query":query})),
        )
        .await;
        let body = response_body(response).await;
        assert_eq!(body["items"].as_array().unwrap().len(), 1);
        assert_eq!(body["items"][0]["slug"], "Release_Notes");
    }
    let literal = request(
        &app.admin,
        Method::POST,
        endpoint,
        &headers,
        Some(json!({"query":"%_"})),
    )
    .await;
    assert_eq!(response_body(literal).await["items"], json!([]));

    let first = request(
        &app.admin,
        Method::POST,
        endpoint,
        &headers,
        Some(json!({"sort":"title_asc","host":"go.example"})),
    )
    .await;
    let first = response_body(first).await;
    assert_eq!(first["items"].as_array().unwrap().len(), 100);
    assert_eq!(first["items"][99]["slug"], "item-100");
    sqlx::query("DELETE FROM links WHERE slug = 'item-100'")
        .execute(&app.pool)
        .await
        .unwrap();
    let next = request(
        &app.admin,
        Method::POST,
        endpoint,
        &headers,
        Some(json!({
            "sort":"title_asc","host":"go.example","after":first["next_cursor"]
        })),
    )
    .await;
    let next = response_body(next).await;
    assert_eq!(next["items"].as_array().unwrap().len(), 2);
    assert_eq!(next["items"][0]["slug"], "item-101");
    assert!(next["next_cursor"].is_null());
}

#[tokio::test]
async fn archive_and_favorite_are_independent_of_the_public_redirect() {
    let app = App::start().await;
    let (cookie, csrf) = app.setup().await;
    let headers = [
        ("cookie", cookie.as_str()),
        ("origin", ORIGIN),
        ("x-csrf-token", csrf.as_str()),
    ];
    let target = "https://example.test/?Signature=A%2f#saved";
    let create = request(
        &app.admin,
        Method::POST,
        "/api/v1/links",
        &headers,
        Some(json!({"host":"go.example", "slug":"Team/Docs", "target":target})),
    )
    .await;
    assert_eq!(create.status(), StatusCode::CREATED);
    let search = "/api/v1/links/search";
    let all = request(&app.admin, Method::POST, search, &headers, Some(json!({}))).await;
    let link = response_body(all).await["items"][0].clone();
    assert_eq!(link["favorite"], false);
    assert_eq!(link["archived"], false);
    let endpoint = format!("/api/v1/links/{}/organization", link["id"]);
    let original_key = &link["selection_key"];
    let change = json!({"key":original_key, "favorite":true, "archived":true});
    let denied = request(
        &app.admin,
        Method::PATCH,
        &endpoint,
        &headers[..2],
        Some(change.clone()),
    )
    .await;
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    let updated = request(&app.admin, Method::PATCH, &endpoint, &headers, Some(change)).await;
    assert_eq!(updated.status(), StatusCode::OK);
    assert_eq!(
        response_body(updated).await,
        json!({"favorite": true, "archived": true})
    );

    for (view, count) in [
        ("unarchived", 0),
        ("favorites", 0),
        ("archived", 1),
        ("all", 1),
    ] {
        let page = request(
            &app.admin,
            Method::POST,
            search,
            &headers,
            Some(json!({"view":view})),
        )
        .await;
        assert_eq!(page.status(), StatusCode::OK);
        assert_eq!(
            response_body(page).await["items"].as_array().unwrap().len(),
            count,
            "{view}"
        );
    }
    let redirect = request(
        &app.public,
        Method::GET,
        "/Team/Docs",
        &[("host", "go.example")],
        None,
    )
    .await;
    assert_eq!(redirect.status(), StatusCode::FOUND);
    assert_eq!(redirect.headers()[header::LOCATION], target);

    let restored = request(
        &app.admin,
        Method::PATCH,
        &endpoint,
        &headers,
        Some(json!({"key":original_key, "archived":false})),
    )
    .await;
    assert_eq!(
        response_body(restored).await,
        json!({"favorite": true, "archived": false})
    );
    let favorites = request(
        &app.admin,
        Method::POST,
        search,
        &headers,
        Some(json!({"view":"favorites"})),
    )
    .await;
    assert_eq!(
        response_body(favorites).await["items"][0]["slug"],
        "Team/Docs"
    );

    sqlx::raw_sql(
        "DELETE FROM links; INSERT INTO links(host, slug, target)
         VALUES ('go.example', 'replacement', 'https://example.test');",
    )
    .execute(&app.pool)
    .await
    .unwrap();
    let stale = request(
        &app.admin,
        Method::PATCH,
        &endpoint,
        &headers,
        Some(json!({"key":original_key, "favorite":true})),
    )
    .await;
    assert_eq!(stale.status(), StatusCode::CONFLICT);
    let favorite: bool =
        sqlx::query_scalar("SELECT favorite FROM links WHERE slug = 'replacement'")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert!(!favorite);
}

#[tokio::test]
async fn catalog_combines_multiple_tags_with_all_or_any() {
    let app = App::start().await;
    let (cookie, csrf) = app.setup().await;
    let headers = [
        ("cookie", cookie.as_str()),
        ("origin", ORIGIN),
        ("x-csrf-token", csrf.as_str()),
    ];
    sqlx::raw_sql(
        "INSERT INTO links(id, host, slug, target) VALUES
            (1, 'go.example', 'both', 'https://example.test/both'),
            (2, 'go.example', 'one', 'https://example.test/one'),
            (3, 's.example', 'elsewhere', 'https://example.test/elsewhere');
         INSERT INTO link_tags(link_id, tag) VALUES
            (1, 'docs'), (1, 'internal'), (2, 'docs'), (3, 'internal');",
    )
    .execute(&app.pool)
    .await
    .unwrap();
    for (mode, expected) in [("all", vec!["both"]), ("any", vec!["both", "one"])] {
        let response = request(
            &app.admin,
            Method::POST,
            "/api/v1/links/search",
            &headers,
            Some(json!({
                "host":"go.example", "tags":["docs", "internal", " docs "],
                "tag_match":mode, "sort":"slug_asc"
            })),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let payload = response_body(response).await;
        let slugs: Vec<_> = payload["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["slug"].as_str().unwrap())
            .collect();
        assert_eq!(slugs, expected);
    }
}

const QR_TEMPLATES: &str = "/api/v1/qr/templates";

#[tokio::test]
async fn crawler_controls_are_host_bound_and_preserve_direct_redirects() {
    let app = App::start().await;
    let (cookie, csrf) = app.setup().await;
    let headers = [
        ("cookie", cookie.as_str()),
        ("origin", ORIGIN),
        ("x-csrf-token", csrf.as_str()),
    ];
    let saved = request(
        &app.admin,
        Method::POST,
        "/api/v1/catalog/domains",
        &headers,
        Some(json!({"host":"go.example", "origin":"https://go.example",
                    "enabled":true, "block_crawlers":true})),
    )
    .await;
    assert_eq!(saved.status(), StatusCode::OK);
    let updated = request(
        &app.admin,
        Method::POST,
        "/api/v1/catalog/domains",
        &headers,
        Some(json!({"host":"go.example", "origin":"https://go.example", "enabled":true})),
    )
    .await;
    assert_eq!(response_body(updated).await["block_crawlers"], true);
    let robots = "/robots.txt";
    for (host, expected) in [
        ("go.example", "User-agent: *\nDisallow: /\n"),
        ("s.example", "User-agent: *\nDisallow:\n"),
    ] {
        let response = request(&app.public, Method::GET, robots, &[("host", host)], None).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "text/plain; charset=utf-8"
        );
        assert_eq!(to_bytes(response.into_body(), 256).await.unwrap(), expected);
    }
    let unknown = request(
        &app.public,
        Method::GET,
        robots,
        &[("host", "other.example")],
        None,
    )
    .await;
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
    let head = request(
        &app.public,
        Method::HEAD,
        robots,
        &[("host", "go.example")],
        None,
    )
    .await;
    assert_eq!(head.status(), StatusCode::OK);
    assert!(to_bytes(head.into_body(), 256).await.unwrap().is_empty());
    let created = request(
        &app.admin,
        Method::POST,
        "/api/v1/links",
        &headers,
        Some(json!({"host":"s.example", "slug":"search", "target":"https://example.test/notes"})),
    )
    .await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let id = response_body(created).await["id"].as_i64().unwrap();
    let policy = request(
        &app.admin,
        Method::PATCH,
        &format!("/api/v1/links/{id}/policy"),
        &headers,
        Some(json!({"noindex":true})),
    )
    .await;
    assert_eq!(policy.status(), StatusCode::OK);
    assert_eq!(response_body(policy).await["noindex"], true);
    let updated = request(
        &app.admin,
        Method::PATCH,
        &format!("/api/v1/links/{id}/policy"),
        &headers,
        Some(json!({"status":302})),
    )
    .await;
    assert_eq!(response_body(updated).await["noindex"], true);
    for method in [Method::GET, Method::HEAD] {
        let redirect = request(
            &app.public,
            method,
            "/search",
            &[("host", "s.example")],
            None,
        )
        .await;
        assert_eq!(redirect.status(), StatusCode::FOUND);
        assert_eq!(
            redirect.headers()[header::LOCATION],
            "https://example.test/notes"
        );
        assert_eq!(redirect.headers()["x-robots-tag"], "noindex, nofollow");
    }
}

#[tokio::test]
async fn simulator_checks_draft_rule_precedence_and_leaves_the_live_link_unchanged() {
    let app = App::start().await;
    let (cookie, csrf) = app.setup().await;
    let headers = [
        ("cookie", cookie.as_str()),
        ("origin", ORIGIN),
        ("x-csrf-token", csrf.as_str()),
    ];
    let created = request(
        &app.admin,
        Method::POST,
        "/api/v1/links",
        &headers,
        Some(json!({"slug":"docs","target":"https://example.test/default"})),
    )
    .await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let id = response_body(created).await["id"].as_i64().unwrap();
    let path = format!("/api/v1/links/{id}/policy");
    let saved = request(
        &app.admin,
        Method::PATCH,
        &path,
        &headers,
        Some(json!({"max_visits":1})),
    )
    .await;
    assert_eq!(saved.status(), StatusCode::OK);
    let preview = format!("{path}/simulate");
    let draft = json!({
        "policy":{"status":307,"forward_query":true,"forward_path":true,"rules":[
            {"target":"https://example.test/first?base=1#keep",
             "conditions":[{"type":"language","value":"de"},{"type":"query","key":"team","value":"ops"}]},
            {"target":"https://example.test/second","conditions":[{"type":"language","value":"de"}]}
        ]},
        "accept_language":"de-AT,en;q=0.7", "query":"team=ops&sig=A%2f", "extra_path":"Guide/PDF"
    });
    let denied = request(
        &app.admin,
        Method::POST,
        &preview,
        &headers[..2],
        Some(draft.clone()),
    )
    .await;
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    let result = request(&app.admin, Method::POST, &preview, &headers, Some(draft)).await;
    assert_eq!(result.status(), StatusCode::OK);
    assert_eq!(
        response_body(result).await,
        json!({
            "target":"https://example.test/first/Guide/PDF?base=1&team=ops&sig=A%2f#keep",
            "status":307,"rule":1
        })
    );
    let fallback = request(
        &app.admin,
        Method::POST,
        &preview,
        &headers,
        Some(json!({"policy":{}})),
    )
    .await;
    assert_eq!(
        response_body(fallback).await,
        json!({
            "target":"https://example.test/default","status":302,"rule":null
        })
    );
    let long_rule = json!({
        "target":format!("https://example.test/{}", "a".repeat(3980)),
        "conditions":[{"type":"language","value":"de"}]
    });
    let large_draft = json!({
        "policy":{"rules":vec![long_rule; 7]},
        "query":format!("q={}", "a".repeat(4094)),
        "user_agent":"a".repeat(1024)
    });
    assert!(large_draft.to_string().len() > 32 * 1024);
    let result = request(
        &app.admin,
        Method::POST,
        &preview,
        &headers,
        Some(large_draft),
    )
    .await;
    assert_eq!(result.status(), StatusCode::OK);
    let policy = request(&app.admin, Method::GET, &path, &headers, None).await;
    let policy = response_body(policy).await;
    assert_eq!(policy["visits_used"], 0);
    assert_eq!(policy["rules"], json!([]));
    let live = request(
        &app.public,
        Method::GET,
        "/docs",
        &[("host", "go.example")],
        None,
    )
    .await;
    assert_eq!(
        live.headers()[header::LOCATION],
        "https://example.test/default"
    );
}

#[tokio::test]
async fn link_descriptions_are_searchable_versioned_and_optional() {
    let app = App::start().await;
    let (cookie, csrf) = app.setup().await;
    let headers = [
        ("cookie", cookie.as_str()),
        ("origin", ORIGIN),
        ("x-csrf-token", csrf.as_str()),
    ];
    let created = request(
        &app.admin,
        Method::POST,
        "/api/v1/links",
        &headers,
        Some(json!({
            "slug":"notes", "target":"https://example.test/guide",
            "description":"  Übergabe\nTeam  ", "enabled":false, "analytics_enabled":false
        })),
    )
    .await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let link = response_body(created).await;
    assert_eq!(link["description"], "Übergabe\nTeam");
    assert_eq!(link["enabled"], false);
    assert_eq!(link["analytics_enabled"], false);
    let redirect = request(
        &app.public,
        Method::GET,
        "/notes",
        &[("host", "go.example")],
        None,
    )
    .await;
    assert_eq!(redirect.status(), StatusCode::NOT_FOUND);

    let found = request(
        &app.admin,
        Method::POST,
        "/api/v1/links/search",
        &headers,
        Some(json!({"query":"übergabe"})),
    )
    .await;
    assert_eq!(response_body(found).await["items"][0]["id"], link["id"]);
    let endpoint = format!("/api/v1/links/{}", link["id"]);
    let invalid = request(
        &app.admin,
        Method::PATCH,
        &endpoint,
        &headers,
        Some(json!({"description":"ü".repeat(501)})),
    )
    .await;
    assert_eq!(invalid.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(response_body(invalid).await["code"], "invalid_description");
    let cleared = request(
        &app.admin,
        Method::PATCH,
        &endpoint,
        &headers,
        Some(json!({"description":""})),
    )
    .await;
    assert_eq!(response_body(cleared).await["description"], "");
    let history_path = format!("{endpoint}/history");
    let history = request(&app.admin, Method::GET, &history_path, &headers, None).await;
    let history = response_body(history).await;
    assert_eq!(
        history["items"][0]["previous"]["value"]["description"],
        "Übergabe\nTeam"
    );
    let restore_path = format!("{history_path}/{}/restore", history["items"][0]["id"]);
    let restored = request(&app.admin, Method::POST, &restore_path, &headers, None).await;
    assert_eq!(restored.status(), StatusCode::NO_CONTENT);
    let matching = request(
        &app.admin,
        Method::POST,
        "/api/v1/links/search",
        &headers,
        Some(json!({"query":"übergabe"})),
    )
    .await;
    assert_eq!(
        response_body(matching).await["items"][0]["description"],
        "Übergabe\nTeam"
    );
    let legacy = json!({"kind":"metadata","value":{
        "target":"https://example.test/guide","title":"Legacy title","tags":[],
        "enabled":false,"analytics_enabled":false
    }});
    let legacy_id: i64 = sqlx::query_scalar(
        "INSERT INTO link_history(link_id, actor_kind, actor_label, action, previous)
         VALUES (?, 'admin', 'Owner', 'metadata', ?) RETURNING id",
    )
    .bind(link["id"].as_i64().unwrap())
    .bind(legacy.to_string())
    .fetch_one(&app.pool)
    .await
    .unwrap();
    let restored = request(
        &app.admin,
        Method::POST,
        &format!("{history_path}/{legacy_id}/restore"),
        &headers,
        None,
    )
    .await;
    assert_eq!(restored.status(), StatusCode::NO_CONTENT);
    let current: (String, String) =
        sqlx::query_as("SELECT title, description FROM links WHERE slug = 'notes'")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(
        current,
        ("Legacy title".to_owned(), "Übergabe\nTeam".to_owned())
    );
}

#[tokio::test]
async fn catalog_usage_counts_retained_redirects_and_reports_last_available_precision() {
    let app = App::start().await;
    let (cookie, csrf) = app.setup().await;
    let headers = [
        ("cookie", cookie.as_str()),
        ("origin", ORIGIN),
        ("x-csrf-token", csrf.as_str()),
    ];
    sqlx::raw_sql(
        "INSERT INTO links(id, host, slug, target) VALUES
            (1, 'go.example', 'precise', 'https://example.test/one'),
            (2, 'go.example', 'aggregate', 'https://example.test/two'),
            (3, 'go.example', 'unused', 'https://example.test/three');
         INSERT INTO analytics_daily(day, host, link_id, kind, visit_class, total) VALUES
            (2, 'go.example', 1, 'redirect', 'human', 2),
            (2, 'go.example', 1, 'redirect', 'bot', 1),
            (3, 'go.example', 1, 'blocked', 'human', 6),
            (1, 'go.example', 2, 'redirect', 'human', 7);
         INSERT INTO analytics_events(occurred_at, host, link_id, kind, visit_class, source) VALUES
            (172801, 'go.example', 1, 'redirect', 'human', 'other'),
            (172805, 'go.example', 1, 'redirect', 'human', 'other'),
            (259201, 'go.example', 1, 'blocked', 'human', 'other');",
    )
    .execute(&app.pool)
    .await
    .unwrap();
    let page = request(
        &app.admin,
        Method::POST,
        "/api/v1/links/search",
        &headers,
        Some(json!({"sort":"slug_asc"})),
    )
    .await;
    assert_eq!(page.status(), StatusCode::OK);
    let body = response_body(page).await;
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 3);
    assert_eq!(items[0]["slug"], "aggregate");
    assert_eq!(items[0]["recorded_redirects"], 7);
    assert_eq!(items[0]["last_redirect_date"], "1970-01-02");
    assert!(items[0]["last_redirect_at"].is_null());
    assert_eq!(items[1]["slug"], "precise");
    assert_eq!(items[1]["recorded_redirects"], 3);
    assert_eq!(items[1]["last_redirect_date"], "1970-01-03");
    assert_eq!(items[1]["last_redirect_at"], 172805);
    assert_eq!(items[2]["recorded_redirects"], 0);
    assert!(items[2]["last_redirect_date"].is_null());
    assert!(items[2]["last_redirect_at"].is_null());
}

#[tokio::test]
async fn selected_link_operations_are_atomic_and_protected_exports_are_disabled() {
    let app = App::start().await;
    let (cookie, csrf) = app.setup().await;
    let headers = [
        ("cookie", cookie.as_str()),
        ("origin", ORIGIN),
        ("x-csrf-token", csrf.as_str()),
    ];
    sqlx::raw_sql(
        "INSERT INTO links(id, host, slug, target, title) VALUES
            (1, 'go.example', 'first', 'https://example.test/first', '=Sample'),
            (2, 's.example', 'second', 'https://example.test/second', '');
         INSERT INTO link_protection(link_id, one_time) VALUES (2, 1);
         WITH RECURSIVE counter(n) AS (VALUES (1) UNION ALL SELECT n + 1 FROM counter WHERE n < 20)
         INSERT INTO link_tags(link_id, tag) SELECT 2, printf('tag%02d', n) FROM counter;",
    )
    .execute(&app.pool)
    .await
    .unwrap();
    let catalog = request(
        &app.admin,
        Method::POST,
        "/api/v1/links/search",
        &headers,
        Some(json!({})),
    )
    .await;
    let catalog = response_body(catalog).await;
    let selection: Vec<_> = catalog["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|link| {
            assert_eq!(link["selection_key"].as_str().unwrap_or("").len(), 32);
            json!({"id":link["id"], "key":link["selection_key"]})
        })
        .collect();
    let bulk = "/api/v1/links/bulk";
    let action = |action: Value| json!({"links":selection, "change":action});
    let overflow = request(
        &app.admin,
        Method::POST,
        bulk,
        &headers,
        Some(action(json!({"action":"add_tags", "tags":["new"]}))),
    )
    .await;
    assert_eq!(overflow.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(response_body(overflow).await["code"], "bulk_tag_limit");
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM link_tags WHERE tag = 'new'")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    let export = request(
        &app.admin,
        Method::POST,
        "/api/v1/links/bulk/export",
        &headers,
        Some(json!({"links":selection, "format":"json"})),
    )
    .await;
    assert_eq!(export.status(), StatusCode::OK);
    let export = response_body(export).await;
    assert_eq!(export["version"], 1);
    assert_eq!(export["items"][0]["slug"], "first");
    assert_eq!(export["items"][0]["enabled"], true);
    assert_eq!(export["items"][1]["enabled"], false);
    assert!(export["items"][1].get("id").is_none());
    let csv = request(
        &app.admin,
        Method::POST,
        "/api/v1/links/bulk/export",
        &headers,
        Some(json!({"links":selection, "format":"csv"})),
    )
    .await;
    assert_eq!(csv.status(), StatusCode::OK);
    let csv = to_bytes(csv.into_body(), 32_768).await.unwrap();
    assert!(std::str::from_utf8(&csv).unwrap().contains("\"'=Sample\""));

    let change = request(
        &app.admin,
        Method::POST,
        bulk,
        &headers,
        Some(action(json!({"action":"set_enabled", "enabled":false}))),
    )
    .await;
    assert_eq!(change.status(), StatusCode::OK);
    assert_eq!(response_body(change).await["items"][1]["enabled"], false);
    let history_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM link_history WHERE action = 'metadata' AND actor_label = 'Owner'",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(history_count, 2);
    let delete = request(
        &app.admin,
        Method::POST,
        bulk,
        &headers,
        Some(action(json!({"action":"delete", "confirmed":false}))),
    )
    .await;
    assert_eq!(delete.status(), StatusCode::UNPROCESSABLE_ENTITY);

    sqlx::raw_sql(
        "DELETE FROM links WHERE id = 2;
         INSERT INTO links(id, host, slug, target) VALUES (2, 's.example', 'second', 'https://example.test/recreated');",
    ).execute(&app.pool).await.unwrap();
    let stale = request(
        &app.admin,
        Method::POST,
        bulk,
        &headers,
        Some(action(json!({"action":"delete", "confirmed":true}))),
    )
    .await;
    assert_eq!(stale.status(), StatusCode::CONFLICT);
    assert_eq!(response_body(stale).await["code"], "bulk_selection_changed");
    let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM links")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(remaining, 2);
}

fn qr_template(name: &str) -> Value {
    json!({
        "name": name, "size": 512, "foreground": "002040", "background": "FFF4E0",
        "transparent": true, "correction": "H", "quiet_zone": 8,
    })
}

#[tokio::test]
async fn qr_templates_are_private_and_validate_styles_before_storing_them() {
    let app = App::start().await;
    let (cookie, csrf) = app.setup().await;
    let headers = [
        ("cookie", cookie.as_str()),
        ("origin", ORIGIN),
        ("x-csrf-token", csrf.as_str()),
    ];
    assert_eq!(
        request(
            &app.public,
            Method::GET,
            QR_TEMPLATES,
            &[("host", "go.example")],
            None
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(&app.admin, Method::GET, QR_TEMPLATES, &[], None)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(
            &app.admin,
            Method::POST,
            QR_TEMPLATES,
            &headers[..2],
            Some(qr_template("Print"))
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );

    for (field, value, code) in [
        ("name", json!("  "), "invalid_qr_template_name"),
        ("foreground", json!("eeeeee"), "invalid_qr_contrast"),
        ("background", json!("invalid"), "invalid_qr_color"),
        ("quiet_zone", json!(3), "invalid_qr_quiet_zone"),
        ("correction", json!("X"), "invalid_qr_correction"),
        ("frame", json!("custom"), "invalid_qr_frame"),
        ("size", json!(127), "invalid_qr_size"),
    ] {
        let mut input = qr_template("Print");
        input[field] = value;
        let response = request(
            &app.admin,
            Method::POST,
            QR_TEMPLATES,
            &headers,
            Some(input),
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(response_body(response).await["code"], code);
    }

    let created = request(
        &app.admin,
        Method::POST,
        QR_TEMPLATES,
        &headers,
        Some(qr_template("  Druckvorlage Ä  ")),
    )
    .await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let created = response_body(created).await;
    let mut expected = qr_template("Druckvorlage Ä");
    expected["id"] = created["id"].clone();
    expected["background"] = json!("fff4e0");
    expected["frame"] = json!("none");
    assert_eq!(created, expected);
    let duplicate = request(
        &app.admin,
        Method::POST,
        QR_TEMPLATES,
        &headers,
        Some(qr_template("Druckvorlage Ä")),
    )
    .await;
    assert_eq!(duplicate.status(), StatusCode::CONFLICT);
    assert_eq!(response_body(duplicate).await["code"], "qr_template_exists");

    let listed = request(&app.admin, Method::GET, QR_TEMPLATES, &headers[..1], None).await;
    assert_eq!(response_body(listed).await, json!({"items":[expected]}));

    let id = created["id"].as_i64().unwrap();
    let resource = format!("{QR_TEMPLATES}/{id}");
    for status in [StatusCode::NO_CONTENT, StatusCode::NOT_FOUND] {
        assert_eq!(
            request(&app.admin, Method::DELETE, &resource, &headers, None)
                .await
                .status(),
            status
        );
    }
}

#[tokio::test]
async fn simultaneous_qr_template_writes_respect_the_installation_limit() {
    let app = App::start().await;
    let (cookie, csrf) = app.setup().await;
    let headers = [
        ("cookie", cookie.as_str()),
        ("origin", ORIGIN),
        ("x-csrf-token", csrf.as_str()),
    ];
    sqlx::query(
        "WITH RECURSIVE counter(n) AS (VALUES (1) UNION ALL SELECT n + 1 FROM counter WHERE n < 31)
         INSERT INTO qr_templates (name, size, foreground, background, transparent, correction, quiet_zone)
         SELECT 'Template ' || n, 512, '000000', 'ffffff', 0, 'M', 4 FROM counter",
    )
    .execute(&app.pool)
    .await
    .unwrap();
    let (first, second) = tokio::join!(
        request(
            &app.admin,
            Method::POST,
            QR_TEMPLATES,
            &headers,
            Some(qr_template("Print"))
        ),
        request(
            &app.admin,
            Method::POST,
            QR_TEMPLATES,
            &headers,
            Some(qr_template("Screen"))
        ),
    );
    let (created, limited) = if first.status() == StatusCode::CREATED {
        (first, second)
    } else {
        (second, first)
    };
    assert_eq!(created.status(), StatusCode::CREATED);
    assert_eq!(limited.status(), StatusCode::CONFLICT);
    assert_eq!(response_body(limited).await["code"], "qr_template_limit");
}

#[tokio::test]
async fn first_run_is_hidden_on_public_and_rejects_other_origins_and_hosts() {
    let app = App::start().await;
    let public = request(
        &app.public,
        Method::GET,
        "/api/v1/auth/session",
        &[("host", "go.example")],
        None,
    )
    .await;
    assert_eq!(public.status(), StatusCode::NOT_FOUND);
    let invalid_host = request(
        &app.admin,
        Method::GET,
        "/api/v1/auth/session",
        &[("host", "wrong.example"), ("x-forwarded-host", HOST)],
        None,
    )
    .await;
    assert_eq!(invalid_host.status(), StatusCode::NOT_FOUND);
    let session = request(&app.admin, Method::GET, "/api/v1/auth/session", &[], None).await;
    assert_eq!(session.headers()[header::X_FRAME_OPTIONS], "DENY");
    let session = response_body(session).await;
    assert_eq!(session["setup_required"], true);
    assert!(session["user"].is_null());

    let body = json!({"token":"invalid","username":"Owner","password":PASSWORD});
    for headers in [
        vec![],
        vec![("origin", "https://untrusted.example")],
        vec![("origin", "http://admin.example")],
    ] {
        assert_eq!(
            request(
                &app.admin,
                Method::POST,
                "/api/v1/auth/setup",
                &headers,
                Some(body.clone())
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
    }
}

#[tokio::test]
async fn session_mutations_require_csrf_and_logout_revokes_the_cookie() {
    let app = App::start().await;
    let (cookie, csrf) = app.setup().await;
    let status = request(
        &app.admin,
        Method::GET,
        "/api/v1/auth/session",
        &[("cookie", &cookie)],
        None,
    )
    .await;
    let value = response_body(status).await;
    assert_eq!(value["user"]["csrf"], csrf);
    assert_eq!(value["setup_required"], false);

    let body = json!({"target":"https://example.test/docs","slug":"docs", "title":"Docs","tags":["manual","manual"]});
    let weak = [("cookie", cookie.as_str()), ("origin", ORIGIN)];
    assert_eq!(
        request(
            &app.admin,
            Method::POST,
            "/api/v1/links",
            &weak,
            Some(body.clone())
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let foreign = [
        ("cookie", cookie.as_str()),
        ("origin", "https://wrong.example"),
        ("x-csrf-token", csrf.as_str()),
    ];
    assert_eq!(
        request(
            &app.admin,
            Method::POST,
            "/api/v1/links",
            &foreign,
            Some(body.clone())
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let authorized = [
        ("cookie", cookie.as_str()),
        ("origin", ORIGIN),
        ("x-csrf-token", csrf.as_str()),
    ];
    let created = request(
        &app.admin,
        Method::POST,
        "/api/v1/links",
        &authorized,
        Some(body),
    )
    .await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let created = response_body(created).await;
    assert_eq!(created["tags"], json!(["manual"]));
    let path = format!("/api/v1/links/{}", created["id"].as_i64().unwrap());
    assert_eq!(
        request(
            &app.admin,
            Method::PATCH,
            &path,
            &authorized,
            Some(json!({"enabled":false}))
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        request(
            &app.public,
            Method::GET,
            "/docs",
            &[("host", "go.example")],
            None
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &app.admin,
            Method::PATCH,
            &path,
            &authorized,
            Some(json!({"enabled":true}))
        )
        .await
        .status(),
        StatusCode::OK
    );
    let redirect = request(
        &app.public,
        Method::GET,
        "/docs",
        &[("host", "go.example")],
        None,
    )
    .await;
    assert_eq!(
        redirect.headers()[header::LOCATION],
        "https://example.test/docs"
    );
    assert_eq!(
        request(&app.admin, Method::DELETE, &path, &authorized, None)
            .await
            .status(),
        StatusCode::NO_CONTENT
    );

    let logout = request(
        &app.admin,
        Method::POST,
        "/api/v1/auth/logout",
        &authorized,
        None,
    )
    .await;
    assert_eq!(logout.status(), StatusCode::NO_CONTENT);
    assert!(
        logout.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .contains("Max-Age=0")
    );
    assert_eq!(
        request(
            &app.admin,
            Method::GET,
            "/api/v1/links",
            &[("cookie", &cookie)],
            None
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn scoped_api_keys_cannot_manage_keys_or_access_another_domain() {
    let app = App::start().await;
    let (cookie, csrf) = app.setup().await;
    let session = [
        ("cookie", cookie.as_str()),
        ("origin", ORIGIN),
        ("x-csrf-token", csrf.as_str()),
    ];
    let key = request(
        &app.admin,
        Method::POST,
        "/api/v1/keys",
        &session,
        Some(json!({"name":"Scoped Key","scopes":["read","write"],"host":"s.example"})),
    )
    .await;
    assert_eq!(key.status(), StatusCode::CREATED);
    let issued = response_body(key).await;
    let bearer = format!("Bearer {}", issued["token"].as_str().unwrap());
    let headers = [("authorization", bearer.as_str())];
    let denied = request(
        &app.admin,
        Method::POST,
        "/api/v1/links",
        &headers,
        Some(json!({"host":"go.example","target":"https://example.test/"})),
    )
    .await;
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    let allowed = request(
        &app.admin,
        Method::POST,
        "/api/v1/links",
        &headers,
        Some(json!({"target":"https://example.test/"})),
    )
    .await;
    assert_eq!(allowed.status(), StatusCode::CREATED);
    let link = response_body(allowed).await;
    assert_eq!(link["host"], "s.example");
    let id = link["id"].as_i64().unwrap();
    let own_policy = format!("/api/v1/links/{id}/policy");
    let own_protection = format!("/api/v1/links/{id}/protection");
    assert_eq!(
        request(
            &app.admin,
            Method::PATCH,
            &own_policy,
            &headers,
            Some(json!({"status": 308})),
        )
        .await
        .status(),
        StatusCode::OK
    );
    let conflict = request(
        &app.admin,
        Method::PATCH,
        &own_protection,
        &headers,
        Some(json!({"one_time": true})),
    )
    .await;
    assert_eq!(conflict.status(), StatusCode::CONFLICT);
    assert_eq!(
        response_body(conflict).await["code"],
        "protection_requires_temporary_redirect"
    );

    let outside_scope = request(
        &app.admin,
        Method::POST,
        "/api/v1/links",
        &session,
        Some(json!({"host": "go.example", "target": "https://example.test/"})),
    )
    .await;
    assert_eq!(outside_scope.status(), StatusCode::CREATED);
    let another_id = response_body(outside_scope).await["id"].as_i64().unwrap();
    let outside_protection = format!("/api/v1/links/{another_id}/protection");
    assert_eq!(
        request(&app.admin, Method::GET, &outside_protection, &headers, None)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &app.admin,
            Method::PATCH,
            &outside_protection,
            &headers,
            Some(json!({"one_time": true})),
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &app.admin,
            Method::DELETE,
            &format!("/api/v1/links/{id}"),
            &headers,
            None
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(&app.admin, Method::GET, "/api/v1/keys", &headers, None)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let listing = request(
        &app.admin,
        Method::GET,
        "/api/v1/keys",
        &[("cookie", cookie.as_str())],
        None,
    )
    .await;
    assert!(
        !response_body(listing)
            .await
            .to_string()
            .contains(issued["token"].as_str().unwrap())
    );
    let id = issued["key"]["id"].as_i64().unwrap();
    assert_eq!(
        request(
            &app.admin,
            Method::DELETE,
            &format!("/api/v1/keys/{id}"),
            &session,
            None
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(&app.admin, Method::GET, "/api/v1/links", &headers, None)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn rotation_requires_browser_reauthentication_and_replaces_the_key_once() {
    let app = App::start().await;
    let (cookie, csrf) = app.setup().await;
    let browser = [
        ("cookie", cookie.as_str()),
        ("origin", ORIGIN),
        ("x-csrf-token", csrf.as_str()),
    ];
    let expires_at: i64 = sqlx::query_scalar("SELECT unixepoch() + 3600")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    let old_key = app
        .auth
        .create_key(
            "Scoped key",
            vec!["read".into()],
            Some("go.example".into()),
            Some(expires_at),
        )
        .await
        .unwrap();
    let id = old_key.key.id;
    let path = format!("/api/v1/keys/{id}/rotate");
    let password = json!({"current_password": PASSWORD});
    let old_bearer = format!("Bearer {}", old_key.token);
    for headers in [&[][..], &[("authorization", old_bearer.as_str())][..]] {
        assert_eq!(
            request(&app.admin, Method::POST, &path, headers, Some(password.clone()))
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            request(&app.admin, Method::GET, "/api/v1/audit", headers, None)
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        request(
            &app.admin,
            Method::POST,
            &path,
            &browser[..2],
            Some(password.clone()),
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let rejected = request(
        &app.admin,
        Method::POST,
        &path,
        &browser,
        Some(json!({"current_password": "synthetic-wrong-password"})),
    )
    .await;
    assert_eq!(rejected.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(response_body(rejected).await["code"], "invalid_credentials");
    assert!(app.auth.key(&old_key.token).await.is_ok());

    let (first, second) = tokio::join!(
        request(&app.admin, Method::POST, &path, &browser, Some(password.clone())),
        request(&app.admin, Method::POST, &path, &browser, Some(password)),
    );
    let (created, absent) = if first.status() == StatusCode::CREATED {
        (first, second)
    } else {
        (second, first)
    };
    assert_eq!(created.status(), StatusCode::CREATED);
    assert_eq!(absent.status(), StatusCode::NOT_FOUND);
    assert_eq!(created.headers()[header::CACHE_CONTROL], "no-store");
    let replacement = response_body(created).await;
    let replacement_token = replacement["token"].as_str().unwrap();
    assert_ne!(replacement["key"]["id"], id);
    assert_eq!(replacement["key"]["name"], "Scoped key");
    assert_eq!(replacement["key"]["host"], "go.example");
    assert_eq!(replacement["key"]["scopes"], json!(["read"]));
    assert_eq!(replacement["key"]["expires_at"], expires_at);
    assert!(app.auth.key(&old_key.token).await.is_err());
    assert!(app.auth.key(replacement_token).await.is_ok());
    assert_eq!(app.auth.list_keys().await.unwrap().len(), 1);

    let audit = request(
        &app.admin,
        Method::GET,
        "/api/v1/audit",
        &browser[..1],
        None,
    )
    .await;
    assert_eq!(audit.status(), StatusCode::OK);
    let audit = response_body(audit).await;
    let rows = audit["items"].as_array().unwrap();
    for outcome in ["success", "rejected"] {
        assert!(rows.iter().any(|row| {
            row["action"] == "key_rotate"
                && row["actor"] == "admin"
                && row["actor_id"] == 1
                && row["object_id"] == id
                && row["outcome"] == outcome
        }));
    }
    let text = audit.to_string();
    for secret in [old_key.token.as_str(), replacement_token, PASSWORD] {
        assert!(!text.contains(secret));
    }
}

#[tokio::test]
async fn admin_api_limit_covers_session_status_and_supplies_retry_after() {
    let config = AdminConfig::new(ORIGIN, 8080).unwrap().with_access_controls(
        TrustedProxies::default(),
        RateLimit::PASSWORD_DEFAULT,
        RateLimit::SENSITIVE_DEFAULT,
        RateLimit::new(1, 10).unwrap(),
    );
    let app = App::start_configured(config).await;
    let path = "/api/v1/auth/session";
    let first = request(&app.admin, Method::GET, path, &[], None).await;
    assert_eq!(first.status(), StatusCode::OK);
    let limited = request(
        &app.admin,
        Method::GET,
        path,
        &[("x-forwarded-for", "192.0.2.3")],
        None,
    )
    .await;
    assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(limited.headers()[header::RETRY_AFTER], "60");
    assert_eq!(response_body(limited).await["code"], "try_again_later");
}

#[tokio::test]
async fn protected_link_creation_and_updates_keep_secrets_out_of_responses() {
    let app = App::start().await;
    let (cookie, csrf) = app.setup().await;
    let headers = [
        ("cookie", cookie.as_str()),
        ("origin", ORIGIN),
        ("x-csrf-token", csrf.as_str()),
    ];
    let invalid = request(
        &app.admin,
        Method::POST,
        "/api/v1/links",
        &headers,
        Some(json!({
            "slug": "Claim",
            "target": "https://example.test/",
            "protection": {"password": "short", "one_time": true}
        })),
    )
    .await;
    assert_eq!(invalid.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        response_body(invalid).await["code"],
        "invalid_link_password"
    );

    let password = "synthetic-link-password";
    let created = request(
        &app.admin,
        Method::POST,
        "/api/v1/links",
        &headers,
        Some(json!({
            "slug": "Claim",
            "target": "https://example.test/",
            "protection": {"password": password, "one_time": true}
        })),
    )
    .await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let created = response_body(created).await;
    let id = created["id"].as_i64().unwrap();
    let path = format!("/api/v1/links/{id}/protection");
    let stored: String =
        sqlx::query_scalar("SELECT password_hash FROM link_protection WHERE link_id = ?")
            .bind(id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert!(stored.starts_with("$argon2id$"));
    assert!(!created.to_string().contains(password));
    assert!(!created.to_string().contains(stored.as_str()));
    let response = request(&app.admin, Method::GET, &path, &headers, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response_body(response).await,
        json!({"password_required": true, "one_time": true, "consumed": false})
    );

    let response = request(
        &app.admin,
        Method::PATCH,
        &path,
        &headers,
        Some(json!({"one_time": true, "remove_password": true})),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response_body(response).await,
        json!({"password_required": false, "one_time": true, "consumed": false})
    );
    let revision: i64 =
        sqlx::query_scalar("SELECT revision FROM link_protection WHERE link_id = ?")
            .bind(id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(revision, 2);

    sqlx::query("UPDATE link_protection SET consumed = 1 WHERE link_id = ?")
        .bind(id)
        .execute(&app.pool)
        .await
        .unwrap();
    let response = request(
        &app.admin,
        Method::PATCH,
        &path,
        &headers,
        Some(json!({"one_time": false})),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(response_body(response).await["code"], "one_time_consumed");
}

#[tokio::test]
async fn unsafe_configuration_and_repeated_login_attempts_are_bounded() {
    assert!(AdminConfig::new("http://admin.example", 8080).is_err());
    assert!(AdminConfig::new("https://admin.example/extra", 8080).is_err());
    assert!(AdminConfig::new("http://[::1]:8081", 8080).is_ok());
    let app = App::start().await;
    for _ in 0..8 {
        let response = request(
            &app.admin,
            Method::POST,
            "/api/v1/auth/login",
            &[("origin", ORIGIN)],
            Some(json!({"username":"Owner","password":PASSWORD})),
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    let limited = request(
        &app.admin,
        Method::POST,
        "/api/v1/auth/login",
        &[("origin", ORIGIN)],
        Some(json!({"username":"Owner","password":PASSWORD})),
    )
    .await;
    assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(limited.headers()[header::RETRY_AFTER], "60");
}
