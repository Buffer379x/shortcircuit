use std::collections::BTreeMap;

use axum::{
    Json, Router,
    extract::{
        Path, Query, State,
        rejection::{JsonRejection, PathRejection, QueryRejection},
    },
    http::{StatusCode, header},
    response::IntoResponse,
    routing::{delete, get},
};
use serde::{Deserialize, Serialize};
use sqlx::{SqliteConnection, SqlitePool};

use crate::{
    analytics::{Analytics, Settings},
    input::normalize_host,
};

use super::{Problem, json_payload, resource_id};

#[derive(Clone)]
struct MetricsState {
    analytics: Analytics,
    pool: SqlitePool,
}

pub(super) fn routes(pool: SqlitePool, analytics: Analytics) -> Router {
    Router::new()
        .route("/api/v1/analytics/summary", get(summary))
        .route("/api/v1/analytics/export", get(export))
        .route(
            "/api/v1/analytics/settings",
            get(settings).patch(save_settings),
        )
        .route("/api/v1/analytics/data", delete(clear))
        .route("/api/v1/analytics/events", get(list_events))
        .route("/api/v1/analytics/events/{id}", delete(delete_event))
        .with_state(MetricsState { analytics, pool })
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Period {
    days: Option<u16>,
    start: Option<String>,
    end: Option<String>,
    host: Option<String>,
}

#[derive(Serialize, sqlx::FromRow, Default)]
struct Totals {
    redirect: i64,
    blocked: i64,
    missing: i64,
    human: i64,
    bot: i64,
    unknown: i64,
}

#[derive(Serialize, sqlx::FromRow)]
struct Day {
    day: i64,
    redirect: i64,
}

#[derive(Serialize, sqlx::FromRow)]
struct RankedLink {
    id: i64,
    host: String,
    slug: String,
    title: String,
    total: i64,
}

#[derive(Serialize, sqlx::FromRow)]
struct RankedDomain {
    host: String,
    total: i64,
}

#[derive(Serialize, sqlx::FromRow)]
struct RankedTag {
    tag: String,
    total: i64,
}

#[derive(Serialize)]
struct RankedCategory {
    category: String,
    total: i64,
}

#[derive(Serialize)]
struct Breakdowns {
    browser: Vec<RankedCategory>,
    os: Vec<RankedCategory>,
    device: Vec<RankedCategory>,
    traffic_source: Vec<RankedCategory>,
}

#[derive(Serialize)]
struct Summary {
    days: u16,
    today: i64,
    start_day: i64,
    end_day: i64,
    host: Option<String>,
    totals: Totals,
    previous_redirects: i64,
    daily: Vec<Day>,
    top_links: Vec<RankedLink>,
    top_domains: Vec<RankedDomain>,
    top_tags: Vec<RankedTag>,
    breakdowns: Breakdowns,
    dropped: u64,
}

#[derive(Serialize)]
struct SummaryExport {
    version: u8,
    granularity: &'static str,
    report: Summary,
}

async fn summary(
    State(state): State<MetricsState>,
    period: Result<Query<Period>, QueryRejection>,
) -> Result<Json<Summary>, Problem> {
    let Query(period) = period.map_err(|error| Problem(error.status(), "invalid_query"))?;
    read_summary(&state.pool, period, state.analytics.dropped())
        .await
        .map(Json)
}

async fn export(
    State(state): State<MetricsState>,
    period: Result<Query<Period>, QueryRejection>,
) -> Result<impl IntoResponse, Problem> {
    let Query(period) = period.map_err(|error| Problem(error.status(), "invalid_query"))?;
    let report = read_summary(&state.pool, period, state.analytics.dropped()).await?;
    Ok((
        [(
            header::CONTENT_DISPOSITION,
            "attachment; filename=\"shortcircuit-analytics-v1.json\"",
        )],
        Json(SummaryExport {
            version: 1,
            granularity: "utc_day",
            report,
        }),
    ))
}

async fn read_summary(pool: &SqlitePool, period: Period, dropped: u64) -> Result<Summary, Problem> {
    let host = period
        .host
        .as_deref()
        .map(|host| normalize_host(host).ok_or_else(|| Problem::invalid("invalid_host")))
        .transpose()?;

    let mut transaction = pool.begin().await.map_err(|_| Problem::unavailable())?;
    // The modifier normalizes invalid calendar dates, so only canonical UTC dates compare equal.
    let (today, start, end): (i64, Option<i64>, Option<i64>) = sqlx::query_as(
        "SELECT unixepoch() / 86400,
                CASE WHEN date(?1, '+0 days') = ?1 THEN unixepoch(?1) / 86400 END,
                CASE WHEN date(?2, '+0 days') = ?2 THEN unixepoch(?2) / 86400 END",
    )
    .bind(period.start.as_deref())
    .bind(period.end.as_deref())
    .fetch_one(&mut *transaction)
    .await
    .map_err(|_| Problem::unavailable())?;

    let invalid_period = || Problem::invalid("invalid_period");
    let (first, last) = match (period.days, period.start, period.end) {
        (days, None, None) => (today - i64::from(days.unwrap_or(30)) + 1, today),
        (None, Some(_), Some(_)) => (
            start.ok_or_else(invalid_period)?,
            end.ok_or_else(invalid_period)?,
        ),
        _ => return Err(invalid_period()),
    };
    let length = last - first + 1;
    if first < 0 || last > today || !(1..=365).contains(&length) {
        return Err(invalid_period());
    }

    let report = query_summary(&mut transaction, first, last, today, host, dropped)
        .await
        .map_err(|_| Problem::unavailable())?;
    transaction
        .commit()
        .await
        .map_err(|_| Problem::unavailable())?;
    Ok(report)
}

async fn query_summary(
    connection: &mut SqliteConnection,
    first: i64,
    last: i64,
    today: i64,
    host: Option<String>,
    dropped: u64,
) -> Result<Summary, sqlx::Error> {
    let days = (last - first + 1) as u16;
    let totals = sqlx::query_as(
        "SELECT COALESCE(SUM(total) FILTER (WHERE kind = 'redirect'), 0) AS redirect,
                COALESCE(SUM(total) FILTER (WHERE kind = 'missing'), 0) AS missing,
                COALESCE(SUM(total) FILTER (WHERE kind = 'blocked'), 0) AS blocked,
                COALESCE(SUM(total) FILTER (WHERE kind = 'redirect' AND visit_class = 'human'), 0) AS human,
                COALESCE(SUM(total) FILTER (WHERE kind = 'redirect' AND visit_class = 'bot'), 0) AS bot,
                COALESCE(SUM(total) FILTER (WHERE kind = 'redirect' AND visit_class = 'unknown'), 0) AS unknown
         FROM analytics_daily WHERE day BETWEEN ?1 AND ?2 AND (?3 IS NULL OR host = ?3)",
    )
    .bind(first)
    .bind(last)
    .bind(host.as_deref())
    .fetch_one(&mut *connection)
    .await?;
    let previous_redirects = sqlx::query_scalar(
        "SELECT COALESCE(SUM(total), 0) FROM analytics_daily
         WHERE day >= ?1 AND day < ?2 AND kind = 'redirect' AND (?3 IS NULL OR host = ?3)",
    )
    .bind(first - i64::from(days))
    .bind(first)
    .bind(host.as_deref())
    .fetch_one(&mut *connection)
    .await?;
    let daily = sqlx::query_as(
        "WITH RECURSIVE days(day) AS (
            SELECT ?1 UNION ALL SELECT day + 1 FROM days WHERE day < ?2
         ), counts AS (
            SELECT day, SUM(total) AS total FROM analytics_daily
            WHERE day BETWEEN ?1 AND ?2 AND kind = 'redirect' AND (?3 IS NULL OR host = ?3)
            GROUP BY day
         )
         SELECT days.day, COALESCE(counts.total, 0) AS redirect
         FROM days LEFT JOIN counts USING (day) ORDER BY days.day",
    )
    .bind(first)
    .bind(last)
    .bind(host.as_deref())
    .fetch_all(&mut *connection)
    .await?;
    let top_links = sqlx::query_as(
        "SELECT links.id, links.host, links.slug, links.title, counts.total
         FROM (
            SELECT link_id, SUM(total) AS total FROM analytics_daily
            WHERE day BETWEEN ?1 AND ?2 AND kind = 'redirect' AND link_id > 0
              AND (?3 IS NULL OR host = ?3)
            GROUP BY link_id ORDER BY total DESC, link_id ASC LIMIT 10
         ) AS counts
         JOIN links ON links.id = counts.link_id
         ORDER BY counts.total DESC, links.id ASC",
    )
    .bind(first)
    .bind(last)
    .bind(host.as_deref())
    .fetch_all(&mut *connection)
    .await?;
    let top_domains = sqlx::query_as(
        "SELECT host, SUM(total) AS total FROM analytics_daily
         WHERE day BETWEEN ?1 AND ?2 AND kind = 'redirect' AND (?3 IS NULL OR host = ?3)
         GROUP BY host ORDER BY total DESC, host ASC LIMIT 10",
    )
    .bind(first)
    .bind(last)
    .bind(host.as_deref())
    .fetch_all(&mut *connection)
    .await?;
    let top_tags = sqlx::query_as(
        "WITH counts AS (
            SELECT link_id, SUM(total) AS total FROM analytics_daily
            WHERE day BETWEEN ?1 AND ?2 AND kind = 'redirect' AND link_id > 0
              AND (?3 IS NULL OR host = ?3)
            GROUP BY link_id
         )
         SELECT link_tags.tag, SUM(counts.total) AS total
         FROM counts JOIN link_tags USING (link_id)
         GROUP BY link_tags.tag ORDER BY total DESC, link_tags.tag ASC LIMIT 10",
    )
    .bind(first)
    .bind(last)
    .bind(host.as_deref())
    .fetch_all(&mut *connection)
    .await?;
    let dimension_rows = sqlx::query_as::<_, (String, String, String, String, i64)>(
        "SELECT browser, os, device, traffic_source, SUM(total) AS total FROM analytics_daily
         WHERE day BETWEEN ?1 AND ?2 AND kind = 'redirect' AND (?3 IS NULL OR host = ?3)
         GROUP BY browser, os, device, traffic_source",
    )
    .bind(first)
    .bind(last)
    .bind(host.as_deref())
    .fetch_all(&mut *connection)
    .await?;

    let mut groups: [BTreeMap<String, i64>; 4] = std::array::from_fn(|_| BTreeMap::new());
    for (browser, os, device, traffic_source, total) in dimension_rows {
        for (group, category) in groups.iter_mut().zip([browser, os, device, traffic_source]) {
            *group.entry(category).or_insert(0) += total;
        }
    }
    let [browser, os, device, traffic_source] = groups.map(|group| {
        let mut ranking: Vec<_> = group
            .into_iter()
            .map(|(category, total)| RankedCategory { category, total })
            .collect();
        ranking.sort_by(|a, b| b.total.cmp(&a.total).then_with(|| a.category.cmp(&b.category)));
        ranking
    });

    Ok(Summary {
        days,
        today,
        start_day: first,
        end_day: last,
        host,
        totals,
        previous_redirects,
        daily,
        top_links,
        top_domains,
        top_tags,
        breakdowns: Breakdowns {
            browser,
            os,
            device,
            traffic_source,
        },
        dropped,
    })
}

async fn settings(State(state): State<MetricsState>) -> Json<Settings> {
    Json(state.analytics.settings())
}

async fn save_settings(
    State(state): State<MetricsState>,
    body: Result<Json<Settings>, JsonRejection>,
) -> Result<Json<Settings>, Problem> {
    let updated = json_payload(body)?;
    if !(1..=3650).contains(&updated.detail_days)
        || !(updated.detail_days..=3650).contains(&updated.aggregate_days)
    {
        return Err(Problem::invalid("invalid_retention"));
    }
    state
        .analytics
        .save_settings(updated)
        .await
        .map(Json)
        .map_err(|_| Problem::unavailable())
}

async fn clear(State(state): State<MetricsState>) -> Result<StatusCode, Problem> {
    state
        .analytics
        .clear()
        .await
        .map_err(|_| Problem::unavailable())?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EventCursor {
    before_id: Option<i64>,
}

#[derive(Serialize, sqlx::FromRow)]
struct VisitEvent {
    id: i64,
    occurred_at: i64,
    link_id: Option<i64>,
    host: String,
    kind: String,
    visit_class: String,
    source: String,
    referrer: Option<String>,
    browser: String,
    os: String,
    device: String,
    traffic_source: String,
}

#[derive(Serialize)]
struct EventPage {
    items: Vec<VisitEvent>,
    next_before_id: Option<i64>,
}

async fn list_events(
    State(state): State<MetricsState>,
    query: Result<Query<EventCursor>, QueryRejection>,
) -> Result<Json<EventPage>, Problem> {
    let Query(query) = query.map_err(|error| Problem(error.status(), "invalid_query"))?;
    let before = query.before_id.unwrap_or(i64::MAX);
    if before <= 0 {
        return Err(Problem::invalid("invalid_event_cursor"));
    }
    let mut items = sqlx::query_as::<_, VisitEvent>(
        "SELECT id, occurred_at, link_id, host, kind, visit_class, source, referrer,
                browser, os, device, traffic_source
         FROM analytics_events WHERE id < ? ORDER BY id DESC LIMIT 101",
    )
    .bind(before)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| Problem::unavailable())?;
    let has_more = items.len() > 100;
    items.truncate(100);
    let next_before_id = if has_more {
        items.last().map(|event| event.id)
    } else {
        None
    };
    Ok(Json(EventPage {
        items,
        next_before_id,
    }))
}

#[derive(sqlx::FromRow)]
struct Bucket {
    day: i64,
    link_id: i64,
    host: String,
    kind: String,
    visit_class: String,
    browser: String,
    os: String,
    device: String,
    traffic_source: String,
}

async fn delete_event_row(pool: &SqlitePool, id: i64) -> Result<bool, sqlx::Error> {
    let mut transaction = pool.begin_with("BEGIN IMMEDIATE").await?;
    let rows = sqlx::query_as::<_, Bucket>(
        "DELETE FROM analytics_events WHERE id = ?
         RETURNING occurred_at / 86400 AS day, COALESCE(link_id, 0) AS link_id,
                   host, kind, visit_class, browser, os, device, traffic_source",
    )
    .bind(id)
    .fetch_all(&mut *transaction)
    .await?;
    let Some(bucket) = rows.into_iter().next() else {
        return Ok(false);
    };
    let deleted = sqlx::query(
        "DELETE FROM analytics_daily
         WHERE day = ? AND link_id = ? AND host = ? AND kind = ? AND visit_class = ?
           AND browser = ? AND os = ? AND device = ? AND traffic_source = ? AND total = 1",
    )
    .bind(bucket.day)
    .bind(bucket.link_id)
    .bind(&bucket.host)
    .bind(&bucket.kind)
    .bind(&bucket.visit_class)
    .bind(&bucket.browser)
    .bind(&bucket.os)
    .bind(&bucket.device)
    .bind(&bucket.traffic_source)
    .execute(&mut *transaction)
    .await?
    .rows_affected();
    if deleted == 0 {
        sqlx::query(
            "UPDATE analytics_daily SET total = total - 1
             WHERE day = ? AND link_id = ? AND host = ? AND kind = ? AND visit_class = ?
               AND browser = ? AND os = ? AND device = ? AND traffic_source = ? AND total > 1",
        )
        .bind(bucket.day)
        .bind(bucket.link_id)
        .bind(&bucket.host)
        .bind(&bucket.kind)
        .bind(&bucket.visit_class)
        .bind(&bucket.browser)
        .bind(&bucket.os)
        .bind(&bucket.device)
        .bind(&bucket.traffic_source)
        .execute(&mut *transaction)
        .await?;
    }
    transaction.commit().await?;
    Ok(true)
}

async fn delete_event(
    State(state): State<MetricsState>,
    id: Result<Path<i64>, PathRejection>,
) -> Result<StatusCode, Problem> {
    if delete_event_row(&state.pool, resource_id(id)?)
        .await
        .map_err(|_| Problem::unavailable())?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(Problem(StatusCode::NOT_FOUND, "not_found"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn reports_compare_adjacent_utc_periods_and_fill_empty_days() {
        let directory = tempfile::tempdir().unwrap();
        let pool = crate::open_database(&directory.path().join("report.db"))
            .await
            .unwrap();
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO links (host, slug, target) VALUES ('go.example', 'notes', 'https://example.test') RETURNING id",
        )
        .fetch_one(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO analytics_daily (day, link_id, host, kind, visit_class, total)
             VALUES (unixepoch() / 86400, ?, 'go.example', 'redirect', 'human', 5),
                    (unixepoch() / 86400, ?, 'go.example', 'redirect', 'bot', 1),
                    (unixepoch() / 86400 - 7, ?, 'go.example', 'redirect', 'unknown', 3),
                    (unixepoch() / 86400 - 2, 0, 'go.example', 'missing', 'unknown', 4)",
        )
        .bind(id)
        .bind(id)
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
        let week = || Period {
            days: Some(7),
            ..Period::default()
        };
        let summary = read_summary(&pool, week(), 2).await.ok().unwrap();
        assert_eq!(summary.totals.redirect, 6);
        assert_eq!(summary.totals.human, 5);
        assert_eq!(summary.totals.bot, 1);
        assert_eq!(summary.totals.unknown, 0);
        assert_eq!(summary.totals.missing, 4);
        assert_eq!(summary.previous_redirects, 3);
        assert_eq!(summary.daily.len(), 7);
        assert_eq!(summary.daily[0].redirect, 0);
        assert_eq!(summary.daily[6].redirect, 6);
        assert_eq!(summary.top_links[0].total, 6);
        assert_eq!(summary.dropped, 2);

        let event: i64 = sqlx::query_scalar(
            "INSERT INTO analytics_events (occurred_at, link_id, host, kind, visit_class, source)
             VALUES (unixepoch(), ?, 'go.example', 'redirect', 'human', 'chrome') RETURNING id",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(delete_event_row(&pool, event).await.unwrap());
        assert!(!delete_event_row(&pool, event).await.unwrap());
        let updated = read_summary(&pool, week(), 2).await.ok().unwrap();
        assert_eq!(updated.totals.redirect, 5);
        assert_eq!(updated.totals.human, 4);
    }
}
