use axum::{
    Json, Router,
    extract::{State, rejection::JsonRejection},
    http::StatusCode,
    routing::get,
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::{
    domains::{self, Domain},
    input::{PublicHosts, normalize_host, valid_target},
};

use super::{Problem, json_payload};

const DELETE_TAG: &str = "DELETE FROM link_tags WHERE tag = ?";

#[derive(Clone)]
struct CatalogState {
    pool: SqlitePool,
    hosts: PublicHosts,
    local_port: u16,
}

pub(super) fn routes(pool: SqlitePool, hosts: PublicHosts, local_port: u16) -> Router {
    Router::new()
        .route(
            "/api/v1/catalog/domains",
            get(list_domains).post(save_domain),
        )
        .route(
            "/api/v1/catalog/tags",
            get(list_tags).patch(rename_tag).delete(remove_tag),
        )
        .with_state(CatalogState {
            pool,
            hosts,
            local_port,
        })
}

#[derive(Serialize)]
struct Catalog<T> {
    items: Vec<T>,
}

async fn list_domains(State(state): State<CatalogState>) -> Result<Json<Catalog<Domain>>, Problem> {
    let items = domains::list(&state.pool, &state.hosts, state.local_port)
        .await
        .map_err(|_| Problem::unavailable())?;
    Ok(Json(Catalog { items }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DomainInput {
    host: String,
    origin: String,
    enabled: bool,
    root_target: Option<String>,
    missing_target: Option<String>,
}

async fn save_domain(
    State(state): State<CatalogState>,
    body: Result<Json<DomainInput>, JsonRejection>,
) -> Result<Json<Domain>, Problem> {
    let input = json_payload(body)?;
    let host = normalize_host(input.host.trim()).ok_or_else(|| Problem::invalid("invalid_host"))?;
    let origin = domains::canonical_origin(&host, &input.origin)
        .ok_or_else(|| Problem::invalid("invalid_origin"))?;
    for target in [&input.root_target, &input.missing_target]
        .into_iter()
        .flatten()
    {
        if !valid_target(target) {
            return Err(Problem::invalid("invalid_fallback"));
        }
    }
    if input
        .root_target
        .as_deref()
        .is_some_and(|target| domains::fallback_loops(&host, "/", target))
    {
        return Err(Problem::invalid("fallback_loop"));
    }
    let domain = Domain {
        host,
        origin,
        enabled: input.enabled,
        root_target: input.root_target,
        missing_target: input.missing_target,
    };
    if !domains::save(&state.pool, &domain)
        .await
        .map_err(|_| Problem::unavailable())?
    {
        return Err(Problem(StatusCode::CONFLICT, "domain_limit"));
    }
    Ok(Json(domain))
}

#[derive(Serialize, sqlx::FromRow)]
struct Tag {
    name: String,
    link_count: i64,
}

async fn list_tags(State(state): State<CatalogState>) -> Result<Json<Catalog<Tag>>, Problem> {
    let items = sqlx::query_as(
        "SELECT tag AS name, COUNT(*) AS link_count FROM link_tags GROUP BY tag ORDER BY tag",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|_| Problem::unavailable())?;
    Ok(Json(Catalog { items }))
}

pub(super) use crate::input::tag_name;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RenameTag {
    name: String,
    new_name: String,
}

async fn rename_tag(
    State(state): State<CatalogState>,
    body: Result<Json<RenameTag>, JsonRejection>,
) -> Result<StatusCode, Problem> {
    let input = json_payload(body)?;
    let name = tag_name(&input.name).ok_or_else(|| Problem::invalid("invalid_tags"))?;
    let new_name = tag_name(&input.new_name).ok_or_else(|| Problem::invalid("invalid_tags"))?;

    if name == new_name {
        let exists =
            sqlx::query_scalar::<_, bool>("SELECT EXISTS (SELECT 1 FROM link_tags WHERE tag = ?)")
                .bind(name)
                .fetch_one(&state.pool)
                .await
                .map_err(|_| Problem::unavailable())?;
        return tag_status(exists);
    }

    let mut transaction = state
        .pool
        .begin()
        .await
        .map_err(|_| Problem::unavailable())?;
    sqlx::query(
        "INSERT INTO link_tags (link_id, tag)
         SELECT link_id, ? FROM link_tags WHERE tag = ?
         ON CONFLICT (link_id, tag) DO NOTHING",
    )
    .bind(new_name)
    .bind(name)
    .execute(&mut *transaction)
    .await
    .map_err(|_| Problem::unavailable())?;
    let deleted = sqlx::query(DELETE_TAG)
        .bind(name)
        .execute(&mut *transaction)
        .await
        .map_err(|_| Problem::unavailable())?;
    transaction
        .commit()
        .await
        .map_err(|_| Problem::unavailable())?;
    tag_status(deleted.rows_affected() != 0)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RemoveTag {
    name: String,
}

async fn remove_tag(
    State(state): State<CatalogState>,
    body: Result<Json<RemoveTag>, JsonRejection>,
) -> Result<StatusCode, Problem> {
    let input = json_payload(body)?;
    let name = tag_name(&input.name).ok_or_else(|| Problem::invalid("invalid_tags"))?;
    let deleted = sqlx::query(DELETE_TAG)
        .bind(name)
        .execute(&state.pool)
        .await
        .map_err(|_| Problem::unavailable())?;
    tag_status(deleted.rows_affected() != 0)
}

fn tag_status(exists: bool) -> Result<StatusCode, Problem> {
    if exists {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(Problem(StatusCode::NOT_FOUND, "not_found"))
    }
}

#[cfg(test)]
mod tests {
    use axum::{
        body::{Body, to_bytes},
        http::{Method, Request, header},
        response::Response,
    };
    use serde_json::{Value, json};
    use tower::ServiceExt;

    use super::*;

    async fn request(router: &Router, method: Method, path: &str, body: Value) -> Response {
        router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn tag_merge_keeps_one_assignment_per_link_and_delete_removes_only_that_tag() {
        let directory = tempfile::tempdir().unwrap();
        let pool = crate::open_database(&directory.path().join("catalog.sqlite"))
            .await
            .unwrap();
        sqlx::raw_sql(
            "INSERT INTO links (id, host, slug, target) VALUES
                (1, 'go.example', 'one', 'https://example.test'),
                (2, 's.example', 'two', 'https://example.test');
             INSERT INTO link_tags (link_id, tag) VALUES
                (1, 'old'), (1, 'new'), (1, 'keep'), (2, 'old');",
        )
        .execute(&pool)
        .await
        .unwrap();
        let router = routes(
            pool.clone(),
            PublicHosts::parse("go.example").unwrap(),
            8080,
        );
        let path = "/api/v1/catalog/tags";
        let merged = request(
            &router,
            Method::PATCH,
            path,
            json!({"name": "old", "new_name": " new "}),
        )
        .await;
        assert_eq!(merged.status(), StatusCode::NO_CONTENT);
        let listed = request(&router, Method::GET, path, Value::Null).await;
        let listed: Value =
            serde_json::from_slice(&to_bytes(listed.into_body(), 8192).await.unwrap()).unwrap();
        assert_eq!(
            listed,
            json!({"items": [{"name": "keep", "link_count": 1}, {"name": "new", "link_count": 2}]})
        );

        let removed = request(&router, Method::DELETE, path, json!({"name": "new"})).await;
        assert_eq!(removed.status(), StatusCode::NO_CONTENT);
        let remaining: Vec<(i64, String)> = sqlx::query_as("SELECT link_id, tag FROM link_tags")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(remaining, [(1, "keep".to_owned())]);
    }

    #[tokio::test]
    async fn catalog_rejects_invalid_origin_and_a_root_fallback_loop() {
        let directory = tempfile::tempdir().unwrap();
        let pool = crate::open_database(&directory.path().join("catalog.sqlite"))
            .await
            .unwrap();
        let router = routes(pool, PublicHosts::parse("go.example").unwrap(), 8080);
        let path = "/api/v1/catalog/domains";
        for body in [
            json!({"host": "go.example", "origin": "https://elsewhere.test", "enabled": true}),
            json!({"host": "go.example", "origin": "https://go.example", "enabled": true,
                "root_target": "https://go.example/?loop=1"}),
        ] {
            let response = request(&router, Method::POST, path, body).await;
            assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        }
    }
}
