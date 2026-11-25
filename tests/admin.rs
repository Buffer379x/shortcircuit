use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
    response::Response,
};
use serde_json::{Value, json};
use shortcircuit::{
    AdminConfig, Auth, BootstrapKind, PublicHosts, admin_router, open_database, public_router,
};
use tower::ServiceExt;

const ORIGIN: &str = "https://admin.example";
const HOST: &str = "admin.example";
const PASSWORD: &str = "synthetic-admin-password";

struct App {
    _temp: tempfile::TempDir,
    auth: Auth,
    admin: Router,
    public: Router,
}

impl App {
    async fn start() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let pool = open_database(&temp.path().join("admin.db")).await.unwrap();
        let hosts = PublicHosts::parse("go.example,s.example").unwrap();
        let config = AdminConfig::new(ORIGIN, 8080).unwrap();
        Self {
            _temp: temp,
            auth: Auth::new(pool.clone()),
            admin: admin_router(pool.clone(), hosts.clone(), config),
            public: public_router(pool, hosts),
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
