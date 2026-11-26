use serde::Serialize;
use sqlx::{QueryBuilder, SqliteConnection, SqlitePool};

use crate::{history::{self, Action, Actor}, store::push_host_scope};

pub(crate) struct ProtectionDraft {
    pub(crate) one_time: bool,
    pub(crate) password_hash: Option<String>,
}

pub(crate) enum PasswordChange {
    Unchanged,
    Remove,
    Set(String),
}

#[derive(Serialize, sqlx::FromRow)]
pub(crate) struct ProtectionDetails {
    password_required: bool,
    one_time: bool,
    consumed: bool,
}

pub(crate) enum ProtectionError {
    Database,
    RequiresTemporaryRedirect,
    Consumed,
}

impl From<sqlx::Error> for ProtectionError {
    fn from(_: sqlx::Error) -> Self {
        Self::Database
    }
}

pub(crate) async fn insert(
    connection: &mut SqliteConnection,
    link_id: i64,
    draft: &ProtectionDraft,
) -> Result<(), sqlx::Error> {
    if draft.one_time || draft.password_hash.is_some() {
        sqlx::query(
            "INSERT INTO link_protection (link_id, password_hash, one_time) VALUES (?, ?, ?)",
        )
        .bind(link_id)
        .bind(draft.password_hash.as_deref())
        .bind(draft.one_time)
        .execute(connection)
        .await?;
    }
    Ok(())
}

pub(crate) async fn read(
    pool: &SqlitePool,
    hosts: &[String],
    id: i64,
) -> Result<Option<ProtectionDetails>, sqlx::Error> {
    let mut query = QueryBuilder::new(
        "SELECT protection.password_hash IS NOT NULL AS password_required,
                COALESCE(protection.one_time, 0) AS one_time,
                COALESCE(protection.consumed, 0) AS consumed
         FROM links LEFT JOIN link_protection AS protection ON protection.link_id = links.id
         WHERE links.id = ",
    );
    query.push_bind(id);
    push_host_scope(&mut query, hosts);
    query.build_query_as().fetch_optional(pool).await
}

pub(crate) async fn replace(
    pool: &SqlitePool,
    hosts: &[String],
    id: i64,
    one_time: bool,
    password: PasswordChange,
    actor: &Actor,
) -> Result<Option<ProtectionDetails>, ProtectionError> {
    let mut transaction = pool.begin_with("BEGIN IMMEDIATE").await?;
    let mut query = QueryBuilder::new(
        "SELECT protection.password_hash, COALESCE(protection.one_time, 0),
                COALESCE(protection.consumed, 0), COALESCE(policy.status, 302)
         FROM links
         LEFT JOIN link_protection AS protection ON protection.link_id = links.id
         LEFT JOIN link_redirect_policies AS policy ON policy.link_id = links.id
         WHERE links.id = ",
    );
    query.push_bind(id);
    push_host_scope(&mut query, hosts);
    let Some((current_hash, current_one_time, consumed, status)) = query
        .build_query_as::<(Option<String>, bool, bool, i64)>()
        .fetch_optional(&mut *transaction)
        .await?
    else {
        return Ok(None);
    };

    if consumed && !one_time {
        return Err(ProtectionError::Consumed);
    }
    let updated_hash = match &password {
        PasswordChange::Unchanged => current_hash.as_deref(),
        PasswordChange::Remove => None,
        PasswordChange::Set(hash) => Some(hash.as_str()),
    };
    let password_required = updated_hash.is_some();
    if (one_time || password_required) && matches!(status, 301 | 308) {
        return Err(ProtectionError::RequiresTemporaryRedirect);
    }

    if updated_hash != current_hash.as_deref() || one_time != current_one_time {
        sqlx::query(
            "INSERT INTO link_protection (link_id, password_hash, one_time) VALUES (?, ?, ?)
             ON CONFLICT(link_id) DO UPDATE SET password_hash = excluded.password_hash,
               one_time = excluded.one_time, revision = link_protection.revision + 1",
        )
        .bind(id)
        .bind(updated_hash)
        .bind(one_time)
        .execute(&mut *transaction)
        .await?;
        history::record(&mut transaction, id, actor, Action::Protection, None).await?;
    }
    transaction.commit().await?;
    Ok(Some(ProtectionDetails {
        password_required,
        one_time,
        consumed,
    }))
}
