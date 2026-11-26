//! Opt-in page title lookups for saved destinations.
//!
//! Lookups run in a single background worker, one destination at a time, through the shared
//! public outbound policy. The public redirect path never waits for or triggers a lookup. A
//! result is applied only while the link still has an empty title and the same host and
//! destination as when the lookup was requested.

use std::{sync::Arc, time::Duration};

use serde::Serialize;
use sqlx::SqlitePool;
use tokio::{
    sync::{Notify, watch},
    time,
};

use crate::{
    destinations,
    history::{Action, Actor},
    input::link_title,
    outbound::{Failure, Fetcher},
    store::{self, LinkUpdate},
};

/// Pending lookups older than this are marked as expired instead of being fetched.
const MAX_PENDING_SECONDS: i64 = 24 * 60 * 60;
/// A failed or empty lookup is not queued again by the batch request within this period.
const RETRY_AFTER_SECONDS: i64 = 24 * 60 * 60;
const BATCH_LIMIT: i64 = 50;
const SPACING: Duration = Duration::from_secs(2);
const IDLE_WAIT: Duration = Duration::from_secs(300);

#[derive(Clone)]
pub struct TitleLookup(Arc<Service>);

struct Service {
    enabled: bool,
    fetcher: Fetcher,
    changed: Notify,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum RequestError {
    Disabled,
    Missing,
    TitlePresent,
    Protected,
    Database,
}

impl From<sqlx::Error> for RequestError {
    fn from(_: sqlx::Error) -> Self {
        Self::Database
    }
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub(crate) struct LookupEntry {
    link_id: i64,
    host: String,
    slug: String,
    status: String,
    failure: Option<String>,
    title: Option<String>,
    requested_at: i64,
    completed_at: Option<i64>,
}

#[derive(sqlx::FromRow)]
struct Pending {
    link_id: i64,
    revision: String,
    host: String,
    target: String,
    requested_by: String,
}

impl TitleLookup {
    pub fn new(enabled: bool) -> Self {
        Self(Arc::new(Service {
            enabled,
            fetcher: Fetcher::new(),
            changed: Notify::new(),
        }))
    }

    pub fn enabled(&self) -> bool {
        self.0.enabled
    }

    /// Queues a lookup for one link with an empty title and no access protection.
    pub(crate) async fn request(
        &self,
        pool: &SqlitePool,
        id: i64,
        requested_by: &str,
    ) -> Result<(), RequestError> {
        if !self.enabled() {
            return Err(RequestError::Disabled);
        }
        let state = sqlx::query_as::<_, (bool, bool)>(
            "SELECT links.title <> '', EXISTS(
                 SELECT 1 FROM link_protection AS protection WHERE protection.link_id = links.id
                 AND (protection.one_time = 1 OR protection.password_hash IS NOT NULL))
             FROM links WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(RequestError::Missing)?;
        match state {
            (true, _) => return Err(RequestError::TitlePresent),
            (_, true) => return Err(RequestError::Protected),
            _ => {}
        }
        sqlx::query(
            "INSERT INTO link_title_lookups (link_id, host, target, requested_by)
             SELECT id, host, target, ? FROM links WHERE id = ? AND title = ''
             ON CONFLICT (link_id) DO UPDATE SET
                 revision = lower(hex(randomblob(16))), host = excluded.host,
                 target = excluded.target, requested_by = excluded.requested_by,
                 status = 'pending', failure = NULL, title = NULL,
                 requested_at = unixepoch(), completed_at = NULL
             WHERE link_title_lookups.status <> 'pending'",
        )
        .bind(requested_by)
        .bind(id)
        .execute(pool)
        .await?;
        self.0.changed.notify_one();
        Ok(())
    }

    /// Queues up to 50 eligible links without a title that have no recent lookup.
    pub(crate) async fn request_untitled(
        &self,
        pool: &SqlitePool,
        requested_by: &str,
    ) -> Result<u64, RequestError> {
        if !self.enabled() {
            return Err(RequestError::Disabled);
        }
        let queued = sqlx::query(
            "INSERT INTO link_title_lookups (link_id, host, target, requested_by)
             SELECT links.id, links.host, links.target, ?1 FROM links
             LEFT JOIN link_title_lookups AS lookup ON lookup.link_id = links.id
             WHERE links.title = ''
               AND NOT EXISTS (
                   SELECT 1 FROM link_protection AS protection WHERE protection.link_id = links.id
                   AND (protection.one_time = 1 OR protection.password_hash IS NOT NULL))
               AND (lookup.link_id IS NULL OR (lookup.status <> 'pending'
                    AND lookup.status <> 'applied'
                    AND lookup.completed_at <= unixepoch() - ?2))
             ORDER BY links.id LIMIT ?3
             ON CONFLICT (link_id) DO UPDATE SET
                 revision = lower(hex(randomblob(16))), host = excluded.host,
                 target = excluded.target, requested_by = excluded.requested_by,
                 status = 'pending', failure = NULL, title = NULL,
                 requested_at = unixepoch(), completed_at = NULL",
        )
        .bind(requested_by)
        .bind(RETRY_AFTER_SECONDS)
        .bind(BATCH_LIMIT)
        .execute(pool)
        .await?
        .rows_affected();
        self.0.changed.notify_one();
        Ok(queued)
    }

    pub(crate) async fn list(
        &self,
        pool: &SqlitePool,
    ) -> Result<(i64, Vec<LookupEntry>), sqlx::Error> {
        let pending =
            sqlx::query_scalar("SELECT count(*) FROM link_title_lookups WHERE status = 'pending'")
                .fetch_one(pool)
                .await?;
        let items = sqlx::query_as(
            "SELECT lookup.link_id, links.host, links.slug, lookup.status, lookup.failure,
                    lookup.title, lookup.requested_at, lookup.completed_at
             FROM link_title_lookups AS lookup JOIN links ON links.id = lookup.link_id
             ORDER BY coalesce(lookup.completed_at, lookup.requested_at) DESC, lookup.link_id DESC
             LIMIT 50",
        )
        .fetch_all(pool)
        .await?;
        Ok((pending, items))
    }

    pub async fn run(&self, pool: SqlitePool, mut stop: watch::Receiver<bool>) {
        if !self.enabled() {
            return;
        }
        loop {
            if *stop.borrow() {
                return;
            }
            let processed = tokio::select! {
                result = self.run_once(&pool) => result.unwrap_or_else(|_| {
                    tracing::warn!("title lookup job failed");
                    false
                }),
                _ = stop.changed() => return,
            };
            let wait = if processed { SPACING } else { IDLE_WAIT };
            tokio::select! {
                _ = stop.changed() => return,
                _ = self.0.changed.notified(), if !processed => {}
                _ = time::sleep(wait) => {}
            }
        }
    }

    /// Processes the oldest pending lookup. Returns whether a lookup was processed.
    pub(crate) async fn run_once(&self, pool: &SqlitePool) -> Result<bool, sqlx::Error> {
        sqlx::query(
            "UPDATE link_title_lookups SET status = 'failed', failure = 'expired',
                 completed_at = unixepoch()
             WHERE status = 'pending' AND requested_at < unixepoch() - ?",
        )
        .bind(MAX_PENDING_SECONDS)
        .execute(pool)
        .await?;
        let Some(pending) = sqlx::query_as::<_, Pending>(
            "SELECT link_id, revision, host, target, requested_by FROM link_title_lookups AS lookup
             WHERE status = 'pending' AND NOT EXISTS (
                 SELECT 1 FROM link_protection AS protection
                 WHERE protection.link_id = lookup.link_id
                   AND (protection.one_time = 1 OR protection.password_hash IS NOT NULL))
             ORDER BY requested_at, link_id LIMIT 1",
        )
        .fetch_optional(pool)
        .await?
        else {
            return Ok(false);
        };
        let outcome = if destinations::load(pool).await?.permits(&pending.target) {
            match self.0.fetcher.fetch_title(&pending.target).await {
                Ok(title) => match title.as_deref().and_then(link_title) {
                    Some(title) if !title.is_empty() => Outcome::Applied(title.to_owned()),
                    _ => Outcome::NoTitle,
                },
                Err(failure) => Outcome::Failed(failure_code(failure)),
            }
        } else {
            Outcome::Skipped("policy")
        };
        finish(pool, &pending, outcome).await?;
        Ok(true)
    }
}

enum Outcome {
    Applied(String),
    NoTitle,
    Failed(&'static str),
    Skipped(&'static str),
}

fn failure_code(failure: Failure) -> &'static str {
    match failure {
        Failure::Blocked => "blocked",
        Failure::Busy => "busy",
        Failure::Dns => "dns",
        Failure::Timeout => "timeout",
        Failure::Connection => "connection",
        Failure::Protocol => "protocol",
        Failure::NotHtml => "not_html",
    }
}

/// Records the outcome. A stale result, for a changed link or a newer request, is dropped.
async fn finish(pool: &SqlitePool, pending: &Pending, outcome: Outcome) -> Result<(), sqlx::Error> {
    let (status, failure, title) = match &outcome {
        Outcome::Applied(title) => ("applied", None, Some(title.as_str())),
        Outcome::NoTitle => ("no_title", None, None),
        Outcome::Failed(code) => ("failed", Some(*code), None),
        Outcome::Skipped(code) => ("skipped", Some(*code), None),
    };
    let mut transaction = pool.begin_with("BEGIN IMMEDIATE").await?;
    let current = sqlx::query(
        "UPDATE link_title_lookups SET status = ?, failure = ?, title = ?, completed_at = unixepoch()
         WHERE link_id = ? AND revision = ? AND status = 'pending' AND EXISTS (
             SELECT 1 FROM links WHERE id = ? AND host = ? AND target = ? AND title = ''
         ) AND NOT EXISTS (
             SELECT 1 FROM link_protection AS protection
             WHERE protection.link_id = link_title_lookups.link_id
               AND (protection.one_time = 1 OR protection.password_hash IS NOT NULL)
         )",
    )
    .bind(status)
    .bind(failure)
    .bind(title)
    .bind(pending.link_id)
    .bind(&pending.revision)
    .bind(pending.link_id)
    .bind(&pending.host)
    .bind(&pending.target)
    .execute(&mut *transaction)
    .await?
    .rows_affected()
        == 1;
    if current && let Some(title) = title {
        store::update_link_in(
            &mut transaction,
            std::slice::from_ref(&pending.host),
            pending.link_id,
            LinkUpdate {
                title: Some(title.to_owned()),
                ..LinkUpdate::default()
            },
            &Actor::Admin(pending.requested_by.clone()),
            Action::Metadata,
        )
        .await?;
    }
    transaction.commit().await
}

/// Cancels lookups that were pending in a restored snapshot. The restore scrub calls it.
pub(crate) async fn discard_pending<'e>(
    executor: impl sqlx::Executor<'e, Database = sqlx::Sqlite>,
) -> Result<u64, sqlx::Error> {
    Ok(sqlx::query(
        "UPDATE link_title_lookups SET status = 'skipped', failure = 'restored',
             completed_at = unixepoch()
         WHERE status = 'pending'",
    )
    .execute(executor)
    .await?
    .rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn database() -> (tempfile::TempDir, SqlitePool) {
        let directory = tempfile::tempdir().unwrap();
        let pool = crate::open_database(&directory.path().join("titles.db"))
            .await
            .unwrap();
        sqlx::raw_sql(
            "INSERT INTO links (id, host, slug, target, title) VALUES
                (1, 'go.example', 'untitled', 'https://203.0.113.7/page?Sig=A%2f', ''),
                (2, 'go.example', 'named', 'https://example.test/', 'Written by a person'),
                (3, 'go.example', 'locked', 'https://example.test/secret', ''),
                (4, 'go.example', 'once', 'https://example.test/once', '');
             INSERT INTO link_protection (link_id, password_hash, one_time) VALUES
                (3, '$argon2id$v=19$m=19456,t=2,p=1$AAAAAAAAAAAAAAAAAAAAAA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA', 0),
                (4, NULL, 1);",
        )
        .execute(&pool)
        .await
        .unwrap();
        (directory, pool)
    }

    fn pending(link_id: i64, revision: &str) -> Pending {
        Pending {
            link_id,
            revision: revision.to_owned(),
            host: "go.example".into(),
            target: "https://203.0.113.7/page?Sig=A%2f".into(),
            requested_by: "Operator".into(),
        }
    }

    async fn revision(pool: &SqlitePool, id: i64) -> Option<String> {
        sqlx::query_scalar("SELECT revision FROM link_title_lookups WHERE link_id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn disabled_lookups_queue_nothing_and_eligibility_preserves_written_titles() {
        let (_directory, pool) = database().await;
        let disabled = TitleLookup::new(false);
        assert_eq!(
            disabled.request(&pool, 1, "Operator").await,
            Err(RequestError::Disabled)
        );
        assert_eq!(
            disabled.request_untitled(&pool, "Operator").await,
            Err(RequestError::Disabled)
        );
        assert_eq!(revision(&pool, 1).await, None);

        let lookup = TitleLookup::new(true);
        assert_eq!(
            lookup.request(&pool, 2, "Operator").await,
            Err(RequestError::TitlePresent)
        );
        assert_eq!(
            lookup.request(&pool, 3, "Operator").await,
            Err(RequestError::Protected)
        );
        assert_eq!(
            lookup.request(&pool, 4, "Operator").await,
            Err(RequestError::Protected)
        );
        assert_eq!(
            lookup.request(&pool, 9, "Operator").await,
            Err(RequestError::Missing)
        );
        assert_eq!(lookup.request_untitled(&pool, "Operator").await, Ok(1));
        assert_eq!(lookup.request_untitled(&pool, "Operator").await, Ok(0));
        let (count, items) = lookup.list(&pool).await.unwrap();
        assert_eq!(count, 1);
        assert_eq!(items[0].slug, "untitled");
    }

    #[tokio::test]
    async fn results_apply_only_to_the_unchanged_link_and_record_history() {
        let (_directory, pool) = database().await;
        let lookup = TitleLookup::new(true);
        lookup.request(&pool, 1, "Operator").await.unwrap();
        let first = revision(&pool, 1).await.unwrap();
        finish(
            &pool,
            &pending(1, &first),
            Outcome::Applied("Fetched <b>".into()),
        )
        .await
        .unwrap();
        let (title, target): (String, String) =
            sqlx::query_as("SELECT title, target FROM links WHERE id = 1")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(title, "Fetched <b>");
        assert_eq!(target, "https://203.0.113.7/page?Sig=A%2f");
        let actor: String = sqlx::query_scalar(
            "SELECT actor_label FROM link_history WHERE link_id = 1 AND action = 'metadata'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(actor, "Operator");

        // A person clears and rewrites the title while a second lookup is pending.
        sqlx::query("UPDATE links SET title = '' WHERE id = 1")
            .execute(&pool)
            .await
            .unwrap();
        lookup.request(&pool, 1, "Operator").await.unwrap();
        let second = revision(&pool, 1).await.unwrap();
        assert_ne!(first, second);
        sqlx::query("UPDATE links SET title = 'Typed' WHERE id = 1")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(revision(&pool, 1).await, None);
        finish(&pool, &pending(1, &second), Outcome::Applied("Late".into()))
            .await
            .unwrap();
        let title: String = sqlx::query_scalar("SELECT title FROM links WHERE id = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(title, "Typed");

        // A changed destination drops the pending lookup and its late result.
        sqlx::query("UPDATE links SET title = '' WHERE id = 1")
            .execute(&pool)
            .await
            .unwrap();
        lookup.request(&pool, 1, "Operator").await.unwrap();
        let third = revision(&pool, 1).await.unwrap();
        sqlx::query("UPDATE links SET target = 'https://203.0.113.8/' WHERE id = 1")
            .execute(&pool)
            .await
            .unwrap();
        finish(
            &pool,
            &pending(1, &third),
            Outcome::Applied("Old page".into()),
        )
        .await
        .unwrap();
        let title: String = sqlx::query_scalar("SELECT title FROM links WHERE id = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(title, "");

        // A stale revision cannot complete a newer request.
        lookup.request(&pool, 1, "Operator").await.unwrap();
        finish(&pool, &pending(1, &third), Outcome::Applied("Stale".into()))
            .await
            .unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT status FROM link_title_lookups WHERE link_id = 1"
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            "pending"
        );
    }

    #[tokio::test]
    async fn protection_added_after_queueing_cancels_the_lookup_before_any_fetch() {
        let (_directory, pool) = database().await;
        let lookup = TitleLookup::new(true);
        lookup.request(&pool, 1, "Operator").await.unwrap();
        sqlx::query("INSERT INTO link_protection (link_id, one_time) VALUES (1, 1)")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(revision(&pool, 1).await, None);
        assert!(!lookup.run_once(&pool).await.unwrap());
        // A protected row that was pending before the trigger existed is not fetched either.
        sqlx::query(
            "INSERT INTO link_title_lookups (link_id, host, target, requested_by)
             VALUES (1, 'go.example', 'https://203.0.113.7/page?Sig=A%2f', 'Operator')",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert!(!lookup.run_once(&pool).await.unwrap());
    }

    #[tokio::test]
    async fn refused_and_unreachable_destinations_fail_safely() {
        let (_directory, pool) = database().await;
        sqlx::query("UPDATE links SET target = 'http://127.0.0.1/private' WHERE id = 1")
            .execute(&pool)
            .await
            .unwrap();
        let lookup = TitleLookup::new(true);
        lookup.request(&pool, 1, "Operator").await.unwrap();
        assert!(lookup.run_once(&pool).await.unwrap());
        let row: (String, Option<String>) =
            sqlx::query_as("SELECT status, failure FROM link_title_lookups WHERE link_id = 1")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(row, ("failed".to_owned(), Some("blocked".to_owned())));
        assert!(!lookup.run_once(&pool).await.unwrap());

        // The destination policy is applied before any connection.
        sqlx::query("UPDATE links SET target = 'https://203.0.113.9/' WHERE id = 1")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::raw_sql(
            "INSERT INTO destination_rules (host, subdomains, action) VALUES ('203.0.113.9', 0, 'block');
             UPDATE destination_policy SET enabled = 1, generation = generation + 1;",
        )
        .execute(&pool)
        .await
        .unwrap();
        lookup.request(&pool, 1, "Operator").await.unwrap();
        assert!(lookup.run_once(&pool).await.unwrap());
        let row: (String, Option<String>) =
            sqlx::query_as("SELECT status, failure FROM link_title_lookups WHERE link_id = 1")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(row, ("skipped".to_owned(), Some("policy".to_owned())));

        // Lookups pending in a restored snapshot are discarded, and old ones expire.
        sqlx::query("UPDATE links SET title = '' WHERE id = 1")
            .execute(&pool)
            .await
            .unwrap();
        lookup.request(&pool, 1, "Operator").await.unwrap();
        assert_eq!(discard_pending(&pool).await.unwrap(), 1);
        lookup.request(&pool, 1, "Operator").await.unwrap();
        sqlx::query("UPDATE link_title_lookups SET requested_at = unixepoch() - 90000")
            .execute(&pool)
            .await
            .unwrap();
        assert!(!lookup.run_once(&pool).await.unwrap());
        let failure: Option<String> =
            sqlx::query_scalar("SELECT failure FROM link_title_lookups WHERE link_id = 1")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(failure.as_deref(), Some("expired"));
    }
}
