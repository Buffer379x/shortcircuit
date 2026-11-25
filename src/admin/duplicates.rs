use axum::{
    Json, Router,
    extract::{State, rejection::JsonRejection},
    routing::post,
};
use serde::{Deserialize, Serialize};
use sqlx::{QueryBuilder, Sqlite, SqlitePool};

use crate::{
    domains,
    input::{PublicHosts, valid_target},
    store,
};

use super::{Problem, json_payload};

const RESULT_LIMIT: usize = 10;

#[derive(Clone)]
struct DuplicateState {
    pool: SqlitePool,
    hosts: PublicHosts,
    local_port: u16,
}

pub(super) fn routes(pool: SqlitePool, hosts: PublicHosts, local_port: u16) -> Router {
    Router::new()
        .route("/api/v1/links/duplicates", post(find_duplicates))
        .with_state(DuplicateState {
            pool,
            hosts,
            local_port,
        })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DuplicateInput {
    target: String,
    exclude_id: Option<i64>,
}

#[derive(Serialize, sqlx::FromRow)]
struct DuplicateLink {
    id: i64,
    host: String,
    slug: String,
    title: String,
    enabled: bool,
}

#[derive(Serialize)]
struct DuplicateResult {
    items: Vec<DuplicateLink>,
    has_more: bool,
}

async fn find_duplicates(
    State(state): State<DuplicateState>,
    body: Result<Json<DuplicateInput>, JsonRejection>,
) -> Result<Json<DuplicateResult>, Problem> {
    let input = json_payload(body)?;
    if !valid_target(&input.target) {
        return Err(Problem::invalid("invalid_target"));
    }
    if input.exclude_id.is_some_and(|id| id <= 0) {
        return Err(Problem::invalid("invalid_link_id"));
    }

    let allowed: Vec<_> = domains::list(&state.pool, &state.hosts, state.local_port)
        .await
        .map_err(|_| Problem::unavailable())?
        .into_iter()
        .map(|domain| domain.host)
        .collect();
    let mut query = QueryBuilder::<Sqlite>::new(
        "SELECT id, host, slug, title, enabled FROM links WHERE target COLLATE BINARY = ",
    );
    query.push_bind(&input.target);
    if let Some(id) = input.exclude_id {
        query.push(" AND id != ").push_bind(id);
    }
    store::push_host_scope(&mut query, &allowed);
    query
        .push(" ORDER BY id DESC LIMIT ")
        .push_bind((RESULT_LIMIT + 1) as i64);
    let mut items = query
        .build_query_as::<DuplicateLink>()
        .fetch_all(&state.pool)
        .await
        .map_err(|_| Problem::unavailable())?;
    let has_more = items.len() > RESULT_LIMIT;
    items.truncate(RESULT_LIMIT);
    Ok(Json(DuplicateResult { items, has_more }))
}

#[cfg(test)]
mod tests {
    use axum::{
        body::{Body, to_bytes},
        http::{Method, Request, StatusCode, header},
    };
    use serde_json::{Value, json};
    use tower::ServiceExt;

    use super::*;

    const TARGET: &str = "https://files.example/A%2fB?x=A+B&x=C#Keep";

    async fn app() -> (tempfile::TempDir, SqlitePool, Router) {
        let directory = tempfile::tempdir().unwrap();
        let pool = crate::open_database(&directory.path().join("duplicates.sqlite"))
            .await
            .unwrap();
        let router = routes(
            pool.clone(),
            PublicHosts::parse("go.example").unwrap(),
            8080,
        );
        (directory, pool, router)
    }

    async fn link(pool: &SqlitePool, host: &str, slug: &str, target: &str, enabled: bool) -> i64 {
        sqlx::query(
            "INSERT INTO links (host, slug, target, title, enabled)
             VALUES (?, ?, ?, 'Saved link', ?)",
        )
        .bind(host)
        .bind(slug)
        .bind(target)
        .bind(enabled)
        .execute(pool)
        .await
        .unwrap()
        .last_insert_rowid()
    }

    async fn lookup(router: &Router, body: Value) -> (StatusCode, Value) {
        let request = Request::builder()
            .method(Method::POST)
            .uri("/api/v1/links/duplicates")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let response = router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 32_768).await.unwrap();
        let body = serde_json::from_slice(&bytes).unwrap();
        (status, body)
    }

    #[tokio::test]
    async fn matching_uses_exact_bytes_and_includes_disabled_known_domains() {
        let (_directory, pool, router) = app().await;
        sqlx::query(
            "INSERT INTO domains (host, origin, enabled)
             VALUES ('disabled.example', 'https://disabled.example', 0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        let active = link(&pool, "go.example", "Team/Docs", TARGET, true).await;
        let disabled = link(&pool, "disabled.example", "Archived", TARGET, false).await;
        link(&pool, "outside.example", "outside", TARGET, true).await;
        for (slug, target) in [
            ("case", "https://files.example/A%2FB?x=A+B&x=C#Keep"),
            ("space", "https://files.example/A%2fB?x=A%20B&x=C#Keep"),
            ("order", "https://files.example/A%2fB?x=C&x=A+B#Keep"),
        ] {
            link(&pool, "go.example", slug, target, true).await;
        }

        let (status, response) = lookup(&router, json!({"target": TARGET})).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            response,
            json!({
                "items": [
                    {"id": disabled, "host": "disabled.example", "slug": "Archived",
                        "title": "Saved link", "enabled": false},
                    {"id": active, "host": "go.example", "slug": "Team/Docs",
                        "title": "Saved link", "enabled": true}
                ],
                "has_more": false
            })
        );
    }

    #[tokio::test]
    async fn exclusion_is_applied_before_the_ten_item_cap() {
        let (_directory, pool, router) = app().await;
        let mut ids = Vec::new();
        for number in 0..11 {
            ids.push(link(&pool, "go.example", &format!("link-{number}"), TARGET, true).await);
        }
        let (status, paged) = lookup(&router, json!({"target": TARGET})).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(paged["has_more"], true);
        assert_eq!(paged["items"].as_array().unwrap().len(), 10);
        assert_eq!(paged["items"][0]["id"], ids[10]);
        assert_eq!(paged["items"][9]["id"], ids[1]);

        let (status, edited) =
            lookup(&router, json!({"target": TARGET, "exclude_id": ids[10]})).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(edited["has_more"], false);
        assert_eq!(edited["items"].as_array().unwrap().len(), 10);
        assert_eq!(edited["items"][0]["id"], ids[9]);
        assert_eq!(edited["items"][9]["id"], ids[0]);
    }

    #[tokio::test]
    async fn validation_errors_do_not_echo_the_destination() {
        let (_directory, _pool, router) = app().await;
        let (status, response) = lookup(
            &router,
            json!({"target": "https://private-user@files.example/secret-target"}),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(response["code"], "invalid_target");
        let response = response.to_string();
        assert!(!response.contains("private-user"));
        assert!(!response.contains("secret-target"));

        let (status, response) = lookup(&router, json!({"target": TARGET, "exclude_id": 0})).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(response["code"], "invalid_link_id");
    }
}
