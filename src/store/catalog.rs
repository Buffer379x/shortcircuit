use std::collections::HashMap;

use futures_util::TryStreamExt;
use serde::{Deserialize, Serialize};
use sqlx::{QueryBuilder, Sqlite, SqliteConnection, SqlitePool};

use crate::domains::Domain;

use super::{Link, PAGE_SIZE, push_host_scope};

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum View {
    #[default]
    All,
    Unarchived,
    Favorites,
    Archived,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TagMatch {
    #[default]
    All,
    Any,
}

#[derive(Serialize, sqlx::FromRow)]
pub(crate) struct Organization {
    favorite: bool,
    archived: bool,
}

pub(crate) async fn organize(
    pool: &SqlitePool,
    hosts: &[String],
    id: i64,
    key: &str,
    favorite: Option<bool>,
    archived: Option<bool>,
) -> Result<Option<Organization>, sqlx::Error> {
    let mut query = QueryBuilder::<Sqlite>::new("UPDATE links SET favorite = COALESCE(");
    query
        .push_bind(favorite)
        .push(", favorite), archived = COALESCE(")
        .push_bind(archived)
        .push(", archived) WHERE id = ")
        .push_bind(id)
        .push(" AND selection_key = ")
        .push_bind(key);
    push_host_scope(&mut query, hosts);
    let rows = query
        .push(" RETURNING favorite, archived")
        .build_query_as::<Organization>()
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().next())
}

#[derive(Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Sort {
    #[default]
    Newest,
    Oldest,
    TitleAsc,
    TitleDesc,
    SlugAsc,
    SlugDesc,
    HostAsc,
    HostDesc,
    TargetAsc,
    TargetDesc,
    EnabledFirst,
    DisabledFirst,
}

impl Sort {
    fn expression(self) -> &'static str {
        match self {
            Self::Newest | Self::Oldest => "created_at",
            Self::TitleAsc | Self::TitleDesc => "title",
            Self::SlugAsc | Self::SlugDesc => "slug",
            Self::HostAsc | Self::HostDesc => "host",
            Self::TargetAsc | Self::TargetDesc => "target",
            Self::EnabledFirst | Self::DisabledFirst => "CAST(enabled AS TEXT)",
        }
    }

    fn descending(self) -> bool {
        matches!(
            self,
            Self::Newest
                | Self::TitleDesc
                | Self::SlugDesc
                | Self::HostDesc
                | Self::TargetDesc
                | Self::EnabledFirst
        )
    }

    fn key(self, link: &Link) -> String {
        match self {
            Self::Newest | Self::Oldest => link.created_at.clone(),
            Self::TitleAsc | Self::TitleDesc => link.title.clone(),
            Self::SlugAsc | Self::SlugDesc => link.slug.clone(),
            Self::HostAsc | Self::HostDesc => link.host.clone(),
            Self::TargetAsc | Self::TargetDesc => link.target.clone(),
            Self::EnabledFirst | Self::DisabledFirst => u8::from(link.enabled).to_string(),
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Cursor {
    pub(crate) id: i64,
    pub(crate) key: String,
    pub(crate) sort: Sort,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Search {
    pub(crate) query: Option<String>,
    pub(crate) host: Option<String>,
    pub(crate) tag: Option<String>,
    #[serde(default)]
    pub(crate) tags: Vec<String>,
    #[serde(default)]
    tag_match: TagMatch,
    #[serde(default)]
    view: View,
    pub(crate) enabled: Option<bool>,
    #[serde(default)]
    pub(crate) sort: Sort,
    pub(crate) after: Option<Cursor>,
}

#[derive(Serialize)]
pub(crate) struct SearchPage {
    items: Vec<CatalogLink>,
    next_cursor: Option<Cursor>,
}

/// One catalog entry with its organization, live state, and traffic summary.
#[derive(Serialize)]
pub(crate) struct CatalogLink {
    #[serde(flatten)]
    link: Link,
    #[serde(flatten)]
    organization: Organization,
    selection_key: String,
    #[serde(flatten)]
    details: Details,
    /// Absent when the caller may read links but not analytics.
    #[serde(flatten)]
    traffic: Option<Traffic>,
}

/// Number of UTC days, including today, that `visits_30d` covers.
const VISIT_DAYS: i64 = 30;
/// Number of UTC days, including today, in the `trend` series.
const TREND_DAYS: i64 = 14;

#[derive(Serialize)]
struct Details {
    state: String,
    starts_at: Option<i64>,
    expires_at: Option<i64>,
    max_visits: Option<i64>,
    visits_used: i64,
    protection: Option<String>,
    rule_count: i64,
    monitor: Option<MonitorSummary>,
}

#[derive(Serialize)]
struct MonitorSummary {
    status: String,
    priority: bool,
    last_checked: Option<i64>,
    http_status: Option<i64>,
    elapsed_ms: Option<i64>,
    failure: Option<String>,
}

#[derive(Serialize)]
struct Traffic {
    #[serde(flatten)]
    usage: Usage,
    /// Null when analytics is off for the link or the installation.
    visits_30d: Option<i64>,
    /// Oldest day first. Empty when analytics is off for the link or the installation.
    trend: Vec<i64>,
}

#[derive(Default, Serialize, sqlx::FromRow)]
struct Usage {
    recorded_redirects: i64,
    last_redirect_date: Option<String>,
    last_redirect_at: Option<i64>,
}

#[derive(sqlx::FromRow)]
struct UsageRow {
    link_id: i64,
    #[sqlx(flatten)]
    usage: Usage,
}

#[derive(sqlx::FromRow)]
struct SearchRow {
    #[sqlx(flatten)]
    link: Link,
    #[sqlx(flatten)]
    organization: Organization,
    selection_key: String,
    tags_json: String,
}

#[derive(sqlx::FromRow)]
struct DetailsRow {
    link_id: i64,
    state: String,
    starts_at: Option<i64>,
    expires_at: Option<i64>,
    max_visits: Option<i64>,
    visits_used: i64,
    protection: Option<String>,
    rule_count: i64,
    tracked: bool,
    monitor_status: Option<String>,
    monitor_priority: Option<bool>,
    monitor_last_checked: Option<i64>,
    monitor_http_status: Option<i64>,
    monitor_elapsed_ms: Option<i64>,
    monitor_failure: Option<String>,
}

const SEARCH_COLUMNS: &str = "SELECT id, host, slug, target, created_at, title, description,
        enabled, analytics_enabled, selection_key,
        favorite, archived,
        (SELECT json_group_array(tag ORDER BY tag) FROM link_tags WHERE link_id = links.id) AS tags_json
 FROM links WHERE 1";

/// Live state for a bounded set of links in one statement. `?3` binds a JSON array of link IDs.
const DETAILS: &str = concat!(
    "SELECT l.id AS link_id, ",
    crate::store::link_state_sql!(),
    " AS state, p.starts_at, p.expires_at, p.max_visits, COALESCE(p.visits_used, 0) AS visits_used,
       CASE WHEN protection.password_hash IS NOT NULL THEN 'password'
            WHEN protection.one_time = 1 THEN 'one_time' END AS protection,
       COALESCE(json_array_length(p.rules), 0) AS rule_count,
       l.analytics_enabled = 1
         AND COALESCE((SELECT enabled FROM analytics_settings WHERE id = 1), 0) = 1 AS tracked,
       m.status AS monitor_status, m.priority AS monitor_priority,
       m.last_checked AS monitor_last_checked, m.http_status AS monitor_http_status,
       m.elapsed_ms AS monitor_elapsed_ms, m.failure AS monitor_failure
     FROM links l",
    crate::store::link_state_joins!(),
    " LEFT JOIN link_monitor m ON m.link_id = l.id
     WHERE l.id IN (SELECT value FROM json_each(?3))"
);

pub(crate) async fn search(
    pool: &SqlitePool,
    domains: &[Domain],
    filter: &Search,
) -> Result<SearchPage, sqlx::Error> {
    let hosts: Vec<_> = domains.iter().map(|domain| domain.host.clone()).collect();
    let active: Vec<_> = domains
        .iter()
        .filter(|domain| domain.enabled)
        .map(|domain| domain.host.clone())
        .collect();
    let origins: HashMap<_, _> = domains
        .iter()
        .map(|domain| (domain.host.as_str(), domain.origin.as_str()))
        .collect();
    let mut query = QueryBuilder::<Sqlite>::new(SEARCH_COLUMNS);
    push_host_scope(&mut query, &hosts);
    if let Some(host) = &filter.host {
        query.push(" AND host = ").push_bind(host);
    }
    if let Some(enabled) = filter.enabled {
        query.push(" AND enabled = ").push_bind(enabled);
    }
    query.push(match filter.view {
        View::All => "",
        View::Unarchived => " AND archived = 0",
        View::Favorites => " AND archived = 0 AND favorite = 1",
        View::Archived => " AND archived = 1",
    });
    if !filter.tags.is_empty() {
        query.push(" AND (SELECT count(*) FROM link_tags WHERE link_id = links.id AND tag IN (");
        let mut tags = query.separated(", ");
        for tag in &filter.tags {
            tags.push_bind(tag);
        }
        tags.push_unseparated("))");
        match filter.tag_match {
            TagMatch::All => query.push(" = ").push_bind(filter.tags.len() as i64),
            TagMatch::Any => query.push(" > 0"),
        };
    }
    let sort = filter.sort;
    let comparison = if sort.descending() { " < " } else { " > " };
    let direction = if sort.descending() { " DESC" } else { " ASC" };
    if let Some(after) = &filter.after {
        query
            .push(" AND (")
            .push(sort.expression())
            .push(" COLLATE NOCASE, id)")
            .push(comparison)
            .push("(")
            .push_bind(&after.key)
            .push(", ")
            .push_bind(after.id)
            .push(")");
    }
    query
        .push(" ORDER BY ")
        .push(sort.expression())
        .push(" COLLATE NOCASE")
        .push(direction)
        .push(", id")
        .push(direction);
    if filter.query.is_none() {
        query.push(" LIMIT ").push_bind((PAGE_SIZE + 1) as i64);
    }
    // One read transaction keeps the page, its state, and its traffic consistent.
    let mut transaction = pool.begin().await?;
    let mut rows = query.build_query_as::<SearchRow>().fetch(&mut *transaction);
    let mut page = Vec::with_capacity(PAGE_SIZE + 1);
    while let Some(row) = rows.try_next().await? {
        let row = decode_tags(row)?;
        let origin = origins.get(row.link.host.as_str()).copied();
        if filter
            .query
            .as_deref()
            .is_none_or(|text| matches_query(&row.link, origin, text))
        {
            page.push(row);
            if page.len() > PAGE_SIZE {
                break;
            }
        }
    }
    drop(rows);
    let has_more = page.len() > PAGE_SIZE;
    page.truncate(PAGE_SIZE);
    let items = complete(&mut transaction, page, &active, true).await?;
    transaction.commit().await?;
    let next_cursor = items.last().filter(|_| has_more).map(|link| Cursor {
        id: link.link.id,
        key: sort.key(&link.link),
        sort,
    });
    Ok(SearchPage { items, next_cursor })
}

/// Reads one catalog entry within the caller's hosts. `active` lists the active public hosts
/// for the lifecycle state. Without `traffic`, analytics-derived fields are left out.
pub(crate) async fn find(
    pool: &SqlitePool,
    hosts: &[String],
    active: &[String],
    id: i64,
    traffic: bool,
) -> Result<Option<CatalogLink>, sqlx::Error> {
    let mut query = QueryBuilder::<Sqlite>::new(SEARCH_COLUMNS);
    query.push(" AND id = ").push_bind(id);
    push_host_scope(&mut query, hosts);
    let mut transaction = pool.begin().await?;
    let row = query
        .build_query_as::<SearchRow>()
        .fetch_optional(&mut *transaction)
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let items = complete(&mut transaction, vec![decode_tags(row)?], active, traffic).await?;
    transaction.commit().await?;
    Ok(items.into_iter().next())
}

fn decode_tags(mut row: SearchRow) -> Result<SearchRow, sqlx::Error> {
    row.link.tags = serde_json::from_str(&row.tags_json)
        .map_err(|error| sqlx::Error::Decode(Box::new(error)))?;
    Ok(row)
}

/// Adds live state and, if requested, traffic to a page of at most `PAGE_SIZE` rows. Each
/// part is one bounded query for the whole page.
async fn complete(
    connection: &mut SqliteConnection,
    rows: Vec<SearchRow>,
    active: &[String],
    traffic: bool,
) -> Result<Vec<CatalogLink>, sqlx::Error> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<i64> = rows.iter().map(|row| row.link.id).collect();
    let ids_json = serde_json::json!(ids).to_string();
    let now: i64 = sqlx::query_scalar("SELECT unixepoch()")
        .fetch_one(&mut *connection)
        .await?;
    let mut details: HashMap<i64, DetailsRow> = sqlx::query_as::<_, DetailsRow>(DETAILS)
        .bind(now)
        .bind(serde_json::json!(active).to_string())
        .bind(&ids_json)
        .fetch_all(&mut *connection)
        .await?
        .into_iter()
        .map(|row| (row.link_id, row))
        .collect();
    let (mut usage, mut series) = if traffic {
        let tracked: Vec<i64> = details
            .values()
            .filter(|row| row.tracked)
            .map(|row| row.link_id)
            .collect();
        (
            load_usage(&mut *connection, &ids_json).await?,
            load_series(&mut *connection, &tracked, now.div_euclid(86_400)).await?,
        )
    } else {
        (HashMap::new(), HashMap::new())
    };
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        // The read transaction keeps every page row; skip defensively if one is missing.
        let Some(state) = details.remove(&row.link.id) else {
            continue;
        };
        let traffic = traffic.then(|| {
            let (visits_30d, trend) = if state.tracked {
                let (total, trend) = series
                    .remove(&row.link.id)
                    .unwrap_or((0, vec![0; TREND_DAYS as usize]));
                (Some(total), trend)
            } else {
                (None, Vec::new())
            };
            Traffic {
                usage: usage.remove(&row.link.id).unwrap_or_default(),
                visits_30d,
                trend,
            }
        });
        items.push(CatalogLink {
            link: row.link,
            organization: row.organization,
            selection_key: row.selection_key,
            details: Details::from(state),
            traffic,
        });
    }
    Ok(items)
}

impl From<DetailsRow> for Details {
    fn from(row: DetailsRow) -> Self {
        let monitor = row.monitor_status.map(|status| MonitorSummary {
            status,
            priority: row.monitor_priority.unwrap_or(false),
            last_checked: row.monitor_last_checked,
            http_status: row.monitor_http_status,
            elapsed_ms: row.monitor_elapsed_ms,
            failure: row.monitor_failure,
        });
        Self {
            state: row.state,
            starts_at: row.starts_at,
            expires_at: row.expires_at,
            max_visits: row.max_visits,
            visits_used: row.visits_used,
            protection: row.protection,
            rule_count: row.rule_count,
            monitor,
        }
    }
}

async fn load_usage(
    connection: &mut SqliteConnection,
    ids_json: &str,
) -> Result<HashMap<i64, Usage>, sqlx::Error> {
    let rows = sqlx::query_as::<_, UsageRow>(
        "WITH recent AS (
            SELECT link_id, sum(total) AS recorded_redirects, max(day) AS last_day
            FROM analytics_daily
            WHERE kind = 'redirect' AND link_id IN (SELECT value FROM json_each(?1))
            GROUP BY link_id)
         SELECT link_id, recorded_redirects, date(last_day * 86400, 'unixepoch') AS last_redirect_date,
             (SELECT max(occurred_at) FROM analytics_events
              WHERE analytics_events.link_id = recent.link_id AND kind = 'redirect'
                AND occurred_at >= last_day * 86400 AND occurred_at < (last_day + 1) * 86400)
             AS last_redirect_at
         FROM recent",
    )
    .bind(ids_json)
    .fetch_all(connection)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| (row.link_id, row.usage))
        .collect())
}

/// Redirects of every visit class per link over the last `VISIT_DAYS` UTC days, including
/// today, and the daily series for the last `TREND_DAYS` of them, oldest first.
async fn load_series(
    connection: &mut SqliteConnection,
    ids: &[i64],
    today: i64,
) -> Result<HashMap<i64, (i64, Vec<i64>)>, sqlx::Error> {
    let mut series = HashMap::new();
    if ids.is_empty() {
        return Ok(series);
    }
    let rows = sqlx::query_as::<_, (i64, i64, i64)>(
        "SELECT link_id, day, sum(total) FROM analytics_daily
         WHERE link_id IN (SELECT value FROM json_each(?1)) AND kind = 'redirect'
           AND day BETWEEN ?2 AND ?3
         GROUP BY link_id, day",
    )
    .bind(serde_json::json!(ids).to_string())
    .bind(today - VISIT_DAYS + 1)
    .bind(today)
    .fetch_all(connection)
    .await?;
    let first_trend_day = today - TREND_DAYS + 1;
    for (link_id, day, total) in rows {
        let (sum, trend) = series
            .entry(link_id)
            .or_insert_with(|| (0, vec![0; TREND_DAYS as usize]));
        *sum += total;
        if day >= first_trend_day {
            trend[(day - first_trend_day) as usize] += total;
        }
    }
    Ok(series)
}

fn matches_query(link: &Link, origin: Option<&str>, text: &str) -> bool {
    let contains = |value: &str| value.contains(text) || value.to_lowercase().contains(text);
    [
        link.title.as_str(),
        &link.description,
        &link.slug,
        &link.host,
        &link.target,
    ]
    .into_iter()
    .chain(link.tags.iter().map(String::as_str))
    .any(contains)
        || contains(&format!("{}/{}", link.host, link.slug))
        || origin.is_some_and(|base| contains(&format!("{base}/{}", link.slug)))
}
