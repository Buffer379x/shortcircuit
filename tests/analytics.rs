use std::time::Duration;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
    response::Response,
};
use serde_json::Value;
use shortcircuit::{
    AdminConfig, Analytics, Auth, BootstrapKind, PublicHosts, admin_router_with_analytics,
    open_database, public_router_with_analytics,
};
use tower::ServiceExt;

async fn send(router: &Router, method: &str, path: &str, headers: &[(&str, &str)]) -> Response {
    let mut request = Request::builder().method(method).uri(path);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    router.clone().oneshot(request.body(Body::empty()).unwrap()).await.unwrap()
}

#[tokio::test]
async fn analytics_respects_request_and_link_privacy() {
    let directory = tempfile::tempdir().unwrap();
    let pool = open_database(&directory.path().join("app.db")).await.unwrap();
    sqlx::query(
        "INSERT INTO links (host, slug, target, analytics_enabled) VALUES
            ('go.example', 'tracked', 'https://example.test', 1),
            ('go.example', 'private', 'https://example.test', 0)",
    )
    .execute(&pool)
    .await
    .unwrap();
    let auth = Auth::new(pool.clone());
    let setup = auth.issue_bootstrap(BootstrapKind::Setup).await.unwrap();
    let session = auth.setup(&setup, "Operator", "synthetic-test-password").await.unwrap();
    let cookie = format!("__Host-shortcircuit_session={}", session.token);
    let hosts = PublicHosts::parse("go.example").unwrap();
    let (analytics, writer) = Analytics::start(pool.clone()).await.unwrap();
    let public = public_router_with_analytics(pool.clone(), hosts.clone(), analytics.clone());
    let admin = admin_router_with_analytics(
        pool.clone(),
        hosts,
        AdminConfig::new("https://admin.example", 8080).unwrap(),
        analytics,
    );
    let host_headers = [("host", "go.example")];
    assert_eq!(send(&public, "HEAD", "/tracked", &host_headers).await.status(), StatusCode::FOUND);
    assert_eq!(send(&public, "GET", "/private", &host_headers).await.status(), StatusCode::FOUND);
    assert_eq!(send(&public, "GET", "/tracked", &[("host", "go.example"), ("DNT", "1")]).await.status(), StatusCode::FOUND);
    assert_eq!(send(&public, "GET", "/missing?private=discarded", &host_headers).await.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        send(&public, "GET", "/tracked", &[
            ("host", "go.example"),
            ("referer", "https://referrer.example/sensitive?q=private"),
            ("user-agent", "Discordbot/2.0"),
        ]).await.status(),
        StatusCode::FOUND
    );
    let summary = "/api/v1/analytics/summary";
    assert_eq!(
        send(&admin, "GET", summary, &[("host", "admin.example")]).await.status(),
        StatusCode::UNAUTHORIZED
    );

    let authenticated: [(&str, &str); 2] = [("host", "admin.example"), ("cookie", &cookie)];
    let report = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let response = send(&admin, "GET", summary, &authenticated).await;
            assert_eq!(response.status(), StatusCode::OK);
            let body = to_bytes(response.into_body(), 100_000).await.unwrap();
            let data: Value = serde_json::from_slice(&body).unwrap();
            if data["totals"]["redirect"] != 0 {
                break data;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.unwrap();
    assert_eq!(report["totals"]["redirect"], 1);
    assert_eq!(report["totals"]["missing"], 1);

    assert_eq!(
        send(&admin, "DELETE", "/api/v1/analytics/data", &[
            ("host", "admin.example"), ("cookie", &cookie),
            ("origin", "https://admin.example"), ("x-csrf-token", &session.session.csrf),
        ]).await.status(),
        StatusCode::NO_CONTENT
    );
    drop(public);
    drop(admin);
    tokio::time::timeout(Duration::from_secs(2), writer).await.unwrap().unwrap();

    let count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM analytics_events")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}
