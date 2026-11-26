use std::{
    error::Error,
    fs::{DirBuilder, OpenOptions},
    io,
    path::Path,
    time::Duration,
};

use serde::Serialize;
use sqlx::{
    Connection, QueryBuilder, Sqlite, SqliteConnection, SqlitePool,
    migrate::Migrator,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};

use crate::{
    history::{self, Action, Actor, Metadata, Snapshot},
    protection::{self, ProtectionDraft},
};

pub(crate) mod bulk;
pub(crate) mod catalog;

static MIGRATIONS: Migrator = sqlx::migrate!();
const MIN_SQLITE: [u32; 3] = [3, 51, 3];
const DATABASE_TIMEOUT: Duration = Duration::from_secs(2);
const MIGRATION_TIMEOUT: Duration = Duration::from_secs(60);
pub(crate) const PAGE_SIZE: usize = 100;

#[derive(Serialize, sqlx::FromRow)]
pub(crate) struct Link {
    pub(crate) id: i64,
    pub(crate) host: String,
    pub(crate) slug: String,
    pub(crate) target: String,
    pub(crate) created_at: String,
    pub(crate) title: String,
    pub(crate) description: String,
    pub(crate) enabled: bool,
    pub(crate) analytics_enabled: bool,
    #[sqlx(skip)]
    pub(crate) tags: Vec<String>,
}

pub(crate) struct LinkDraft {
    pub(crate) host: String,
    pub(crate) slug: String,
    pub(crate) target: String,
    pub(crate) title: String,
    pub(crate) description: String,
    pub(crate) enabled: bool,
    pub(crate) analytics_enabled: bool,
    pub(crate) tags: Vec<String>,
}

#[derive(Default)]
pub(crate) struct LinkUpdate {
    pub(crate) target: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) enabled: Option<bool>,
    pub(crate) analytics_enabled: Option<bool>,
    pub(crate) tags: Option<Vec<String>>,
}

#[derive(sqlx::FromRow)]
struct TaggedLink {
    #[sqlx(flatten)]
    link: Link,
    tag: Option<String>,
}

pub async fn open_database(path: &Path) -> Result<SqlitePool, Box<dyn Error + Send + Sync>> {
    open_database_with_timeout(path, DATABASE_TIMEOUT).await
}

pub async fn open_database_with_timeout(
    path: &Path,
    busy_timeout: Duration,
) -> Result<SqlitePool, Box<dyn Error + Send + Sync>> {
    if !(Duration::from_millis(1)..=Duration::from_secs(10)).contains(&busy_timeout) {
        return Err(io::Error::other("SQLite busy timeout must be 1 to 10000 milliseconds").into());
    }
    let mut probe =
        SqliteConnection::connect_with(&SqliteConnectOptions::new().in_memory(true)).await?;
    let version: String = sqlx::query_scalar("SELECT sqlite_version()")
        .fetch_one(&mut probe)
        .await?;
    probe.close().await?;
    let parts = version
        .split('.')
        .map(str::parse)
        .collect::<Result<Vec<u32>, _>>()?;
    if parts.as_slice() < MIN_SQLITE.as_slice() {
        return Err(io::Error::other(format!(
            "SQLite {version} does not meet the minimum version 3.51.3"
        ))
        .into());
    }
    prepare_file(path)?;

    let opts = SqliteConnectOptions::new()
        .filename(path)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Full)
        .foreign_keys(true)
        .busy_timeout(busy_timeout);
    // Run migrations on a dedicated single connection before the request pool exists. A long
    // upgrade or a busy host then cannot exceed the pool's short acquisition deadline.
    let migration = SqlitePoolOptions::new()
        .max_connections(2)
        .acquire_timeout(MIGRATION_TIMEOUT)
        .connect_with(opts.clone())
        .await?;
    snapshot_before_upgrade(&migration, path).await?;
    MIGRATIONS.run(&migration).await?;
    migration.close().await;
    let pool = SqlitePoolOptions::new()
        .min_connections(1)
        .max_connections(4)
        .acquire_timeout(DATABASE_TIMEOUT)
        .connect_with(opts)
        .await?;
    Ok(pool)
}

/// Number of verified pre-upgrade snapshots to keep.
const PRE_UPGRADE_KEEP: usize = 2;

/// Before pending migrations change an existing database, writes a verified snapshot to
/// `backups/pre-upgrade`. The previous release can restore it if the upgrade has to be undone.
/// Startup stops if the snapshot cannot be written, so no upgrade runs without one.
async fn snapshot_before_upgrade(
    pool: &SqlitePool,
    database: &Path,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let tracked: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = '_sqlx_migrations')",
    )
    .fetch_one(pool)
    .await?;
    if !tracked {
        return Ok(());
    }
    let current: Option<i64> =
        sqlx::query_scalar("SELECT max(version) FROM _sqlx_migrations WHERE success = 1")
            .fetch_one(pool)
            .await?;
    let latest = MIGRATIONS.iter().map(|migration| migration.version).max();
    let Some(current) = current.filter(|current| latest.is_some_and(|latest| *current < latest))
    else {
        return Ok(());
    };
    let directory = database.with_file_name("backups").join("pre-upgrade");
    let snapshot = crate::backup::create_snapshot(pool, &directory)
        .await
        .map_err(|error| {
            io::Error::other(format!(
                "could not write the pre-upgrade snapshot of schema {current} to {}: {error}. \
                 Free space on the data volume or fix its permissions, then start again",
                directory.display()
            ))
        })?;
    tracing::info!(
        schema = current,
        snapshot = %snapshot.display(),
        "verified pre-upgrade snapshot before applying migrations"
    );
    let kept = snapshot.clone();
    if !matches!(
        tokio::task::spawn_blocking(move || crate::backup_schedule::prune(&kept, PRE_UPGRADE_KEEP))
            .await,
        Ok(Ok(()))
    ) {
        tracing::warn!("could not remove older pre-upgrade snapshots");
    }
    Ok(())
}

/// Creates a database that stops at an earlier schema version for upgrade tests.
/// Open it again with `open_database` to apply the remaining migrations.
#[cfg(test)]
pub(crate) async fn open_database_at_version(path: &Path, version: i64) -> SqlitePool {
    prepare_file(path).unwrap();
    let opts = SqliteConnectOptions::new()
        .filename(path)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await
        .unwrap();
    MIGRATIONS.run_to(version, &pool).await.unwrap();
    pool
}

/// Creates the directory named by `SQLITE_TMPDIR`, if set. SQLite writes statement journals,
/// sort spills, and `VACUUM` copies there. The container image points it at the data volume
/// because its root filesystem is read-only.
pub(crate) fn prepare_temp_directory() -> io::Result<()> {
    let Some(directory) = std::env::var_os("SQLITE_TMPDIR").filter(|value| !value.is_empty())
    else {
        return Ok(());
    };
    let mut builder = DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&directory).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "cannot create SQLITE_TMPDIR {}: {error}",
                Path::new(&directory).display()
            ),
        )
    })
}

fn prepare_file(path: &Path) -> io::Result<()> {
    prepare_temp_directory()?;
    let mut directories = DirBuilder::new();
    directories.recursive(true);
    let mut file = OpenOptions::new();
    file.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
        directories.mode(0o700);
        file.mode(0o600);
    }
    let context = |error: io::Error, target: &Path| {
        io::Error::new(
            error.kind(),
            format!(
                "cannot prepare {}: {error}. The data directory must be writable by the user \
                 that runs ShortCircuit (UID 10001 in the container image)",
                target.display()
            ),
        )
    };
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        directories
            .create(parent)
            .map_err(|error| context(error, parent))?;
    }
    match file.open(path) {
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(context(error, path)),
    }
}

pub(crate) async fn insert_link(
    pool: &SqlitePool,
    draft: &LinkDraft,
    protection: Option<&ProtectionDraft>,
) -> Result<Option<Link>, sqlx::Error> {
    let mut transaction = pool.begin().await?;
    let rows = sqlx::query_as::<_, Link>(
        "INSERT INTO links (host, slug, target, title, description, enabled, analytics_enabled)
         VALUES (?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(host, slug) DO NOTHING
         RETURNING id, host, slug, target, created_at, title, description, enabled, analytics_enabled",
    )
    .bind(&draft.host)
    .bind(&draft.slug)
    .bind(&draft.target)
    .bind(&draft.title)
    .bind(&draft.description)
    .bind(draft.enabled)
    .bind(draft.analytics_enabled)
    // The statement can fail at completion after SQLite has emitted the RETURNING row.
    .fetch_all(&mut *transaction)
    .await?;
    let link = rows.into_iter().next();
    let result = if let Some(mut link) = link {
        link.tags = insert_tags(&mut transaction, link.id, draft.tags.clone()).await?;
        if let Some(settings) = protection {
            protection::insert(&mut transaction, link.id, settings).await?;
        }
        Some(link)
    } else {
        None
    };
    transaction.commit().await?;
    Ok(result)
}

pub(crate) async fn update_link(
    pool: &SqlitePool,
    hosts: &[String],
    id: i64,
    update: LinkUpdate,
    actor: &Actor,
) -> Result<Option<Link>, sqlx::Error> {
    let mut transaction = pool.begin_with("BEGIN IMMEDIATE").await?;
    let result =
        update_link_in(&mut transaction, hosts, id, update, actor, Action::Metadata).await?;
    transaction.commit().await?;
    Ok(result)
}

pub(crate) async fn read_link(
    connection: &mut SqliteConnection,
    hosts: &[String],
    id: i64,
) -> Result<Option<Link>, sqlx::Error> {
    let mut query = QueryBuilder::new(
        "SELECT id, host, slug, target, created_at, title, description, enabled, analytics_enabled
         FROM links WHERE id = ",
    );
    query.push_bind(id);
    push_host_scope(&mut query, hosts);
    let Some(mut link) = query
        .build_query_as::<Link>()
        .fetch_optional(&mut *connection)
        .await?
    else {
        return Ok(None);
    };
    link.tags = sqlx::query_scalar("SELECT tag FROM link_tags WHERE link_id = ? ORDER BY tag")
        .bind(id)
        .fetch_all(connection)
        .await?;
    Ok(Some(link))
}

pub(crate) async fn update_link_in(
    connection: &mut SqliteConnection,
    hosts: &[String],
    id: i64,
    update: LinkUpdate,
    actor: &Actor,
    action: Action,
) -> Result<Option<Link>, sqlx::Error> {
    let Some(link) = read_link(connection, hosts, id).await? else {
        return Ok(None);
    };
    write_link_in(connection, link, update, actor, action)
        .await
        .map(Some)
}

pub(crate) async fn write_link_in(
    connection: &mut SqliteConnection,
    mut link: Link,
    update: LinkUpdate,
    actor: &Actor,
    action: Action,
) -> Result<Link, sqlx::Error> {
    let id = link.id;
    let previous = Metadata::from(&link);
    link.target = update.target.unwrap_or(link.target);
    link.title = update.title.unwrap_or(link.title);
    link.description = update.description.unwrap_or(link.description);
    link.enabled = update.enabled.unwrap_or(link.enabled);
    link.analytics_enabled = update.analytics_enabled.unwrap_or(link.analytics_enabled);
    let tags_changed = if let Some(mut tags) = update.tags {
        tags.sort_unstable();
        tags.dedup();
        let changed = tags != link.tags;
        link.tags = tags;
        changed
    } else {
        false
    };
    if previous == Metadata::from(&link) {
        return Ok(link);
    }

    sqlx::query(
        "UPDATE links SET target = ?, title = ?, description = ?, enabled = ?,
         analytics_enabled = ? WHERE id = ?",
    )
    .bind(&link.target)
    .bind(&link.title)
    .bind(&link.description)
    .bind(link.enabled)
    .bind(link.analytics_enabled)
    .bind(id)
    .execute(&mut *connection)
    .await?;
    if tags_changed {
        sqlx::query("DELETE FROM link_tags WHERE link_id = ?")
            .bind(id)
            .execute(&mut *connection)
            .await?;
        link.tags = insert_tags(connection, id, link.tags).await?;
    }
    history::record(
        connection,
        id,
        actor,
        action,
        Some(Snapshot::Metadata(previous)),
    )
    .await?;
    Ok(link)
}

async fn insert_tags(
    connection: &mut SqliteConnection,
    id: i64,
    mut tags: Vec<String>,
) -> Result<Vec<String>, sqlx::Error> {
    if !tags.is_empty() {
        tags.sort_unstable();
        tags.dedup();
        let mut query = QueryBuilder::new("INSERT INTO link_tags (link_id, tag) ");
        query.push_values(&tags, |mut values, tag| {
            values.push_bind(id).push_bind(tag);
        });
        query.build().execute(connection).await?;
    }
    Ok(tags)
}

pub(crate) async fn remove_link(
    pool: &SqlitePool,
    hosts: &[String],
    id: i64,
) -> Result<bool, sqlx::Error> {
    let mut query = QueryBuilder::new("DELETE FROM links WHERE id = ");
    query.push_bind(id);
    push_host_scope(&mut query, hosts);
    Ok(query.build().execute(pool).await?.rows_affected() == 1)
}

#[cfg(test)]
pub(crate) async fn resolve(
    pool: &SqlitePool,
    host: &str,
    slug: &str,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar("SELECT target FROM links WHERE host = ? AND slug = ? AND enabled = 1")
        .bind(host)
        .bind(slug)
        .fetch_optional(pool)
        .await
}

pub(crate) async fn list(
    pool: &SqlitePool,
    hosts: &[String],
    after_id: i64,
) -> Result<Vec<Link>, sqlx::Error> {
    let mut query = QueryBuilder::new(
        "WITH page AS (
            SELECT id, host, slug, target, created_at, title, description, enabled, analytics_enabled
            FROM links WHERE id > ",
    );
    query.push_bind(after_id);
    push_host_scope(&mut query, hosts);
    query
        .push(" ORDER BY id LIMIT ")
        .push_bind((PAGE_SIZE + 1) as i64)
        .push(
            ") SELECT page.*, link_tags.tag FROM page
              LEFT JOIN link_tags ON link_tags.link_id = page.id
              ORDER BY page.id, link_tags.tag",
        );
    let rows = query.build_query_as::<TaggedLink>().fetch_all(pool).await?;
    let mut links: Vec<Link> = Vec::new();
    for TaggedLink { link, tag } in rows {
        if links.last().is_none_or(|previous| previous.id != link.id) {
            links.push(link);
        }
        if let (Some(link), Some(tag)) = (links.last_mut(), tag) {
            link.tags.push(tag);
        }
    }
    Ok(links)
}

pub(crate) fn push_host_scope(query: &mut QueryBuilder<Sqlite>, hosts: &[String]) {
    query.push(" AND host IN (");
    let mut placeholders = query.separated(", ");
    for host in hosts {
        placeholders.push_bind(host);
    }
    placeholders.push_unseparated(")");
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOST: &str = "go.example";
    const TARGET: &str = "https://target.example/file?Signature=XL%2f&key=A+B&key=C#original";

    fn draft() -> LinkDraft {
        LinkDraft {
            host: HOST.to_owned(),
            slug: "Team/Docs".to_owned(),
            target: TARGET.to_owned(),
            title: "Team docs".to_owned(),
            description: String::new(),
            enabled: true,
            analytics_enabled: true,
            tags: vec!["work".to_owned(), "docs".to_owned(), "work".to_owned()],
        }
    }

    #[tokio::test]
    async fn upgrading_preserves_legacy_link_identity_and_destination() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::raw_sql(include_str!("../migrations/0001_links.sql"))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO links (id, host, slug, target, created_at) VALUES (42, ?, 'Team/Docs', ?, ?)",
        )
        .bind(HOST)
        .bind(TARGET)
        .bind("2026-11-25T00:00:00.000Z")
        .execute(&pool)
        .await
        .unwrap();
        sqlx::raw_sql(include_str!("../migrations/0003_link_metadata.sql"))
            .execute(&pool)
            .await
            .unwrap();

        sqlx::raw_sql(include_str!("../migrations/0006_analytics.sql"))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::raw_sql(include_str!("../migrations/0013_link_description.sql"))
            .execute(&pool)
            .await
            .unwrap();
        let links = list(&pool, &[HOST.to_owned()], 0).await.unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].id, 42);
        assert_eq!(links[0].slug, "Team/Docs");
        assert_eq!(links[0].created_at, "2026-11-25T00:00:00.000Z");
        assert_eq!(links[0].target, TARGET);
        assert_eq!(links[0].title, "");
        assert!(links[0].enabled);
        assert!(links[0].tags.is_empty());
        assert_eq!(
            resolve(&pool, HOST, "Team/Docs").await.unwrap().as_deref(),
            Some(TARGET)
        );
    }

    #[tokio::test]
    async fn pending_migrations_write_a_verified_snapshot_first() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("upgrade.sqlite");
        let pre_upgrade = directory.path().join("backups").join("pre-upgrade");
        // A new database has nothing to protect.
        open_database(&path).await.unwrap().close().await;
        assert!(!pre_upgrade.exists());
        for (index, version) in [15, 18, 20].into_iter().enumerate() {
            let other = directory.path().join(format!("old-{index}.sqlite"));
            let legacy = open_database_at_version(&other, version).await;
            sqlx::query("INSERT INTO links (host, slug, target) VALUES ('go.example', 'kept', ?)")
                .bind(TARGET)
                .execute(&legacy)
                .await
                .unwrap();
            legacy.close().await;
            std::fs::rename(&other, &path).unwrap();
            let _ = std::fs::remove_file(path.with_extension("sqlite-wal"));
            let _ = std::fs::remove_file(path.with_extension("sqlite-shm"));
            open_database(&path).await.unwrap().close().await;
        }
        let snapshots: Vec<_> = std::fs::read_dir(&pre_upgrade)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(snapshots.len(), PRE_UPGRADE_KEEP, "{snapshots:?}");
        for snapshot in snapshots {
            let pool = SqlitePoolOptions::new()
                .connect_with(
                    SqliteConnectOptions::new()
                        .filename(&snapshot)
                        .read_only(true),
                )
                .await
                .unwrap();
            let (version, target): (i64, String) = sqlx::query_as(
                "SELECT (SELECT max(version) FROM _sqlx_migrations), target FROM links",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            assert!(version < 23);
            assert_eq!(target, TARGET);
        }
        // An up-to-date database is opened without another snapshot.
        open_database(&path).await.unwrap().close().await;
        assert_eq!(
            std::fs::read_dir(&pre_upgrade).unwrap().count(),
            PRE_UPGRADE_KEEP
        );
    }

    #[tokio::test]
    async fn disabled_link_remains_listed_and_stops_resolving() {
        let directory = tempfile::tempdir().unwrap();
        let pool = open_database(&directory.path().join("links.sqlite"))
            .await
            .unwrap();
        let hosts = [HOST.to_owned()];
        let link = insert_link(&pool, &draft(), None).await.unwrap().unwrap();
        assert_eq!(link.tags, ["docs", "work"]);

        update_link(
            &pool,
            &hosts,
            link.id,
            LinkUpdate {
                target: None,
                title: Some("Archived docs".to_owned()),
                description: None,
                enabled: Some(false),
                analytics_enabled: Some(false),
                tags: None,
            },
            &Actor::Admin("test".to_owned()),
        )
        .await
        .unwrap()
        .unwrap();

        assert!(resolve(&pool, HOST, "Team/Docs").await.unwrap().is_none());
        let links = list(&pool, &hosts, 0).await.unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].id, link.id);
        assert!(!links[0].enabled);
        assert!(!links[0].analytics_enabled);
        assert_eq!(links[0].title, "Archived docs");
        assert_eq!(links[0].target, TARGET);
        assert_eq!(links[0].tags, ["docs", "work"]);
    }

    #[tokio::test]
    async fn failed_tag_commit_rolls_back_the_link_and_its_tags() {
        let directory = tempfile::tempdir().unwrap();
        let pool = open_database(&directory.path().join("links.sqlite"))
            .await
            .unwrap();
        let hosts = [HOST.to_owned()];
        let link = insert_link(&pool, &draft(), None).await.unwrap().unwrap();
        sqlx::raw_sql(
            "CREATE TABLE deferred_tag_check (
                link_id INTEGER REFERENCES links(id) DEFERRABLE INITIALLY DEFERRED
            );
            CREATE TRIGGER reject_tag_at_commit AFTER INSERT ON link_tags
            WHEN NEW.tag = 'reject'
            BEGIN
                INSERT INTO deferred_tag_check VALUES (-1);
            END;",
        )
        .execute(&pool)
        .await
        .unwrap();

        let result = update_link(
            &pool,
            &hosts,
            link.id,
            LinkUpdate {
                target: Some("https://target.example/changed".to_owned()),
                title: None,
                description: None,
                enabled: None,
                analytics_enabled: None,
                tags: Some(vec!["reject".to_owned()]),
            },
            &Actor::Admin("test".to_owned()),
        )
        .await;

        assert!(result.is_err());
        let links = list(&pool, &hosts, 0).await.unwrap();
        assert_eq!(links[0].target, TARGET);
        assert_eq!(links[0].tags, ["docs", "work"]);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM link_history")
                .fetch_one(&pool)
                .await
                .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn failed_history_write_rolls_back_metadata_and_tags() {
        let directory = tempfile::tempdir().unwrap();
        let pool = open_database(&directory.path().join("links.sqlite"))
            .await
            .unwrap();
        let hosts = [HOST.to_owned()];
        let link = insert_link(&pool, &draft(), None).await.unwrap().unwrap();
        sqlx::raw_sql(
            "CREATE TRIGGER reject_link_history BEFORE INSERT ON link_history
             BEGIN SELECT RAISE(ABORT, 'history write failed'); END;",
        )
        .execute(&pool)
        .await
        .unwrap();
        let update = LinkUpdate {
            target: Some("https://target.example/changed".to_owned()),
            tags: Some(vec!["edited".to_owned()]),
            ..LinkUpdate::default()
        };
        let actor = Actor::Admin("test".to_owned());
        assert!(
            update_link(&pool, &hosts, link.id, update, &actor)
                .await
                .is_err()
        );
        let links = list(&pool, &hosts, 0).await.unwrap();
        assert_eq!(links[0].target, TARGET);
        assert_eq!(links[0].tags, ["docs", "work"]);
    }

    #[tokio::test]
    async fn failed_protection_write_rolls_back_new_link_and_tags() {
        let directory = tempfile::tempdir().unwrap();
        let pool = open_database(&directory.path().join("links.sqlite"))
            .await
            .unwrap();
        sqlx::raw_sql(
            "CREATE TRIGGER reject_protection BEFORE INSERT ON link_protection
             BEGIN SELECT RAISE(ABORT, 'synthetic protection failure'); END;",
        )
        .execute(&pool)
        .await
        .unwrap();

        let draft = draft();
        let protection = ProtectionDraft {
            one_time: true,
            password_hash: None,
        };
        assert!(insert_link(&pool, &draft, Some(&protection)).await.is_err());
        let links = list(&pool, &[HOST.to_owned()], 0).await.unwrap();
        assert!(links.is_empty());
        let tags: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM link_tags")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(tags, 0);
    }

    #[tokio::test]
    async fn only_matching_hosts_can_delete_and_deleted_ids_stay_behind_cursors() {
        let directory = tempfile::tempdir().unwrap();
        let pool = open_database(&directory.path().join("links.sqlite"))
            .await
            .unwrap();
        let hosts = [HOST.to_owned()];
        let link = insert_link(&pool, &draft(), None).await.unwrap().unwrap();

        assert!(
            !remove_link(&pool, &["other.example".to_owned()], link.id)
                .await
                .unwrap()
        );
        assert!(remove_link(&pool, &hosts, link.id).await.unwrap());
        let new_link = insert_link(&pool, &draft(), None).await.unwrap().unwrap();
        let links = list(&pool, &hosts, link.id).await.unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].id, new_link.id);
    }
}
