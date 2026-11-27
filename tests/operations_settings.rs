//! Settings > Operations: precedence, runtime application, confirmation, and secrecy.

use std::{collections::HashMap, time::Duration};

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
    response::Response,
};
use serde_json::{Value, json};
use shortcircuit::{
    AdminConfig, AdminServices, Auth, BootstrapKind, Monitor, Notifier, Operations,
    OperationsEnvironment, PublicHosts, RuntimeSettings, TitleLookup, admin_router_with,
    open_database,
};
use sqlx::SqlitePool;
use tokio::sync::watch;
use tower::ServiceExt;

const ORIGIN: &str = "https://admin.example";
const HOST: &str = "admin.example";
const PASSWORD: &str = "synthetic-admin-password";
/// A secret path segment and authorization value that must never appear in a response.
const WEBHOOK: &str = "https://hooks.example/services/synthetic-secret-path";
const AUTHORIZATION: &str = "Bearer synthetic-secret-token";

struct App {
    _temp: tempfile::TempDir,
    admin: Router,
    pool: SqlitePool,
    monitor: Monitor,
    cookie: String,
    csrf: String,
}

impl App {
    async fn start(container: &[(&str, &str)]) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let database = temp.path().join("operations.db");
        let pool = open_database(&database).await.unwrap();
        let values: HashMap<String, String> = container
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect();
        let environment =
            OperationsEnvironment::from_env(move |name| values.get(name).cloned()).unwrap();
        let settings = RuntimeSettings::load(&pool, environment).await.unwrap();
        let notifier = Notifier::new(&settings);
        let monitor = Monitor::new(&settings, notifier.clone());
        let admin = admin_router_with(
            pool.clone(),
            PublicHosts::parse("go.example").unwrap(),
            AdminConfig::new(ORIGIN, 8080).unwrap(),
            AdminServices {
                operations: Some(Operations::new(database).with_settings(settings.clone())),
                monitor: Some(monitor.clone()),
                titles: Some(TitleLookup::new(&settings)),
                notifications: Some(notifier),
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
        Self {
            _temp: temp,
            admin,
            pool,
            monitor,
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

    async fn settings(&self) -> Value {
        let response = self
            .call(Method::GET, "/api/v1/settings/operations", None)
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        body(response).await
    }

    async fn patch(&self, input: Value) -> (StatusCode, Value) {
        let response = self
            .call(Method::PATCH, "/api/v1/settings/operations", Some(input))
            .await;
        let status = response.status();
        (status, body(response).await)
    }

    async fn authorize(&self, password: &str) -> StatusCode {
        self.call(
            Method::POST,
            "/api/v1/settings/operations/authorize",
            Some(json!({"password": password})),
        )
        .await
        .status()
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

async fn text(response: Response) -> String {
    String::from_utf8(
        to_bytes(response.into_body(), 256_000)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}

async fn body(response: Response) -> Value {
    let text = text(response).await;
    if text.is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&text).unwrap()
    }
}

#[tokio::test]
async fn saved_values_win_over_container_values_until_reset() {
    let app = App::start(&[("MONITOR_INTERVAL_HOURS", "12"), ("BACKUP_KEEP", "3")]).await;
    let initial = app.settings().await;
    // Nothing is saved after an upgrade: container values and defaults apply unchanged.
    assert_eq!(
        initial["monitor_interval_hours"],
        json!({"value": 12, "source": "container", "container": 12})
    );
    assert_eq!(initial["monitor_priority_hours"]["value"], 6);
    assert_eq!(initial["monitor_priority_hours"]["source"], "default");
    assert_eq!(
        initial["monitor_enabled"],
        json!({"value": false, "source": "default", "container": null})
    );
    assert_eq!(initial["backup_interval_hours"]["value"], 24);
    assert_eq!(initial["backup_keep"]["source"], "container");
    assert_eq!(initial["notifications_active"], false);

    let (status, saved) = app
        .patch(json!({"monitor_interval_hours": 8, "backup_keep": 10}))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        saved["monitor_interval_hours"],
        json!({"value": 8, "source": "saved", "container": 12})
    );
    assert_eq!(saved["backup_keep"]["value"], 10);
    // System diagnostics report the same effective values and sources.
    let diagnostics = body(app.call(Method::GET, "/api/v1/system", None).await).await;
    assert_eq!(
        diagnostics["operations_settings"]["monitor_interval_hours"]["source"],
        "saved"
    );
    assert_eq!(diagnostics["scheduled_backups"], true);

    let (status, reset) = app
        .patch(json!({"reset": ["monitor_interval_hours", "backup_keep"]}))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reset["monitor_interval_hours"]["source"], "container");
    assert_eq!(reset["monitor_interval_hours"]["value"], 12);
    assert_eq!(reset["backup_keep"]["value"], 3);

    // Turning scheduled backups off applies at once.
    let (_, off) = app.patch(json!({"backup_interval_hours": 0})).await;
    assert_eq!(off["backup_interval_hours"]["value"], 0);
    let diagnostics = body(app.call(Method::GET, "/api/v1/system", None).await).await;
    assert_eq!(diagnostics["scheduled_backups"], false);
}

#[tokio::test]
async fn turning_health_checks_on_and_off_applies_without_a_restart() {
    let app = App::start(&[]).await;
    sqlx::raw_sql(
        "INSERT INTO links (id, host, slug, target) VALUES
            (1, 'go.example', 'sample', 'http://127.0.0.1/private');",
    )
    .execute(&app.pool)
    .await
    .unwrap();
    // Off by default: enrollment and manual checks are refused.
    let refused = app
        .call(
            Method::PATCH,
            "/api/v1/monitoring/links/1",
            Some(json!({"enabled": true})),
        )
        .await;
    assert_eq!(refused.status(), StatusCode::CONFLICT);

    let (stop, signal) = watch::channel(false);
    let monitor = app.monitor.clone();
    let pool = app.pool.clone();
    let worker = tokio::spawn(async move { monitor.run(pool, signal).await });

    let (status, enabled) = app.patch(json!({"monitor_enabled": true})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(enabled["monitor_enabled"]["source"], "saved");
    let enrolled = app
        .call(
            Method::PATCH,
            "/api/v1/monitoring/links/1",
            Some(json!({"enabled": true})),
        )
        .await;
    assert_eq!(enrolled.status(), StatusCode::NO_CONTENT);
    sqlx::query("UPDATE link_monitor SET next_check = 0")
        .execute(&app.pool)
        .await
        .unwrap();
    app.call(Method::POST, "/api/v1/monitoring/check", None)
        .await;
    let checked = || async {
        sqlx::query_scalar::<_, Option<i64>>("SELECT last_checked FROM link_monitor")
            .fetch_one(&app.pool)
            .await
            .unwrap()
    };
    // The running scheduler wakes up and probes the due link. The policy refuses the private
    // address without connecting.
    tokio::time::timeout(Duration::from_secs(10), async {
        while checked().await.is_none() {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();

    let (status, _) = app.patch(json!({"monitor_enabled": false})).await;
    assert_eq!(status, StatusCode::OK);
    // A probe already in flight may still finish (see SECURITY.md); let it settle first, then
    // check that no new probe starts.
    tokio::time::sleep(Duration::from_millis(800)).await;
    sqlx::query("UPDATE link_monitor SET next_check = 0, last_checked = 1")
        .execute(&app.pool)
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(800)).await;
    // No new probe is scheduled while health checks are off.
    assert_eq!(checked().await, Some(1));
    let manual = app
        .call(Method::POST, "/api/v1/monitoring/links/1/check", None)
        .await;
    assert_eq!(manual.status(), StatusCode::CONFLICT);
    let overview = body(app.call(Method::GET, "/api/v1/overview", None).await).await;
    assert_eq!(overview["monitoring"]["enabled"], false);
    stop.send_replace(true);
    tokio::time::timeout(Duration::from_secs(2), worker)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn the_notification_target_needs_a_password_and_is_never_returned() {
    let app = App::start(&[]).await;
    let target = json!({
        "notification_webhook_url": WEBHOOK,
        "notification_authorization": AUTHORIZATION,
    });
    let (status, refused) = app.patch(target.clone()).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(refused["code"], "operation_authorization_required");
    assert_eq!(
        app.authorize("not-the-password").await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(app.authorize(PASSWORD).await, StatusCode::NO_CONTENT);

    // Validation runs before the confirmation is used.
    let (status, invalid) = app
        .patch(json!({"notification_webhook_url": "https://10.0.0.1/notify"}))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(invalid["code"], "notification_webhook_not_public");

    let response = app
        .call(Method::PATCH, "/api/v1/settings/operations", Some(target))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let saved = text(response).await;
    let saved: Value = serde_json::from_str(&saved)
        .map(|value: Value| {
            assert!(!value.to_string().contains("synthetic-secret"), "{value}");
            value
        })
        .unwrap();
    assert_eq!(
        saved["notification_webhook"],
        json!({
            "configured": true,
            "origin": "https://hooks.example",
            "source": "saved",
            "container_configured": false,
        })
    );
    assert_eq!(saved["notification_authorization"]["configured"], true);
    assert!(saved["notification_authorization"].get("origin").is_none());
    assert_eq!(saved["notifications_active"], true);

    // The confirmation is used once.
    let (status, _) = app.patch(json!({"notification_authorization": ""})).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    // Settings that do not change the target need no confirmation.
    let (status, events) = app
        .patch(json!({"notification_events": ["security", "backup_failed"]}))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        events["notification_events"]["value"],
        json!(["backup_failed", "security"])
    );

    for path in [
        "/api/v1/settings/operations",
        "/api/v1/system",
        "/api/v1/notifications",
        "/api/v1/audit",
    ] {
        let response = app.call(Method::GET, path, None).await;
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        let text = text(response).await;
        assert!(!text.contains("synthetic-secret"), "{path}: {text}");
    }
    let status = body(app.call(Method::GET, "/api/v1/notifications", None).await).await;
    assert_eq!(status["enabled"], true);
    assert_eq!(status["origin"], "https://hooks.example");

    // Each change is audited by setting name, without values.
    let audit = body(app.call(Method::GET, "/api/v1/audit", None).await).await;
    let entries: Vec<&Value> = audit["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| {
            item["action"]
                .as_str()
                .unwrap()
                .starts_with("operations_settings")
        })
        .collect();
    let outcomes: Vec<(&str, &str)> = entries
        .iter()
        .map(|item| {
            (
                item["action"].as_str().unwrap(),
                item["outcome"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        outcomes,
        [
            ("operations_settings_update", "success"),
            ("operations_settings_update", "rejected"),
            ("operations_settings_update", "success"),
            ("operations_settings_authorize", "success"),
            ("operations_settings_authorize", "rejected"),
            ("operations_settings_update", "rejected"),
        ]
    );
    assert_eq!(
        entries[2]["details"]["changed"],
        json!(["notification_webhook_url", "notification_authorization"])
    );

    // Turning notifications off is a target change and also needs a confirmation.
    assert_eq!(app.authorize(PASSWORD).await, StatusCode::NO_CONTENT);
    let (status, off) = app.patch(json!({"notification_webhook_url": ""})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(off["notifications_active"], false);
    assert_eq!(off["notification_webhook"]["configured"], false);
    let test = app
        .call(Method::POST, "/api/v1/notifications/test", None)
        .await;
    assert_eq!(test.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn invalid_changes_and_api_keys_are_refused() {
    let app = App::start(&[]).await;
    for (input, code) in [
        (
            json!({"monitor_interval_hours": 0}),
            "invalid_monitor_intervals",
        ),
        (
            json!({"monitor_interval_hours": 4, "monitor_priority_hours": 5}),
            "invalid_monitor_intervals",
        ),
        (
            json!({"backup_interval_hours": 721}),
            "invalid_backup_interval",
        ),
        (json!({"backup_keep": 0}), "invalid_backup_keep"),
        (
            json!({"notification_events": ["everything"]}),
            "invalid_notification_events",
        ),
        (
            json!({"notification_events": ["click_milestone"]}),
            "click_milestones_required",
        ),
        (
            json!({"notification_click_milestones": [0]}),
            "invalid_click_milestones",
        ),
        (
            json!({"notification_expiry_notice_hours": 721}),
            "invalid_expiry_notice",
        ),
        (json!({"reset": ["log_level"]}), "invalid_reset"),
        (
            json!({"notification_webhook_url": "http://hooks.example/"}),
            "invalid_notification_webhook",
        ),
        (
            json!({"notification_webhook_url": "https://hooks.example:8443/"}),
            "invalid_notification_webhook",
        ),
    ] {
        let (status, problem) = app.patch(input.clone()).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{input}");
        assert_eq!(problem["code"], code, "{input}");
    }
    let (status, _) = app.patch(json!({"log_level": "debug"})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    // Nothing was saved by a refused change.
    let settings = app.settings().await;
    assert_eq!(settings["monitor_interval_hours"]["source"], "default");

    let key = body(
        app.call(
            Method::POST,
            "/api/v1/keys",
            Some(json!({"name": "automation", "scopes": ["read", "write", "delete"]})),
        )
        .await,
    )
    .await;
    let bearer = format!("Bearer {}", key["token"].as_str().unwrap());
    for method in [Method::GET, Method::PATCH] {
        let response = send(
            &app.admin,
            method.clone(),
            "/api/v1/settings/operations",
            &[("authorization", bearer.as_str()), ("origin", ORIGIN)],
            (method == Method::PATCH).then(|| json!({"monitor_enabled": true})),
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{method}");
    }
    assert_eq!(app.settings().await["monitor_enabled"]["value"], false);
}
