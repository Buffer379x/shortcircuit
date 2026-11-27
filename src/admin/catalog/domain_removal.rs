//! Domain deletion: a read-only preview and a single-transaction delete that moves or deletes
//! the domain's links first.
//!
//! Hosts from `PUBLIC_HOSTS` can be deleted too. The delete removes their saved settings and
//! links, and the host stays served with default settings until it is removed from the
//! environment. The preview and the result report this as `environment`.

use axum::{
    Extension, Json,
    extract::{Query, State, rejection::JsonRejection, rejection::QueryRejection},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::SqliteConnection;

use crate::{
    admin::admin_security::{AuditAction, AuditActor, AuditOutcome, record_audit_details},
    domains,
    history::Actor,
    input::{PublicHosts, normalize_host},
    store::bulk,
};

use super::{CatalogState, Problem, json_payload};

/// The preview lists at most this many conflicting slugs and reports the total separately.
const LISTED_CONFLICTS: i64 = 20;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PreviewQuery {
    host: String,
    target: Option<String>,
}

#[derive(Serialize)]
pub(super) struct Preview {
    host: String,
    /// Saved settings exist for the host.
    managed: bool,
    /// The host is listed in `PUBLIC_HOSTS` and stays served after deletion.
    environment: bool,
    /// Links on the host, archived and disabled ones included.
    links: i64,
    /// Unexpired API keys restricted to this host. A host that is not in `PUBLIC_HOSTS` cannot
    /// be deleted while any exist.
    api_keys: i64,
    /// Active domains that can receive the links.
    targets: Vec<String>,
    #[serde(rename = "move", skip_serializing_if = "Option::is_none")]
    move_to: Option<MovePreview>,
}

#[derive(Serialize)]
struct MovePreview {
    target: String,
    conflicts: i64,
    conflicting_slugs: Vec<String>,
}

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum LinkAction {
    Move,
    Delete,
    None,
}

impl LinkAction {
    fn label(self) -> &'static str {
        match self {
            Self::Move => "move",
            Self::Delete => "delete",
            Self::None => "none",
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Removal {
    host: String,
    links: LinkAction,
    target: Option<String>,
    confirm: Option<String>,
    /// The link count from the preview. A different count inside the transaction returns 409.
    expected_links: i64,
}

#[derive(Serialize)]
pub(super) struct Removed {
    host: String,
    links: LinkAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    target: Option<String>,
    moved: u64,
    deleted: u64,
    environment: bool,
}

struct Facts {
    managed: bool,
    environment: bool,
    links: i64,
    api_keys: i64,
}

pub(super) async fn preview(
    State(state): State<CatalogState>,
    query: Result<Query<PreviewQuery>, QueryRejection>,
) -> Result<Json<Preview>, Problem> {
    let Query(query) = query.map_err(|_| Problem::invalid("invalid_domain_deletion"))?;
    let host = host(&query.host)?;
    let mut connection = state
        .pool
        .acquire()
        .await
        .map_err(|_| Problem::unavailable())?;
    let facts = facts(&mut connection, &state.hosts, &host).await?;
    let targets = targets(&mut connection, &state.hosts, &host).await?;
    let move_to = match query.target {
        Some(target) => {
            let target = move_target(&targets, &target)?;
            let (conflicts, conflicting_slugs) = conflicts(&mut connection, &host, &target)
                .await
                .map_err(|_| Problem::unavailable())?;
            Some(MovePreview {
                target,
                conflicts,
                conflicting_slugs,
            })
        }
        None => None,
    };
    Ok(Json(Preview {
        host,
        managed: facts.managed,
        environment: facts.environment,
        links: facts.links,
        api_keys: facts.api_keys,
        targets,
        move_to,
    }))
}

pub(super) async fn delete(
    State(state): State<CatalogState>,
    Extension(actor): Extension<Actor>,
    body: Result<Json<Removal>, JsonRejection>,
) -> Result<Json<Removed>, Problem> {
    // The catalog accepts browser sessions only; this guards against a future route change.
    if !matches!(actor, Actor::Admin(_)) {
        return Err(Problem(StatusCode::FORBIDDEN, "insufficient_scope"));
    }
    let input = json_payload(body)?;
    let host = host(&input.host)?;
    if input.expected_links < 0
        || (input.links == LinkAction::Move) != input.target.is_some()
        || (input.links == LinkAction::None && input.expected_links != 0)
    {
        return Err(Problem::invalid("invalid_domain_deletion"));
    }
    let confirmed = input.confirm.as_deref().map(str::trim);
    if confirmed.is_some_and(|value| normalize_host(value).as_deref() != Some(host.as_str()))
        || (input.links == LinkAction::Delete && confirmed.is_none())
    {
        return Err(Problem::invalid("domain_delete_confirmation"));
    }

    let mut transaction = state
        .pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(|_| Problem::unavailable())?;
    let facts = facts(&mut transaction, &state.hosts, &host).await?;
    if facts.links != input.expected_links {
        return Err(changed());
    }
    if !facts.environment && facts.api_keys > 0 {
        return Err(Problem(StatusCode::CONFLICT, "domain_has_api_keys"));
    }
    let mut target = None;
    let (mut moved, mut deleted) = (0, 0);
    match input.links {
        LinkAction::None => {}
        LinkAction::Move => {
            let targets = targets(&mut transaction, &state.hosts, &host).await?;
            let destination = move_target(&targets, input.target.as_deref().unwrap_or_default())?;
            let (conflicts, _) = conflicts(&mut transaction, &host, &destination)
                .await
                .map_err(|_| Problem::unavailable())?;
            if conflicts > 0 {
                return Err(Problem(StatusCode::CONFLICT, "domain_move_conflict"));
            }
            moved = sqlx::query("UPDATE links SET host = ? WHERE host = ?")
                .bind(&destination)
                .bind(&host)
                .execute(&mut *transaction)
                .await
                .map_err(|_| Problem::unavailable())?
                .rows_affected();
            target = Some(destination);
        }
        LinkAction::Delete => {
            deleted = bulk::delete_on_host(&mut transaction, &host)
                .await
                .map_err(|_| Problem::unavailable())?;
        }
    }
    if moved + deleted != facts.links as u64 {
        return Err(changed());
    }
    sqlx::query("DELETE FROM domains WHERE host = ?")
        .bind(&host)
        .execute(&mut *transaction)
        .await
        .map_err(|_| Problem::unavailable())?;
    let details = json!({
        "host": host,
        "links": input.links.label(),
        "target": target,
        "count": facts.links,
        "environment": facts.environment,
    });
    record_audit_details(
        &mut transaction,
        AuditAction::DomainDelete,
        AuditActor::Admin,
        None,
        AuditOutcome::Success,
        Some(&details),
    )
    .await
    .map_err(|_| Problem::unavailable())?;
    transaction
        .commit()
        .await
        .map_err(|_| Problem::unavailable())?;
    tracing::info!(
        links_moved = moved,
        links_deleted = deleted,
        environment = facts.environment,
        "domain deletion committed"
    );
    Ok(Json(Removed {
        host,
        links: input.links,
        target,
        moved,
        deleted,
        environment: facts.environment,
    }))
}

fn host(value: &str) -> Result<String, Problem> {
    normalize_host(value.trim()).ok_or_else(|| Problem::invalid("invalid_host"))
}

fn changed() -> Problem {
    Problem(StatusCode::CONFLICT, "domain_deletion_changed")
}

/// Reads what deleting `host` affects. A host that is neither saved nor in `PUBLIC_HOSTS` is
/// not found.
async fn facts(
    connection: &mut SqliteConnection,
    hosts: &PublicHosts,
    host: &str,
) -> Result<Facts, Problem> {
    let (managed, links, api_keys) = sqlx::query_as::<_, (bool, i64, i64)>(
        "SELECT EXISTS (SELECT 1 FROM domains WHERE host = ?1),
                (SELECT count(*) FROM links WHERE host = ?1),
                (SELECT count(*) FROM admin_api_keys
                 WHERE host = ?1 AND (expires_at IS NULL OR expires_at > unixepoch()))",
    )
    .bind(host)
    .fetch_one(&mut *connection)
    .await
    .map_err(|_| Problem::unavailable())?;
    let environment = hosts.contains(host);
    if !managed && !environment {
        return Err(Problem(StatusCode::NOT_FOUND, "not_found"));
    }
    Ok(Facts {
        managed,
        environment,
        links,
        api_keys,
    })
}

/// Active domains other than `host`. These are the hosts a new link may use.
async fn targets(
    connection: &mut SqliteConnection,
    hosts: &PublicHosts,
    host: &str,
) -> Result<Vec<String>, Problem> {
    Ok(domains::active_hosts(&mut *connection, hosts)
        .await
        .map_err(|_| Problem::unavailable())?
        .into_iter()
        .filter(|item| item != host)
        .collect())
}

fn move_target(targets: &[String], value: &str) -> Result<String, Problem> {
    normalize_host(value.trim())
        .filter(|target| targets.contains(target))
        .ok_or_else(|| Problem::invalid("invalid_move_target"))
}

/// Slugs on `host` that already exist on `target`, the first few in slug order, and their total.
async fn conflicts(
    connection: &mut SqliteConnection,
    host: &str,
    target: &str,
) -> Result<(i64, Vec<String>), sqlx::Error> {
    let rows = sqlx::query_as::<_, (String, i64)>(
        "SELECT moving.slug, count(*) OVER ()
         FROM links AS moving
         WHERE moving.host = ?1
           AND EXISTS (SELECT 1 FROM links WHERE host = ?2 AND slug = moving.slug)
         ORDER BY moving.slug
         LIMIT ?3",
    )
    .bind(host)
    .bind(target)
    .bind(LISTED_CONFLICTS)
    .fetch_all(connection)
    .await?;
    let total = rows.first().map_or(0, |row| row.1);
    Ok((total, rows.into_iter().map(|row| row.0).collect()))
}
