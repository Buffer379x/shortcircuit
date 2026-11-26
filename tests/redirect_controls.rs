use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
    response::Response,
};
use serde_json::{Value, json};
use shortcircuit::{
    AdminConfig, Auth, BootstrapKind, PublicHosts, admin_router, open_database,
    public_router_on_port,
};
use tower::ServiceExt;

const ADMIN: &str = "admin.example";
const ORIGIN: &str = "https://admin.example";

struct Fixture {
    _directory: tempfile::TempDir,
    public: Router,
    admin: Router,
    cookie: String,
    csrf: String,
    auth: Auth,
}

impl Fixture {
    async fn start() -> Self {
        Self::on_port("go.example,s.example", 8080).await
    }

    async fn on_port(hosts: &str, port: u16) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let pool = open_database(&directory.path().join("test.db"))
            .await
            .unwrap();
        let hosts = PublicHosts::parse(hosts).unwrap();
        let auth = Auth::new(pool.clone());
        let claim = auth.issue_bootstrap(BootstrapKind::Setup).await.unwrap();
        let session = auth
            .setup(&claim, "Owner", "synthetic-test-password")
            .await
            .unwrap();
        let public = public_router_on_port(pool.clone(), hosts.clone(), port, None);
        let admin = admin_router(pool, hosts, AdminConfig::new(ORIGIN, port).unwrap());
        Self {
            _directory: directory,
            auth,
            admin,
            public,
            cookie: format!("__Host-shortcircuit_session={}", session.token),
            csrf: session.session.csrf,
        }
    }

    async fn admin_request(&self, method: Method, path: &str, body: Option<Value>) -> Response {
        send(
            &self.admin,
            method,
            path,
            &[
                ("host", ADMIN),
                ("origin", ORIGIN),
                ("cookie", &self.cookie),
                ("x-csrf-token", &self.csrf),
            ],
            body,
        )
        .await
    }

    async fn create(&self, host: &str, slug: &str, target: &str, tags: &[&str]) -> i64 {
        let response = self
            .admin_request(
                Method::POST,
                "/api/v1/links",
                Some(json!({
                    "host":host,"slug":slug,"target":target,"tags":tags
                })),
            )
            .await;
        assert_eq!(response.status(), StatusCode::CREATED);
        read_json(response).await["id"].as_i64().unwrap()
    }

    async fn visit(&self, host: &str, method: Method, path: &str) -> Response {
        send(&self.public, method, path, &[("host", host)], None).await
    }
}

async fn send(
    router: &Router,
    method: Method,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<Value>,
) -> Response {
    let mut request = Request::builder().method(method).uri(path);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let body = match body {
        Some(data) => {
            request = request.header(header::CONTENT_TYPE, "application/json");
            Body::from(data.to_string())
        }
        None => Body::empty(),
    };
    router
        .clone()
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap()
}

async fn read_json(resp: Response) -> Value {
    let data = to_bytes(resp.into_body(), 100_000).await.unwrap();
    serde_json::from_slice(&data).unwrap()
}

#[tokio::test]
async fn link_history_restores_exact_metadata_and_can_be_cleared() {
    let app = Fixture::start().await;
    let target = "https://example.test/one?Signature=A%2fb#Original";
    let id = app.create("go.example", "history/Case", target, &["first"]).await;
    let link = format!("/api/v1/links/{id}");
    let history = format!("{link}/history");
    let edit = json!({"target":"https://example.test/two","title":"Updated","tags":["second"],"enabled":false,"analytics_enabled":false});
    for _ in 0..2 {
        let response = app.admin_request(Method::PATCH, &link, Some(edit.clone())).await;
        assert_eq!(response.status(), StatusCode::OK);
    }
    let response = app.admin_request(Method::GET, &history, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let payload = read_json(response).await;
    let items = payload["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["action"], "metadata");
    assert_eq!(items[0]["actor"], json!({"kind":"admin","label":"Owner"}));
    assert_eq!(items[0]["previous"]["value"]["target"], target);
    assert_eq!(payload["current"]["metadata"]["tags"], json!(["second"]));

    let revision = items[0]["id"].as_i64().unwrap();
    let restore = format!("{history}/{revision}/restore");
    let missing_csrf = send(&app.admin, Method::POST, &restore, &[("host", ADMIN), ("origin", ORIGIN), ("cookie", &app.cookie)], None).await;
    assert_eq!(missing_csrf.status(), StatusCode::FORBIDDEN);
    assert_eq!(app.admin_request(Method::POST, &restore, None).await.status(), StatusCode::NO_CONTENT);
    let restored = read_json(app.admin_request(Method::GET, &history, None).await).await;
    assert_eq!(restored["current"]["metadata"], items[0]["previous"]["value"]);
    assert_eq!(restored["items"][0]["action"], "restore");
    assert_eq!(app.visit("go.example", Method::GET, "/history/Case").await.headers()[header::LOCATION], target);

    assert_eq!(app.admin_request(Method::DELETE, &history, None).await.status(), StatusCode::NO_CONTENT);
    let after_clear = read_json(app.admin_request(Method::GET, &history, None).await).await;
    assert_eq!(after_clear["items"], json!([]));
    assert_eq!(after_clear["current"]["metadata"]["target"], target);
    assert_eq!(app.admin_request(Method::POST, &restore, None).await.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn link_history_does_not_restore_one_time_consumption_or_expose_credentials() {
    let app = Fixture::start().await;
    let id = app.create("go.example", "private-history", "https://example.test/original", &[]).await;
    let base = format!("/api/v1/links/{id}");
    let history = format!("{base}/history");
    let key = app.auth.create_key("Automation", vec!["read".into(), "write".into()], Some("go.example".into()), None).await.unwrap();
    let bearer = format!("Bearer {}", key.token);
    let headers = [("host", ADMIN), ("authorization", bearer.as_str())];
    let update = send(&app.admin, Method::PATCH, &base, &headers, Some(json!({"target":"https://example.test/new"}))).await;
    assert_eq!(update.status(), StatusCode::OK);
    assert_eq!(send(&app.admin, Method::GET, &history, &headers, None).await.status(), StatusCode::FORBIDDEN);

    let protection = app.admin_request(Method::PATCH, &format!("{base}/protection"), Some(json!({"one_time":true,"password":"history-secret-password"}))).await;
    assert_eq!(protection.status(), StatusCode::OK);
    let proof = confirmation(&app, "/private-history").await;
    assert_eq!(confirm(&app, "/private-history", "https://go.example", &proof, "history-secret-password").await.status(), StatusCode::SEE_OTHER);
    let payload = read_json(app.admin_request(Method::GET, &history, None).await).await;
    let items = payload["items"].as_array().unwrap();
    assert_eq!(items[0]["action"], "protection");
    assert_eq!(items[0]["previous"], Value::Null);
    assert_eq!(items[1]["actor"], json!({"kind":"api_key","label":key.key.id.to_string()}));
    let dump = payload.to_string();
    assert!(!dump.contains("history-secret-password"));
    assert!(!dump.contains("argon2id"));
    assert!(!dump.contains(&key.token));

    let revision = items[1]["id"].as_i64().unwrap();
    let restore = format!("{history}/{revision}/restore");
    assert_eq!(app.admin_request(Method::POST, &restore, None).await.status(), StatusCode::NO_CONTENT);
    let result = read_json(app.admin_request(Method::GET, &format!("{base}/protection"), None).await).await;
    assert_eq!(result, json!({"password_required":true,"one_time":true,"consumed":true}));
    assert_eq!(app.visit("go.example", Method::GET, "/private-history").await.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn link_history_restores_redirect_rules_without_resetting_visits() {
    let app = Fixture::start().await;
    let id = app.create("go.example", "limited-history", "https://example.test/target", &[]).await;
    let base = format!("/api/v1/links/{id}");
    let policy = format!("{base}/policy");
    let history = format!("{base}/history");
    assert_eq!(app.admin_request(Method::PATCH, &policy, Some(json!({"max_visits":2,"forward_query":true}))).await.status(), StatusCode::OK);
    assert_eq!(app.visit("go.example", Method::GET, "/limited-history?raw=yes").await.headers()[header::LOCATION], "https://example.test/target?raw=yes");
    assert_eq!(app.admin_request(Method::PATCH, &policy, Some(json!({"max_visits":9}))).await.status(), StatusCode::OK);
    let recorded = read_json(app.admin_request(Method::GET, &history, None).await).await;
    assert_eq!(recorded["items"][0]["previous"]["value"]["max_visits"], 2);
    let revision = recorded["items"][0]["id"].as_i64().unwrap();
    let restored = app.admin_request(Method::POST, &format!("{history}/{revision}/restore"), None).await;
    assert_eq!(restored.status(), StatusCode::NO_CONTENT);
    let current = read_json(app.admin_request(Method::GET, &policy, None).await).await;
    assert_eq!(current["max_visits"], 2);
    assert_eq!(current["visits_used"], 1);
    assert_eq!(app.visit("go.example", Method::GET, "/limited-history").await.status(), StatusCode::FOUND);
    assert_eq!(app.visit("go.example", Method::GET, "/limited-history").await.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn loopback_confirmation_form_uses_the_configured_port() {
    let app = Fixture::on_port("localhost", 9471).await;
    let created = app
        .admin_request(
            Method::POST,
            "/api/v1/links",
            Some(json!({
                "slug":"once", "target":"https://example.test/once", "protection":{"one_time":true}
            })),
        )
        .await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let response = app
        .visit("localhost:9471", Method::GET, "/once?x=1&y=2")
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 10_000).await.unwrap();
    assert!(
        std::str::from_utf8(&body)
            .unwrap()
            .contains("action=\"http://localhost:9471/once?x=1&amp;y=2\"")
    );
}

async fn confirmation(app: &Fixture, path: &str) -> String {
    let page = app.visit("go.example", Method::GET, path).await;
    assert_eq!(page.status(), StatusCode::OK);
    assert!(!page.headers().contains_key(header::LOCATION));
    assert_eq!(page.headers()[header::CACHE_CONTROL], "no-store");
    let body = to_bytes(page.into_body(), 10_000).await.unwrap();
    let body = std::str::from_utf8(&body).unwrap();
    assert!(!body.contains("https://example.test"));
    body.split_once("name=\"challenge\" value=\"")
        .unwrap()
        .1
        .split_once('"')
        .unwrap()
        .0
        .to_owned()
}

async fn confirm(
    app: &Fixture,
    path: &str,
    origin: &str,
    challenge: &str,
    password: &str,
) -> Response {
    let body = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("challenge", challenge)
        .append_pair("password", password)
        .finish();
    app.public
        .clone()
        .oneshot(
            Request::post(path)
                .header(header::HOST, "go.example")
                .header(header::ORIGIN, origin)
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn one_time_link_requires_a_same_origin_confirmation_and_only_one_request_can_consume_it() {
    let app = Fixture::start().await;
    let create = app
        .admin_request(
            Method::POST,
            "/api/v1/links",
            Some(json!({"slug":"once","target":"https://example.test/once","protection":{"one_time":true}})),
        )
        .await;
    assert_eq!(create.status(), StatusCode::CREATED);
    let head = app.visit("go.example", Method::HEAD, "/once").await;
    assert_eq!(head.status(), StatusCode::OK);
    assert!(!head.headers().contains_key(header::LOCATION));
    let proof = confirmation(&app, "/once?exact=1").await;
    assert_eq!(
        confirm(&app, "/once?exact=1", "https://foreign.example", &proof, "")
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        confirm(&app, "/once?exact=2", "https://go.example", &proof, "")
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    let (first, second) = tokio::join!(
        confirm(&app, "/once?exact=1", "https://go.example", &proof, ""),
        confirm(&app, "/once?exact=1", "https://go.example", &proof, ""),
    );
    let statuses = [first.status(), second.status()];
    assert!(statuses.contains(&StatusCode::SEE_OTHER));
    assert!(statuses.contains(&StatusCode::NOT_FOUND));
    assert_eq!(
        app.visit("go.example", Method::GET, "/once").await.status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn password_confirmation_is_revision_bound_and_always_redirects_with_get() {
    let app = Fixture::start().await;
    let id = app
        .create("go.example", "password", "https://example.test/secret", &[])
        .await;
    let policy = format!("/api/v1/links/{id}/policy");
    assert_eq!(
        app.admin_request(Method::PATCH, &policy, Some(json!({"status":307})))
            .await
            .status(),
        StatusCode::OK
    );
    let path = format!("/api/v1/links/{id}/protection");
    let update = app
        .admin_request(
            Method::PATCH,
            &path,
            Some(json!({"one_time":false,"password":"first-test-password"})),
        )
        .await;
    assert_eq!(update.status(), StatusCode::OK);
    assert_eq!(
        read_json(update).await,
        json!({"password_required":true,"one_time":false,"consumed":false})
    );
    assert_eq!(
        app.admin_request(Method::PATCH, &policy, Some(json!({"status":301})))
            .await
            .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let old = confirmation(&app, "/password").await;
    assert_eq!(
        confirm(&app, "/password", "https://go.example", &old, "invalid")
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        app.admin_request(
            Method::PATCH,
            &path,
            Some(json!({"one_time":false,"password":"second-test-password"}))
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        confirm(
            &app,
            "/password",
            "https://go.example",
            &old,
            "second-test-password"
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let updated = confirmation(&app, "/password").await;
    let result = confirm(
        &app,
        "/password",
        "https://go.example",
        &updated,
        "second-test-password",
    )
    .await;
    assert_eq!(result.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        result.headers()[header::LOCATION],
        "https://example.test/secret"
    );
    let transfer = app
        .admin_request(Method::GET, "/api/v1/exchange/links", None)
        .await;
    assert_eq!(transfer.status(), StatusCode::OK);
    let rows = read_json(transfer).await;
    assert_eq!(rows["items"][0]["slug"], "password");
    assert_eq!(rows["items"][0]["enabled"], false);
    assert!(!rows.to_string().contains("second-test-password"));
}

#[tokio::test]
async fn managed_domains_preserve_links_and_use_explicit_fallbacks() {
    let app = Fixture::start().await;
    let domain = "new.example";
    assert_eq!(
        app.visit(domain, Method::GET, "/").await.status(),
        StatusCode::NOT_FOUND
    );
    let mut settings = json!({
        "host":domain, "origin":"https://new.example:8443", "enabled":true,
        "root_target":"https://example.test/home", "missing_target":"https://example.test/help"
    });
    let set = app
        .admin_request(
            Method::POST,
            "/api/v1/catalog/domains",
            Some(settings.clone()),
        )
        .await;
    assert_eq!(set.status(), StatusCode::OK);
    assert_eq!(
        app.visit(domain, Method::GET, "/").await.headers()[header::LOCATION],
        "https://example.test/home"
    );
    assert_eq!(
        app.visit(domain, Method::GET, "/not-here").await.headers()[header::LOCATION],
        "https://example.test/help"
    );
    app.create(
        domain,
        "docs",
        "https://example.test/docs?use=as%2fis#start",
        &[],
    )
    .await;
    settings["enabled"] = json!(false);
    assert_eq!(
        app.admin_request(
            Method::POST,
            "/api/v1/catalog/domains",
            Some(settings.clone())
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        app.visit(domain, Method::GET, "/docs").await.status(),
        StatusCode::NOT_FOUND
    );
    let listed = read_json(app.admin_request(Method::GET, "/api/v1/links", None).await).await;
    assert_eq!(listed["items"][0]["slug"], "docs");
    settings["enabled"] = json!(true);
    assert_eq!(
        app.admin_request(
            Method::POST,
            "/api/v1/catalog/domains",
            Some(settings.clone())
        )
        .await
        .status(),
        StatusCode::OK
    );
    let effective = app.visit(domain, Method::GET, "/docs").await;
    assert_eq!(
        effective.headers()[header::LOCATION],
        "https://example.test/docs?use=as%2fis#start"
    );
}

#[tokio::test]
async fn ordered_policy_uses_raw_forwarding_and_durable_limit() {
    let app = Fixture::start().await;
    let id = app
        .create(
            "go.example",
            "guide",
            "https://example.test/guide?source=stored#introduction",
            &[],
        )
        .await;
    let path = format!("/api/v1/links/{id}/policy");
    let policy = json!({
        "status":307,"starts_at":null,"expires_at":null,"max_visits":1,"forward_query":true,"forward_path":true,
        "rules":[{"target":"https://example.test/local?value=A%2Fb#help","conditions":[{"type":"query","key":"mode","value":"local"}]}]
    });
    assert_eq!(
        app.admin_request(Method::PATCH, &path, Some(policy))
            .await
            .status(),
        StatusCode::OK
    );
    let peek = app
        .visit(
            "go.example",
            Method::HEAD,
            "/guide/child?mode=local&term=a%2fb",
        )
        .await;
    assert_eq!(peek.status(), StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(
        peek.headers()[header::LOCATION],
        "https://example.test/local/child?value=A%2Fb&mode=local&term=a%2fb#help"
    );
    let hit = app
        .visit(
            "go.example",
            Method::GET,
            "/guide/child?mode=local&term=a%2fb",
        )
        .await;
    assert_eq!(hit.status(), StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(
        app.visit("go.example", Method::GET, "/guide")
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    let details = read_json(app.admin_request(Method::GET, &path, None).await).await;
    assert_eq!(details["visits_used"], 1);
}

#[tokio::test]
async fn tag_merge_and_qr_respect_existing_link_identity() {
    let app = Fixture::start().await;
    let id = app
        .create(
            "go.example",
            "test/guide",
            "https://example.test/",
            &["first", "second"],
        )
        .await;
    let changed = app
        .admin_request(
            Method::PATCH,
            "/api/v1/catalog/tags",
            Some(json!({"name":"first","new_name":"second"})),
        )
        .await;
    assert_eq!(changed.status(), StatusCode::NO_CONTENT);
    let items = read_json(app.admin_request(Method::GET, "/api/v1/links", None).await).await;
    assert_eq!(items["items"][0]["tags"], json!(["second"]));

    let png = app
        .admin_request(
            Method::GET,
            &format!("/api/v1/links/{id}/qr?format=png&size=320"),
            None,
        )
        .await;
    assert_eq!(png.status(), StatusCode::OK);
    assert_eq!(png.headers()[header::CONTENT_TYPE], "image/png");
    assert!(
        to_bytes(png.into_body(), 1_000_000)
            .await
            .unwrap()
            .starts_with(b"\x89PNG\r\n\x1a\n")
    );

    let scope = app
        .auth
        .create_key(
            "Other domain",
            vec!["read".into()],
            Some("s.example".into()),
            None,
        )
        .await
        .unwrap();
    let denied = send(
        &app.admin,
        Method::GET,
        &format!("/api/v1/links/{id}/qr"),
        &[
            ("host", ADMIN),
            ("authorization", &format!("Bearer {}", scope.token)),
        ],
        None,
    )
    .await;
    assert_eq!(denied.status(), StatusCode::NOT_FOUND);
}
