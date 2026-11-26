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

use crate::{history, input::normalize_host};

const QUEUE_CAPACITY: usize = 1024;
// Larger batches amortize the synchronous commit when a burst fills the queue.
const BATCH_SIZE: usize = 256;
const BATCH_WINDOW: Duration = Duration::from_millis(25);
const RETENTION_INTERVAL: Duration = Duration::from_secs(3600);
const PRUNE_BATCH_SIZE: u64 = 512;
const MAX_PRUNE_BATCHES: usize = 64;
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
    dropped: Arc<AtomicU64>,
    /// Events lost because a batch could not be written, a subset of `dropped`.
    failed: AtomicU64,
    /// Unix seconds of the last loss warning, to log at most once a minute.
    warned_at: AtomicU64,
}

impl Shared {
    fn report_loss(&self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs());
        let last = self.warned_at.load(Ordering::Relaxed);
        if now >= last.saturating_add(60)
            && self
                .warned_at
                .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
        {
            tracing::warn!(
                dropped = self.dropped.load(Ordering::Relaxed),
                write_failures = self.failed.load(Ordering::Relaxed),
                "analytics events were not recorded; redirects were not affected"
            );
        }
    }
}

impl Shared {
    fn snapshot(&self) -> Snapshot {
        *self.snapshot.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn update(&self, next: Snapshot) {
        let mut current = self
            .snapshot
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        if next.generation >= current.generation {
            *current = next;
        }
    }
}

#[derive(Clone)]
pub struct DroppedCounter(Arc<AtomicU64>);

impl DroppedCounter {
    pub fn get(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
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
    /// A domain root request answered by the domain's root destination.
    RootFallback,
    /// A domain root request without a usable root destination.
    RootMissing,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Redirect => "redirect",
            Self::Blocked => "blocked",
            Self::Missing => "missing",
            Self::RootFallback => "root_fallback",
            Self::RootMissing => "root_missing",
        }
    }
}

struct Event {
    generation: i64,
    occurred_at: i64,
    host: String,
    link_id: Option<i64>,
    kind: Kind,
    client: Client,
    referrer: Option<String>,
}

struct Client {
    visit_class: &'static str,
    source: &'static str,
    browser: &'static str,
    os: &'static str,
    device: &'static str,
    traffic_source: &'static str,
}

impl Default for Client {
    fn default() -> Self {
        Self {
            visit_class: "unknown",
            source: "unknown",
            browser: "unknown",
            os: "unknown",
            device: "unknown",
            traffic_source: "unknown",
        }
    }
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
            dropped: Arc::new(AtomicU64::new(0)),
            failed: AtomicU64::new(0),
            warned_at: AtomicU64::new(0),
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
            self.shared.report_loss();
        }
    }

    pub(crate) fn dropped(&self) -> u64 {
        self.shared.dropped.load(Ordering::Relaxed)
    }

    /// A read-only view of the loss counter that does not keep the writer alive.
    pub fn dropped_counter(&self) -> DroppedCounter {
        DroppedCounter(Arc::clone(&self.shared.dropped))
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
    let referrer = if settings.referrers {
        referrer_host(headers)
    } else {
        None
    };
    let client = if settings.classify {
        let traffic_source = match referrer.as_deref() {
            Some(referrer) if referrer == host => "internal",
            Some(_) => "referral",
            None if settings.referrers && !headers.contains_key(header::REFERER) => "direct",
            None => "unknown",
        };
        classify(headers, traffic_source)
    } else {
        Client::default()
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
        client,
        referrer,
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

fn classify(headers: &HeaderMap, traffic_source: &'static str) -> Client {
    let agent = headers
        .get(header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .filter(|value| value.len() <= MAX_HEADER_BYTES)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let contains = |markers: &[&str]| markers.iter().any(|marker| agent.contains(*marker));
    if preview_hint(headers)
        || contains(&[
            "facebookexternalhit",
            "slackbot",
            "discordbot",
            "twitterbot",
            "telegrambot",
            "linkedinbot",
            "whatsapp/",
        ])
    {
        return Client {
            visit_class: "bot",
            source: "preview",
            traffic_source: "preview",
            ..Client::default()
        };
    }
    if contains(&[
        "bot",
        "crawl",
        "spider",
        "headless",
        "monitor",
        "curl/",
        "wget/",
        "python-requests/",
    ]) {
        return Client {
            visit_class: "bot",
            source: "bot",
            traffic_source: "automated",
            ..Client::default()
        };
    }

    let browser = if contains(&["edge/", "edg/", "edga/", "edgios/"]) {
        "edge"
    } else if contains(&["opera", "opr/", "opios/"]) {
        "opera"
    } else if contains(&["firefox/", "fxios/"]) {
        "firefox"
    } else if contains(&["chrome/", "chromium/", "crios/"]) {
        "chrome"
    } else if contains(&["safari/"]) {
        "safari"
    } else {
        "unknown"
    };
    let os = if contains(&["windows"]) {
        "windows"
    } else if contains(&["android"]) {
        "android"
    } else if contains(&["iphone", "ipad", "ipod"]) {
        "ios"
    } else if contains(&["cros"]) {
        "chromeos"
    } else if contains(&["macintosh", "mac os x"]) {
        "macos"
    } else if contains(&["linux"]) {
        "linux"
    } else {
        "unknown"
    };
    let device = if contains(&["ipad", "tablet"]) {
        "tablet"
    } else if contains(&["mobile", "iphone", "ipod", "windows phone"]) {
        "mobile"
    } else if os == "android" {
        "tablet"
    } else if os == "unknown" {
        "unknown"
    } else {
        "desktop"
    };
    Client {
        visit_class: if browser == "unknown" {
            "unknown"
        } else {
            "human"
        },
        source: browser,
        browser,
        os,
        device,
        traffic_source,
    }
}

fn preview_hint(headers: &HeaderMap) -> bool {
    ["sec-purpose", "purpose"].into_iter().any(|name| {
        headers.get_all(name).iter().any(|value| {
            value.to_str().is_ok_and(|value| {
                value.len() <= MAX_HEADER_BYTES
                    && value.split(';').any(|token| {
                        ["prefetch", "prerender", "preview"]
                            .iter()
                            .any(|purpose| token.trim().eq_ignore_ascii_case(purpose))
                    })
            })
        })
    })
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
                if history::prune(&pool).await.is_err() {
                    tracing::warn!("link history retention failed");
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

        if let Err(error) = write_batch(&pool, &shared, &batch).await {
            let lost = batch.len() as u64;
            shared.dropped.fetch_add(lost, Ordering::Relaxed);
            shared.failed.fetch_add(lost, Ordering::Relaxed);
            tracing::debug!(%error, "analytics batch write failed");
            shared.report_loss();
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

    let current: Vec<_> = batch
        .iter()
        .filter(|event: &&Event| {
            snapshot.settings.enabled && event.generation == snapshot.generation
        })
        .collect();
    if current.is_empty() {
        return transaction.commit().await;
    }

    let mut details = QueryBuilder::<Sqlite>::new(
        "WITH pending (occurred_at, link_id, host, kind, visit_class, source, referrer,
                       browser, os, device, traffic_source) AS (",
    );
    details.push_values(current, |mut values, event| {
        values
            .push_bind(event.occurred_at)
            .push_bind(event.link_id)
            .push_bind(event.host.as_str())
            .push_bind(event.kind.as_str())
            .push_bind(event.client.visit_class)
            .push_bind(event.client.source)
            .push_bind(event.referrer.as_deref())
            .push_bind(event.client.browser)
            .push_bind(event.client.os)
            .push_bind(event.client.device)
            .push_bind(event.client.traffic_source);
    });
    details.push(
        ") INSERT INTO analytics_events (occurred_at, link_id, host, kind, visit_class, source,
                                        referrer, browser, os, device, traffic_source)
         SELECT occurred_at, link_id, host, kind, visit_class, source, referrer,
                browser, os, device, traffic_source
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
            "INSERT INTO analytics_daily (day, link_id, host, kind, visit_class, browser, os,
                                          device, traffic_source, total)
             SELECT occurred_at / 86400, COALESCE(link_id, 0), host, kind, visit_class,
                    browser, os, device, traffic_source, count(*)
             FROM analytics_events WHERE id IN (",
        );
        let mut ids = rollups.separated(", ");
        for id in inserted {
            ids.push_bind(id);
        }
        rollups.push(
            ") GROUP BY occurred_at / 86400, COALESCE(link_id, 0), host, kind, visit_class,
                        browser, os, device, traffic_source
             ON CONFLICT(day, host, link_id, kind, visit_class, browser, os, device, traffic_source)
             DO UPDATE SET total = total + excluded.total",
        );
        rollups.build().execute(&mut *transaction).await?;
    }
    transaction.commit().await
}

async fn prune(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    for _ in 0..MAX_PRUNE_BATCHES {
        let expired_details = sqlx::query(
            "DELETE FROM analytics_events WHERE id IN (
                SELECT id FROM analytics_events
                WHERE occurred_at < unixepoch() - 86400 * (
                    SELECT detail_days FROM analytics_settings WHERE id = 1
                )
                ORDER BY occurred_at, id LIMIT ?
             )",
        )
        .bind(PRUNE_BATCH_SIZE as i64)
        .execute(pool)
        .await?
        .rows_affected();
        let expired_rollups = sqlx::query(
            "DELETE FROM analytics_daily
             WHERE (day, host, link_id, kind, visit_class, browser, os, device, traffic_source) IN (
                SELECT day, host, link_id, kind, visit_class, browser, os, device, traffic_source
                FROM analytics_daily
                WHERE day < unixepoch() / 86400 + 1 - (
                    SELECT aggregate_days FROM analytics_settings WHERE id = 1
                )
                ORDER BY day, host, link_id, kind, visit_class, browser, os, device, traffic_source
                LIMIT ?
             )",
        )
        .bind(PRUNE_BATCH_SIZE as i64)
        .execute(pool)
        .await?
        .rows_affected();
        if expired_details < PRUNE_BATCH_SIZE && expired_rollups < PRUNE_BATCH_SIZE {
            break;
        }
        tokio::task::yield_now().await;
    }
    Ok(())
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
        write_batch(&pool, &analytics.shared, &[event])
            .await
            .unwrap();

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

    #[tokio::test]
    async fn upgrade_keeps_existing_outcomes_and_reports_no_root_requests() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("upgrade.sqlite");
        let legacy = crate::store::open_database_at_version(&path, 19).await;
        sqlx::raw_sql(
            "INSERT INTO links (id, host, slug, target) VALUES (5, 'go.example', 'docs', 'https://example.test');
             INSERT INTO analytics_events (occurred_at, link_id, host, kind, visit_class, source, browser)
                 VALUES (unixepoch(), 5, 'go.example', 'redirect', 'human', 'firefox', 'firefox'),
                        (unixepoch(), NULL, 'go.example', 'missing', 'unknown', 'unknown', 'unknown'),
                        (unixepoch(), NULL, 'go.example', 'blocked', 'unknown', 'unknown', 'unknown'),
                        (unixepoch(), NULL, 'go.example', 'missing', 'unknown', 'unknown', 'unknown');
             DELETE FROM analytics_events WHERE id = 4;
             INSERT INTO analytics_daily (day, host, link_id, kind, visit_class, browser, total)
                 VALUES (unixepoch() / 86400, 'go.example', 5, 'redirect', 'human', 'firefox', 3),
                        (unixepoch() / 86400, 'go.example', 0, 'missing', 'unknown', 'unknown', 2),
                        (unixepoch() / 86400, 'go.example', 0, 'blocked', 'unknown', 'unknown', 1);",
        )
        .execute(&legacy)
        .await
        .unwrap();
        // Schema 19 cannot store the new categories.
        assert!(
            sqlx::query(
                "INSERT INTO analytics_daily (day, host, link_id, kind, visit_class, total)
                 VALUES (1, 'go.example', 0, 'root_missing', 'unknown', 1)"
            )
            .execute(&legacy)
            .await
            .is_err()
        );
        let totals = |pool: SqlitePool| async move {
            sqlx::query_as::<_, (String, i64)>(
                "SELECT kind, SUM(total) FROM analytics_daily GROUP BY kind ORDER BY kind",
            )
            .fetch_all(&pool)
            .await
            .unwrap()
        };
        let before = totals(legacy.clone()).await;
        let events_before =
            sqlx::query_as::<_, (i64, String)>("SELECT id, kind FROM analytics_events ORDER BY id")
                .fetch_all(&legacy)
                .await
                .unwrap();
        legacy.close().await;

        let pool = open_database(&path).await.unwrap();
        assert_eq!(totals(pool.clone()).await, before);
        let events_after =
            sqlx::query_as::<_, (i64, String)>("SELECT id, kind FROM analytics_events ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(events_after, events_before);
        let (analytics, worker) = Analytics::start(pool.clone()).await.unwrap();
        let event = capture(
            analytics.shared.snapshot(),
            HOST,
            None,
            Kind::RootMissing,
            &request_headers(),
        )
        .unwrap();
        write_batch(&pool, &analytics.shared, &[event])
            .await
            .unwrap();
        drop(analytics);
        worker.await.unwrap();
        // The deleted event ID is not reused after the table rebuild.
        let newest: i64 = sqlx::query_scalar("SELECT max(id) FROM analytics_events")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(newest, 5);
        // The link deletion trigger still removes the link's rollups after the rebuild.
        sqlx::query("DELETE FROM links WHERE id = 5")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            totals(pool.clone()).await,
            [
                ("blocked".to_owned(), 1),
                ("missing".to_owned(), 2),
                ("root_missing".to_owned(), 1)
            ]
        );
        assert!(
            sqlx::query(
                "INSERT INTO analytics_daily (day, host, link_id, kind, visit_class, total)
                 VALUES (1, 'go.example', 7, 'root_fallback', 'unknown', 1)"
            )
            .execute(&pool)
            .await
            .is_err()
        );
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

    #[test]
    fn classification_keeps_coarse_categories_and_prefetch_takes_precedence() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::USER_AGENT,
            axum::http::HeaderValue::from_static(
                "Mozilla/5.0 (Linux; Android 14; Sample Device) Chrome/140.1 Mobile Safari/537.36",
            ),
        );
        let client = classify(&headers, "referral");
        assert_eq!(
            (
                client.browser,
                client.os,
                client.device,
                client.traffic_source
            ),
            ("chrome", "android", "mobile", "referral"),
        );

        headers.insert(
            "sec-purpose",
            axum::http::HeaderValue::from_static("prefetch;prerender"),
        );
        let client = classify(&headers, "referral");
        assert_eq!(
            (client.visit_class, client.traffic_source),
            ("bot", "preview"),
        );
        assert_eq!(
            (client.browser, client.os, client.device),
            ("unknown", "unknown", "unknown"),
        );
    }
}
