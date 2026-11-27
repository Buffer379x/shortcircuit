mod csv_format;

use std::collections::{HashMap, HashSet};

use axum::{
    Json, Router,
    body::Bytes,
    extract::{
        DefaultBodyLimit, Query, State,
        rejection::{BytesRejection, JsonRejection, QueryRejection},
    },
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use sqlx::{QueryBuilder, Sqlite, SqliteConnection, SqlitePool};

use crate::{
    domains,
    input::{PublicHosts, normalize_host, plain_description, valid_slug, valid_target},
    store::Link,
};

use super::{BODY_LIMIT, Problem, RowProblem, catalog::tag_name};

const EXCHANGE_VERSION: u8 = 1;
const MAX_ITEMS: usize = 100;
const MAX_SERIALIZED_ITEMS: usize = BODY_LIMIT - 768;

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ExportFormat {
    Json,
    Csv,
}

#[derive(Serialize)]
struct SelectedFile {
    version: u8,
    items: Vec<LinkMetadata>,
}

pub(super) fn export_selected(
    links: Vec<(Link, bool)>,
    format: ExportFormat,
) -> Result<Response, Problem> {
    let rows = links.into_iter().map(|(link, protected)| {
        (
            link.id,
            LinkMetadata {
                host: link.host,
                slug: link.slug,
                target: link.target,
                title: link.title,
                description: link.description,
                tags: link.tags,
                enabled: link.enabled && !protected,
                analytics_enabled: link.analytics_enabled,
            },
        )
    });
    let too_large = || Problem::invalid("bulk_export_too_large");
    let (bytes, media, disposition) = match format {
        ExportFormat::Json => {
            let file = SelectedFile {
                version: EXCHANGE_VERSION,
                items: rows.map(|(_, item)| item).collect(),
            };
            let data = serde_json::to_vec(&file).map_err(|_| Problem::unavailable())?;
            if data.len() > MAX_SERIALIZED_ITEMS {
                return Err(too_large());
            }
            (
                data,
                "application/json",
                "attachment; filename=shortcircuit-selected-links.json",
            )
        }
        ExportFormat::Csv => {
            let (data, next) =
                csv_format::page(rows.collect()).map_err(|_| Problem::unavailable())?;
            if next.is_some() {
                return Err(too_large());
            }
            (
                data,
                "text/csv; charset=utf-8",
                "attachment; filename=shortcircuit-selected-links.csv",
            )
        }
    };
    Ok((
        [
            (header::CONTENT_TYPE, media),
            (header::CONTENT_DISPOSITION, disposition),
        ],
        Bytes::from(bytes),
    )
        .into_response())
}

#[derive(Clone)]
struct ExchangeState {
    pool: SqlitePool,
    hosts: PublicHosts,
}

pub(super) fn routes(pool: SqlitePool, hosts: PublicHosts) -> Router {
    Router::new()
        .route(
            "/api/v1/exchange/links",
            get(export_links).post(import_links),
        )
        .route(
            "/api/v1/exchange/links.csv",
            get(export_csv).post(import_csv),
        )
        .route("/api/v1/exchange/links.csv/columns", post(csv_columns))
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .with_state(ExchangeState { pool, hosts })
}

#[derive(Serialize, Deserialize, PartialEq, Eq, sqlx::FromRow)]
#[serde(deny_unknown_fields)]
struct LinkMetadata {
    host: String,
    slug: String,
    target: String,
    title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    description: String,
    #[sqlx(skip)]
    tags: Vec<String>,
    enabled: bool,
    analytics_enabled: bool,
}

#[derive(sqlx::FromRow)]
struct TaggedLink {
    id: i64,
    #[sqlx(flatten)]
    link: LinkMetadata,
    tag: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExportQuery {
    after_id: Option<i64>,
}

#[derive(Serialize)]
struct ExportPage {
    version: u8,
    items: Vec<LinkMetadata>,
    next_after_id: Option<i64>,
}

async fn export_links(
    State(state): State<ExchangeState>,
    query: Result<Query<ExportQuery>, QueryRejection>,
) -> Result<Json<ExportPage>, Problem> {
    let rows = export_rows(&state.pool, query).await?;

    let mut items = Vec::new();
    let mut used_bytes = 0;
    let mut last_id = None;
    let mut next_after_id = None;
    for (id, item) in rows {
        // Keep the page small enough to post back with the import wrapper.
        let bytes = serde_json::to_vec(&item)
            .map_err(|_| Problem::unavailable())?
            .len()
            + 1;
        if items.len() == MAX_ITEMS || used_bytes + bytes > MAX_SERIALIZED_ITEMS {
            next_after_id = Some(last_id.ok_or_else(Problem::unavailable)?);
            break;
        }
        used_bytes += bytes;
        last_id = Some(id);
        items.push(item);
    }

    Ok(Json(ExportPage {
        version: EXCHANGE_VERSION,
        items,
        next_after_id,
    }))
}

async fn export_rows(
    pool: &SqlitePool,
    query: Result<Query<ExportQuery>, QueryRejection>,
) -> Result<Vec<(i64, LinkMetadata)>, Problem> {
    let Query(query) = query.map_err(|error| Problem(error.status(), "invalid_exchange"))?;
    let after_id = query.after_id.unwrap_or(0);
    if after_id < 0 {
        return Err(invalid());
    }
    let rows = sqlx::query_as::<_, TaggedLink>(
        "WITH page AS (
             SELECT id, host, slug, target, title, description, analytics_enabled,
                 (enabled AND NOT EXISTS (
                     SELECT 1 FROM link_protection AS protection WHERE protection.link_id = links.id
                     AND (protection.one_time = 1 OR protection.password_hash IS NOT NULL)
                 )) AS enabled
             FROM links WHERE id > ? ORDER BY id LIMIT ?
         )
         SELECT page.*, link_tags.tag FROM page
         LEFT JOIN link_tags ON link_tags.link_id = page.id
         ORDER BY page.id, link_tags.tag",
    )
    .bind(after_id)
    .bind((MAX_ITEMS + 1) as i64)
    .fetch_all(pool)
    .await
    .map_err(|_| Problem::unavailable())?;

    Ok(combine_tags(rows))
}

async fn export_csv(
    State(state): State<ExchangeState>,
    query: Result<Query<ExportQuery>, QueryRejection>,
) -> Result<Response, Problem> {
    let rows = export_rows(&state.pool, query).await?;
    let (content, next) = csv_format::page(rows).map_err(|_| Problem::unavailable())?;
    let mut response = (
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=shortcircuit-links.csv",
            ),
        ],
        Bytes::from(content),
    )
        .into_response();
    if let Some(next) = next {
        let cursor =
            HeaderValue::from_str(&next.to_string()).map_err(|_| Problem::unavailable())?;
        response.headers_mut().insert("x-next-after-id", cursor);
    }
    Ok(response)
}

async fn csv_columns(
    headers: HeaderMap,
    bytes: Result<Bytes, BytesRejection>,
) -> Result<Json<csv_format::Columns>, RowProblem> {
    let bytes = csv_body(&headers, bytes)?;
    csv_format::inspect(&bytes).map(Json).map_err(row_problem)
}

async fn import_csv(
    State(state): State<ExchangeState>,
    headers: HeaderMap,
    query: Result<Query<csv_format::ImportQuery>, QueryRejection>,
    bytes: Result<Bytes, BytesRejection>,
) -> Result<(StatusCode, Json<ImportReport>), RowProblem> {
    let Query(query) = query.map_err(|error| Problem(error.status(), "invalid_exchange"))?;
    let bytes = csv_body(&headers, bytes)?;
    let items = if query.is_mapped() {
        csv_format::parse_mapped(&bytes, &query)
    } else {
        csv_format::parse(&bytes)
    }
    .map_err(row_problem)?;
    apply_import(
        &state,
        ImportPage {
            version: EXCHANGE_VERSION,
            preview: query.preview.unwrap_or(true),
            items,
        },
    )
    .await
}

fn csv_body(headers: &HeaderMap, bytes: Result<Bytes, BytesRejection>) -> Result<Bytes, Problem> {
    let csv_body = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("text/csv"));
    if !csv_body {
        return Err(Problem(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "invalid_exchange",
        ));
    }
    bytes.map_err(|error| Problem(error.status(), "invalid_exchange"))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportPage {
    version: u8,
    preview: bool,
    items: Vec<LinkMetadata>,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ImportOutcome {
    Create,
    Unchanged,
    Conflict,
}

#[derive(Serialize)]
struct ImportRow {
    host: String,
    slug: String,
    status: ImportOutcome,
}

#[derive(Serialize)]
struct ImportReport {
    version: u8,
    preview: bool,
    applied: bool,
    items: Vec<ImportRow>,
}

async fn import_links(
    State(state): State<ExchangeState>,
    body: Result<Json<ImportPage>, JsonRejection>,
) -> Result<(StatusCode, Json<ImportReport>), RowProblem> {
    let Json(input) = body.map_err(|error| Problem(error.status(), "invalid_exchange"))?;
    apply_import(&state, input).await
}

async fn apply_import(
    state: &ExchangeState,
    mut input: ImportPage,
) -> Result<(StatusCode, Json<ImportReport>), RowProblem> {
    validate(&mut input)?;

    let statement = if input.preview {
        "BEGIN"
    } else {
        "BEGIN IMMEDIATE"
    };
    let mut transaction = state
        .pool
        .begin_with(statement)
        .await
        .map_err(|_| Problem::unavailable())?;
    let active = domains::active_hosts(&mut *transaction, &state.hosts)
        .await
        .map_err(|_| Problem::unavailable())?;
    if let Some(row) = input
        .items
        .iter()
        .position(|item| !active.contains(&item.host))
    {
        return Err(RowProblem::at(invalid(), row + 1, Some("host")));
    }
    let outcomes = compare_existing(&mut transaction, &input.items)
        .await
        .map_err(|_| Problem::unavailable())?;
    // The policy check stops at the first refused destination, which is the last one yielded.
    let checked = std::sync::atomic::AtomicUsize::new(0);
    let created = input
        .items
        .iter()
        .zip(&outcomes)
        .enumerate()
        .filter(|(_, (_, outcome))| **outcome == ImportOutcome::Create)
        .map(|(index, (item, _))| {
            checked.store(index + 1, std::sync::atomic::Ordering::Relaxed);
            item.target.as_str()
        });
    if crate::destinations::check_writes(&mut *transaction, created, &[])
        .await
        .map_err(|_| Problem::unavailable())?
        .is_err()
    {
        return Err(RowProblem::at(
            Problem::invalid("destination_not_allowed"),
            checked.load(std::sync::atomic::Ordering::Relaxed),
            Some("target"),
        ));
    }
    let conflict = outcomes.contains(&ImportOutcome::Conflict);
    let applied = !input.preview && !conflict;
    if applied {
        for (item, outcome) in input.items.iter().zip(&outcomes) {
            if *outcome == ImportOutcome::Create {
                insert_link(&mut transaction, item)
                    .await
                    .map_err(|_| Problem::unavailable())?;
            }
        }
    }
    transaction
        .commit()
        .await
        .map_err(|_| Problem::unavailable())?;

    let status = if conflict && !input.preview {
        StatusCode::CONFLICT
    } else {
        StatusCode::OK
    };
    let items = input
        .items
        .into_iter()
        .zip(outcomes)
        .map(|(item, status)| ImportRow {
            host: item.host,
            slug: item.slug,
            status,
        })
        .collect();
    Ok((
        status,
        Json(ImportReport {
            version: EXCHANGE_VERSION,
            preview: input.preview,
            applied,
            items,
        }),
    ))
}

fn invalid() -> Problem {
    Problem::invalid("invalid_exchange")
}

fn row_problem(error: csv_format::RowError) -> RowProblem {
    match error.row {
        Some(row) => RowProblem::at(invalid(), row, error.field),
        None => invalid().into(),
    }
}

/// Validates and normalizes every item. The first invalid item is reported by its 1-based
/// position and the field that failed, without echoing its value.
fn validate(input: &mut ImportPage) -> Result<(), RowProblem> {
    if input.version != EXCHANGE_VERSION || !(1..=MAX_ITEMS).contains(&input.items.len()) {
        return Err(invalid().into());
    }
    for (index, item) in input.items.iter_mut().enumerate() {
        let failed = |field| RowProblem::at(invalid(), index + 1, Some(field));
        item.host = normalize_host(item.host.trim()).ok_or_else(|| failed("host"))?;
        if !valid_slug(&item.slug) {
            return Err(failed("slug"));
        }
        if !valid_target(&item.target) {
            return Err(failed("target"));
        }
        if item.title.chars().count() > 160 || item.title.chars().any(char::is_control) {
            return Err(failed("title"));
        }
        if item.tags.len() > 20 {
            return Err(failed("tags"));
        }
        for tag in &mut item.tags {
            *tag = tag_name(tag).ok_or_else(|| failed("tags"))?.to_owned();
        }
        item.description = plain_description(&item.description)
            .ok_or_else(|| failed("description"))?
            .to_owned();
        item.tags.sort_unstable();
        item.tags.dedup();
    }
    let mut seen = HashSet::with_capacity(input.items.len());
    if let Some(row) = input
        .items
        .iter()
        .position(|item| !seen.insert((item.host.as_str(), item.slug.as_str())))
    {
        // A repeated host and slug is reported at its second occurrence.
        return Err(RowProblem::at(invalid(), row + 1, Some("slug")));
    }
    Ok(())
}

async fn compare_existing(
    connection: &mut SqliteConnection,
    items: &[LinkMetadata],
) -> Result<Vec<ImportOutcome>, sqlx::Error> {
    let mut query = QueryBuilder::<Sqlite>::new("WITH requested(host, slug) AS (");
    query.push_values(items, |mut values, item| {
        values.push_bind(&item.host).push_bind(&item.slug);
    });
    query.push(
        ") SELECT links.id, links.host, links.slug, links.target, links.title, links.description,
                  links.enabled, links.analytics_enabled, link_tags.tag
           FROM requested
           JOIN links ON links.host = requested.host AND links.slug = requested.slug
           LEFT JOIN link_tags ON link_tags.link_id = links.id
           ORDER BY links.id, link_tags.tag",
    );
    let existing = combine_tags(
        query
            .build_query_as::<TaggedLink>()
            .fetch_all(connection)
            .await?,
    );
    let by_identity: HashMap<_, _> = existing
        .iter()
        .map(|(_, item)| ((item.host.as_str(), item.slug.as_str()), item))
        .collect();
    Ok(items
        .iter()
        .map(
            |item| match by_identity.get(&(item.host.as_str(), item.slug.as_str())) {
                Some(previous) if *previous == item => ImportOutcome::Unchanged,
                Some(_) => ImportOutcome::Conflict,
                None => ImportOutcome::Create,
            },
        )
        .collect())
}

fn combine_tags(rows: Vec<TaggedLink>) -> Vec<(i64, LinkMetadata)> {
    let mut items: Vec<(i64, LinkMetadata)> = Vec::new();
    for TaggedLink { id, link, tag } in rows {
        if items.last().is_none_or(|(previous, _)| *previous != id) {
            items.push((id, link));
        }
        if let (Some((_, item)), Some(tag)) = (items.last_mut(), tag) {
            item.tags.push(tag);
        }
    }
    items
}

async fn insert_link(
    connection: &mut SqliteConnection,
    item: &LinkMetadata,
) -> Result<(), sqlx::Error> {
    let id = sqlx::query(
        "INSERT INTO links (host, slug, target, title, description, enabled, analytics_enabled)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&item.host)
    .bind(&item.slug)
    .bind(&item.target)
    .bind(&item.title)
    .bind(&item.description)
    .bind(item.enabled)
    .bind(item.analytics_enabled)
    .execute(&mut *connection)
    .await?
    .last_insert_rowid();
    if !item.tags.is_empty() {
        let mut query = QueryBuilder::<Sqlite>::new("INSERT INTO link_tags (link_id, tag) ");
        query.push_values(&item.tags, |mut values, tag| {
            values.push_bind(id).push_bind(tag);
        });
        query.build().execute(connection).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use axum::{
        body::{Body, to_bytes},
        http::{Method, Request},
    };
    use serde_json::{Value, json};
    use tower::ServiceExt;

    use super::*;

    const PATH: &str = "/api/v1/exchange/links";

    async fn app() -> (tempfile::TempDir, SqlitePool, Router) {
        let directory = tempfile::tempdir().unwrap();
        let pool = crate::open_database(&directory.path().join("exchange.sqlite"))
            .await
            .unwrap();
        let router = routes(pool.clone(), PublicHosts::parse("go.example").unwrap());
        (directory, pool, router)
    }

    fn item(slug: &str, target: &str) -> Value {
        json!({
            "host": "GO.example.",
            "slug": slug,
            "target": target,
            "title": "Released",
            "tags": [" docs ", "team", "docs"],
            "enabled": false,
            "analytics_enabled": false
        })
    }

    async fn request(
        router: &Router,
        method: Method,
        path: &str,
        data: Value,
    ) -> (StatusCode, Value) {
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(data.to_string()))
            .unwrap();
        let response = router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), 200_000).await.unwrap();
        (status, serde_json::from_slice(&body).unwrap())
    }

    async fn import(router: &Router, preview: bool, items: Vec<Value>) -> (StatusCode, Value) {
        request(
            router,
            Method::POST,
            PATH,
            json!({"version": 1, "preview": preview, "items": items}),
        )
        .await
    }

    async fn post_csv(router: &Router, path: &str, data: &'static str) -> (StatusCode, Value) {
        let request = Request::builder()
            .method(Method::POST)
            .uri(path)
            .header(header::CONTENT_TYPE, "text/csv")
            .body(Body::from(data))
            .unwrap();
        let response = router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 200_000).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    async fn link_count(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM links")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn preview_and_retry_preserve_destination_spellings_and_normalize_tag_sets() {
        let (_directory, pool, router) = app().await;
        let destination = "https://files.example/A%2fB?x=A+B&x=C#Keep";
        let archived = item("Team/Docs", destination);

        let (status, preview) = import(&router, true, vec![archived.clone()]).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            preview,
            json!({
                "version": 1, "preview": true, "applied": false,
                "items": [{"host": "go.example", "slug": "Team/Docs", "status": "create"}]
            })
        );
        assert_eq!(link_count(&pool).await, 0);

        let (status, report) = import(&router, false, vec![archived.clone()]).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(report["applied"], true);
        assert_eq!(report["items"][0]["status"], "create");
        let (status, repeated) = import(&router, false, vec![archived]).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(repeated["items"][0]["status"], "unchanged");

        let (status, exported) = request(&router, Method::GET, PATH, Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            exported,
            json!({
                "version": 1, "next_after_id": null,
                "items": [{"host": "go.example", "slug": "Team/Docs", "target": destination,
                           "title": "Released", "tags": ["docs", "team"], "enabled": false,
                           "analytics_enabled": false}]
            })
        );
    }

    #[tokio::test]
    async fn mapped_csv_uses_the_same_preview_and_apply_pipeline() {
        let (_directory, pool, router) = app().await;
        let file = "Code,Address,Notes,Site\n\
            Case/One,https://example.test/A%2fB,\"A comma, and\r\na second line\",GO.example.\n";
        let (status, columns) = post_csv(&router, "/api/v1/exchange/links.csv/columns", file).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            columns,
            json!({"columns": ["Code", "Address", "Notes", "Site"], "native": false})
        );
        let path = "/api/v1/exchange/links.csv?slug_column=0&target_column=1&description_column=2&host_column=3";
        let (status, preview) = post_csv(&router, path, file).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(preview["preview"], true);
        assert_eq!(preview["items"][0]["status"], "create");
        assert_eq!(link_count(&pool).await, 0);
        let (status, applied) = post_csv(&router, &format!("{path}&preview=false"), file).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(applied["applied"], true);
        let (description, enabled, analytics): (String, bool, bool) = sqlx::query_as(
            "SELECT description, enabled, analytics_enabled FROM links
             WHERE host = 'go.example' AND slug = 'Case/One'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(description, "A comma, and\na second line");
        assert!(!enabled);
        assert!(!analytics);
    }

    #[tokio::test]
    async fn apply_checks_conflicts_again_and_leaves_the_whole_page_unchanged() {
        let (_directory, pool, router) = app().await;
        let items = vec![
            item("one", "https://example.test/a%2fb"),
            item("two", "https://example.test/two"),
        ];
        let (_, preview) = import(&router, true, items.clone()).await;
        assert_eq!(preview["items"][0]["status"], "create");
        let (status, competing) = import(
            &router,
            false,
            vec![item("one", "https://example.test/a%2Fb")],
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(competing["applied"], true);

        let (status, rejected) = import(&router, false, items).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(rejected["applied"], false);
        assert_eq!(rejected["items"][0]["status"], "conflict");
        assert_eq!(rejected["items"][1]["status"], "create");
        let targets: Vec<String> = sqlx::query_scalar("SELECT target FROM links ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(targets, ["https://example.test/a%2Fb"]);
    }

    #[tokio::test]
    async fn invalid_items_reject_the_document_without_echoing_private_fields() {
        let (_directory, pool, router) = app().await;
        let valid = item("one", "https://example.test/a");
        let mut duplicate = valid.clone();
        duplicate["host"] = json!("go.example");
        let mut inactive = item("two", "https://secret.example/private");
        inactive["host"] = json!("inactive.example");
        inactive["tags"] = json!(["private tag"]);
        let mut invalid_url = inactive.clone();
        invalid_url["host"] = json!("go.example");
        invalid_url["target"] = json!("https://secret@example.test/private");

        for (rejected, field) in [
            (duplicate, "slug"),
            (inactive, "host"),
            (invalid_url, "target"),
        ] {
            let (status, body) = import(&router, false, vec![valid.clone(), rejected]).await;
            assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
            assert_eq!(body["code"], "invalid_exchange");
            // The first failing item is named by position and field, never by its values.
            assert_eq!(body["row"], 2, "{field}");
            assert_eq!(body["field"], field);
            let body = body.to_string();
            assert!(!body.contains("private"));
            assert!(!body.contains("secret"));
        }
        sqlx::query(
            "INSERT INTO domains (host, origin, enabled)
             VALUES ('go.example', 'https://go.example', 0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        let (status, body) = import(&router, true, vec![valid]).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["code"], "invalid_exchange");
        assert_eq!(
            (body["row"].as_u64(), body["field"].as_str()),
            (Some(1), Some("host"))
        );
        assert_eq!(link_count(&pool).await, 0);
    }

    #[tokio::test]
    async fn every_rejected_import_field_is_reported_with_its_item() {
        let (_directory, pool, router) = app().await;
        let valid = item("one", "https://example.test/a");
        for (key, value, field) in [
            ("slug", json!("../escape"), "slug"),
            ("title", json!("line\u{7}"), "title"),
            ("title", json!("x".repeat(161)), "title"),
            ("tags", json!(["", "team"]), "tags"),
            ("description", json!("<b>markup</b>\u{0}"), "description"),
            ("host", json!("not a host"), "host"),
        ] {
            let mut rejected = item("three", "https://example.test/three");
            rejected[key] = value;
            let items = vec![
                valid.clone(),
                item("two", "https://example.test/two"),
                rejected,
            ];
            let (status, body) = import(&router, true, items).await;
            assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{key}");
            assert_eq!(body["code"], "invalid_exchange");
            assert_eq!(body["row"], 3, "{key}");
            assert_eq!(body["field"], field, "{key}");
        }
        // Whole-document problems name no row.
        let (status, body) = request(
            &router,
            Method::POST,
            PATH,
            json!({"version": 2, "preview": true, "items": [valid]}),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(body.get("row").is_none() && body.get("field").is_none());
        assert_eq!(link_count(&pool).await, 0);
    }

    #[tokio::test]
    async fn csv_imports_report_the_first_failing_data_row() {
        let (_directory, pool, router) = app().await;
        let header = "version,host,slug,target,title,tags,enabled,analytics_enabled\n";
        let good = "1,go.example,one,https://example.test/one,One,[],true,false\n";
        let cases: [(&'static str, Value, Value); 5] = [
            // Native rows: a bad boolean, an unknown version, and an invalid destination.
            (
                "1,go.example,two,https://example.test/two,Two,[],maybe,false\n",
                json!(2),
                json!("enabled"),
            ),
            (
                "2,go.example,two,https://example.test/two,Two,[],true,false\n",
                json!(2),
                json!("version"),
            ),
            (
                "1,go.example,two,https://user@example.test/two,Two,[],true,false\n",
                json!(2),
                json!("target"),
            ),
            // A record with a different number of fields has no field.
            ("1,go.example,two\n", json!(2), Value::Null),
            (
                "1,go.example,two,https://example.test/two,Two,not-json,true,false\n",
                json!(2),
                json!("tags"),
            ),
        ];
        for (row, expected_row, expected_field) in cases {
            let file: &'static str = Box::leak(format!("{header}{good}{row}").into_boxed_str());
            let (status, body) = post_csv(&router, "/api/v1/exchange/links.csv", file).await;
            assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{row}");
            assert_eq!(body["code"], "invalid_exchange");
            assert_eq!(body["row"], expected_row, "{row}");
            assert_eq!(
                body.get("field").cloned().unwrap_or(Value::Null),
                expected_field,
                "{row}"
            );
            assert!(!body.to_string().contains("user@"));
        }

        // Mapped files use the same 1-based data row, not counting the header.
        let mapped = "Code,Address\nfirst,https://example.test/1\nsecond,https://example.test/2\nbad slug,https://example.test/3\n";
        let path = "/api/v1/exchange/links.csv?slug_column=0&target_column=1&host=go.example";
        let (status, body) = post_csv(&router, path, mapped).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            (body["row"].as_u64(), body["field"].as_str()),
            (Some(3), Some("slug"))
        );
        let flags = "Code,Address,On\nfirst,https://example.test/1,yes\n";
        let path = "/api/v1/exchange/links.csv?slug_column=0&target_column=1&enabled_column=2&host=go.example";
        let (status, body) = post_csv(&router, path, flags).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            (body["row"].as_u64(), body["field"].as_str()),
            (Some(1), Some("enabled"))
        );
        // Column inspection reports malformed records by row as well.
        let (status, body) = post_csv(
            &router,
            "/api/v1/exchange/links.csv/columns",
            "Code,Address\nfirst,https://example.test/1\nshort\n",
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["row"], 2);
        assert!(body.get("field").is_none());
        assert_eq!(link_count(&pool).await, 0);
    }

    #[tokio::test]
    async fn a_refused_destination_names_its_item() {
        let (_directory, pool, router) = app().await;
        sqlx::raw_sql(
            "INSERT INTO destination_rules (host, subdomains, action)
                 VALUES ('blocked.example', 0, 'block');
             UPDATE destination_policy SET enabled = 1, generation = generation + 1 WHERE id = 1;",
        )
        .execute(&pool)
        .await
        .unwrap();
        let items = vec![
            item("one", "https://example.test/one"),
            item("two", "https://example.test/two"),
            item("three", "https://blocked.example/three"),
        ];
        let (status, body) = import(&router, true, items).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["code"], "destination_not_allowed");
        assert_eq!(
            (body["row"].as_u64(), body["field"].as_str()),
            (Some(3), Some("target"))
        );
        assert!(!body.to_string().contains("blocked.example"));
        assert_eq!(link_count(&pool).await, 0);
    }

    #[tokio::test]
    async fn a_write_error_rolls_back_every_new_link_on_the_page() {
        let (_directory, pool, router) = app().await;
        sqlx::raw_sql(
            "CREATE TRIGGER deny_rejected_tag BEFORE INSERT ON link_tags
             WHEN NEW.tag = 'reject' BEGIN SELECT RAISE(ABORT, 'test veto'); END;",
        )
        .execute(&pool)
        .await
        .unwrap();
        let mut rejected = item("two", "https://example.test/two");
        rejected["tags"] = json!(["reject"]);
        let (status, _) = import(
            &router,
            false,
            vec![item("one", "https://example.test/one"), rejected],
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(link_count(&pool).await, 0);
    }

    #[tokio::test]
    async fn byte_capped_export_can_be_imported_without_skipping_rows() {
        let (_directory, pool, router) = app().await;
        let target = format!("https://files.example/{}", "a".repeat(4070));
        for number in 1..=10 {
            sqlx::query("INSERT INTO links (host, slug, target) VALUES ('go.example', ?, ?)")
                .bind(format!("link-{number}"))
                .bind(&target)
                .execute(&pool)
                .await
                .unwrap();
        }

        let (status, first) = request(&router, Method::GET, PATH, Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(first["items"].as_array().unwrap().len(), 7);
        assert_eq!(first["next_after_id"], 7);
        let first_items = first["items"].as_array().unwrap().clone();
        let (status, report) = import(&router, true, first_items).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(report["items"][6]["status"], "unchanged");
        let (_, last) = request(
            &router,
            Method::GET,
            &format!("{PATH}?after_id=7"),
            Value::Null,
        )
        .await;
        assert_eq!(last["items"].as_array().unwrap().len(), 3);
        assert_eq!(last["items"][0]["slug"], "link-8");
        assert_eq!(last["next_after_id"], Value::Null);
    }

    #[tokio::test]
    async fn export_pages_all_stored_metadata_in_id_order() {
        let (_directory, pool, router) = app().await;
        sqlx::query(
            "WITH RECURSIVE ids(n) AS (VALUES (1) UNION ALL SELECT n + 1 FROM ids WHERE n < 101)
             INSERT INTO links (host, slug, target)
             SELECT 'past.example', printf('link-%d', n), 'https://example.test' FROM ids",
        )
        .execute(&pool)
        .await
        .unwrap();
        let (_, first) = request(&router, Method::GET, PATH, Value::Null).await;
        assert_eq!(first["items"].as_array().unwrap().len(), 100);
        assert_eq!(first["items"][0]["slug"], "link-1");
        assert_eq!(first["next_after_id"], 100);
        let (_, last) = request(
            &router,
            Method::GET,
            &format!("{PATH}?after_id=100"),
            Value::Null,
        )
        .await;
        assert_eq!(last["next_after_id"], Value::Null);
        assert_eq!(last["items"].as_array().unwrap().len(), 1);
        assert_eq!(last["items"][0]["slug"], "link-101");
    }
}
