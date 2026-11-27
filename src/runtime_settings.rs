//! Operational settings that Settings > Operations can change without a restart.
//!
//! Each setting resolves in one order: a value saved in the web interface, otherwise the
//! container variable, otherwise the built-in default. Saving makes the web interface
//! authoritative for that setting until it is reset. Nothing is saved after an upgrade, so
//! existing installations keep their container values.
//!
//! The effective settings are published through a watch channel. The monitor, title lookup
//! with its preview images, backup scheduler, and notifier read the current value on each pass
//! and wake on a change.
//! Connector infrastructure, hosts, proxies, rate limits, listeners, and the log level stay
//! environment-only.

use std::{collections::BTreeSet, sync::Arc};

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tokio::sync::{Mutex, watch};

use crate::{
    admin::admin_security::{AuditAction, AuditActor, AuditOutcome, record_audit_details},
    notifications::{
        self, EventKind, NotificationConfig, NotificationEnv, default_events, event_name,
        parse_event,
    },
    outbound::valid_authorization,
};

const DEFAULT_MONITOR_INTERVAL: u16 = 24;
const DEFAULT_PRIORITY_CAP: u16 = 6;
/// Daily scheduled backups, keeping a week. The Unraid template used to set the same values.
const DEFAULT_BACKUP_INTERVAL: u16 = 24;
const DEFAULT_BACKUP_KEEP: u16 = 7;
const DEFAULT_EXPIRY_NOTICE: u16 = 24;
const MAX_MILESTONES: usize = 10;
const MAX_MILESTONE: i64 = 1_000_000_000_000;

/// Where an effective value comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Source {
    Saved,
    Container,
    Default,
}

/// One effective value, its source, and the container value a reset would return to.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct Setting<T> {
    pub(crate) value: T,
    pub(crate) source: Source,
    pub(crate) container: Option<T>,
}

/// A secret setting. Only whether it is set, and for the webhook its origin, is ever shown.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct Secret {
    pub(crate) configured: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) origin: Option<String>,
    pub(crate) source: Source,
    pub(crate) container_configured: bool,
}

/// Container values, validated once at startup. Invalid values stop the start as before.
pub struct OperationsEnvironment {
    monitor_enabled: Option<bool>,
    monitor_interval: Option<u16>,
    monitor_priority: Option<u16>,
    title_lookup: Option<bool>,
    preview_images: Option<bool>,
    backup_interval: Option<u16>,
    backup_keep: Option<u16>,
    notifications: NotificationEnv,
}

impl OperationsEnvironment {
    pub fn from_env(read: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        let flag = |name: &str| -> Result<Option<bool>, String> {
            match read(name).as_deref() {
                None => Ok(None),
                Some("true") => Ok(Some(true)),
                Some("false") => Ok(Some(false)),
                Some(_) => Err(format!("{name} must be true or false")),
            }
        };
        let number = |name: &str| -> Result<Option<u16>, String> {
            read(name)
                .map(|value| value.parse::<u16>().map_err(|_| format!("invalid {name}")))
                .transpose()
        };
        let monitor_interval = number("MONITOR_INTERVAL_HOURS")?;
        let monitor_priority = number("MONITOR_PRIORITY_HOURS")?;
        let interval = monitor_interval.unwrap_or(DEFAULT_MONITOR_INTERVAL);
        let priority = monitor_priority.unwrap_or(interval.min(DEFAULT_PRIORITY_CAP));
        if !valid_monitor(interval, priority) {
            return Err(
                "monitor intervals require 1 <= priority hours <= regular hours <= 720".into(),
            );
        }
        let backup_interval = number("BACKUP_INTERVAL_HOURS")?;
        if backup_interval.is_some_and(|hours| hours > 720) {
            return Err("scheduled backup interval must be between 1 and 720 hours".into());
        }
        let mut backup_keep = number("BACKUP_KEEP")?;
        if backup_keep.is_some_and(|keep| !(1..=365).contains(&keep)) {
            // Disabled schedules never checked retention, so keep accepting them unchanged.
            if backup_interval == Some(0) {
                tracing::warn!("ignoring BACKUP_KEEP outside 1 to 365 while backups are off");
                backup_keep = None;
            } else {
                return Err(
                    "scheduled backup retention must be between 1 and 365 snapshots".into(),
                );
            }
        }
        Ok(Self {
            monitor_enabled: flag("MONITOR_ENABLED")?,
            monitor_interval,
            monitor_priority,
            title_lookup: flag("TITLE_LOOKUP_ENABLED")?,
            preview_images: flag("PREVIEW_IMAGES_ENABLED")?,
            backup_interval,
            backup_keep,
            notifications: NotificationEnv::from_env(&read)?,
        })
    }
}

fn valid_monitor(interval: u16, priority: u16) -> bool {
    (1..=720).contains(&interval) && (1..=interval).contains(&priority)
}

/// The saved row. `None` means not saved here.
#[derive(Clone, Debug, Default, PartialEq, sqlx::FromRow)]
struct Saved {
    monitor_enabled: Option<bool>,
    monitor_interval_hours: Option<u16>,
    monitor_priority_hours: Option<u16>,
    title_lookup_enabled: Option<bool>,
    preview_images_enabled: Option<bool>,
    backup_interval_hours: Option<u16>,
    backup_keep: Option<u16>,
    notification_webhook_url: Option<String>,
    notification_authorization: Option<String>,
    notification_events: Option<String>,
    notification_click_milestones: Option<String>,
    notification_expiry_notice_hours: Option<u16>,
}

/// The effective settings that the background jobs and the admin API read.
pub(crate) struct Effective {
    pub(crate) monitor_enabled: Setting<bool>,
    pub(crate) monitor_interval_hours: Setting<u16>,
    pub(crate) monitor_priority_hours: Setting<u16>,
    pub(crate) title_lookup_enabled: Setting<bool>,
    /// Effective only while title lookup is on too; see `previews_active`.
    pub(crate) preview_images_enabled: Setting<bool>,
    pub(crate) backup_interval_hours: Setting<u16>,
    pub(crate) backup_keep: Setting<u16>,
    webhook_url: Option<String>,
    webhook: Secret,
    authorization: Secret,
    events: Setting<Vec<EventKind>>,
    click_milestones: Setting<Vec<i64>>,
    expiry_notice_hours: Setting<u16>,
    connector: bool,
    /// The active notification configuration, or `None` while notifications are off.
    pub(crate) notifications: Option<Arc<NotificationConfig>>,
}

impl Effective {
    /// Whether preview images are fetched. They come from the title lookup page request, so
    /// they need both switches.
    pub(crate) fn previews_active(&self) -> bool {
        self.title_lookup_enabled.value && self.preview_images_enabled.value
    }
}

/// The API representation. It never contains the webhook URL or the authorization value.
#[derive(Serialize)]
pub(crate) struct View {
    monitor_enabled: Setting<bool>,
    monitor_interval_hours: Setting<u16>,
    monitor_priority_hours: Setting<u16>,
    title_lookup_enabled: Setting<bool>,
    preview_images_enabled: Setting<bool>,
    /// Whether previews are fetched: both preview images and title lookup are on.
    preview_images_active: bool,
    backup_interval_hours: Setting<u16>,
    backup_keep: Setting<u16>,
    notification_webhook: Secret,
    notification_authorization: Secret,
    notification_events: Setting<Vec<EventKind>>,
    notification_click_milestones: Setting<Vec<i64>>,
    notification_expiry_notice_hours: Setting<u16>,
    notifications_active: bool,
    connector: bool,
}

fn pick<T: Clone>(saved: Option<T>, container: Option<T>, default: T) -> Setting<T> {
    let (value, source) = match (saved, &container) {
        (Some(value), _) => (value, Source::Saved),
        (None, Some(value)) => (value.clone(), Source::Container),
        (None, None) => (default, Source::Default),
    };
    Setting {
        value,
        source,
        container,
    }
}

fn parse_events(list: &str) -> Option<BTreeSet<EventKind>> {
    list.split(',').map(parse_event).collect()
}

fn join_events(events: &BTreeSet<EventKind>) -> String {
    events
        .iter()
        .map(|kind| event_name(*kind))
        .collect::<Vec<_>>()
        .join(",")
}

fn parse_saved_milestones(list: &str) -> Option<Vec<i64>> {
    if list.is_empty() {
        return Some(Vec::new());
    }
    notifications::parse_milestones(list).ok()
}

/// Resolves saved and container values. With `strict`, an invalid combination is an error, as
/// for a save. Otherwise it is repaired conservatively and logged, so a changed container value
/// cannot prevent the start.
fn resolve(
    env: &OperationsEnvironment,
    saved: &Saved,
    strict: bool,
) -> Result<Effective, &'static str> {
    let monitor_enabled = pick(saved.monitor_enabled, env.monitor_enabled, false);
    let monitor_interval_hours = pick(
        saved.monitor_interval_hours,
        env.monitor_interval,
        DEFAULT_MONITOR_INTERVAL,
    );
    let interval = monitor_interval_hours.value;
    let mut monitor_priority_hours = pick(
        saved.monitor_priority_hours,
        env.monitor_priority,
        interval.min(DEFAULT_PRIORITY_CAP),
    );
    if !valid_monitor(interval, monitor_priority_hours.value) {
        if strict {
            return Err("invalid_monitor_intervals");
        }
        tracing::warn!(
            "the priority health check interval exceeds the regular one; using the regular interval"
        );
        monitor_priority_hours.value = interval;
    }
    let title_lookup_enabled = pick(saved.title_lookup_enabled, env.title_lookup, false);
    // Off by default: every preview is an outgoing request that must be turned on explicitly.
    let preview_images_enabled = pick(saved.preview_images_enabled, env.preview_images, false);
    if !strict && preview_images_enabled.value && !title_lookup_enabled.value {
        tracing::warn!("preview images need title lookup; previews stay off until it is on");
    }
    let backup_interval_hours = pick(
        saved.backup_interval_hours,
        env.backup_interval,
        DEFAULT_BACKUP_INTERVAL,
    );
    let backup_keep = pick(saved.backup_keep, env.backup_keep, DEFAULT_BACKUP_KEEP);

    let notification = &env.notifications;
    let webhook_setting = pick(
        saved
            .notification_webhook_url
            .clone()
            .map(|url| Some(url).filter(|url| !url.is_empty())),
        notification.webhook.clone().map(Some),
        None,
    );
    let webhook_url = webhook_setting.value.clone();
    let legacy = webhook_setting.source == Source::Container && notification.legacy;
    let authorization = pick(
        saved
            .notification_authorization
            .clone()
            .map(|value| Some(value).filter(|value| !value.is_empty())),
        notification.authorization.clone().map(Some),
        None,
    );
    let saved_milestones = saved
        .notification_click_milestones
        .as_deref()
        .and_then(parse_saved_milestones);
    let click_milestones = pick(
        saved_milestones,
        notification.milestones.clone(),
        Vec::new(),
    );
    let saved_events = saved.notification_events.as_deref().and_then(parse_events);
    let container_events = notification.events.clone();
    let mut events = pick(
        saved_events,
        container_events,
        default_events(legacy, !click_milestones.value.is_empty()),
    );
    if events.value.contains(&EventKind::ClickMilestone) && click_milestones.value.is_empty() {
        if strict {
            return Err("click_milestones_required");
        }
        tracing::warn!("click milestone notifications need milestones; skipping that event");
        events.value.remove(&EventKind::ClickMilestone);
    }
    let expiry_notice_hours = pick(
        saved.notification_expiry_notice_hours,
        notification.expiry_hours,
        DEFAULT_EXPIRY_NOTICE,
    );
    let config = match &webhook_url {
        Some(url) => match NotificationConfig::new(
            url,
            authorization.value.as_deref(),
            notification.connector.clone(),
            events.value.clone(),
            click_milestones.value.clone(),
            expiry_notice_hours.value,
        ) {
            Ok(config) => Some(Arc::new(config)),
            Err(_) if strict => return Err("invalid_notification_webhook"),
            Err(_) => {
                tracing::warn!("the saved notification webhook is invalid; notifications are off");
                None
            }
        },
        None => None,
    };
    let webhook = Secret {
        configured: webhook_url.is_some(),
        origin: config.as_ref().map(|config| config.origin()),
        source: webhook_setting.source,
        container_configured: notification.webhook.is_some(),
    };
    let authorization = Secret {
        configured: authorization.value.is_some(),
        origin: None,
        source: authorization.source,
        container_configured: notification.authorization.is_some(),
    };
    let list = |setting: Setting<BTreeSet<EventKind>>| Setting {
        value: setting.value.into_iter().collect(),
        source: setting.source,
        container: setting.container.map(|events| events.into_iter().collect()),
    };
    Ok(Effective {
        monitor_enabled,
        monitor_interval_hours,
        monitor_priority_hours,
        title_lookup_enabled,
        preview_images_enabled,
        backup_interval_hours,
        backup_keep,
        webhook_url,
        webhook,
        authorization,
        events: list(events),
        click_milestones,
        expiry_notice_hours,
        connector: notification.connector.is_some(),
        notifications: config,
    })
}

/// A requested change. Fields that are present are saved; `reset` names fields whose saved value
/// is deleted, so the container value or the default applies again.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Patch {
    monitor_enabled: Option<bool>,
    monitor_interval_hours: Option<i64>,
    monitor_priority_hours: Option<i64>,
    title_lookup_enabled: Option<bool>,
    /// Requires title lookup to be on when it is turned on.
    preview_images_enabled: Option<bool>,
    backup_interval_hours: Option<i64>,
    backup_keep: Option<i64>,
    /// An HTTPS URL, or an empty string to turn notifications off.
    notification_webhook_url: Option<String>,
    /// An `Authorization` header value, or an empty string for none.
    notification_authorization: Option<String>,
    notification_events: Option<Vec<String>>,
    notification_click_milestones: Option<Vec<i64>>,
    notification_expiry_notice_hours: Option<i64>,
    #[serde(default)]
    reset: Vec<String>,
}

const FIELDS: [&str; 12] = [
    "monitor_enabled",
    "monitor_interval_hours",
    "monitor_priority_hours",
    "title_lookup_enabled",
    "preview_images_enabled",
    "backup_interval_hours",
    "backup_keep",
    "notification_webhook_url",
    "notification_authorization",
    "notification_events",
    "notification_click_milestones",
    "notification_expiry_notice_hours",
];

fn hours(
    value: Option<i64>,
    range: std::ops::RangeInclusive<u16>,
    code: &'static str,
) -> Result<Option<u16>, &'static str> {
    value
        .map(|value| {
            u16::try_from(value)
                .ok()
                .filter(|value| range.contains(value))
                .ok_or(code)
        })
        .transpose()
}

impl Patch {
    /// Whether the change touches the notification target or its authorization, which requires
    /// a fresh password confirmation.
    pub(crate) fn requires_confirmation(&self) -> bool {
        self.notification_webhook_url.is_some()
            || self.notification_authorization.is_some()
            || self.reset.iter().any(|field| {
                field == "notification_webhook_url" || field == "notification_authorization"
            })
    }

    /// Validates each field on its own and applies the change to a saved row.
    fn apply(&self, mut saved: Saved) -> Result<Saved, &'static str> {
        let mut reset = BTreeSet::new();
        for field in &self.reset {
            let field = FIELDS
                .into_iter()
                .find(|name| name == field)
                .ok_or("invalid_reset")?;
            if !reset.insert(field) {
                return Err("invalid_reset");
            }
        }
        let set = |name: &str, present: bool| -> Result<bool, &'static str> {
            if present && reset.contains(name) {
                return Err("invalid_reset");
            }
            Ok(present || reset.contains(name))
        };
        if set("monitor_enabled", self.monitor_enabled.is_some())? {
            saved.monitor_enabled = self.monitor_enabled;
        }
        if set(
            "monitor_interval_hours",
            self.monitor_interval_hours.is_some(),
        )? {
            saved.monitor_interval_hours = hours(
                self.monitor_interval_hours,
                1..=720,
                "invalid_monitor_intervals",
            )?;
        }
        if set(
            "monitor_priority_hours",
            self.monitor_priority_hours.is_some(),
        )? {
            saved.monitor_priority_hours = hours(
                self.monitor_priority_hours,
                1..=720,
                "invalid_monitor_intervals",
            )?;
        }
        if set("title_lookup_enabled", self.title_lookup_enabled.is_some())? {
            saved.title_lookup_enabled = self.title_lookup_enabled;
        }
        if set(
            "preview_images_enabled",
            self.preview_images_enabled.is_some(),
        )? {
            saved.preview_images_enabled = self.preview_images_enabled;
        }
        if set(
            "backup_interval_hours",
            self.backup_interval_hours.is_some(),
        )? {
            saved.backup_interval_hours = hours(
                self.backup_interval_hours,
                0..=720,
                "invalid_backup_interval",
            )?;
        }
        if set("backup_keep", self.backup_keep.is_some())? {
            saved.backup_keep = hours(self.backup_keep, 1..=365, "invalid_backup_keep")?;
        }
        if set(
            "notification_webhook_url",
            self.notification_webhook_url.is_some(),
        )? {
            saved.notification_webhook_url = match self.notification_webhook_url.as_deref() {
                Some(url) if !url.is_empty() && !url.starts_with("https://") => {
                    return Err("invalid_notification_webhook");
                }
                other => other.map(str::to_owned),
            };
        }
        if set(
            "notification_authorization",
            self.notification_authorization.is_some(),
        )? {
            saved.notification_authorization = match self.notification_authorization.as_deref() {
                Some(value) if !value.is_empty() && !valid_authorization(value) => {
                    return Err("invalid_notification_authorization");
                }
                other => other.map(str::to_owned),
            };
        }
        if set("notification_events", self.notification_events.is_some())? {
            saved.notification_events = match &self.notification_events {
                Some(names) => {
                    let events = names
                        .iter()
                        .map(|name| parse_event(name))
                        .collect::<Option<BTreeSet<_>>>()
                        .filter(|events| !events.is_empty())
                        .ok_or("invalid_notification_events")?;
                    Some(join_events(&events))
                }
                None => None,
            };
        }
        if set(
            "notification_click_milestones",
            self.notification_click_milestones.is_some(),
        )? {
            saved.notification_click_milestones = match &self.notification_click_milestones {
                Some(values) => {
                    let mut values = values.clone();
                    if values
                        .iter()
                        .any(|value| !(1..=MAX_MILESTONE).contains(value))
                    {
                        return Err("invalid_click_milestones");
                    }
                    values.sort_unstable();
                    values.dedup();
                    if values.len() > MAX_MILESTONES {
                        return Err("invalid_click_milestones");
                    }
                    Some(
                        values
                            .iter()
                            .map(i64::to_string)
                            .collect::<Vec<_>>()
                            .join(","),
                    )
                }
                None => None,
            };
        }
        if set(
            "notification_expiry_notice_hours",
            self.notification_expiry_notice_hours.is_some(),
        )? {
            saved.notification_expiry_notice_hours = hours(
                self.notification_expiry_notice_hours,
                1..=720,
                "invalid_expiry_notice",
            )?;
        }
        Ok(saved)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SaveError {
    Invalid(&'static str),
    /// The change needs a password confirmation that was not granted.
    Unauthorized,
    Database,
}

impl From<sqlx::Error> for SaveError {
    fn from(_: sqlx::Error) -> Self {
        Self::Database
    }
}

/// The shared runtime settings handle.
#[derive(Clone)]
pub struct RuntimeSettings(Arc<Shared>);

struct Shared {
    env: OperationsEnvironment,
    current: watch::Sender<Arc<Effective>>,
    /// One save at a time, so the published value always matches the database.
    saving: Mutex<()>,
}

const SELECT_SAVED: &str = "SELECT monitor_enabled, monitor_interval_hours, monitor_priority_hours,
        title_lookup_enabled, preview_images_enabled, backup_interval_hours, backup_keep, notification_webhook_url,
        notification_authorization, notification_events, notification_click_milestones,
        notification_expiry_notice_hours
    FROM runtime_settings WHERE id = 1";

impl RuntimeSettings {
    /// Reads the saved values and publishes the effective settings.
    pub async fn load(pool: &SqlitePool, env: OperationsEnvironment) -> Result<Self, sqlx::Error> {
        let saved: Saved = sqlx::query_as(SELECT_SAVED).fetch_one(pool).await?;
        Ok(Self::with_saved(env, &saved))
    }

    fn with_saved(env: OperationsEnvironment, saved: &Saved) -> Self {
        let effective = resolve(&env, saved, false)
            .unwrap_or_else(|_| unreachable!("lenient resolution repairs every combination"));
        Self(Arc::new(Shared {
            env,
            current: watch::Sender::new(Arc::new(effective)),
            saving: Mutex::new(()),
        }))
    }

    /// Settings from container values only, for tests of the background jobs.
    #[cfg(test)]
    pub(crate) fn detached(read: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        Ok(Self::with_saved(
            OperationsEnvironment::from_env(read)?,
            &Saved::default(),
        ))
    }

    pub(crate) fn current(&self) -> Arc<Effective> {
        self.0.current.borrow().clone()
    }

    pub(crate) fn subscribe(&self) -> watch::Receiver<Arc<Effective>> {
        self.0.current.subscribe()
    }

    pub(crate) fn view(&self) -> View {
        let effective = self.current();
        View {
            monitor_enabled: effective.monitor_enabled.clone(),
            monitor_interval_hours: effective.monitor_interval_hours.clone(),
            monitor_priority_hours: effective.monitor_priority_hours.clone(),
            title_lookup_enabled: effective.title_lookup_enabled.clone(),
            preview_images_enabled: effective.preview_images_enabled.clone(),
            preview_images_active: effective.previews_active(),
            backup_interval_hours: effective.backup_interval_hours.clone(),
            backup_keep: effective.backup_keep.clone(),
            notification_webhook: effective.webhook.clone(),
            notification_authorization: effective.authorization.clone(),
            notification_events: effective.events.clone(),
            notification_click_milestones: effective.click_milestones.clone(),
            notification_expiry_notice_hours: effective.expiry_notice_hours.clone(),
            notifications_active: effective.notifications.is_some(),
            connector: effective.connector,
        }
    }

    /// Validates and saves a change, records it in the audit log, and publishes the result.
    /// `confirm` runs after validation and before anything is written; it consumes the password
    /// confirmation when the change requires one.
    pub(crate) async fn save(
        &self,
        pool: &SqlitePool,
        patch: &Patch,
        confirm: impl FnOnce() -> bool,
    ) -> Result<(), SaveError> {
        let _saving = self.0.saving.lock().await;
        let mut transaction = pool.begin_with("BEGIN IMMEDIATE").await?;
        let saved: Saved = sqlx::query_as(SELECT_SAVED)
            .fetch_one(&mut *transaction)
            .await?;
        let next = patch.apply(saved.clone()).map_err(SaveError::Invalid)?;
        let effective = resolve(&self.0.env, &next, true).map_err(SaveError::Invalid)?;
        // Previews come from the title lookup page request. Turning them on while title lookup
        // stays off would do nothing, so say so. Turning title lookup off later only pauses them.
        if patch.preview_images_enabled == Some(true) && !effective.title_lookup_enabled.value {
            return Err(SaveError::Invalid("title_lookup_required"));
        }
        // Delivery would refuse a private literal address; say so when it is entered.
        if patch
            .notification_webhook_url
            .as_deref()
            .is_some_and(|url| !url.is_empty())
            && effective
                .notifications
                .as_ref()
                .is_some_and(|config| config.webhook().targets_private_literal())
        {
            return Err(SaveError::Invalid("notification_webhook_not_public"));
        }
        if patch.requires_confirmation() && !confirm() {
            return Err(SaveError::Unauthorized);
        }
        let changed = changed_fields(&saved, &next);
        if changed.is_empty() {
            return Ok(());
        }
        sqlx::query(
            "UPDATE runtime_settings SET monitor_enabled = ?, monitor_interval_hours = ?,
                 monitor_priority_hours = ?, title_lookup_enabled = ?, preview_images_enabled = ?,
                 backup_interval_hours = ?, backup_keep = ?, notification_webhook_url = ?, notification_authorization = ?,
                 notification_events = ?, notification_click_milestones = ?,
                 notification_expiry_notice_hours = ?, updated_at = unixepoch()
             WHERE id = 1",
        )
        .bind(next.monitor_enabled)
        .bind(next.monitor_interval_hours)
        .bind(next.monitor_priority_hours)
        .bind(next.title_lookup_enabled)
        .bind(next.preview_images_enabled)
        .bind(next.backup_interval_hours)
        .bind(next.backup_keep)
        .bind(&next.notification_webhook_url)
        .bind(&next.notification_authorization)
        .bind(&next.notification_events)
        .bind(&next.notification_click_milestones)
        .bind(next.notification_expiry_notice_hours)
        .execute(&mut *transaction)
        .await?;
        let previous = self.current();
        if previous.webhook_url != effective.webhook_url {
            // Before the audit entry, so a new target is told about this change.
            notifications::target_changed(&mut transaction).await?;
        }
        let intervals = (
            effective.monitor_interval_hours.value,
            effective.monitor_priority_hours.value,
        );
        if intervals.0 < previous.monitor_interval_hours.value
            || intervals.1 < previous.monitor_priority_hours.value
        {
            reschedule_sooner(&mut transaction, intervals).await?;
        }
        record_audit_details(
            &mut transaction,
            AuditAction::OperationsSettingsUpdate,
            AuditActor::Admin,
            None,
            AuditOutcome::Success,
            Some(&serde_json::json!({ "changed": changed })),
        )
        .await?;
        transaction.commit().await?;
        self.0.current.send_replace(Arc::new(effective));
        Ok(())
    }
}

/// The names of saved columns that a change sets or resets. Values are never listed.
fn changed_fields(before: &Saved, after: &Saved) -> Vec<&'static str> {
    let pairs = [
        before.monitor_enabled != after.monitor_enabled,
        before.monitor_interval_hours != after.monitor_interval_hours,
        before.monitor_priority_hours != after.monitor_priority_hours,
        before.title_lookup_enabled != after.title_lookup_enabled,
        before.preview_images_enabled != after.preview_images_enabled,
        before.backup_interval_hours != after.backup_interval_hours,
        before.backup_keep != after.backup_keep,
        before.notification_webhook_url != after.notification_webhook_url,
        before.notification_authorization != after.notification_authorization,
        before.notification_events != after.notification_events,
        before.notification_click_milestones != after.notification_click_milestones,
        before.notification_expiry_notice_hours != after.notification_expiry_notice_hours,
    ];
    FIELDS
        .into_iter()
        .zip(pairs)
        .filter_map(|(name, changed)| changed.then_some(name))
        .collect()
}

/// A shorter interval moves later checks forward. Longer intervals apply after the next check,
/// and failing links keep their retry backoff.
async fn reschedule_sooner(
    connection: &mut sqlx::SqliteConnection,
    (interval, priority): (u16, u16),
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE link_monitor SET next_check = coalesce(last_checked, unixepoch())
             + CASE WHEN priority = 1 THEN ?1 ELSE ?2 END + link_id % 301
         WHERE consecutive_failures = 0 AND next_check > coalesce(last_checked, unixepoch())
             + CASE WHEN priority = 1 THEN ?1 ELSE ?2 END + link_id % 301",
    )
    .bind(i64::from(priority) * 3600)
    .bind(i64::from(interval) * 3600)
    .execute(connection)
    .await?;
    Ok(())
}

/// Removes saved values from a verified snapshot that would activate outgoing requests or that
/// are secrets: the health check, title lookup, and preview image switches, the webhook URL, and
/// its authorization. A restored installation then uses its own container values for them until
/// an administrator saves them again. Older schemas without the table are left unchanged.
pub(crate) async fn scrub_snapshot(
    connection: &mut sqlx::SqliteConnection,
) -> Result<(), sqlx::Error> {
    let present: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = 'runtime_settings')",
    )
    .fetch_one(&mut *connection)
    .await?;
    if present {
        sqlx::query(
            "UPDATE runtime_settings SET monitor_enabled = NULL, title_lookup_enabled = NULL,
                 notification_webhook_url = NULL, notification_authorization = NULL",
        )
        .execute(&mut *connection)
        .await?;
        let previews: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM pragma_table_info('runtime_settings')
                            WHERE name = 'preview_images_enabled')",
        )
        .fetch_one(&mut *connection)
        .await?;
        if previews {
            sqlx::query("UPDATE runtime_settings SET preview_images_enabled = NULL")
                .execute(connection)
                .await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn env(values: &[(&str, &str)]) -> OperationsEnvironment {
        let values: HashMap<String, String> = values
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect();
        OperationsEnvironment::from_env(move |name| values.get(name).cloned()).unwrap()
    }

    fn env_error(values: &[(&str, &str)]) -> String {
        let values: HashMap<String, String> = values
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect();
        OperationsEnvironment::from_env(move |name| values.get(name).cloned())
            .err()
            .unwrap()
    }

    #[test]
    fn defaults_then_container_values_then_saved_values() {
        let defaults = resolve(&env(&[]), &Saved::default(), true).unwrap();
        assert!(!defaults.monitor_enabled.value);
        assert_eq!(defaults.monitor_enabled.source, Source::Default);
        assert_eq!(defaults.monitor_interval_hours.value, 24);
        assert_eq!(defaults.monitor_priority_hours.value, 6);
        assert_eq!(defaults.backup_interval_hours.value, 24);
        assert_eq!(defaults.backup_keep.value, 7);
        assert!(defaults.notifications.is_none());

        let container = env(&[
            ("MONITOR_ENABLED", "true"),
            ("MONITOR_INTERVAL_HOURS", "4"),
            ("BACKUP_INTERVAL_HOURS", "0"),
            ("MONITOR_WEBHOOK_URL", "https://hooks.example/health"),
        ]);
        let from_container = resolve(&container, &Saved::default(), true).unwrap();
        assert_eq!(from_container.monitor_enabled.source, Source::Container);
        assert_eq!(from_container.monitor_priority_hours.value, 4);
        assert_eq!(from_container.backup_interval_hours.value, 0);
        assert_eq!(from_container.events.value, [EventKind::LinkHealth]);
        assert_eq!(
            from_container.webhook.origin.as_deref(),
            Some("https://hooks.example")
        );

        let saved = Saved {
            monitor_enabled: Some(false),
            notification_webhook_url: Some(String::new()),
            ..Saved::default()
        };
        let overridden = resolve(&container, &saved, true).unwrap();
        assert!(!overridden.monitor_enabled.value);
        assert_eq!(overridden.monitor_enabled.source, Source::Saved);
        assert_eq!(overridden.monitor_enabled.container, Some(true));
        assert!(overridden.notifications.is_none());
        assert_eq!(overridden.webhook.source, Source::Saved);
        assert!(overridden.webhook.container_configured);
    }

    #[test]
    fn container_values_keep_their_startup_validation() {
        for (values, message) in [
            (vec![("MONITOR_ENABLED", "yes")], "true or false"),
            (vec![("MONITOR_INTERVAL_HOURS", "0")], "priority hours"),
            (vec![("MONITOR_PRIORITY_HOURS", "30")], "priority hours"),
            (vec![("BACKUP_INTERVAL_HOURS", "721")], "720 hours"),
            (vec![("BACKUP_KEEP", "0")], "365 snapshots"),
            (vec![("TITLE_LOOKUP_ENABLED", "1")], "true or false"),
        ] {
            let error = env_error(&values);
            assert!(error.contains(message), "{error} / {message}");
        }
        // A disabled schedule never checked retention.
        env(&[("BACKUP_INTERVAL_HOURS", "0"), ("BACKUP_KEEP", "0")]);
    }

    #[test]
    fn saves_are_validated_and_repairs_apply_only_at_load() {
        let container = env(&[("MONITOR_PRIORITY_HOURS", "6")]);
        let short = Saved {
            monitor_interval_hours: Some(2),
            ..Saved::default()
        };
        assert_eq!(
            resolve(&container, &short, true).err(),
            Some("invalid_monitor_intervals")
        );
        assert_eq!(
            resolve(&container, &short, false)
                .unwrap()
                .monitor_priority_hours
                .value,
            2
        );
        let milestones = Saved {
            notification_events: Some("click_milestone".into()),
            ..Saved::default()
        };
        assert_eq!(
            resolve(&container, &milestones, true).err(),
            Some("click_milestones_required")
        );
        let invalid = |patch: Patch| patch.apply(Saved::default()).err();
        assert_eq!(
            invalid(Patch {
                backup_keep: Some(366),
                ..Patch::default()
            }),
            Some("invalid_backup_keep")
        );
        assert_eq!(
            invalid(Patch {
                notification_webhook_url: Some("http://hooks.example/".into()),
                ..Patch::default()
            }),
            Some("invalid_notification_webhook")
        );
        assert_eq!(
            invalid(Patch {
                notification_authorization: Some("bad\nvalue".into()),
                ..Patch::default()
            }),
            Some("invalid_notification_authorization")
        );
        assert_eq!(
            invalid(Patch {
                monitor_enabled: Some(true),
                reset: vec!["monitor_enabled".into()],
                ..Patch::default()
            }),
            Some("invalid_reset")
        );
        assert_eq!(
            invalid(Patch {
                notification_events: Some(vec![]),
                ..Patch::default()
            }),
            Some("invalid_notification_events")
        );
        let applied = Patch {
            notification_click_milestones: Some(vec![100, 10, 10]),
            reset: vec!["backup_keep".into()],
            ..Patch::default()
        }
        .apply(Saved {
            backup_keep: Some(3),
            ..Saved::default()
        })
        .unwrap();
        assert_eq!(applied.backup_keep, None);
        assert_eq!(
            applied.notification_click_milestones.as_deref(),
            Some("10,100")
        );
    }
}
