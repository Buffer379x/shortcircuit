use std::{
    io,
    sync::{
        Arc, PoisonError, RwLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::http::{HeaderMap, header};
use serde::{Deserialize, Serialize};
use sqlx::{Executor, QueryBuilder, Sqlite, SqlitePool};
use tokio::{
    sync::{Mutex, mpsc},
    task::JoinHandle,
    time::{self, Instant, MissedTickBehavior},
};
use url::{Host, Url};

use crate::input::normalize_host;

const QUEUE_CAPACITY: usize = 1024;
const BATCH_SIZE: usize = 64;
const BATCH_WINDOW: Duration = Duration::from_millis(25);
const RETENTION_INTERVAL: Duration = Duration::from_secs(86_400);
const MAX_HEADER_BYTES: usize = 4096;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, sqlx::FromRow)]
#[serde(deny_unknown_fields)]
pub(crate) struct Settings {
    pub(crate) enabled: bool,
    pub(crate) referrers: bool,
    pub(crate) classify: bool,
    pub(crate) honor_dnt: bool,
    pub(crate) detail_days: i64,
    pub(crate) aggregate_days: i64,
}

#[derive(Clone, Copy, sqlx::FromRow)]
struct Snapshot {
    #[sqlx(flatten)]
    settings: Settings,
    generation: i64,
}

struct Shared {
    snapshot: RwLock<Snapshot>,
    changes: Mutex<()>,
    dropped: AtomicU64,
}

impl Shared {
    fn snapshot(&self) -> Snapshot {
        *self.snapshot.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn update(&self, next: Snapshot) {
        let mut current = self.snapshot.write().unwrap_or_else(PoisonError::into_inner);
        if next.generation >= current.generation {
            *current = next;
        }
    }
}

#[derive(Clone)]
pub struct Analytics {
    pool: SqlitePool,
    queue: mpsc::Sender<Event>,
    shared: Arc<Shared>,
}

#[derive(Clone, Copy)]
pub(crate) enum Kind {
    Redirect,
    Blocked,
    Missing,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Redirect => "redirect",
            Self::Blocked => "blocked",
            Self::Missing => "missing",
        }
    }
}

struct Event {
    generation: i64,
    occurred_at: i64,
    host: String,
    link_id: Option<i64>,
    kind: Kind,
    visit_class: &'static str,
    source: &'static str,
    referrer: Option<String>,
}

enum Change {
    Settings(Settings),
    Clear,
}

impl Analytics {
    pub async fn start(pool: SqlitePool) -> Result<(Self, JoinHandle<()>), sqlx::Error> {
        let snapshot = load_snapshot(&pool).await?;
        let shared = Arc::new(Shared {
            snapshot: RwLock::new(snapshot),
            changes: Mutex::new(()),
            dropped: AtomicU64::new(0),
        });
        let (queue, receiver) = mpsc::channel(QUEUE_CAPACITY);
        let worker = tokio::spawn(run_writer(pool.clone(), Arc::clone(&shared), receiver));
        Ok((
            Self {
                pool,
                queue,
                shared,
            },
            worker,
        ))
    }

    pub(crate) fn settings(&self) -> Settings {
        self.shared.snapshot().settings
    }

    pub(crate) async fn save_settings(&self, settings: Settings) -> Result<Settings, sqlx::Error> {
        if !(1..=3650).contains(&settings.detail_days)
            || !(settings.detail_days..=3650).contains(&settings.aggregate_days)
        {
            return Err(sqlx::Error::InvalidArgument(
                "invalid analytics retention periods".to_owned(),
            ));
        }
        self.change(Change::Settings(settings)).await
    }

    pub(crate) async fn clear(&self) -> Result<(), sqlx::Error> {
        self.change(Change::Clear).await.map(|_| ())
    }

    async fn change(&self, change: Change) -> Result<Settings, sqlx::Error> {
        let pool = self.pool.clone();
        let shared = Arc::clone(&self.shared);
        // Run publication to completion even when the admin request is canceled.
        tokio::spawn(async move {
            let _change = shared.changes.lock().await;
            let mut transaction = pool.begin_with("BEGIN IMMEDIATE").await?;
            match change {
                Change::Settings(settings) => {
                    sqlx::query(
                        "UPDATE analytics_settings SET enabled = ?, referrers = ?, classify = ?,
                            honor_dnt = ?, detail_days = ?, aggregate_days = ?,
                            generation = generation + 1 WHERE id = 1",
                    )
                    .bind(settings.enabled)
                    .bind(settings.referrers)
                    .bind(settings.classify)
                    .bind(settings.honor_dnt)
                    .bind(settings.detail_days)
                    .bind(settings.aggregate_days)
                    .execute(&mut *transaction)
                    .await?;
                }
                Change::Clear => {
                    sqlx::raw_sql(
                        "UPDATE analytics_settings SET generation = generation + 1 WHERE id = 1;
                         DELETE FROM analytics_events;
                         DELETE FROM analytics_daily;",
                    )
                    .execute(&mut *transaction)
                    .await?;
                }
            }
            let snapshot = load_snapshot(&mut *transaction).await?;
            transaction.commit().await?;
            shared.update(snapshot);
            Ok(snapshot.settings)
        })
        .await
        .map_err(|error| sqlx::Error::Io(io::Error::other(error)))?
    }

    pub(crate) fn record(&self, host: &str, link_id: Option<i64>, kind: Kind, headers: &HeaderMap) {
        if let Some(event) = capture(self.shared.snapshot(), host, link_id, kind, headers)
            && self.queue.try_send(event).is_err()
        {
            self.shared.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(crate) fn dropped(&self) -> u64 {
        self.shared.dropped.load(Ordering::Relaxed)
    }
}

async fn load_snapshot<'e>(
    executor: impl Executor<'e, Database = Sqlite>,
) -> Result<Snapshot, sqlx::Error> {
    sqlx::query_as(
        "SELECT enabled, referrers, classify, honor_dnt, detail_days, aggregate_days, generation
         FROM analytics_settings WHERE id = 1",
    )
    .fetch_one(executor)
    .await
}

fn capture(
    snapshot: Snapshot,
    host: &str,
    link_id: Option<i64>,
    kind: Kind,
    headers: &HeaderMap,
) -> Option<Event> {
    let settings = snapshot.settings;
    if !settings.enabled
        || (settings.honor_dnt
            && headers
                .get_all("dnt")
                .iter()
                .any(|value| value.to_str().is_ok_and(|value| value.trim() == "1")))
    {
        return None;
    }
    let (visit_class, source) = if settings.classify {
        classify(headers)
    } else {
        ("unknown", "unknown")
    };
    let occurred_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    Some(Event {
        generation: snapshot.generation,
        occurred_at: i64::try_from(occurred_at).unwrap_or(i64::MAX),
        host: host.to_owned(),
        link_id,
        kind,
        visit_class,
        source,
        referrer: if settings.referrers {
            referrer_host(headers)
        } else {
            None
        },
    })
}

fn referrer_host(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(header::REFERER)?.to_str().ok()?;
    if value.len() > MAX_HEADER_BYTES {
        return None;
    }
    let url = Url::parse(value).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    match url.host()? {
        Host::Domain(host) => normalize_host(host),
        Host::Ipv4(_) | Host::Ipv6(_) => None,
    }
}

fn classify(headers: &HeaderMap) -> (&'static str, &'static str) {
    let Some(agent) = headers
        .get(header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .filter(|value| value.len() <= MAX_HEADER_BYTES)
    else {
        return ("unknown", "unknown");
    };
    let agent = agent.to_ascii_lowercase();
    let contains = |markers: &[&str]| markers.iter().any(|marker| agent.contains(*marker));
    if contains(&[
        "facebookexternalhit",
        "slackbot",
        "discordbot",
        "twitterbot",
        "telegrambot",
        "linkedinbot",
        "whatsapp/",
    ]) {
        ("bot", "preview")
    } else if contains(&[
        "bot",
        "crawl",
        "spider",
        "headless",
        "monitor",
        "curl/",
        "wget/",
        "python-requests/",
    ]) {
        ("bot", "bot")
    } else if contains(&["edge/", "edg/", "edga/", "edgios/"]) {
        ("human", "edge")
    } else if contains(&["opera", "opr/", "opios/"]) {
        ("human", "opera")
    } else if contains(&["firefox/", "fxios/"]) {
        ("human", "firefox")
    } else if contains(&["chrome/", "chromium/", "crios/"]) {
        ("human", "chrome")
    } else if contains(&["safari/"]) {
        ("human", "safari")
    } else {
        ("unknown", "unknown")
    }
}

async fn run_writer(pool: SqlitePool, shared: Arc<Shared>, mut receiver: mpsc::Receiver<Event>) {
    let mut retention = time::interval(RETENTION_INTERVAL);
    retention.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut batch = Vec::with_capacity(BATCH_SIZE);
    loop {
        let received = tokio::select! {
            received = receiver.recv_many(&mut batch, BATCH_SIZE) => received,
            _ = retention.tick() => {
                if prune(&pool).await.is_err() {
                    tracing::warn!("analytics retention failed");
                }
                continue;
            }
        };
        if received == 0 {
            break;
        }

        let deadline = Instant::now() + BATCH_WINDOW;
        while batch.len() < BATCH_SIZE {
            let remaining = BATCH_SIZE - batch.len();
            match time::timeout_at(deadline, receiver.recv_many(&mut batch, remaining)).await {
                Ok(1..) => {}
                _ => break,
            }
        }

        if write_batch(&pool, &shared, &batch).await.is_err() {
            shared
                .dropped
                .fetch_add(batch.len() as u64, Ordering::Relaxed);
        }
        batch.clear();
    }
}

async fn write_batch(
    pool: &SqlitePool,
    shared: &Shared,
    batch: &[Event],
) -> Result<(), sqlx::Error> {
    let mut transaction = pool.begin_with("BEGIN IMMEDIATE").await?;
    let snapshot = load_snapshot(&mut *transaction).await?;
    shared.update(snapshot);

    let mut current = batch
        .iter()
        .filter(|event| snapshot.settings.enabled && event.generation == snapshot.generation)
        .peekable();
    if current.peek().is_none() {
        return transaction.commit().await;
    }

    let mut details = QueryBuilder::<Sqlite>::new(
        "WITH pending (occurred_at, link_id, host, kind, visit_class, source, referrer) AS (",
    );
    details.push_values(current, |mut values, event| {
        values
            .push_bind(event.occurred_at)
            .push_bind(event.link_id)
            .push_bind(event.host.as_str())
            .push_bind(event.kind.as_str())
            .push_bind(event.visit_class)
            .push_bind(event.source)
            .push_bind(event.referrer.as_deref());
    });
    details.push(
        ") INSERT INTO analytics_events (occurred_at, link_id, host, kind, visit_class, source, referrer)
         SELECT occurred_at, link_id, host, kind, visit_class, source, referrer
         FROM pending
         WHERE link_id IS NULL OR EXISTS (
             SELECT 1 FROM links WHERE links.id = pending.link_id
             AND links.host = pending.host AND links.analytics_enabled = 1
         )
         RETURNING id",
    );
    let inserted = details
        .build_query_scalar::<i64>()
        .fetch_all(&mut *transaction)
        .await?;
    if !inserted.is_empty() {
        let mut rollups = QueryBuilder::<Sqlite>::new(
            "INSERT INTO analytics_daily (day, link_id, host, kind, visit_class, total)
             SELECT occurred_at / 86400, COALESCE(link_id, 0), host, kind, visit_class, count(*)
             FROM analytics_events WHERE id IN (",
        );
        let mut ids = rollups.separated(", ");
        for id in inserted {
            ids.push_bind(id);
        }
        rollups.push(
            ") GROUP BY occurred_at / 86400, COALESCE(link_id, 0), host, kind, visit_class
             ON CONFLICT(day, host, link_id, kind, visit_class)
             DO UPDATE SET total = total + excluded.total",
        );
        rollups.build().execute(&mut *transaction).await?;
    }
    transaction.commit().await
}

async fn prune(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let mut transaction = pool.begin().await?;
    sqlx::raw_sql(
        "DELETE FROM analytics_events
         WHERE occurred_at < unixepoch() - 86400 * (
             SELECT detail_days FROM analytics_settings WHERE id = 1
         );
         DELETE FROM analytics_daily
         WHERE day < unixepoch() / 86400 + 1 - (
             SELECT aggregate_days FROM analytics_settings WHERE id = 1
         );",
    )
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::open_database;

    const HOST: &str = "go.example";

    async fn database() -> (tempfile::TempDir, SqlitePool) {
        let directory = tempfile::tempdir().unwrap();
        let pool = open_database(&directory.path().join("analytics.sqlite"))
            .await
            .unwrap();
        (directory, pool)
    }

    fn request_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::USER_AGENT,
            axum::http::HeaderValue::from_static("Mozilla/5.0 Chrome/140.1 Safari/537.36"),
        );
        headers.insert(
            header::REFERER,
            axum::http::HeaderValue::from_static(
                "https://REFERRER.example:8443/synthetic-path?tag=sample#fragment",
            ),
        );
        headers
    }

    #[tokio::test]
    async fn worker_drains_sanitized_events_and_groups_outcomes() {
        let (_directory, pool) = database().await;
        let (analytics, worker) = Analytics::start(pool.clone()).await.unwrap();
        let mut headers = request_headers();
        analytics.record(HOST, None, Kind::Missing, &headers);
        analytics.record(HOST, None, Kind::Missing, &headers);
        headers.insert("dnt", axum::http::HeaderValue::from_static("1"));
        analytics.record(HOST, None, Kind::Blocked, &headers);

        drop(analytics);
        worker.await.unwrap();

        let details = sqlx::query_as::<_, (String, String, String, Option<String>)>(
            "SELECT host, visit_class, source, referrer FROM analytics_events ORDER BY id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            details,
            vec![
                (
                    HOST.to_owned(),
                    "human".to_owned(),
                    "chrome".to_owned(),
                    Some("referrer.example".to_owned())
                );
                2
            ]
        );
        let totals = sqlx::query_as::<_, (i64, String, i64)>(
            "SELECT link_id, kind, total FROM analytics_daily",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(totals, [(0, "missing".to_owned(), 2)]);
    }

    #[tokio::test]
    async fn clearing_analytics_invalidates_events_already_captured() {
        let (_directory, pool) = database().await;
        let (analytics, worker) = Analytics::start(pool.clone()).await.unwrap();
        let before_clear = analytics.shared.snapshot();
        let old = capture(before_clear, HOST, None, Kind::Missing, &request_headers()).unwrap();
        write_batch(&pool, &analytics.shared, std::slice::from_ref(&old))
            .await
            .unwrap();

        analytics.clear().await.unwrap();
        write_batch(&pool, &analytics.shared, &[old]).await.unwrap();
        let (details, daily) = sqlx::query_as::<_, (i64, i64)>(
            "SELECT (SELECT count(*) FROM analytics_events), (SELECT count(*) FROM analytics_daily)",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!((details, daily), (0, 0));

        analytics.record(HOST, None, Kind::Blocked, &request_headers());
        drop(analytics);
        worker.await.unwrap();

        let status: String = sqlx::query_scalar("SELECT kind FROM analytics_events")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(status, "blocked");
    }

    #[tokio::test]
    async fn the_writer_rechecks_link_privacy_and_database_settings() {
        let (_directory, pool) = database().await;
        let (analytics, worker) = Analytics::start(pool.clone()).await.unwrap();
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO links (host, slug, target, analytics_enabled)
             VALUES (?, 'opted-out', 'https://destination.example', 0) RETURNING id",
        )
        .bind(HOST)
        .fetch_one(&pool)
        .await
        .unwrap();
        let event = capture(
            analytics.shared.snapshot(),
            HOST,
            Some(id),
            Kind::Redirect,
            &request_headers(),
        )
        .unwrap();
        write_batch(&pool, &analytics.shared, &[event]).await.unwrap();

        let mut settings = analytics.settings();
        settings.referrers = false;
        settings.classify = false;
        settings.honor_dnt = false;
        assert_eq!(analytics.save_settings(settings).await.unwrap(), settings);
        let mut headers = request_headers();
        headers.insert("dnt", axum::http::HeaderValue::from_static("1"));
        analytics.record(HOST, None, Kind::Missing, &headers);
        drop(analytics);
        worker.await.unwrap();

        let details = sqlx::query_as::<_, (String, String, String, Option<String>)>(
            "SELECT kind, visit_class, source, referrer FROM analytics_events",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            details,
            [(
                "missing".to_owned(),
                "unknown".to_owned(),
                "unknown".to_owned(),
                None,
            )]
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_full_queue_counts_overflow_without_waiting_for_the_writer() {
        let (_directory, pool) = database().await;
        let (analytics, worker) = Analytics::start(pool).await.unwrap();
        let headers = HeaderMap::new();
        for _ in 0..=QUEUE_CAPACITY {
            analytics.record(HOST, None, Kind::Missing, &headers);
        }
        assert_eq!(analytics.dropped(), 1);

        drop(analytics);
        worker.await.unwrap();
    }

    #[tokio::test]
    async fn shrinking_detail_retention_keeps_daily_counts_until_their_own_cutoff() {
        let (_directory, pool) = database().await;
        let (analytics, worker) = Analytics::start(pool.clone()).await.unwrap();
        drop(analytics);
        worker.await.unwrap();
        sqlx::raw_sql(
            "UPDATE analytics_settings SET detail_days = 1, aggregate_days = 10;
             INSERT INTO analytics_events (occurred_at, host, kind, visit_class, source)
                 VALUES (unixepoch() - 3 * 86400, 'go.example', 'missing', 'unknown', 'unknown');
             INSERT INTO analytics_daily (day, link_id, host, kind, visit_class, total)
                 VALUES (unixepoch() / 86400 - 3, 0, 'go.example', 'missing', 'unknown', 2),
                        (unixepoch() / 86400 - 15, 0, 'go.example', 'missing', 'unknown', 3);",
        )
        .execute(&pool)
        .await
        .unwrap();
        prune(&pool).await.unwrap();

        let details: i64 = sqlx::query_scalar("SELECT count(*) FROM analytics_events")
            .fetch_one(&pool)
            .await
            .unwrap();
        let totals: Vec<i64> = sqlx::query_scalar("SELECT total FROM analytics_daily")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(details, 0);
        assert_eq!(totals, [2]);
    }

    #[test]
    fn captured_referrers_drop_ip_addresses_and_url_components() {
        let mut headers = request_headers();
        for (value, expected) in [
            (
                "https://Docs.Example:8080/path?tag=sample",
                Some("docs.example"),
            ),
            ("http://192.0.2.3/docs", None),
            ("http://[2001:db8::1]/docs", None),
            ("/no-origin", None),
            ("ftp://files.example/path", None),
        ] {
            headers.insert(header::REFERER, axum::http::HeaderValue::from_static(value));
            assert_eq!(referrer_host(&headers).as_deref(), expected);
        }
    }
}
