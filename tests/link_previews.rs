//! Link preview images: the switch and its title lookup requirement, the image endpoint and its
//! headers, API key scopes, the catalog field, refreshes, and snapshot scrubbing.

use std::collections::HashMap;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
    response::Response,
};
use serde_json::{Value, json};
use shortcircuit::{
    AdminConfig, AdminServices, Auth, BootstrapKind, Notifier, Operations, OperationsEnvironment,
    PublicHosts, RuntimeSettings, TitleLookup, admin_router_with, create_snapshot, open_database,
};
use sqlx::{Connection, SqliteConnection, SqlitePool, sqlite::SqliteConnectOptions};
use tower::ServiceExt;

const ORIGIN: &str = "https://admin.example";
const HOST: &str = "admin.example";
const PASSWORD: &str = "synthetic-admin-password";
/// A synthetic JPEG payload; the endpoint serves stored bytes unchanged.
const JPEG: &[u8] = b"\xff\xd8\xff\xe0synthetic-preview-bytes\xff\xd9";

struct App {
    temp: tempfile::TempDir,
    admin: Router,
    pool: SqlitePool,
    cookie: String,
    csrf: String,
}

impl App {
    async fn start(container: &[(&str, &str)]) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let database = temp.path().join("previews.db");
        let pool = open_database(&database).await.unwrap();
        let values: HashMap<String, String> = container
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect();
        let environment =
            OperationsEnvironment::from_env(move |name| values.get(name).cloned()).unwrap();
        let settings = RuntimeSettings::load(&pool, environment).await.unwrap();
        let admin = admin_router_with(
            pool.clone(),
            PublicHosts::parse("go.example,other.example").unwrap(),
            AdminConfig::new(ORIGIN, 8080).unwrap(),
            AdminServices {
                operations: Some(Operations::new(database).with_settings(settings.clone())),
                titles: Some(TitleLookup::new(&settings)),
                notifications: Some(Notifier::new(&settings)),
                runtime_settings: Some(settings),
                ..AdminServices::default()
            },
        );
        let token = Auth::new(pool.clone())
            .issue_bootstrap(BootstrapKind::Setup)
            .await
            .unwrap();
        let response = send(
            &admin,
            Method::POST,
            "/api/v1/auth/setup",
            &[("origin", ORIGIN)],
            Some(json!({"token": token, "username": "Owner", "password": PASSWORD})),
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
        let csrf = body(response).await["csrf"].as_str().unwrap().to_owned();
        sqlx::raw_sql(
            "INSERT INTO links (id, host, slug, target, title) VALUES
                (1, 'go.example', 'card', 'https://example.test/article', 'Article'),
                (2, 'go.example', 'plain', 'https://example.test/plain', 'Plain'),
                (3, 'other.example', 'elsewhere', 'https://example.test/other', 'Other');
             INSERT INTO link_previews
                (link_id, status, kind, mime, bytes, width, height, source_origin, fetched_at)
             VALUES
                (1, 'ready', 'image', 'image/jpeg', X'FFD8FFE073796E7468657469632D707265766965772D6279746573FFD9',
                 480, 270, 'https://cdn.example', 1700000000),
                (3, 'ready', 'icon', 'image/jpeg', X'FFD8FFD9', 64, 64, 'https://example.test', 1700000100);",
        )
        .execute(&pool)
        .await
        .unwrap();
        Self {
            temp,
            admin,
            pool,
            cookie,
            csrf,
        }
    }

    async fn call(&self, method: Method, path: &str, input: Option<Value>) -> Response {
        let headers = [
            ("cookie", self.cookie.as_str()),
            ("origin", ORIGIN),
            ("x-csrf-token", self.csrf.as_str()),
        ];
        send(&self.admin, method, path, &headers, input).await
    }

    async fn patch(&self, input: Value) -> (StatusCode, Value) {
        let response = self
            .call(Method::PATCH, "/api/v1/settings/operations", Some(input))
            .await;
        let status = response.status();
        (status, body(response).await)
    }

    async fn key(&self, scopes: &[&str], host: Option<&str>) -> String {
        let response = self
            .call(
                Method::POST,
                "/api/v1/keys",
                Some(json!({"name": "automation", "scopes": scopes, "host": host})),
            )
            .await;
        assert_eq!(response.status(), StatusCode::CREATED);
        format!("Bearer {}", body(response).await["token"].as_str().unwrap())
    }

    async fn with_key(&self, method: Method, path: &str, bearer: &str) -> Response {
        send(
            &self.admin,
            method,
            path,
            &[("authorization", bearer), ("origin", ORIGIN)],
            None,
        )
        .await
    }

    async fn preview_status(&self, id: i64) -> Option<String> {
        sqlx::query_scalar("SELECT status FROM link_previews WHERE link_id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .unwrap()
    }
}

async fn send(
    router: &Router,
    method: Method,
    path: &str,
    headers: &[(&str, &str)],
    input: Option<Value>,
) -> Response {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, HOST);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    let request = match input {
        Some(input) => builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(input.to_string())),
        None => builder.body(Body::empty()),
    };
    router.clone().oneshot(request.unwrap()).await.unwrap()
}

async fn bytes(response: Response) -> Vec<u8> {
    to_bytes(response.into_body(), 256_000)
        .await
        .unwrap()
        .to_vec()
}

async fn body(response: Response) -> Value {
    let bytes = bytes(response).await;
    if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    }
}

#[tokio::test]
async fn the_preview_switch_is_off_by_default_and_needs_title_lookup() {
    let app = App::start(&[]).await;
    let settings = body(
        app.call(Method::GET, "/api/v1/settings/operations", None)
            .await,
    )
    .await;
    assert_eq!(
        settings["preview_images_enabled"],
        json!({"value": false, "source": "default", "container": null})
    );
    assert_eq!(settings["preview_images_active"], false);

    let (status, problem) = app.patch(json!({"preview_images_enabled": true})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(problem["code"], "title_lookup_required");
    let (status, problem) = app
        .patch(json!({"title_lookup_enabled": false, "preview_images_enabled": true}))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(problem["code"], "title_lookup_required");

    let (status, saved) = app
        .patch(json!({"title_lookup_enabled": true, "preview_images_enabled": true}))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(saved["preview_images_enabled"]["source"], "saved");
    assert_eq!(saved["preview_images_active"], true);
    // Turning title lookup off later pauses previews without changing their switch.
    let (status, saved) = app.patch(json!({"title_lookup_enabled": false})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(saved["preview_images_enabled"]["value"], true);
    assert_eq!(saved["preview_images_active"], false);
    let refresh = app
        .call(Method::POST, "/api/v1/links/2/preview/refresh", None)
        .await;
    assert_eq!(refresh.status(), StatusCode::CONFLICT);
    assert_eq!(body(refresh).await["code"], "title_lookup_required");

    // Verified snapshots drop the switch, like the title lookup switch.
    let snapshot = create_snapshot(&app.pool, &app.temp.path().join("backups"))
        .await
        .unwrap();
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(&snapshot)
            .read_only(true),
    )
    .await
    .unwrap();
    let saved: (Option<i64>, Option<i64>) =
        sqlx::query_as("SELECT preview_images_enabled, title_lookup_enabled FROM runtime_settings")
            .fetch_one(&mut connection)
            .await
            .unwrap();
    assert_eq!(saved, (None, None));
    // Stored thumbnails are part of the snapshot.
    let previews: i64 = sqlx::query_scalar("SELECT count(*) FROM link_previews")
        .fetch_one(&mut connection)
        .await
        .unwrap();
    assert_eq!(previews, 2);

    let container = App::start(&[("PREVIEW_IMAGES_ENABLED", "true")]).await;
    let settings = body(
        container
            .call(Method::GET, "/api/v1/settings/operations", None)
            .await,
    )
    .await;
    assert_eq!(settings["preview_images_enabled"]["source"], "container");
    assert_eq!(settings["preview_images_active"], false);
    assert!(
        OperationsEnvironment::from_env(|name| {
            (name == "PREVIEW_IMAGES_ENABLED").then(|| "yes".to_owned())
        })
        .is_err()
    );
}

#[tokio::test]
async fn the_image_endpoint_serves_stored_thumbnails_with_safe_headers() {
    let app = App::start(&[]).await;
    let response = app.call(Method::GET, "/api/v1/links/1/preview", None).await;
    assert_eq!(response.status(), StatusCode::OK);
    let headers = response.headers().clone();
    assert_eq!(headers[header::CONTENT_TYPE], "image/jpeg");
    assert_eq!(headers[header::CACHE_CONTROL], "private, max-age=3600");
    assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    assert_eq!(headers["cross-origin-resource-policy"], "same-origin");
    let policy = headers[header::CONTENT_SECURITY_POLICY].to_str().unwrap();
    assert!(policy.starts_with("default-src 'none'"), "{policy}");
    assert!(policy.contains("img-src 'self'"), "{policy}");
    assert_eq!(bytes(response).await, JPEG);
    // Other responses keep `no-store`.
    let detail = app.call(Method::GET, "/api/v1/links/1", None).await;
    assert_eq!(detail.headers()[header::CACHE_CONTROL], "no-store");
    let detail = body(detail).await;
    assert_eq!(
        detail["preview"],
        json!({"kind": "image", "width": 480, "height": 270, "fetched_at": 1700000000,
               "origin": "https://cdn.example"})
    );

    for path in [
        "/api/v1/links/2/preview",
        "/api/v1/links/99/preview",
        "/api/v1/links/0/preview",
    ] {
        let response = app.call(Method::GET, path, None).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
        assert_eq!(
            response.headers()[header::CACHE_CONTROL],
            "no-store",
            "{path}"
        );
    }
    let plain = body(app.call(Method::GET, "/api/v1/links/2", None).await).await;
    assert_eq!(plain["preview"], Value::Null);
    let page = body(
        app.call(
            Method::POST,
            "/api/v1/links/search",
            Some(json!({"sort": "oldest"})),
        )
        .await,
    )
    .await;
    let kinds: Vec<Value> = page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["preview"]["kind"].clone())
        .collect();
    assert_eq!(kinds, [json!("image"), Value::Null, json!("icon")]);

    // Only ready previews are served.
    sqlx::query(
        "UPDATE link_previews SET status = 'pending', kind = NULL, mime = NULL, bytes = NULL,
             width = NULL, height = NULL, source_origin = NULL, fetched_at = NULL
         WHERE link_id = 1",
    )
    .execute(&app.pool)
    .await
    .unwrap();
    let response = app.call(Method::GET, "/api/v1/links/1/preview", None).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn api_keys_read_previews_within_their_scope_and_cannot_refresh() {
    let app = App::start(&[]).await;
    let reader = app.key(&["read"], None).await;
    let response = app
        .with_key(Method::GET, "/api/v1/links/1/preview", &reader)
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(bytes(response).await, JPEG);
    let head = app
        .with_key(Method::HEAD, "/api/v1/links/1/preview", &reader)
        .await;
    assert_eq!(head.status(), StatusCode::OK);

    let writer = app.key(&["write"], None).await;
    let response = app
        .with_key(Method::GET, "/api/v1/links/1/preview", &writer)
        .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(body(response).await["code"], "insufficient_scope");

    // A host-bound key sees previews of its own host only.
    let bound = app.key(&["read"], Some("other.example")).await;
    let response = app
        .with_key(Method::GET, "/api/v1/links/1/preview", &bound)
        .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let response = app
        .with_key(Method::GET, "/api/v1/links/3/preview", &bound)
        .await;
    assert_eq!(response.status(), StatusCode::OK);

    // Refreshing starts outgoing requests, so it is for browser sessions only.
    let full = app.key(&["read", "write", "delete"], None).await;
    let response = app
        .with_key(Method::POST, "/api/v1/links/1/preview/refresh", &full)
        .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(app.preview_status(1).await.as_deref(), Some("ready"));
}

#[tokio::test]
async fn refreshing_queues_a_new_fetch_only_while_previews_are_on() {
    let app = App::start(&[]).await;
    let response = app
        .call(Method::POST, "/api/v1/links/1/preview/refresh", None)
        .await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(body(response).await["code"], "preview_images_disabled");
    assert_eq!(app.preview_status(1).await.as_deref(), Some("ready"));

    let (status, _) = app
        .patch(json!({"title_lookup_enabled": true, "preview_images_enabled": true}))
        .await;
    assert_eq!(status, StatusCode::OK);
    let response = app
        .call(Method::POST, "/api/v1/links/1/preview/refresh", None)
        .await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(app.preview_status(1).await.as_deref(), Some("pending"));
    let response = app.call(Method::GET, "/api/v1/links/1/preview", None).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let response = app
        .call(Method::POST, "/api/v1/links/99/preview/refresh", None)
        .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // Protected links are never fetched: protection removes the preview and refuses a refresh.
    sqlx::query("INSERT INTO link_protection (link_id, one_time) VALUES (3, 1)")
        .execute(&app.pool)
        .await
        .unwrap();
    assert_eq!(app.preview_status(3).await, None);
    let response = app
        .call(Method::POST, "/api/v1/links/3/preview/refresh", None)
        .await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(body(response).await["code"], "preview_protected");

    // A changed destination clears the stored preview and queues a new fetch.
    sqlx::query(
        "UPDATE link_previews SET status = 'ready', kind = 'icon', mime = 'image/jpeg',
             bytes = X'FFD8FFD9', width = 32, height = 32, source_origin = 'https://example.test',
             fetched_at = unixepoch() WHERE link_id = 1",
    )
    .execute(&app.pool)
    .await
    .unwrap();
    let response = app
        .call(
            Method::PATCH,
            "/api/v1/links/1",
            Some(json!({"target": "https://example.test/moved"})),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body(response).await["preview"], Value::Null);
    assert_eq!(app.preview_status(1).await.as_deref(), Some("pending"));
}
