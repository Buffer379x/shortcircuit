//! Opt-in webhook notifications with a durable, deduplicated outbox.
//!
//! Configuring a webhook, as a container variable or in Settings > Operations, activates
//! notifications. The worker reads the effective settings on each pass. Events get a deterministic key so repeated
//! detection, retries, and restarts never create a second delivery. A bounded worker detects
//! scheduled events, delivers due rows with capped exponential backoff, and expires stale rows.
//! After a restore, pending rows are discarded and only events after the restore are generated.

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use serde::Serialize;
use serde_json::{Value, json};
use sqlx::{Executor, Sqlite, SqlitePool};
use tokio::{
    sync::{Notify, watch},
    time,
};

use crate::{
    analytics::DroppedCounter,
    outbound::{Connector, Failure, Fetcher, Webhook},
    resources::{self, StorageStatus},
    runtime_settings::RuntimeSettings,
};

const MAX_ATTEMPTS: i64 = 8;
const MAX_BACKOFF_SECONDS: i64 = 3600;
/// Undelivered rows older than this are not sent.
const STALE_SECONDS: i64 = 24 * 60 * 60;
/// Expired links are reported only for this long after their expiry.
const EXPIRED_LOOKBACK_SECONDS: i64 = 7 * 24 * 60 * 60;
const SCAN_INTERVAL: Duration = Duration::from_secs(60);
const DELIVERY_BATCH: i64 = 10;
const SCAN_LIMIT: i64 = 100;
/// Links with traffic today that one scan evaluates for click milestones.
const MILESTONE_SCAN_LIMIT: i64 = 5_000;
const MAX_MILESTONES: usize = 10;
const MAX_FILE_BYTES: u64 = 4097;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EventKind {
    LinkHealth,
    LinkExpiring,
    LinkExpired,
    ClickMilestone,
    BackupFailed,
    JobFailed,
    StorageLow,
    Security,
}

impl EventKind {
    const ALL: [Self; 8] = [
        Self::LinkHealth,
        Self::LinkExpiring,
        Self::LinkExpired,
        Self::ClickMilestone,
        Self::BackupFailed,
        Self::JobFailed,
        Self::StorageLow,
        Self::Security,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::LinkHealth => "link_health",
            Self::LinkExpiring => "link_expiring",
            Self::LinkExpired => "link_expired",
            Self::ClickMilestone => "click_milestone",
            Self::BackupFailed => "backup_failed",
            Self::JobFailed => "job_failed",
            Self::StorageLow => "storage_low",
            Self::Security => "security",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.name() == value)
    }
}

/// Audit actions that produce a security notification on success. Any rejected or failed
/// audited action also produces one, except for sign-in attempts, which rate limits cover.
const SECURITY_ACTIONS: &[&str] = &[
    "recovery_token_issued",
    "account_recovery",
    "password_change",
    "key_create",
    "key_rotate",
    "key_revoke",
    "restore_stage",
    "restore_cancel",
    "restore_apply",
    "destination_policy_update",
    "operations_settings_update",
    "public_pages_update",
    "public_pages_reset",
    "public_pages_import",
];

/// The notification handle. It reads the effective settings on every use, so a changed target,
/// event selection, or deactivation in Settings > Operations applies without a restart.
#[derive(Clone)]
pub struct Notifier(Arc<Inner>);

struct Inner {
    settings: RuntimeSettings,
    fetcher: Fetcher,
    changed: Notify,
}

/// An active notification configuration: a validated webhook and the selected events.
pub(crate) struct NotificationConfig {
    webhook: Webhook,
    events: BTreeSet<EventKind>,
    milestones: Vec<i64>,
    expiry_notice: i64,
}

/// Notification values from container variables. Each is only a start value: a value saved in
/// Settings > Operations takes precedence.
#[derive(Default)]
pub(crate) struct NotificationEnv {
    pub(crate) webhook: Option<String>,
    /// The webhook came from the `MONITOR_WEBHOOK_URL` alias, which selects only link health.
    pub(crate) legacy: bool,
    pub(crate) authorization: Option<String>,
    pub(crate) events: Option<BTreeSet<EventKind>>,
    pub(crate) milestones: Option<Vec<i64>>,
    pub(crate) expiry_hours: Option<u16>,
    /// Connector infrastructure stays environment-only.
    pub(crate) connector: Option<Connector>,
}

/// A notification to record. Each variant has a deterministic identity.
pub(crate) enum Event {
    Test {
        nonce: u128,
    },
    LinkHealth {
        link_id: i64,
        status: &'static str,
        revision: String,
        checked_at: i64,
    },
    BackupFailed {
        at: i64,
    },
}

/// A notification event, from its API name.
pub(crate) fn parse_event(name: &str) -> Option<EventKind> {
    EventKind::parse(name)
}

pub(crate) fn event_name(kind: EventKind) -> &'static str {
    kind.name()
}

/// The event selection when none is saved or configured.
pub(crate) fn default_events(legacy: bool, milestones: bool) -> BTreeSet<EventKind> {
    if legacy {
        return BTreeSet::from([EventKind::LinkHealth]);
    }
    EventKind::ALL
        .into_iter()
        .filter(|kind| *kind != EventKind::ClickMilestone || milestones)
        .collect()
}

impl NotificationEnv {
    /// Reads and validates the notification variables.
    pub(crate) fn from_env(read: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        let value = |name: &str| read(name).filter(|value| !value.is_empty());
        let file = |name: &str| -> Result<Option<String>, String> {
            match value(name) {
                Some(path) => read_secret(Path::new(&path))
                    .map(Some)
                    .map_err(|_| format!("could not read {name}")),
                None => Ok(None),
            }
        };
        let exclusive = |plain: &str, path: &str| -> Result<Option<String>, String> {
            match (value(plain), file(path)?) {
                (Some(_), Some(_)) => Err(format!("set only one of {plain} and {path}")),
                (plain, path) => Ok(plain.or(path)),
            }
        };
        let notification = exclusive("NOTIFICATION_WEBHOOK_URL", "NOTIFICATION_WEBHOOK_URL_FILE")?;
        let legacy = exclusive("MONITOR_WEBHOOK_URL", "MONITOR_WEBHOOK_URL_FILE")?;
        let (webhook, legacy) = match (notification, legacy) {
            (Some(_), Some(_)) => {
                return Err(
                    "MONITOR_WEBHOOK_URL is an alias; set only NOTIFICATION_WEBHOOK_URL".into(),
                );
            }
            (Some(url), None) => (Some(url), false),
            (None, Some(url)) => (Some(url), true),
            (None, None) => (None, false),
        };
        let milestones = match value("NOTIFICATION_CLICK_MILESTONES") {
            Some(list) => Some(parse_milestones(&list)?),
            None => None,
        };
        let events = match value("NOTIFICATION_EVENTS") {
            Some(list) => {
                let mut events = BTreeSet::new();
                for name in list.split(',').map(str::trim) {
                    events
                        .insert(EventKind::parse(name).ok_or_else(|| {
                            format!("unknown NOTIFICATION_EVENTS entry {name:?}")
                        })?);
                }
                Some(events)
            }
            None => None,
        };
        if events
            .as_ref()
            .is_some_and(|events| events.contains(&EventKind::ClickMilestone))
            && milestones.as_ref().is_none_or(Vec::is_empty)
        {
            return Err("click_milestone requires NOTIFICATION_CLICK_MILESTONES".into());
        }
        let expiry_hours = match value("NOTIFICATION_EXPIRY_NOTICE_HOURS") {
            Some(hours) => Some(
                hours
                    .parse::<u16>()
                    .ok()
                    .filter(|hours| (1..=720).contains(hours))
                    .ok_or("NOTIFICATION_EXPIRY_NOTICE_HOURS must be 1 to 720")?,
            ),
            None => None,
        };
        let connector = match (
            value("NOTIFICATION_CONNECTOR_ORIGIN"),
            value("NOTIFICATION_CONNECTOR_NETWORKS"),
        ) {
            (Some(origin), Some(networks)) => {
                let roots = match value("NOTIFICATION_CONNECTOR_CA_FILE") {
                    Some(path) => Some(
                        std::fs::read(&path)
                            .map_err(|_| "could not read NOTIFICATION_CONNECTOR_CA_FILE")?,
                    ),
                    None => None,
                };
                Some(Connector::new(&origin, &networks, roots.as_deref())?)
            }
            (None, None) if value("NOTIFICATION_CONNECTOR_CA_FILE").is_none() => None,
            _ => {
                return Err(
                    "a connector needs NOTIFICATION_CONNECTOR_ORIGIN and NOTIFICATION_CONNECTOR_NETWORKS"
                        .into(),
                );
            }
        };
        let authorization = file("NOTIFICATION_WEBHOOK_AUTHORIZATION_FILE")?;
        // A configured webhook is validated as before, so a broken container value still stops
        // the start instead of silently disabling notifications.
        if let Some(url) = &webhook {
            Webhook::new(url, authorization.as_deref(), connector.clone())?;
        }
        Ok(Self {
            webhook,
            legacy,
            authorization,
            events,
            milestones,
            expiry_hours,
            connector,
        })
    }
}

impl NotificationConfig {
    pub(crate) fn new(
        url: &str,
        authorization: Option<&str>,
        connector: Option<Connector>,
        events: BTreeSet<EventKind>,
        milestones: Vec<i64>,
        expiry_hours: u16,
    ) -> Result<Self, &'static str> {
        Ok(Self {
            webhook: Webhook::new(url, authorization, connector)?,
            events,
            milestones,
            expiry_notice: i64::from(expiry_hours) * 3600,
        })
    }

    fn wants(&self, kind: EventKind) -> bool {
        self.events.contains(&kind)
    }

    pub(crate) fn webhook(&self) -> &Webhook {
        &self.webhook
    }

    pub(crate) fn origin(&self) -> String {
        self.webhook.origin()
    }

    pub(crate) fn uses_connector(&self) -> bool {
        self.webhook.uses_connector()
    }

    pub(crate) fn events(&self) -> Vec<EventKind> {
        self.events.iter().copied().collect()
    }

    pub(crate) fn milestones(&self) -> &[i64] {
        &self.milestones
    }

    pub(crate) fn expiry_notice_hours(&self) -> i64 {
        self.expiry_notice / 3600
    }
}

impl Notifier {
    pub fn new(settings: &RuntimeSettings) -> Self {
        Self(Arc::new(Inner {
            settings: settings.clone(),
            fetcher: Fetcher::new(),
            changed: Notify::new(),
        }))
    }

    /// The active configuration, or `None` while notifications are off.
    pub(crate) fn config(&self) -> Option<Arc<NotificationConfig>> {
        self.0.settings.current().notifications.clone()
    }

    /// Records an event if its kind is enabled. Duplicate keys are ignored.
    pub(crate) async fn record<'e>(
        &self,
        executor: impl Executor<'e, Database = Sqlite>,
        event: Event,
    ) -> Result<(), sqlx::Error> {
        // Nothing is recorded while notifications are off.
        let Some(config) = self.config() else {
            return Ok(());
        };
        let (kind, key, occurred_at, payload) = match event {
            Event::Test { nonce } => ("test", format!("test:{nonce:032x}"), None, json!({})),
            Event::LinkHealth {
                link_id,
                status,
                revision,
                checked_at,
            } => {
                if !config.wants(EventKind::LinkHealth) {
                    return Ok(());
                }
                (
                    "link_health",
                    format!("link_health:{link_id}:{revision}:{checked_at}"),
                    Some(checked_at),
                    json!({"link_id": link_id, "status": status}),
                )
            }
            Event::BackupFailed { at } => {
                if !config.wants(EventKind::BackupFailed) {
                    return Ok(());
                }
                (
                    "backup_failed",
                    format!("backup_failed:{at}"),
                    Some(at),
                    json!({"source": "scheduled"}),
                )
            }
        };
        insert(executor, kind, &key, occurred_at, &payload).await?;
        self.0.changed.notify_one();
        Ok(())
    }

    /// Runs detection and delivery until shutdown.
    pub async fn run(
        &self,
        pool: SqlitePool,
        data_directory: PathBuf,
        dropped_events: Option<DroppedCounter>,
        mut stop: watch::Receiver<bool>,
    ) {
        let mut settings = self.0.settings.subscribe();
        let mut initialized = false;
        let mut dropped = dropped_events.as_ref().map_or(0, DroppedCounter::get);
        let mut scan = time::interval(SCAN_INTERVAL);
        scan.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
        loop {
            // While notifications are off, wait for a settings change or shutdown.
            if self.config().is_none() {
                tokio::select! {
                    _ = stop.changed() => return,
                    Ok(()) = settings.changed() => {}
                }
                continue;
            }
            if !initialized {
                if start(&pool).await.is_err() {
                    tracing::warn!("could not initialize notification state");
                }
                initialized = true;
            }
            let scan_due = tokio::select! {
                _ = stop.changed() => return,
                _ = scan.tick() => true,
                _ = self.0.changed.notified() => false,
                Ok(()) = settings.changed() => continue,
            };
            if scan_due {
                let current = dropped_events.as_ref().map_or(0, DroppedCounter::get);
                let newly_dropped = current.saturating_sub(dropped);
                dropped = current;
                if self
                    .detect(&pool, &data_directory, newly_dropped)
                    .await
                    .is_err()
                {
                    tracing::warn!("notification detection failed");
                }
            }
            // Shutdown aborts this task. An interrupted delivery stays pending and is retried,
            // so receivers can see a delivery twice and should deduplicate by its id.
            if self.deliver_due(&pool).await.is_err() {
                tracing::warn!("notification delivery bookkeeping failed");
            }
        }
    }

    /// Generates scheduled events from durable state. Every query is bounded.
    pub(crate) async fn detect(
        &self,
        pool: &SqlitePool,
        data_directory: &Path,
        newly_dropped: u64,
    ) -> Result<(), sqlx::Error> {
        let Some(config) = self.config() else {
            return Ok(());
        };
        let not_before: i64 =
            sqlx::query_scalar("SELECT coalesce(not_before, unixepoch()) FROM notification_state")
                .fetch_one(pool)
                .await?;
        let now: i64 = sqlx::query_scalar("SELECT unixepoch()")
            .fetch_one(pool)
            .await?;
        if config.wants(EventKind::LinkExpiring) {
            let rows = sqlx::query_as::<_, (i64, i64)>(
                "SELECT links.id, policy.expires_at FROM link_redirect_policies AS policy
                 JOIN links ON links.id = policy.link_id
                 WHERE links.enabled = 1 AND policy.expires_at > ?1
                   AND policy.expires_at - ?2 <= ?1 AND policy.expires_at - ?2 >= ?3
                   AND NOT EXISTS (SELECT 1 FROM notification_outbox WHERE event_key =
                       'link_expiring:' || links.id || ':' || policy.expires_at)
                 ORDER BY policy.expires_at, links.id LIMIT ?4",
            )
            .bind(now)
            .bind(config.expiry_notice)
            .bind(not_before)
            .bind(SCAN_LIMIT)
            .fetch_all(pool)
            .await?;
            for (id, expires_at) in rows {
                insert(
                    pool,
                    "link_expiring",
                    &format!("link_expiring:{id}:{expires_at}"),
                    Some(expires_at - config.expiry_notice),
                    &json!({"link_id": id, "expires_at": expires_at}),
                )
                .await?;
            }
        }
        if config.wants(EventKind::LinkExpired) {
            let rows = sqlx::query_as::<_, (i64, i64)>(
                "SELECT links.id, policy.expires_at FROM link_redirect_policies AS policy
                 JOIN links ON links.id = policy.link_id
                 WHERE policy.expires_at <= ?1 AND policy.expires_at >= max(?2, ?1 - ?3)
                   AND NOT EXISTS (SELECT 1 FROM notification_outbox WHERE event_key =
                       'link_expired:' || links.id || ':' || policy.expires_at)
                 ORDER BY policy.expires_at, links.id LIMIT ?4",
            )
            .bind(now)
            .bind(not_before)
            .bind(EXPIRED_LOOKBACK_SECONDS)
            .bind(SCAN_LIMIT)
            .fetch_all(pool)
            .await?;
            for (id, expires_at) in rows {
                insert(
                    pool,
                    "link_expired",
                    &format!("link_expired:{id}:{expires_at}"),
                    Some(expires_at),
                    &json!({"link_id": id, "expires_at": expires_at}),
                )
                .await?;
            }
        }
        if config.wants(EventKind::ClickMilestone) {
            // One query per scan: links with redirects today, their total before today, and
            // their total now. A milestone counts when today's redirects cross it.
            let rows = sqlx::query_as::<_, (i64, i64, i64)>(
                "WITH today AS (
                     SELECT DISTINCT link_id FROM analytics_daily
                     WHERE day = ?1 AND kind = 'redirect' AND link_id > 0
                 )
                 SELECT daily.link_id,
                        coalesce(sum(daily.total) FILTER (WHERE daily.day < ?1), 0),
                        sum(daily.total)
                 FROM today JOIN analytics_daily AS daily ON daily.link_id = today.link_id
                 WHERE daily.kind = 'redirect'
                 GROUP BY daily.link_id ORDER BY daily.link_id LIMIT ?2",
            )
            .bind(now / 86_400)
            .bind(MILESTONE_SCAN_LIMIT)
            .fetch_all(pool)
            .await?;
            for (id, before, total) in rows {
                for milestone in config
                    .milestones
                    .iter()
                    .filter(|milestone| before < **milestone && **milestone <= total)
                {
                    insert(
                        pool,
                        "click_milestone",
                        &format!("click_milestone:{id}:{milestone}"),
                        None,
                        &json!({"link_id": id, "milestone": milestone}),
                    )
                    .await?;
                }
            }
        }
        if config.wants(EventKind::StorageLow) {
            let database_bytes: i64 = sqlx::query_scalar(
                "SELECT (SELECT page_count FROM pragma_page_count()) *
                        (SELECT page_size FROM pragma_page_size())",
            )
            .fetch_one(pool)
            .await?;
            let directory = data_directory.to_owned();
            let storage = tokio::task::spawn_blocking(move || {
                resources::storage(
                    &directory,
                    u64::try_from(database_bytes).unwrap_or_default(),
                )
            })
            .await
            .map_err(|error| sqlx::Error::Io(std::io::Error::other(error)))?;
            if storage.status == StorageStatus::Low {
                insert(
                    pool,
                    "storage_low",
                    &format!("storage_low:{}", now / 86_400),
                    None,
                    &json!({
                        "available_bytes": storage.available_bytes,
                        "low_threshold_bytes": storage.low_threshold_bytes,
                    }),
                )
                .await?;
            }
        }
        if config.wants(EventKind::JobFailed) && newly_dropped > 0 {
            insert(
                pool,
                "job_failed",
                &format!("job_failed:analytics:{}", now / 86_400),
                None,
                &json!({"job": "analytics", "dropped_events": newly_dropped}),
            )
            .await?;
        }
        self.detect_security(pool, &config).await
    }

    async fn detect_security(
        &self,
        pool: &SqlitePool,
        config: &NotificationConfig,
    ) -> Result<(), sqlx::Error> {
        let mut transaction = pool.begin_with("BEGIN IMMEDIATE").await?;
        let cursor: i64 = sqlx::query_scalar("SELECT audit_cursor FROM notification_state")
            .fetch_one(&mut *transaction)
            .await?;
        let rows = sqlx::query_as::<_, (i64, i64, String, String)>(
            "SELECT id, created_at, action, outcome FROM admin_audit
             WHERE id > ? ORDER BY id LIMIT ?",
        )
        .bind(cursor)
        .bind(SCAN_LIMIT)
        .fetch_all(&mut *transaction)
        .await?;
        let Some(last) = rows.last().map(|row| row.0) else {
            return transaction.commit().await;
        };
        if config.wants(EventKind::Security) {
            for (id, created_at, action, outcome) in &rows {
                let notable = (outcome == "success" && SECURITY_ACTIONS.contains(&action.as_str()))
                    || (outcome != "success" && action != "login");
                if notable {
                    insert(
                        &mut *transaction,
                        "security",
                        &format!("security:{id}"),
                        Some(*created_at),
                        &json!({"audit_id": id, "action": action, "outcome": outcome}),
                    )
                    .await?;
                }
            }
        }
        sqlx::query("UPDATE notification_state SET audit_cursor = ?")
            .bind(last)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await
    }

    /// Sends due rows. Failures back off exponentially and stop after `MAX_ATTEMPTS`.
    pub(crate) async fn deliver_due(&self, pool: &SqlitePool) -> Result<(), sqlx::Error> {
        let Some(config) = self.config() else {
            return Ok(());
        };
        sqlx::query(
            "UPDATE notification_outbox SET status = 'failed', last_error = 'stale'
             WHERE status = 'pending' AND created_at < unixepoch() - ?",
        )
        .bind(STALE_SECONDS)
        .execute(pool)
        .await?;
        let due = sqlx::query_as::<_, (i64, String, String, i64, String, i64)>(
            "SELECT id, event_key, event, occurred_at, payload, attempts FROM notification_outbox
             WHERE status = 'pending' AND next_attempt_at <= unixepoch()
             ORDER BY next_attempt_at, id LIMIT ?",
        )
        .bind(DELIVERY_BATCH)
        .fetch_all(pool)
        .await?;
        if due.is_empty() {
            return Ok(());
        }
        let epoch: String = sqlx::query_scalar("SELECT epoch FROM notification_state")
            .fetch_one(pool)
            .await?;
        for (id, key, event, occurred_at, payload, attempts) in due {
            let mut body: Value = serde_json::from_str(&payload).unwrap_or_else(|_| json!({}));
            if let Some(fields) = body.as_object_mut() {
                fields.insert("version".into(), json!(1));
                fields.insert("id".into(), json!(format!("{epoch}:{key}")));
                fields.insert("event".into(), json!(event));
                fields.insert("occurred_at".into(), json!(occurred_at));
            }
            let result = self
                .0
                .fetcher
                .deliver(config.webhook(), body.to_string().into_bytes())
                .await;
            match result {
                Ok(()) => {
                    sqlx::query(
                        "UPDATE notification_outbox SET status = 'delivered', attempts = attempts + 1,
                             delivered_at = unixepoch(), last_error = NULL
                         WHERE id = ? AND status = 'pending'",
                    )
                    .bind(id)
                    .execute(pool)
                    .await?;
                }
                Err(failure) => {
                    let attempts = attempts + 1;
                    let give_up = attempts >= MAX_ATTEMPTS || failure == Failure::Blocked;
                    sqlx::query(
                        "UPDATE notification_outbox SET attempts = ?, last_error = ?,
                             status = CASE WHEN ? THEN 'failed' ELSE 'pending' END,
                             next_attempt_at = unixepoch() + ?
                         WHERE id = ? AND status = 'pending'",
                    )
                    .bind(attempts)
                    .bind(failure_code(failure))
                    .bind(give_up)
                    .bind(backoff(attempts, id))
                    .bind(id)
                    .execute(pool)
                    .await?;
                }
            }
        }
        Ok(())
    }
}

fn backoff(attempts: i64, id: i64) -> i64 {
    let exponent = (attempts - 1).clamp(0, 16) as u32;
    (60_i64 << exponent).min(MAX_BACKOFF_SECONDS) + id.rem_euclid(30)
}

fn failure_code(failure: Failure) -> &'static str {
    match failure {
        Failure::Blocked => "blocked",
        Failure::Busy => "busy",
        Failure::Dns => "dns",
        Failure::Timeout => "timeout",
        Failure::Connection => "connection",
        Failure::Protocol | Failure::NotHtml => "protocol",
    }
}

async fn insert<'e>(
    executor: impl Executor<'e, Database = Sqlite>,
    kind: &str,
    key: &str,
    occurred_at: Option<i64>,
    payload: &Value,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO notification_outbox (event_key, event, payload, occurred_at)
         VALUES (?, ?, ?, coalesce(?, unixepoch())) ON CONFLICT (event_key) DO NOTHING",
    )
    .bind(key)
    .bind(kind)
    .bind(payload.to_string())
    .bind(occurred_at)
    .execute(executor)
    .await?;
    Ok(())
}

/// Marks the activation time so earlier history does not produce notifications.
async fn start(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE notification_state SET not_before = unixepoch(),
             audit_cursor = (SELECT coalesce(max(id), 0) FROM admin_audit)
         WHERE not_before IS NULL",
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Called once after a restore: pending deliveries are discarded and only later events are
/// generated, so the restored timeline does not resend notifications from before the restore.
/// A new epoch keeps delivery IDs unique although restored database IDs can repeat.
pub(crate) async fn after_restore<'e>(
    executor: impl Executor<'e, Database = Sqlite>,
) -> Result<(), sqlx::Error> {
    sqlx::raw_sql(
        "UPDATE notification_outbox SET status = 'discarded', last_error = 'restored'
             WHERE status = 'pending';
         UPDATE notification_state SET not_before = unixepoch(),
             audit_cursor = (SELECT coalesce(max(id), 0) FROM admin_audit),
             epoch = lower(hex(randomblob(16)));",
    )
    .execute(executor)
    .await?;
    Ok(())
}

/// Called in the transaction that changes the effective webhook, including turning it on or off.
/// Pending deliveries were meant for the previous target and are discarded, and only events after
/// the change are generated, so a new target never receives earlier history.
pub(crate) async fn target_changed(
    connection: &mut sqlx::SqliteConnection,
) -> Result<(), sqlx::Error> {
    sqlx::raw_sql(
        "UPDATE notification_outbox SET status = 'discarded', last_error = NULL
             WHERE status = 'pending';
         UPDATE notification_state SET not_before = unixepoch(),
             audit_cursor = (SELECT coalesce(max(id), 0) FROM admin_audit);",
    )
    .execute(connection)
    .await?;
    Ok(())
}

pub(crate) fn parse_milestones(list: &str) -> Result<Vec<i64>, String> {
    let invalid =
        || "NOTIFICATION_CLICK_MILESTONES must list up to 10 positive integers".to_owned();
    let mut milestones = list
        .split(',')
        .map(|value| {
            value
                .trim()
                .parse::<i64>()
                .ok()
                .filter(|value| (1..=1_000_000_000_000).contains(value))
                .ok_or_else(invalid)
        })
        .collect::<Result<Vec<_>, _>>()?;
    milestones.sort_unstable();
    milestones.dedup();
    if milestones.is_empty() || milestones.len() > MAX_MILESTONES {
        return Err(invalid());
    }
    Ok(milestones)
}

fn read_secret(path: &Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut value = String::new();
    std::fs::File::open(path)?
        .take(MAX_FILE_BYTES)
        .read_to_string(&mut value)?;
    let value = value.trim_end_matches(['\r', '\n']);
    if value.is_empty() || value.len() >= MAX_FILE_BYTES as usize {
        return Err(std::io::Error::other("invalid secret file"));
    }
    Ok(value.to_owned())
}

#[derive(Serialize, sqlx::FromRow)]
pub(crate) struct Delivery {
    id: i64,
    event: String,
    occurred_at: i64,
    status: String,
    attempts: i64,
    last_error: Option<String>,
    next_attempt_at: i64,
    delivered_at: Option<i64>,
}

#[derive(Default, Serialize, sqlx::FromRow)]
pub(crate) struct Counts {
    pending: i64,
    delivered: i64,
    failed: i64,
    discarded: i64,
}

pub(crate) async fn status(pool: &SqlitePool) -> Result<(Counts, Vec<Delivery>), sqlx::Error> {
    let counts = sqlx::query_as(
        "SELECT count(*) FILTER (WHERE status = 'pending') AS pending,
                count(*) FILTER (WHERE status = 'delivered') AS delivered,
                count(*) FILTER (WHERE status = 'failed') AS failed,
                count(*) FILTER (WHERE status = 'discarded') AS discarded
         FROM notification_outbox",
    )
    .fetch_one(pool)
    .await?;
    let items = sqlx::query_as(
        "SELECT id, event, occurred_at, status, attempts, last_error, next_attempt_at, delivered_at
         FROM notification_outbox ORDER BY id DESC LIMIT 25",
    )
    .fetch_all(pool)
    .await?;
    Ok((counts, items))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn settings(values: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let values: HashMap<String, String> = values
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect();
        move |name| values.get(name).cloned()
    }

    fn notifier(values: &[(&str, &str)]) -> Notifier {
        Notifier::new(&RuntimeSettings::detached(settings(values)).unwrap())
    }

    fn config(values: &[(&str, &str)]) -> Arc<NotificationConfig> {
        notifier(values).config().unwrap()
    }

    async fn database() -> (tempfile::TempDir, SqlitePool) {
        let directory = tempfile::tempdir().unwrap();
        let pool = crate::open_database(&directory.path().join("notify.db"))
            .await
            .unwrap();
        (directory, pool)
    }

    async fn rows(pool: &SqlitePool) -> Vec<(String, String, String)> {
        sqlx::query_as("SELECT event, event_key, status FROM notification_outbox ORDER BY id")
            .fetch_all(pool)
            .await
            .unwrap()
    }

    #[test]
    fn notifications_are_off_without_a_webhook_and_settings_are_validated() {
        assert!(notifier(&[]).config().is_none());
        // Without a webhook, other values are start values for Settings > Operations.
        assert!(
            notifier(&[("NOTIFICATION_EVENTS", "security")])
                .config()
                .is_none()
        );
        for (values, message) in [
            (
                vec![("NOTIFICATION_WEBHOOK_URL", "http://hooks.example/")],
                "HTTPS",
            ),
            (
                vec![("NOTIFICATION_WEBHOOK_URL", "https://hooks.example:8443/")],
                "port 443",
            ),
            (
                vec![
                    ("NOTIFICATION_WEBHOOK_URL", "https://hooks.example/"),
                    ("NOTIFICATION_EVENTS", "everything"),
                ],
                "unknown",
            ),
            (
                vec![
                    ("NOTIFICATION_WEBHOOK_URL", "https://hooks.example/"),
                    ("NOTIFICATION_EVENTS", "click_milestone"),
                ],
                "NOTIFICATION_CLICK_MILESTONES",
            ),
            (
                vec![
                    ("NOTIFICATION_WEBHOOK_URL", "https://hooks.example/"),
                    ("NOTIFICATION_CLICK_MILESTONES", "0"),
                ],
                "positive",
            ),
            (
                vec![
                    ("NOTIFICATION_WEBHOOK_URL", "https://hooks.example/"),
                    ("NOTIFICATION_EXPIRY_NOTICE_HOURS", "0"),
                ],
                "1 to 720",
            ),
            (
                vec![
                    ("NOTIFICATION_WEBHOOK_URL", "https://hooks.example/"),
                    ("MONITOR_WEBHOOK_URL", "https://other.example/"),
                ],
                "alias",
            ),
            (
                vec![
                    ("NOTIFICATION_WEBHOOK_URL", "https://hooks.example/"),
                    ("NOTIFICATION_CONNECTOR_ORIGIN", "https://hooks.lan"),
                ],
                "NOTIFICATION_CONNECTOR_NETWORKS",
            ),
            (
                vec![
                    ("NOTIFICATION_WEBHOOK_URL", "https://hooks.example/"),
                    ("NOTIFICATION_CONNECTOR_ORIGIN", "https://hooks.lan"),
                    ("NOTIFICATION_CONNECTOR_NETWORKS", "10.0.0.0/8"),
                ],
                "connector origin",
            ),
        ] {
            let error = RuntimeSettings::detached(settings(&values)).err().unwrap();
            assert!(error.contains(message), "{error} / {message}");
        }

        let defaults = config(&[("NOTIFICATION_WEBHOOK_URL", "https://hooks.example/notify")]);
        assert!(!defaults.events().contains(&EventKind::ClickMilestone));
        assert!(defaults.events().contains(&EventKind::Security));
        assert_eq!(defaults.expiry_notice_hours(), 24);
        assert_eq!(defaults.origin(), "https://hooks.example");
        let legacy = config(&[("MONITOR_WEBHOOK_URL", "https://hooks.example/health")]);
        assert_eq!(legacy.events(), [EventKind::LinkHealth]);
        let milestones = config(&[
            ("NOTIFICATION_WEBHOOK_URL", "https://hooks.example/"),
            ("NOTIFICATION_CLICK_MILESTONES", "1000, 10,10"),
        ]);
        assert_eq!(milestones.milestones(), [10, 1000]);
        assert!(milestones.events().contains(&EventKind::ClickMilestone));
        let internal = config(&[
            ("NOTIFICATION_WEBHOOK_URL", "https://hooks.lan:8443/notify"),
            ("NOTIFICATION_CONNECTOR_ORIGIN", "https://hooks.lan:8443"),
            ("NOTIFICATION_CONNECTOR_NETWORKS", "10.20.0.0/16"),
        ]);
        assert!(internal.uses_connector());
    }

    #[tokio::test]
    async fn events_are_detected_once_from_durable_state() {
        let (directory, pool) = database().await;
        let notifier = notifier(&[
            ("NOTIFICATION_WEBHOOK_URL", "https://hooks.example/"),
            ("NOTIFICATION_CLICK_MILESTONES", "10"),
            ("NOTIFICATION_EXPIRY_NOTICE_HOURS", "2"),
        ]);
        // History before activation does not produce notifications.
        sqlx::raw_sql(
            "INSERT INTO admin_audit (action, actor, actor_id, outcome) VALUES ('key_create', 'admin', 1, 'success');",
        )
        .execute(&pool)
        .await
        .unwrap();
        start(&pool).await.unwrap();
        sqlx::raw_sql(
            "UPDATE notification_state SET not_before = unixepoch() - 7200;
             INSERT INTO links (id, host, slug, target) VALUES
                (1, 'go.example', 'soon', 'https://example.test/1'),
                (2, 'go.example', 'gone', 'https://example.test/2'),
                (3, 'go.example', 'later', 'https://example.test/3'),
                (4, 'go.example', 'popular', 'https://example.test/4'),
                (5, 'go.example', 'old', 'https://example.test/5');
             INSERT INTO link_redirect_policies (link_id, status, expires_at, forward_query, forward_path, rules) VALUES
                (1, 302, unixepoch() + 3600, 0, 0, '[]'),
                (2, 302, unixepoch() - 60, 0, 0, '[]'),
                (3, 302, unixepoch() + 86400, 0, 0, '[]'),
                (5, 302, unixepoch() - 86400, 0, 0, '[]');
             INSERT INTO analytics_daily (day, host, link_id, kind, visit_class, total) VALUES
                (unixepoch() / 86400 - 1, 'go.example', 4, 'redirect', 'human', 6),
                (unixepoch() / 86400, 'go.example', 4, 'redirect', 'human', 5),
                (unixepoch() / 86400 - 2, 'go.example', 3, 'redirect', 'human', 50);
             INSERT INTO admin_audit (action, actor, actor_id, outcome) VALUES
                ('login', 'admin', 1, 'success'),
                ('key_rotate', 'admin', 1, 'success'),
                ('backup_authorize', 'admin', 1, 'rejected');",
        )
        .execute(&pool)
        .await
        .unwrap();
        for _ in 0..2 {
            notifier.detect(&pool, directory.path(), 3).await.unwrap();
        }
        let detected: Vec<_> = rows(&pool).await.into_iter().map(|row| row.1).collect();
        let expected_prefixes = [
            "link_expiring:1:",
            "link_expired:2:",
            "click_milestone:4:10",
            "job_failed:analytics:",
            "security:",
            "security:",
        ];
        assert_eq!(detected.len(), expected_prefixes.len(), "{detected:?}");
        for prefix in expected_prefixes {
            assert!(
                detected.iter().any(|key| key.starts_with(prefix)),
                "{prefix} in {detected:?}"
            );
        }
        let actions: Vec<String> = sqlx::query_scalar(
            "SELECT json_extract(payload, '$.action') FROM notification_outbox WHERE event = 'security' ORDER BY id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(actions, ["key_rotate", "backup_authorize"]);
        let payloads: Vec<String> = sqlx::query_scalar("SELECT payload FROM notification_outbox")
            .fetch_all(&pool)
            .await
            .unwrap();
        for payload in payloads {
            assert!(
                !payload.contains("example.test") && !payload.contains("go.example"),
                "{payload}"
            );
        }
    }

    #[tokio::test]
    async fn detection_reaches_every_candidate_and_restores_start_a_new_id_epoch() {
        let (directory, pool) = database().await;
        let notifier = notifier(&[
            ("NOTIFICATION_WEBHOOK_URL", "https://hooks.example/"),
            ("NOTIFICATION_EVENTS", "link_expiring"),
        ]);
        start(&pool).await.unwrap();
        sqlx::raw_sql(
            "UPDATE notification_state SET not_before = unixepoch() - 86400;
             WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 150)
             INSERT INTO links (id, host, slug, target)
             SELECT i, 'go.example', 'expiring-' || i, 'https://example.test/' || i FROM n;
             INSERT INTO link_redirect_policies (link_id, status, expires_at, forward_query, forward_path, rules)
             SELECT id, 302, unixepoch() + 3600, 0, 0, '[]' FROM links;",
        )
        .execute(&pool)
        .await
        .unwrap();
        notifier.detect(&pool, directory.path(), 0).await.unwrap();
        notifier.detect(&pool, directory.path(), 0).await.unwrap();
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM notification_outbox WHERE event = 'link_expiring'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count, 150);
        let epoch = || async {
            sqlx::query_scalar::<_, String>("SELECT epoch FROM notification_state")
                .fetch_one(&pool)
                .await
                .unwrap()
        };
        let before = epoch().await;
        assert_eq!(before.len(), 32);
        after_restore(&pool).await.unwrap();
        assert_ne!(epoch().await, before);
        let pending: i64 =
            sqlx::query_scalar("SELECT count(*) FROM notification_outbox WHERE status = 'pending'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(pending, 0);
    }

    #[tokio::test]
    async fn failed_deliveries_back_off_then_stop_and_restores_discard_pending_rows() {
        let (_directory, pool) = database().await;
        // The literal private address is refused by the public policy before any connection.
        let refused = notifier(&[("NOTIFICATION_WEBHOOK_URL", "https://10.0.0.1/notify")]);
        refused
            .record(&pool, Event::Test { nonce: 1 })
            .await
            .unwrap();
        refused
            .record(&pool, Event::Test { nonce: 1 })
            .await
            .unwrap();
        refused.deliver_due(&pool).await.unwrap();
        let state: (String, i64, Option<String>) =
            sqlx::query_as("SELECT status, attempts, last_error FROM notification_outbox")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(state, ("failed".to_owned(), 1, Some("blocked".to_owned())));

        // A transient failure is retried with growing delays until the attempt limit.
        let unreachable = notifier(&[("NOTIFICATION_WEBHOOK_URL", "https://hooks.invalid/notify")]);
        unreachable
            .record(&pool, Event::Test { nonce: 2 })
            .await
            .unwrap();
        let mut delays = Vec::new();
        for _ in 0..MAX_ATTEMPTS {
            sqlx::query(
                "UPDATE notification_outbox SET next_attempt_at = 0 WHERE status = 'pending'",
            )
            .execute(&pool)
            .await
            .unwrap();
            unreachable.deliver_due(&pool).await.unwrap();
            delays.push(
                sqlx::query_scalar::<_, i64>(
                    "SELECT next_attempt_at - unixepoch() FROM notification_outbox WHERE event_key = ?",
                )
                .bind(format!("test:{:032x}", 2))
                .fetch_one(&pool)
                .await
                .unwrap(),
            );
        }
        assert!(
            delays.windows(2).all(|pair| pair[0] <= pair[1] + 2),
            "{delays:?}"
        );
        assert!(
            delays
                .iter()
                .all(|delay| *delay <= MAX_BACKOFF_SECONDS + 30)
        );
        let state: (String, i64) =
            sqlx::query_as("SELECT status, attempts FROM notification_outbox WHERE event_key = ?")
                .bind(format!("test:{:032x}", 2))
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(state, ("failed".to_owned(), MAX_ATTEMPTS));

        // Rows pending in a restored snapshot are discarded, and stale rows are not sent.
        unreachable
            .record(&pool, Event::Test { nonce: 3 })
            .await
            .unwrap();
        after_restore(&pool).await.unwrap();
        unreachable
            .record(&pool, Event::Test { nonce: 4 })
            .await
            .unwrap();
        sqlx::query(
            "UPDATE notification_outbox SET created_at = unixepoch() - 90000 WHERE event_key = ?",
        )
        .bind(format!("test:{:032x}", 4))
        .execute(&pool)
        .await
        .unwrap();
        unreachable.deliver_due(&pool).await.unwrap();
        let settled: Vec<(String, Option<String>)> = sqlx::query_as(
            "SELECT status, last_error FROM notification_outbox WHERE event_key IN (?, ?) ORDER BY id",
        )
        .bind(format!("test:{:032x}", 3))
        .bind(format!("test:{:032x}", 4))
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            settled,
            [
                ("discarded".to_owned(), Some("restored".to_owned())),
                ("failed".to_owned(), Some("stale".to_owned()))
            ]
        );
        // Disabled event kinds are not recorded.
        let legacy = notifier(&[("MONITOR_WEBHOOK_URL", "https://hooks.example/")]);
        legacy
            .record(&pool, Event::BackupFailed { at: 5 })
            .await
            .unwrap();
        assert!(rows(&pool).await.iter().all(|row| row.0 != "backup_failed"));
    }
}
