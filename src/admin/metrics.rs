use axum::{
    Json, Router,
    extract::{
        Query, State,
        rejection::{JsonRejection, QueryRejection},
    },
    http::StatusCode,
    routing::{delete, get},
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::analytics::{Analytics, Settings};

use super::{Problem, json_payload};

#[derive(Clone)]
struct MetricsState {
    analytics: Analytics,
    pool: SqlitePool,
}

pub(super) fn routes(pool: SqlitePool, analytics: Analytics) -> Router {
    Router::new()
        .route("/api/v1/analytics/summary", get(summary))
        .route("/api/v1/analytics/settings", get(settings).patch(save_settings))
        .route("/api/v1/analytics/data", delete(clear))
        .with_state(MetricsState { analytics, pool })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Period {
    days: Option<u16>,
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

#[derive(Serialize)]
struct Summary {
    days: u16,
    today: i64,
    totals: Totals,
    previous_redirects: i64,
    daily: Vec<Day>,
    top_links: Vec<RankedLink>,
    dropped: u64,
}

async fn summary(
    State(state): State<MetricsState>,
    period: Result<Query<Period>, QueryRejection>,
) -> Result<Json<Summary>, Problem> {
    let Query(period) = period.map_err(|error| Problem(error.status(), "invalid_query"))?;
    let days = period.days.unwrap_or(30);
    if !(1..=365).contains(&days) {
        return Err(Problem::invalid("invalid_period"));
    }
    read_summary(&state.pool, days, state.analytics.dropped())
        .await
        .map(Json)
        .map_err(|_| Problem::unavailable())
}

async fn read_summary(pool: &SqlitePool, days: u16, dropped: u64) -> Result<Summary, sqlx::Error> {
    let mut transaction = pool.begin().await?;
    let today: i64 = sqlx::query_scalar("SELECT unixepoch() / 86400")
        .fetch_one(&mut *transaction)
        .await?;
    let first = today - i64::from(days) + 1;
    let totals = sqlx::query_as(
        "SELECT COALESCE(SUM(total) FILTER (WHERE kind = 'redirect'), 0) AS redirect,
                COALESCE(SUM(total) FILTER (WHERE kind = 'missing'), 0) AS missing,
                COALESCE(SUM(total) FILTER (WHERE kind = 'blocked'), 0) AS blocked,
                COALESCE(SUM(total) FILTER (WHERE kind = 'redirect' AND visit_class = 'human'), 0) AS human,
                COALESCE(SUM(total) FILTER (WHERE kind = 'redirect' AND visit_class = 'bot'), 0) AS bot,
                COALESCE(SUM(total) FILTER (WHERE kind = 'redirect' AND visit_class = 'unknown'), 0) AS unknown
         FROM analytics_daily WHERE day BETWEEN ? AND ?",
    )
    .bind(first)
    .bind(today)
    .fetch_one(&mut *transaction)
    .await?;
    let previous_redirects = sqlx::query_scalar(
        "SELECT COALESCE(SUM(total), 0) FROM analytics_daily
         WHERE day >= ? AND day < ? AND kind = 'redirect'",
    )
    .bind(first - i64::from(days))
    .bind(first)
    .fetch_one(&mut *transaction)
    .await?;
    let daily = sqlx::query_as(
        "WITH RECURSIVE days(day) AS (
            SELECT ? UNION ALL SELECT day + 1 FROM days WHERE day < ?
         ), counts AS (
            SELECT day, SUM(total) AS total FROM analytics_daily
            WHERE day BETWEEN ? AND ? AND kind = 'redirect' GROUP BY day
         )
         SELECT days.day, COALESCE(counts.total, 0) AS redirect
         FROM days LEFT JOIN counts USING (day) ORDER BY days.day",
    )
    .bind(first)
    .bind(today)
    .bind(first)
    .bind(today)
    .fetch_all(&mut *transaction)
    .await?;
    let top_links = sqlx::query_as(
        "SELECT links.id, links.host, links.slug, links.title, counts.total
         FROM (
            SELECT link_id, SUM(total) AS total FROM analytics_daily
            WHERE day BETWEEN ? AND ? AND kind = 'redirect' AND link_id > 0
            GROUP BY link_id ORDER BY total DESC, link_id ASC LIMIT 10
         ) AS counts
         JOIN links ON links.id = counts.link_id
         ORDER BY counts.total DESC, links.id ASC",
    )
    .bind(first)
    .bind(today)
    .fetch_all(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Summary { days, today, totals, previous_redirects, daily, top_links, dropped })
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
    state.analytics.save_settings(updated).await.map(Json).map_err(|_| Problem::unavailable())
}

async fn clear(State(state): State<MetricsState>) -> Result<StatusCode, Problem> {
    state.analytics.clear().await.map_err(|_| Problem::unavailable())?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn reports_compare_adjacent_utc_periods_and_fill_empty_days() {
        let directory = tempfile::tempdir().unwrap();
        let pool = crate::open_database(&directory.path().join("report.db")).await.unwrap();
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
        .bind(id).bind(id).bind(id).execute(&pool).await.unwrap();
        let summary = read_summary(&pool, 7, 2).await.unwrap();
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
    }
}
