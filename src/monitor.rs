use std::{sync::Arc, time::Duration};

use serde::Serialize;
use sqlx::SqlitePool;
use tokio::{
    sync::{Notify, Semaphore, watch},
    time,
};

use crate::{
    notifications::{Event, Notifier},
    outbound::{Failure, Fetcher, Probe},
};

const MIN_RECHECK: i64 = 60;
const MAX_DUE: i64 = 16;

#[derive(Clone)]
pub struct Monitor(Arc<Service>);

struct Service {
    enabled: bool,
    notifier: Option<Notifier>,
    interval: i64,
    priority_interval: i64,
    fetcher: Fetcher,
    job: Semaphore,
    changed: Notify,
}

#[derive(sqlx::FromRow)]
struct DueLink {
    id: i64,
    target: String,
    revision: String,
    priority: bool,
    status: String,
    consecutive_failures: u8,
}

#[derive(Serialize, sqlx::FromRow)]
pub(crate) struct LinkCheck {
    pub(crate) id: i64,
    host: String,
    slug: String,
    priority: bool,
    status: String,
    last_checked: Option<i64>,
    http_status: Option<u16>,
    elapsed_ms: Option<u32>,
    redirects: Option<u8>,
    failure: Option<String>,
    consecutive_failures: u8,
}

#[derive(Debug)]
pub(crate) enum CheckError {
    Disabled,
    Busy,
    Inactive,
    Missing,
    Database,
}

impl From<sqlx::Error> for CheckError {
    fn from(_: sqlx::Error) -> Self {
        Self::Database
    }
}

impl Monitor {
    pub fn new(
        enabled: bool,
        notifier: Option<Notifier>,
        interval_hours: u16,
        priority_hours: u16,
    ) -> Result<Self, &'static str> {
        if !(1..=720).contains(&interval_hours) || !(1..=interval_hours).contains(&priority_hours) {
            return Err("monitor intervals require 1 <= priority hours <= regular hours <= 720");
        }
        Ok(Self(Arc::new(Service {
            enabled,
            notifier,
            interval: i64::from(interval_hours) * 3_600,
            priority_interval: i64::from(priority_hours) * 3_600,
            fetcher: Fetcher::new(),
            job: Semaphore::new(1),
            changed: Notify::new(),
        })))
    }

    pub fn enabled(&self) -> bool {
        self.0.enabled
    }

    pub(crate) fn changed(&self) {
        self.0.changed.notify_one();
    }

    pub(crate) async fn list(
        &self,
        pool: &SqlitePool,
        after: i64,
        limit: i64,
    ) -> Result<Vec<LinkCheck>, sqlx::Error> {
        sqlx::query_as(
            "SELECT l.id, l.host, l.slug, m.priority, m.status, m.last_checked,
                    m.http_status, m.elapsed_ms, m.redirects, m.failure, m.consecutive_failures
             FROM link_monitor m JOIN links l ON l.id = m.link_id
             WHERE l.id > ? ORDER BY l.id LIMIT ?",
        )
        .bind(after)
        .bind(limit)
        .fetch_all(pool)
        .await
    }

    pub(crate) async fn queue_checks(&self, pool: &SqlitePool) -> Result<u64, CheckError> {
        if !self.enabled() {
            return Err(CheckError::Disabled);
        }
        let updated = sqlx::query(
            "UPDATE link_monitor SET next_check = unixepoch()
             WHERE link_id IN (
                 SELECT m.link_id FROM link_monitor m JOIN links l ON l.id = m.link_id
                 LEFT JOIN link_redirect_policies p ON p.link_id = l.id
                 WHERE m.next_check > unixepoch() AND coalesce(m.last_checked, 0) <= unixepoch() - ?
                   AND l.enabled = 1 AND l.archived = 0
                   AND (p.starts_at IS NULL OR p.starts_at <= unixepoch())
                   AND (p.expires_at IS NULL OR p.expires_at > unixepoch())
                   AND NOT EXISTS (SELECT 1 FROM domains WHERE host = l.host AND enabled = 0)
                 ORDER BY m.next_check, m.link_id LIMIT 50
             )",
        )
        .bind(MIN_RECHECK)
        .execute(pool)
        .await?;
        self.changed();
        Ok(updated.rows_affected())
    }

    pub(crate) async fn check(&self, pool: &SqlitePool, id: i64) -> Result<LinkCheck, CheckError> {
        let _job = self.try_job()?;
        let row = sqlx::query_as::<_, DueLink>(
            "SELECT l.id, l.target, m.revision, m.priority, m.status, m.consecutive_failures
             FROM link_monitor m JOIN links l ON l.id = m.link_id
             LEFT JOIN link_redirect_policies p ON p.link_id = l.id
             WHERE l.id = ? AND l.enabled = 1 AND l.archived = 0
               AND (p.starts_at IS NULL OR p.starts_at <= unixepoch())
               AND (p.expires_at IS NULL OR p.expires_at > unixepoch())
               AND NOT EXISTS (SELECT 1 FROM domains WHERE host = l.host AND enabled = 0)
               AND coalesce(m.last_checked, 0) <= unixepoch() - ?",
        )
        .bind(id)
        .bind(MIN_RECHECK)
        .fetch_optional(pool)
        .await?;
        let Some(row) = row else {
            let checked = sqlx::query_scalar::<_, bool>(
                "SELECT coalesce(last_checked, 0) > unixepoch() - ?
                 FROM link_monitor WHERE link_id = ?",
            )
            .bind(MIN_RECHECK)
            .bind(id)
            .fetch_optional(pool)
            .await?;
            return Err(match checked {
                Some(true) => CheckError::Busy,
                Some(false) => CheckError::Inactive,
                None => CheckError::Missing,
            });
        };
        self.process_rows(pool, &[&row]).await?;
        self.list(pool, id - 1, 1)
            .await?
            .into_iter()
            .find(|check| check.id == id)
            .ok_or(CheckError::Missing)
    }

    pub(crate) async fn check_domain(
        &self,
        origin: &str,
    ) -> Result<Result<Probe, Failure>, CheckError> {
        let _job = self.try_job()?;
        Ok(self.0.fetcher.probe(origin).await)
    }

    fn try_job(&self) -> Result<tokio::sync::SemaphorePermit<'_>, CheckError> {
        if !self.enabled() {
            return Err(CheckError::Disabled);
        }
        self.0.job.try_acquire().map_err(|_| CheckError::Busy)
    }

    pub async fn run(&self, pool: SqlitePool, mut stop: watch::Receiver<bool>) {
        if !self.enabled() {
            return;
        }
        loop {
            if *stop.borrow() {
                return;
            }
            let wait = tokio::select! {
                result = self.run_due(&pool) => match result {
                    Ok(wait) => wait,
                    Err(_) => {
                        tracing::warn!("could not update link monitor");
                        Duration::from_secs(60)
                    }
                },
                _ = stop.changed() => return,
            };
            tokio::select! {
                _ = stop.changed() => return,
                _ = self.0.changed.notified() => {},
                _ = time::sleep(wait) => {},
            }
        }
    }

    async fn run_due(&self, pool: &SqlitePool) -> Result<Duration, sqlx::Error> {
        let Ok(_job) = self.try_job() else {
            return Ok(Duration::from_secs(15));
        };
        let due = sqlx::query_as::<_, DueLink>(
            "SELECT l.id, l.target, m.revision, m.priority, m.status, m.consecutive_failures
             FROM link_monitor m JOIN links l ON l.id = m.link_id
             LEFT JOIN link_redirect_policies p ON p.link_id = l.id
             WHERE m.next_check <= unixepoch() AND l.enabled = 1 AND l.archived = 0
               AND (p.starts_at IS NULL OR p.starts_at <= unixepoch())
               AND (p.expires_at IS NULL OR p.expires_at > unixepoch())
               AND NOT EXISTS (SELECT 1 FROM domains WHERE host = l.host AND enabled = 0)
             ORDER BY m.next_check, l.id LIMIT ?",
        )
        .bind(MAX_DUE)
        .fetch_all(pool)
        .await?;
        if let Some(first) = due.first() {
            let matching: Vec<_> = due
                .iter()
                .filter(|row| row.target == first.target)
                .collect();
            self.process_rows(pool, &matching).await?;
            return Ok(Duration::from_secs(3));
        }
        let next = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT min(max(m.next_check, coalesce(p.starts_at, 0))) - unixepoch()
             FROM link_monitor m JOIN links l ON l.id = m.link_id
             LEFT JOIN link_redirect_policies p ON p.link_id = l.id
             WHERE l.enabled = 1 AND l.archived = 0
               AND (p.expires_at IS NULL OR p.expires_at > unixepoch())
               AND NOT EXISTS (SELECT 1 FROM domains WHERE host = l.host AND enabled = 0)",
        )
        .fetch_one(pool)
        .await?;
        Ok(Duration::from_secs(
            next.unwrap_or(900).clamp(15, 900) as u64
        ))
    }

    async fn process_rows(&self, pool: &SqlitePool, rows: &[&DueLink]) -> Result<(), sqlx::Error> {
        let result = self.0.fetcher.probe(&rows[0].target).await;
        let checked_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs() as i64);
        for row in rows {
            let (status, failures, failure) = classify(&result, row.consecutive_failures);
            let (http_status, elapsed_ms, redirects) =
                result.as_ref().map_or((None, None, None), |p| {
                    (Some(p.status), Some(p.elapsed_ms as i64), Some(p.redirects))
                });
            // The status change and its notification commit together or not at all.
            let mut transaction = pool.begin_with("BEGIN IMMEDIATE").await?;
            let updated = sqlx::query(
                "UPDATE link_monitor SET status = ?, last_checked = unixepoch(),
                    next_check = unixepoch() + ?, http_status = ?, elapsed_ms = ?, redirects = ?,
                    failure = ?, consecutive_failures = ?
                 WHERE link_id = ? AND revision = ? AND EXISTS (
                     SELECT 1 FROM links l LEFT JOIN link_redirect_policies p ON p.link_id = l.id
                     WHERE l.id = ? AND l.target = ? AND l.enabled = 1 AND l.archived = 0
                     AND (p.starts_at IS NULL OR p.starts_at <= unixepoch())
                     AND (p.expires_at IS NULL OR p.expires_at > unixepoch())
                     AND NOT EXISTS (SELECT 1 FROM domains WHERE host = l.host AND enabled = 0)
                 )",
            )
            .bind(status)
            .bind(self.interval(row, failures, status))
            .bind(http_status)
            .bind(elapsed_ms)
            .bind(redirects)
            .bind(failure)
            .bind(failures)
            .bind(row.id)
            .bind(&row.revision)
            .bind(row.id)
            .bind(&row.target)
            .execute(&mut *transaction)
            .await?;
            if updated.rows_affected() != 0
                && changed_health(&row.status, status)
                && let Some(notifier) = &self.0.notifier
            {
                let event = Event::LinkHealth {
                    link_id: row.id,
                    status,
                    revision: row.revision.clone(),
                    checked_at,
                };
                notifier.record(&mut *transaction, event).await?;
            }
            transaction.commit().await?;
        }
        Ok(())
    }

    fn interval(&self, row: &DueLink, failures: u8, status: &str) -> i64 {
        let jitter = row.id % 301;
        let seconds = if status == "unknown" {
            self.0.interval
        } else if failures > 0 {
            1_800 * (1 << (failures - 1).min(5))
        } else if row.priority {
            self.0.priority_interval
        } else {
            self.0.interval
        };
        seconds + jitter
    }
}

fn changed_health(previous: &str, current: &str) -> bool {
    let failed = |status| matches!(status, "broken" | "unreachable");
    previous != current
        && (failed(current) || (failed(previous) && matches!(current, "healthy" | "redirected")))
}

fn classify(
    result: &Result<Probe, Failure>,
    previous: u8,
) -> (&'static str, u8, Option<&'static str>) {
    let failures = previous.saturating_add(1);
    match result {
        Ok(probe) => match probe.status {
            200..=299 => (
                if probe.redirects == 0 {
                    "healthy"
                } else {
                    "redirected"
                },
                0,
                None,
            ),
            401 | 403 | 429 => ("warning", failures, None),
            404 | 410 if failures >= 2 => ("broken", failures, None),
            500..=599 if failures >= 2 => ("unreachable", failures, None),
            _ => ("warning", failures, None),
        },
        Err(error) => {
            let code = match error {
                Failure::Blocked => "blocked",
                Failure::Busy => "busy",
                Failure::Dns => "dns",
                Failure::Timeout => "timeout",
                Failure::Connection => "connection",
                Failure::Protocol => "protocol",
                Failure::NotHtml => "protocol",
            };
            let status = match error {
                Failure::Blocked | Failure::Busy => "unknown",
                _ if failures >= 2 => "unreachable",
                _ => "warning",
            };
            (status, failures, Some(code))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(status: u16, redirects: u8) -> Result<Probe, Failure> {
        Ok(Probe {
            status,
            redirects,
            elapsed_ms: 10,
        })
    }

    #[tokio::test]
    async fn scheduled_enrollment_checks_obey_the_outbound_policy() {
        let directory = tempfile::tempdir().unwrap();
        let pool = crate::open_database(&directory.path().join("monitor.db"))
            .await
            .unwrap();
        sqlx::raw_sql(
            "INSERT INTO links (host, slug, target) VALUES
                ('go.example', 'sample', 'http://127.0.0.1/private'),
                ('go.example', 'future', 'http://127.0.0.1/private');
             INSERT INTO link_monitor (link_id, priority, next_check)
             VALUES (1, 0, unixepoch() + 600), (2, 0, unixepoch() + 600);
             INSERT INTO link_redirect_policies
                (link_id, status, starts_at, forward_query, forward_path, rules)
             VALUES (2, 302, unixepoch() + 3600, 0, 0, '[]');",
        )
        .execute(&pool)
        .await
        .unwrap();
        let disabled = Monitor::new(false, None, 24, 6).unwrap();
        assert!(matches!(
            disabled.check(&pool, 1).await,
            Err(CheckError::Disabled)
        ));

        let active = Monitor::new(true, None, 24, 6).unwrap();
        assert_eq!(active.queue_checks(&pool).await.unwrap(), 1);
        assert_eq!(active.queue_checks(&pool).await.unwrap(), 0);
        active.run_due(&pool).await.unwrap();
        let items = active.list(&pool, 0, 2).await.unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].status, "unknown");
        assert_eq!(items[0].failure.as_deref(), Some("blocked"));
        assert_eq!(items[1].status, "not_checked");
    }

    #[test]
    fn distinguishes_transient_failures_and_blocked_probes() {
        assert_eq!(classify(&response(404, 0), 0).0, "warning");
        assert_eq!(classify(&response(404, 0), 1).0, "broken");
        assert_eq!(classify(&response(429, 0), 4).0, "warning");
        assert_eq!(classify(&response(200, 1), 4), ("redirected", 0, None));
        assert_eq!(classify(&Err(Failure::Blocked), 2).0, "unknown");
        assert_eq!(classify(&Err(Failure::Timeout), 1).0, "unreachable");
        assert!(changed_health("warning", "broken"));
        assert!(!changed_health("broken", "broken"));
    }
}
