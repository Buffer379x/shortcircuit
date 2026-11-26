use serde::{Deserialize, Serialize};
use sqlx::{QueryBuilder, SqliteConnection, SqlitePool};

use crate::{
    destinations,
    input::{link_tags, link_title, plain_description, valid_target},
    redirect::{self, Policy, PolicyError},
    store::{self, Link, LinkUpdate, push_host_scope},
};

const MAX_ENTRIES: i64 = 25;
const RETENTION_SECONDS: i64 = 30 * 24 * 60 * 60;
const PRUNE_BATCH: i64 = 512;

#[derive(Clone)]
pub(crate) enum Actor {
    Admin(String),
    ApiKey(i64),
}

impl Actor {
    pub(crate) fn is_admin(&self) -> bool {
        matches!(self, Self::Admin(_))
    }

    fn identity(&self) -> (&'static str, String) {
        match self {
            Self::Admin(username) => ("admin", username.clone()),
            Self::ApiKey(id) => ("api_key", id.to_string()),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Action {
    Metadata,
    Policy,
    Protection,
    Restore,
}

impl Action {
    fn as_str(self) -> &'static str {
        match self {
            Self::Metadata => "metadata",
            Self::Policy => "policy",
            Self::Protection => "protection",
            Self::Restore => "restore",
        }
    }
}

#[derive(Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Metadata {
    target: String,
    title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    tags: Vec<String>,
    enabled: bool,
    analytics_enabled: bool,
}

impl From<&Link> for Metadata {
    fn from(link: &Link) -> Self {
        Self {
            target: link.target.clone(),
            title: link.title.clone(),
            description: Some(link.description.clone()),
            tags: link.tags.clone(),
            enabled: link.enabled,
            analytics_enabled: link.analytics_enabled,
        }
    }
}

impl Metadata {
    fn into_update(self) -> Result<LinkUpdate, RestoreError> {
        if !valid_target(&self.target) {
            return Err(RestoreError::Invalid("invalid_target"));
        }
        let title = link_title(&self.title).ok_or(RestoreError::Invalid("invalid_title"))?;
        let description = self
            .description
            .as_deref()
            .map(|value| {
                plain_description(value)
                    .map(str::to_owned)
                    .ok_or(RestoreError::Invalid("invalid_description"))
            })
            .transpose()?;
        let tags = link_tags(self.tags).ok_or(RestoreError::Invalid("invalid_tags"))?;
        Ok(LinkUpdate {
            target: Some(self.target),
            title: Some(title.to_owned()),
            description,
            enabled: Some(self.enabled),
            analytics_enabled: Some(self.analytics_enabled),
            tags: Some(tags),
        })
    }
}

#[derive(Deserialize, Serialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub(crate) enum Snapshot {
    Metadata(Metadata),
    Policy(Policy),
}

#[derive(Serialize)]
struct Current {
    metadata: Metadata,
    policy: Policy,
}

#[derive(Serialize)]
struct Attribution {
    kind: String,
    label: String,
}

#[derive(Serialize)]
struct Entry {
    id: i64,
    created_at: i64,
    actor: Attribution,
    action: String,
    previous: Option<Snapshot>,
}

#[derive(sqlx::FromRow)]
struct StoredEntry {
    id: i64,
    created_at: i64,
    actor_kind: String,
    actor_label: String,
    action: String,
    previous: Option<String>,
}

impl TryFrom<StoredEntry> for Entry {
    type Error = sqlx::Error;

    fn try_from(row: StoredEntry) -> Result<Self, Self::Error> {
        Ok(Self {
            id: row.id,
            created_at: row.created_at,
            actor: Attribution {
                kind: row.actor_kind,
                label: row.actor_label,
            },
            action: row.action,
            previous: row.previous.as_deref().map(decode).transpose()?,
        })
    }
}

#[derive(Serialize)]
pub(crate) struct LinkHistory {
    current: Current,
    items: Vec<Entry>,
}

pub(crate) enum RestoreError {
    NotFound,
    NotRestorable,
    Invalid(&'static str),
    Database,
}

impl From<sqlx::Error> for RestoreError {
    fn from(_: sqlx::Error) -> Self {
        Self::Database
    }
}

impl From<PolicyError> for RestoreError {
    fn from(error: PolicyError) -> Self {
        match error {
            PolicyError::Invalid(code) => Self::Invalid(code),
            PolicyError::Database(_) => Self::Database,
        }
    }
}

fn decode(value: &str) -> Result<Snapshot, sqlx::Error> {
    serde_json::from_str(value).map_err(|error| sqlx::Error::Decode(Box::new(error)))
}

pub(crate) async fn record(
    connection: &mut SqliteConnection,
    link_id: i64,
    actor: &Actor,
    action: Action,
    previous: Option<Snapshot>,
) -> Result<(), sqlx::Error> {
    let (kind, label) = actor.identity();
    let previous = previous
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|error| sqlx::Error::Encode(Box::new(error)))?;
    sqlx::query(
        "INSERT INTO link_history (link_id, actor_kind, actor_label, action, previous)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(link_id)
    .bind(kind)
    .bind(label)
    .bind(action.as_str())
    .bind(previous)
    .execute(&mut *connection)
    .await?;

    sqlx::query(
        "DELETE FROM link_history WHERE link_id = ?
         AND (created_at <= unixepoch() - ? OR id <= COALESCE(
             (SELECT id FROM link_history WHERE link_id = ? ORDER BY id DESC LIMIT 1 OFFSET ?), 0
         ))",
    )
    .bind(link_id)
    .bind(RETENTION_SECONDS)
    .bind(link_id)
    .bind(MAX_ENTRIES)
    .execute(connection)
    .await?;
    Ok(())
}

pub(crate) async fn read(
    pool: &SqlitePool,
    hosts: &[String],
    link_id: i64,
) -> Result<Option<LinkHistory>, sqlx::Error> {
    let mut transaction = pool.begin().await?;
    let Some(link) = store::read_link(&mut transaction, hosts, link_id).await? else {
        return Ok(None);
    };
    let Some(policy) = redirect::read_policy(&mut *transaction, hosts, link_id).await? else {
        return Ok(None);
    };
    let items = sqlx::query_as::<_, StoredEntry>(
        "SELECT id, created_at, actor_kind, actor_label, action, previous FROM link_history
         WHERE link_id = ? AND created_at > unixepoch() - ? ORDER BY id DESC LIMIT ?",
    )
    .bind(link_id)
    .bind(RETENTION_SECONDS)
    .bind(MAX_ENTRIES)
    .fetch_all(&mut *transaction)
    .await?
    .into_iter()
    .map(Entry::try_from)
    .collect::<Result<_, _>>()?;
    transaction.commit().await?;
    Ok(Some(LinkHistory {
        current: Current {
            metadata: Metadata::from(&link),
            policy: policy.policy,
        },
        items,
    }))
}

pub(crate) async fn purge(
    pool: &SqlitePool,
    hosts: &[String],
    link_id: i64,
) -> Result<bool, sqlx::Error> {
    let mut transaction = pool.begin_with("BEGIN IMMEDIATE").await?;
    let mut query = QueryBuilder::new("SELECT EXISTS(SELECT 1 FROM links WHERE id = ");
    query.push_bind(link_id);
    push_host_scope(&mut query, hosts);
    query.push(")");
    let visible = query
        .build_query_scalar::<bool>()
        .fetch_one(&mut *transaction)
        .await?;
    if !visible {
        return Ok(false);
    }
    sqlx::query("DELETE FROM link_history WHERE link_id = ?")
        .bind(link_id)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    Ok(true)
}

pub(crate) async fn restore(
    pool: &SqlitePool,
    hosts: &[String],
    link_id: i64,
    revision: i64,
    actor: &Actor,
) -> Result<(), RestoreError> {
    let mut transaction = pool.begin_with("BEGIN IMMEDIATE").await?;
    let mut query = QueryBuilder::new(
        "SELECT history.previous FROM link_history AS history
         JOIN links ON links.id = history.link_id WHERE links.id = ",
    );
    query
        .push_bind(link_id)
        .push(" AND history.id = ")
        .push_bind(revision)
        .push(" AND history.created_at > unixepoch() - ")
        .push_bind(RETENTION_SECONDS);
    push_host_scope(&mut query, hosts);
    let previous = query
        .build_query_scalar::<Option<String>>()
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(RestoreError::NotFound)?
        .ok_or(RestoreError::NotRestorable)?;
    match decode(&previous)? {
        Snapshot::Metadata(metadata) => {
            let update = metadata.into_update()?;
            let current: Option<String> =
                sqlx::query_scalar("SELECT target FROM links WHERE id = ?")
                    .bind(link_id)
                    .fetch_optional(&mut *transaction)
                    .await?;
            let unchanged: Vec<&str> = current.as_deref().into_iter().collect();
            if destinations::check_writes(&mut *transaction, update.target.as_deref(), &unchanged)
                .await?
                .is_err()
            {
                return Err(RestoreError::Invalid("destination_not_allowed"));
            }
            store::update_link_in(
                &mut transaction,
                hosts,
                link_id,
                update,
                actor,
                Action::Restore,
            )
            .await?
            .ok_or(RestoreError::NotFound)?;
        }
        Snapshot::Policy(policy) => {
            redirect::replace_policy_in(
                &mut transaction,
                hosts,
                link_id,
                policy,
                actor,
                Action::Restore,
            )
            .await?
            .ok_or(RestoreError::NotFound)?;
        }
    }
    transaction.commit().await?;
    Ok(())
}

pub(crate) async fn prune(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "DELETE FROM link_history WHERE id IN (
             SELECT id FROM link_history WHERE created_at <= unixepoch() - ? ORDER BY created_at LIMIT ?
         )",
    )
    .bind(RETENTION_SECONDS)
    .bind(PRUNE_BATCH)
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn history_caps_each_link_and_expires_before_physical_cleanup() {
        let directory = tempfile::tempdir().unwrap();
        let pool = store::open_database(&directory.path().join("links.sqlite"))
            .await
            .unwrap();
        let host = "go.example".to_owned();
        let link = store::insert_link(
            &pool,
            &store::LinkDraft {
                host: host.clone(),
                slug: "history".to_owned(),
                target: "https://example.test/".to_owned(),
                title: "Initial title".to_owned(),
                description: String::new(),
                enabled: true,
                analytics_enabled: true,
                tags: Vec::new(),
            },
            None,
        )
        .await
        .unwrap()
        .unwrap();
        let hosts = [host];
        let actor = Actor::Admin("owner".to_owned());
        for index in 0..(MAX_ENTRIES + 2) {
            let update = LinkUpdate {
                title: Some(format!("Title {index}")),
                ..LinkUpdate::default()
            };
            store::update_link(&pool, &hosts, link.id, update, &actor)
                .await
                .unwrap()
                .unwrap();
        }
        let history = read(&pool, &hosts, link.id).await.unwrap().unwrap();
        assert_eq!(history.items.len(), MAX_ENTRIES as usize);
        assert_eq!(history.items.last().unwrap().id, 3);
        let expired = history.items[0].id;
        sqlx::query("UPDATE link_history SET created_at = unixepoch() - ? WHERE id = ?")
            .bind(RETENTION_SECONDS)
            .bind(expired)
            .execute(&pool)
            .await
            .unwrap();
        let visible = read(&pool, &hosts, link.id).await.unwrap().unwrap();
        assert_eq!(visible.items.len(), MAX_ENTRIES as usize - 1);
        assert!(matches!(
            restore(&pool, &hosts, link.id, expired, &actor).await,
            Err(RestoreError::NotFound)
        ));
        prune(&pool).await.unwrap();
        let stored: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM link_history WHERE link_id = ?")
            .bind(link.id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(stored, MAX_ENTRIES - 1);
    }
}
