use std::net::SocketAddr;

use axum::{
    Router,
    body::{Body, to_bytes},
    extract::ConnectInfo,
    http::{Method, Request, StatusCode, header},
    response::Response,
};
use serde_json::{Value, json};
use shortcircuit::{
    AdminConfig, Auth, BootstrapKind, PublicControls, PublicHosts, RateLimit, admin_router,
    open_database, public_router_with_controls,
};
use tower::ServiceExt;

const ORIGIN: &str = "https://admin.example";

struct App {
    _directory: tempfile::TempDir,
    admin: Router,
    public: Router,
    cookie: String,
    csrf: String,
}

impl App {
    async fn start(controls: PublicControls) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let pool = open_database(&directory.path().join("test.db"))
            .await
            .unwrap();
        let hosts = PublicHosts::parse("go.example").unwrap();
        let auth = Auth::new(pool.clone());
        let claim = auth.issue_bootstrap(BootstrapKind::Setup).await.unwrap();
        let session = auth
            .setup(&claim, "Owner", "synthetic-test-password")
            .await
            .unwrap();
        Self {
            _directory: directory,
            admin: admin_router(
                pool.clone(),
                hosts.clone(),
                AdminConfig::new(ORIGIN, 8080).unwrap(),
            ),
            public: public_router_with_controls(pool, hosts, 8080, None, controls),
            cookie: format!("__Host-shortcircuit_session={}", session.token),
            csrf: session.session.csrf,
        }
    }

    async fn admin(&self, method: Method, path: &str, body: Value) -> (StatusCode, Value) {
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header(header::HOST, "admin.example")
            .header(header::ORIGIN, ORIGIN)
            .header(header::COOKIE, &self.cookie)
            .header("x-csrf-token", &self.csrf)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let response = self.admin.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), 100_000).await.unwrap();
        (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
    }

    /// Creates a temporary redirect protected by the synthetic test password.
    async fn protected(&self, slug: &str) {
        let (status, link) = self
            .admin(
                Method::POST,
                "/api/v1/links",
                json!({"host":"go.example","slug":slug,"target":format!("https://example.test/{slug}")}),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED);
        let id = link["id"].as_i64().unwrap();
        let policy = format!("/api/v1/links/{id}/policy");
        let (status, _) = self
            .admin(Method::PATCH, &policy, json!({"status":307}))
            .await;
        assert_eq!(status, StatusCode::OK);
        let protection = format!("/api/v1/links/{id}/protection");
        let (status, _) = self
            .admin(
                Method::PATCH,
                &protection,
                json!({"one_time":false,"password":"budget-test-password"}),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
    }

    async fn public(&self, request: Request<Body>, client: &str) -> Response {
        let mut request = request;
        request
            .extensions_mut()
            .insert(ConnectInfo(client.parse::<SocketAddr>().unwrap()));
        self.public.clone().oneshot(request).await.unwrap()
    }

    async fn challenge(&self, path: &str) -> String {
        let request = Request::get(path)
            .header(header::HOST, "go.example")
            .body(Body::empty())
            .unwrap();
        let page = self.public(request, "192.0.2.200:4000").await;
        assert_eq!(page.status(), StatusCode::OK);
        let body = to_bytes(page.into_body(), 10_000).await.unwrap();
        std::str::from_utf8(&body)
            .unwrap()
            .split_once("name=\"challenge\" value=\"")
            .unwrap()
            .1
            .split_once('"')
            .unwrap()
            .0
            .to_owned()
    }

    async fn attempt(&self, path: &str, challenge: &str, password: &str, client: &str) -> Response {
        let body = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("challenge", challenge)
            .append_pair("password", password)
            .finish();
        let request = Request::post(path)
            .header(header::HOST, "go.example")
            .header(header::ORIGIN, "https://go.example")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(body))
            .unwrap();
        self.public(request, client).await
    }
}

fn limited(response: &Response) -> bool {
    response.status() == StatusCode::TOO_MANY_REQUESTS
        && response.headers()[header::RETRY_AFTER] == "60"
}

#[tokio::test]
async fn one_link_has_its_own_password_budget_per_client_and_across_clients() {
    let app = App::start(PublicControls {
        password: RateLimit::new(8, 60).unwrap(),
        password_link: RateLimit::new(2, 3).unwrap(),
        ..PublicControls::default()
    })
    .await;
    app.protected("alpha").await;
    app.protected("beta").await;
    let alpha = app.challenge("/alpha").await;
    let beta = app.challenge("/beta").await;
    let (first, second, third) = (
        "192.0.2.1:1000",
        "[2001:db8:5:6::1]:1000",
        "198.51.100.7:1000",
    );

    for _ in 0..2 {
        let wrong = app
            .attempt("/alpha", &alpha, "wrong-password-1", first)
            .await;
        assert_eq!(wrong.status(), StatusCode::FORBIDDEN);
    }
    // The client exhausted its budget for this link only.
    assert!(limited(
        &app.attempt("/alpha", &alpha, "wrong-password-1", first)
            .await
    ));
    let other_link = app.attempt("/beta", &beta, "wrong-password-1", first).await;
    assert_eq!(other_link.status(), StatusCode::FORBIDDEN);

    // Another client may try once more before the link's ceiling stops everyone.
    let wrong = app
        .attempt("/alpha", &alpha, "wrong-password-2", second)
        .await;
    assert_eq!(wrong.status(), StatusCode::FORBIDDEN);
    let distributed = app
        .attempt("/alpha", &alpha, "budget-test-password", third)
        .await;
    assert!(limited(&distributed));
    assert!(!distributed.headers().contains_key(header::LOCATION));

    // Other links keep working with the correct password.
    let unlocked = app
        .attempt("/beta", &beta, "budget-test-password", third)
        .await;
    assert_eq!(unlocked.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        unlocked.headers()[header::LOCATION],
        "https://example.test/beta"
    );
}

#[tokio::test]
async fn the_process_budget_still_bounds_attempts_across_links() {
    let app = App::start(PublicControls {
        password: RateLimit::new(2, 2).unwrap(),
        password_link: RateLimit::new(5, 30).unwrap(),
        ..PublicControls::default()
    })
    .await;
    app.protected("alpha").await;
    app.protected("beta").await;
    let alpha = app.challenge("/alpha").await;
    let beta = app.challenge("/beta").await;
    let client = "192.0.2.1:1000";
    assert_eq!(
        app.attempt("/alpha", &alpha, "wrong-password-1", client)
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        app.attempt("/beta", &beta, "wrong-password-1", client)
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert!(limited(
        &app.attempt("/beta", &beta, "budget-test-password", "198.51.100.7:1000")
            .await
    ));
}
