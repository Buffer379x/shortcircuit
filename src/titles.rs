//! Opt-in page title lookups and preview images for saved destinations.
//!
//! Lookups run in a single background worker, one destination at a time, through the shared
//! public outbound policy. The public redirect path never waits for or triggers a lookup. A
//! result is applied only while the link still has an empty title and the same host and
//! destination as when the lookup was requested.
//!
//! While preview images are on as well, the same worker reads the page head from the same
//! response and fetches the page's preview image (see `previews`). One page request serves both
//! the title and the preview. Links with access protection are never fetched.

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
    previews,
    runtime_settings::RuntimeSettings,
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
    /// Whether lookups are allowed, read on every use.
    settings: RuntimeSettings,
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
    pub fn new(settings: &RuntimeSettings) -> Self {
        Self::with_fetcher(settings, Fetcher::new())
    }

    fn with_fetcher(settings: &RuntimeSettings, fetcher: Fetcher) -> Self {
        Self(Arc::new(Service {
            settings: settings.clone(),
            fetcher,
            changed: Notify::new(),
        }))
    }

    /// Whether title lookup is on. Settings > Operations can change it at any time.
    pub fn enabled(&self) -> bool {
        self.0.settings.current().title_lookup_enabled.value
    }

    /// Whether preview images are fetched: both preview images and title lookup are on.
    pub(crate) fn previews_enabled(&self) -> bool {
        self.0.settings.current().previews_active()
    }

    /// Clears the preview of one link and queues a new fetch.
    pub(crate) async fn request_preview(
        &self,
        pool: &SqlitePool,
        id: i64,
    ) -> Result<(), previews::RequestError> {
        let settings = self.0.settings.current();
        if !settings.preview_images_enabled.value {
            return Err(previews::RequestError::Disabled);
        }
        if !settings.title_lookup_enabled.value {
            return Err(previews::RequestError::TitleLookupRequired);
        }
        previews::request(pool, id).await?;
        self.0.changed.notify_one();
        Ok(())
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

    /// Runs lookups until shutdown. While title lookup is off, pending lookups wait and no
    /// request is sent; a lookup that already started finishes and is applied only if the link
    /// is unchanged.
    pub async fn run(&self, pool: SqlitePool, mut stop: watch::Receiver<bool>) {
        let mut settings = self.0.settings.subscribe();
        loop {
            if *stop.borrow() {
                return;
            }
            if !self.enabled() {
                tokio::select! {
                    _ = stop.changed() => return,
                    Ok(()) = settings.changed() => {}
                }
                continue;
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
                Ok(()) = settings.changed() => {}
                _ = time::sleep(wait) => {}
            }
        }
    }

    /// Processes the oldest pending title lookup, otherwise the next preview. A title lookup
    /// also serves the link's first preview from the same page response. Returns whether a
    /// link was processed.
    pub(crate) async fn run_once(&self, pool: &SqlitePool) -> Result<bool, sqlx::Error> {
        if !self.enabled() {
            return Ok(false);
        }
        sqlx::query(
            "UPDATE link_title_lookups SET status = 'failed', failure = 'expired',
                 completed_at = unixepoch()
             WHERE status = 'pending' AND requested_at < unixepoch() - ?",
        )
        .bind(MAX_PENDING_SECONDS)
        .execute(pool)
        .await?;
        let title = sqlx::query_as::<_, Pending>(
            "SELECT link_id, revision, host, target, requested_by FROM link_title_lookups AS lookup
             WHERE status = 'pending' AND NOT EXISTS (
                 SELECT 1 FROM link_protection AS protection
                 WHERE protection.link_id = lookup.link_id
                   AND (protection.one_time = 1 OR protection.password_hash IS NOT NULL))
             ORDER BY requested_at, link_id LIMIT 1",
        )
        .fetch_optional(pool)
        .await?;
        let preview = match (&title, self.previews_enabled()) {
            (_, false) => None,
            (Some(title), true) => previews::claim(pool, title.link_id)
                .await?
                .filter(|job| job.host == title.host && job.target == title.target),
            (None, true) => previews::next_job(pool).await?,
        };
        let target = match (&title, &preview) {
            (Some(title), _) => title.target.clone(),
            (None, Some(preview)) => preview.target.clone(),
            (None, None) => return Ok(false),
        };
        let policy = destinations::load(pool).await?;
        let (title_outcome, preview_outcome) = if !policy.permits(&target) {
            (
                Outcome::Skipped("policy"),
                Some(previews::Outcome::Skipped("policy")),
            )
        } else {
            match self.0.fetcher.fetch_page(&target, preview.is_some()).await {
                Ok(page) => {
                    let title = match page.title.as_deref().and_then(link_title) {
                        Some(title) if !title.is_empty() => Outcome::Applied(title.to_owned()),
                        _ => Outcome::NoTitle,
                    };
                    let preview = match &preview {
                        Some(_) => Some(previews::build(&self.0.fetcher, &policy, &page).await),
                        None => None,
                    };
                    (title, preview)
                }
                // A busy fetcher leaves the preview pending for the next pass.
                Err(Failure::Busy) => (Outcome::Failed("busy"), None),
                Err(failure) => (
                    Outcome::Failed(failure_code(failure)),
                    Some(previews::Outcome::Failed(previews::failure_code(failure))),
                ),
            }
        };
        if let Some(title) = &title {
            finish(pool, title, title_outcome).await?;
        }
        if let (Some(job), Some(outcome)) = (&preview, preview_outcome) {
            previews::finish(pool, job, outcome).await?;
        }
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
    previews::failure_code(failure)
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
        let disabled = TitleLookup::new(&RuntimeSettings::detached(|_| None).unwrap());
        assert_eq!(
            disabled.request(&pool, 1, "Operator").await,
            Err(RequestError::Disabled)
        );
        assert_eq!(
            disabled.request_untitled(&pool, "Operator").await,
            Err(RequestError::Disabled)
        );
        assert_eq!(revision(&pool, 1).await, None);

        let lookup = TitleLookup::new(
            &RuntimeSettings::detached(|name| {
                (name == "TITLE_LOOKUP_ENABLED").then(|| "true".to_owned())
            })
            .unwrap(),
        );
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
        let lookup = TitleLookup::new(
            &RuntimeSettings::detached(|name| {
                (name == "TITLE_LOOKUP_ENABLED").then(|| "true".to_owned())
            })
            .unwrap(),
        );
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
        let lookup = TitleLookup::new(
            &RuntimeSettings::detached(|name| {
                (name == "TITLE_LOOKUP_ENABLED").then(|| "true".to_owned())
            })
            .unwrap(),
        );
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
        let lookup = TitleLookup::new(
            &RuntimeSettings::detached(|name| {
                (name == "TITLE_LOOKUP_ENABLED").then(|| "true".to_owned())
            })
            .unwrap(),
        );
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

    fn settings(title: bool, previews: bool) -> RuntimeSettings {
        RuntimeSettings::detached(move |name| match name {
            "TITLE_LOOKUP_ENABLED" => Some(title.to_string()),
            "PREVIEW_IMAGES_ENABLED" => Some(previews.to_string()),
            _ => None,
        })
        .unwrap()
    }

    async fn preview_row(pool: &SqlitePool, id: i64) -> Option<(String, Option<String>, bool)> {
        sqlx::query_as(
            "SELECT status, kind, bytes IS NOT NULL FROM link_previews WHERE link_id = ?",
        )
        .bind(id)
        .fetch_optional(pool)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn one_page_request_serves_the_title_and_the_preview() {
        use crate::previews::test_server::{self, Route};
        let server = test_server::start(vec![
            (
                "/page",
                Route::ok(
                    "text/html",
                    r#"<html><head><title>Synthetic page</title>
                       <meta property="og:image" content="/card.png"></head>"#,
                ),
            ),
            (
                "/other",
                Route::ok(
                    "text/html",
                    r#"<head><link rel="icon" href="/icon.png"></head>"#,
                ),
            ),
            (
                "/card.png",
                Route::ok("image/png", crate::previews::tests::png(800, 450)),
            ),
            (
                "/icon.png",
                Route::ok("image/png", crate::previews::tests::png(32, 32)),
            ),
        ])
        .await;
        let (_directory, pool) = database().await;
        // A documentation address, refused without a DNS query.
        sqlx::query("UPDATE links SET target = 'https://198.51.100.5/' WHERE id = 2")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO links (id, host, slug, target, title) VALUES
                (10, 'go.example', 'local', ?, ''), (11, 'go.example', 'titled', ?, 'Kept')",
        )
        .bind(server.url("/page"))
        .bind(server.url("/other"))
        .execute(&pool)
        .await
        .unwrap();
        let lookup =
            TitleLookup::with_fetcher(&settings(true, true), Fetcher::with_test_port(server.port));
        lookup.request(&pool, 10, "Operator").await.unwrap();
        assert!(lookup.run_once(&pool).await.unwrap());
        let title: String = sqlx::query_scalar("SELECT title FROM links WHERE id = 10")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(title, "Synthetic page");
        assert_eq!(
            preview_row(&pool, 10).await,
            Some(("ready".into(), Some("image".into()), true))
        );
        assert_eq!(*server.requests.lock().await, ["/page", "/card.png"]);
        let (width, height, origin): (i64, i64, String) = sqlx::query_as(
            "SELECT width, height, source_origin FROM link_previews WHERE link_id = 10",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!((width, height), (480, 270));
        assert_eq!(origin, server.url(""));

        // Without a pending title lookup, the worker fills in previews for links that never had
        // one, newest first. Protected links are never fetched, and link 1's address is refused.
        assert!(lookup.run_once(&pool).await.unwrap());
        assert_eq!(
            preview_row(&pool, 11).await,
            Some(("ready".into(), Some("icon".into()), true))
        );
        assert!(lookup.run_once(&pool).await.unwrap());
        assert_eq!(
            preview_row(&pool, 2).await.map(|row| row.0),
            Some("failed".into())
        );
        assert!(lookup.run_once(&pool).await.unwrap());
        assert!(!lookup.run_once(&pool).await.unwrap());
        for protected in [3, 4] {
            assert_eq!(preview_row(&pool, protected).await, None);
            assert_eq!(
                lookup.request_preview(&pool, protected).await,
                Err(previews::RequestError::Protected)
            );
        }
        assert_eq!(
            lookup.request_preview(&pool, 99).await,
            Err(previews::RequestError::Missing)
        );

        // A changed destination clears the preview and queues a new fetch; a stale result for
        // the old destination is dropped.
        let stale = previews::claim(&pool, 10).await.unwrap();
        assert!(stale.is_none());
        sqlx::query("UPDATE links SET target = ? WHERE id = 10")
            .bind(server.url("/other"))
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            preview_row(&pool, 10).await,
            Some(("pending".into(), None, false))
        );
        let job = previews::claim(&pool, 10).await.unwrap().unwrap();
        sqlx::query("UPDATE links SET target = ? WHERE id = 10")
            .bind(server.url("/page"))
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            !previews::finish(&pool, &job, previews::Outcome::None)
                .await
                .unwrap()
        );
        assert!(lookup.run_once(&pool).await.unwrap());
        assert_eq!(
            preview_row(&pool, 10).await,
            Some(("ready".into(), Some("image".into()), true))
        );

        // Refreshing clears the stored image until the new fetch completes, and protection
        // removes the preview entirely.
        lookup.request_preview(&pool, 11).await.unwrap();
        assert_eq!(
            preview_row(&pool, 11).await,
            Some(("pending".into(), None, false))
        );
        sqlx::query("INSERT INTO link_protection (link_id, one_time) VALUES (11, 1)")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(preview_row(&pool, 11).await, None);
        sqlx::query("INSERT INTO link_protection (link_id, one_time) VALUES (10, 0)")
            .execute(&pool)
            .await
            .unwrap();
        assert!(preview_row(&pool, 10).await.is_some());
        sqlx::query("UPDATE link_protection SET one_time = 1 WHERE link_id = 10")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(preview_row(&pool, 10).await, None);

        // Pending fetches in a restored snapshot are dropped.
        sqlx::query("DELETE FROM link_protection WHERE link_id IN (10, 11)")
            .execute(&pool)
            .await
            .unwrap();
        lookup.request_preview(&pool, 11).await.unwrap();
        assert_eq!(previews::discard_pending(&pool).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn previews_are_off_by_default_and_need_title_lookup() {
        let (_directory, pool) = database().await;
        sqlx::query("UPDATE links SET title = 'Kept' WHERE id = 1")
            .execute(&pool)
            .await
            .unwrap();
        let defaults = TitleLookup::new(&settings(true, false));
        assert!(!defaults.previews_enabled());
        assert!(
            !TitleLookup::new(&RuntimeSettings::detached(|_| None).unwrap()).previews_enabled()
        );
        assert_eq!(
            defaults.request_preview(&pool, 1).await,
            Err(previews::RequestError::Disabled)
        );
        // With title lookup on but previews off, the worker never queues a preview.
        assert!(!defaults.run_once(&pool).await.unwrap());
        let without_titles = TitleLookup::new(&settings(false, true));
        assert!(!without_titles.previews_enabled());
        assert_eq!(
            without_titles.request_preview(&pool, 1).await,
            Err(previews::RequestError::TitleLookupRequired)
        );
        assert!(!without_titles.run_once(&pool).await.unwrap());
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM link_previews")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }
}
