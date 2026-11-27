use axum::{Extension, Json, Router, extract::State, routing::get};
use serde::Serialize;
use sqlx::SqlitePool;

use crate::{domains, input::PublicHosts};

use super::{AppState, Problem};

macro_rules! lifecycle_sql {
    ($columns:literal, $source:literal, $output:literal) => {
        concat!(
            $columns,
            "CASE WHEN l.archived = 1 THEN 'archived' ELSE ",
            crate::store::link_state_sql!(),
            " END AS state ",
            $source,
            crate::store::link_state_joins!(),
            $output
        )
    };
}

const LINK_COUNTS: &str = lifecycle_sql!(
    "WITH classified AS (SELECT p.expires_at, ",
    "FROM links l",
    ")
     SELECT count(*) AS total,
            count(*) FILTER (WHERE state = 'active') AS active,
            count(*) FILTER (WHERE state = 'scheduled') AS scheduled,
            count(*) FILTER (WHERE state = 'expired') AS expired,
            count(*) FILTER (WHERE state = 'disabled') AS disabled,
            count(*) FILTER (WHERE state = 'archived') AS archived,
            count(*) FILTER (WHERE state = 'exhausted') AS exhausted,
            count(*) FILTER (WHERE state = 'active' AND expires_at <= ?1 + 7 * 86400) AS expiring_soon
     FROM classified"
);

const RECENT_LINKS: &str = lifecycle_sql!(
    "WITH recent AS (
        SELECT id, host, slug, title, created_at, enabled, archived FROM links ORDER BY id DESC LIMIT 5
     )
     SELECT l.id, l.host, l.slug, l.title, l.created_at, ",
    "FROM recent l",
    "ORDER BY l.id DESC"
);

/// Days before expiry when an unarchived link needs attention.
const EXPIRY_NOTICE_DAYS: i64 = 14;
/// Most links that the overview lists as needing attention.
const MAX_ATTENTION: i64 = 8;

/// Unarchived links that need attention, at most one entry per link. Unreachable destinations
/// come first, then links that expire within the notice period, then links that have used at
/// least 80 percent of their visit limit. `?1` binds the current Unix time.
const ATTENTION: &str = "WITH candidates AS (
        SELECT l.id, l.host, l.slug, l.title, p.expires_at, p.max_visits,
               COALESCE(p.visits_used, 0) AS visits_used,
               CASE
                   WHEN m.status IN ('unreachable', 'broken') THEN 0
                   WHEN p.expires_at > ?1 AND p.expires_at <= ?1 + ?2 * 86400 THEN 1
                   WHEN p.max_visits IS NOT NULL AND p.visits_used * 5 >= p.max_visits * 4 THEN 2
               END AS rank
        FROM links l
        LEFT JOIN link_redirect_policies p ON p.link_id = l.id
        LEFT JOIN link_monitor m ON m.link_id = l.id
        WHERE l.archived = 0
     )
     SELECT id, host, slug, title,
            CASE rank WHEN 0 THEN 'unreachable' WHEN 1 THEN 'expiring' ELSE 'limit' END AS reason,
            expires_at, max_visits, visits_used
     FROM candidates WHERE rank IS NOT NULL
     ORDER BY rank,
              CASE WHEN rank = 1 THEN expires_at END,
              CASE WHEN rank = 2 THEN CAST(visits_used AS REAL) / max_visits END DESC,
              id DESC
     LIMIT ?3";

#[derive(Clone)]
struct MonitoringEnabled(bool);

pub(super) fn routes(monitoring_enabled: bool) -> Router<AppState> {
    Router::new()
        .route("/api/v1/overview", get(overview))
        .layer(Extension(MonitoringEnabled(monitoring_enabled)))
}

#[derive(Serialize, sqlx::FromRow)]
struct LinkCounts {
    total: i64,
    active: i64,
    scheduled: i64,
    expired: i64,
    disabled: i64,
    archived: i64,
    exhausted: i64,
    expiring_soon: i64,
}

#[derive(Serialize, sqlx::FromRow)]
struct RecentLink {
    id: i64,
    host: String,
    slug: String,
    title: String,
    created_at: String,
    state: String,
}

#[derive(Serialize, sqlx::FromRow)]
struct AttentionLink {
    id: i64,
    host: String,
    slug: String,
    title: String,
    reason: String,
    expires_at: Option<i64>,
    max_visits: Option<i64>,
    visits_used: i64,
}

#[derive(Serialize, sqlx::FromRow)]
struct MonitorCounts {
    enabled: bool,
    total: i64,
    healthy: i64,
    attention: i64,
    pending: i64,
    last_checked: Option<i64>,
}

#[derive(Serialize)]
struct Overview {
    links: LinkCounts,
    active_domains: usize,
    recent_links: Vec<RecentLink>,
    attention: Vec<AttentionLink>,
    monitoring: MonitorCounts,
}

async fn overview(
    State(state): State<AppState>,
    Extension(MonitoringEnabled(enabled)): Extension<MonitoringEnabled>,
) -> Result<Json<Overview>, Problem> {
    read_overview(&state.pool, &state.hosts, enabled)
        .await
        .map(Json)
        .map_err(|_| Problem::unavailable())
}

async fn read_overview(
    pool: &SqlitePool,
    hosts: &PublicHosts,
    monitoring_enabled: bool,
) -> Result<Overview, sqlx::Error> {
    let mut transaction = pool.begin().await?;
    let now: i64 = sqlx::query_scalar("SELECT unixepoch()")
        .fetch_one(&mut *transaction)
        .await?;
    let hosts = domains::active_hosts(&mut *transaction, hosts).await?;
    let active_domains = hosts.len();
    let hosts_json = serde_json::json!(hosts).to_string();

    let links = sqlx::query_as(LINK_COUNTS)
        .bind(now)
        .bind(&hosts_json)
        .fetch_one(&mut *transaction)
        .await?;

    let recent_links = sqlx::query_as(RECENT_LINKS)
        .bind(now)
        .bind(&hosts_json)
        .fetch_all(&mut *transaction)
        .await?;

    let attention = sqlx::query_as(ATTENTION)
        .bind(now)
        .bind(EXPIRY_NOTICE_DAYS)
        .bind(MAX_ATTENTION)
        .fetch_all(&mut *transaction)
        .await?;

    let monitoring = sqlx::query_as(
        "SELECT ? AS enabled, count(*) AS total,
                count(*) FILTER (WHERE status IN ('healthy', 'redirected')) AS healthy,
                count(*) FILTER (WHERE status IN ('warning', 'unreachable', 'broken')) AS attention,
                count(*) FILTER (WHERE status IN ('not_checked', 'unknown')) AS pending,
                max(last_checked) AS last_checked
         FROM link_monitor",
    )
    .bind(monitoring_enabled)
    .fetch_one(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Overview {
        links,
        active_domains,
        recent_links,
        attention,
        monitoring,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn reports_link_lifecycle_and_last_monitor_results() {
        let directory = tempfile::tempdir().unwrap();
        let pool = crate::open_database(&directory.path().join("overview.db"))
            .await
            .unwrap();
        sqlx::raw_sql(
            "INSERT INTO links (id, host, slug, target, enabled, archived) VALUES
                (1, 'go.example', 'active', 'https://example.test', 1, 0),
                (2, 'go.example', 'scheduled', 'https://example.test', 1, 0),
                (3, 'go.example', 'expired', 'https://example.test', 1, 0),
                (4, 'go.example', 'disabled', 'https://example.test', 0, 0),
                (5, 'go.example', 'archived', 'https://example.test', 1, 1),
                (6, 'go.example', 'exhausted', 'https://example.test', 1, 0),
                (7, 'go.example', 'consumed', 'https://example.test', 1, 0),
                (8, 'off.example', 'hidden', 'https://example.test', 1, 0);
             INSERT INTO domains (host, origin, enabled) VALUES ('off.example', 'https://off.example', 0);
             INSERT INTO link_redirect_policies
                (link_id, status, starts_at, expires_at, max_visits, visits_used, forward_query, forward_path, rules)
             VALUES (1, 302, NULL, unixepoch() + 3600, NULL, 0, 0, 0, '[]'),
                    (2, 302, unixepoch() + 3600, NULL, NULL, 0, 0, 0, '[]'),
                    (3, 302, NULL, unixepoch() - 1, NULL, 0, 0, 0, '[]'),
                    (6, 302, NULL, NULL, 1, 1, 0, 0, '[]');
             INSERT INTO link_protection (link_id, one_time, consumed) VALUES (7, 1, 1);
             INSERT INTO link_monitor (link_id, priority, status, last_checked) VALUES
                (1, 0, 'redirected', 1000), (2, 0, 'warning', 2000), (3, 0, 'not_checked', NULL);",
        )
        .execute(&pool)
        .await
        .unwrap();
        let hosts = PublicHosts::parse("go.example,off.example").unwrap();
        let summary = read_overview(&pool, &hosts, true).await.unwrap();
        let links = summary.links;
        assert_eq!((links.total, links.active, links.expiring_soon), (8, 1, 1));
        assert_eq!((links.scheduled, links.expired, links.archived), (1, 1, 1));
        assert_eq!((links.disabled, links.exhausted), (2, 2));
        assert_eq!(summary.active_domains, 1);
        assert_eq!(
            summary
                .recent_links
                .iter()
                .map(|link| link.id)
                .collect::<Vec<_>>(),
            [8, 7, 6, 5, 4],
        );
        assert_eq!(summary.recent_links[0].state, "disabled");
        let monitor = summary.monitoring;
        assert!(monitor.enabled);
        assert_eq!(
            (
                monitor.total,
                monitor.healthy,
                monitor.attention,
                monitor.pending
            ),
            (3, 1, 1, 1)
        );
        assert_eq!(monitor.last_checked, Some(2000));
    }
}
