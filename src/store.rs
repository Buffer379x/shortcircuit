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

static MIGRATIONS: Migrator = sqlx::migrate!();
const MIN_SQLITE: [u32; 3] = [3, 51, 3];
const DATABASE_TIMEOUT: Duration = Duration::from_secs(2);
pub(crate) const PAGE_SIZE: usize = 100;

#[derive(Serialize, sqlx::FromRow)]
pub(crate) struct Link {
    pub(crate) id: i64,
    pub(crate) host: String,
    pub(crate) slug: String,
    pub(crate) target: String,
    pub(crate) created_at: String,
    pub(crate) title: String,
    pub(crate) enabled: bool,
    #[sqlx(skip)]
    pub(crate) tags: Vec<String>,
}

pub(crate) struct LinkDraft {
    pub(crate) host: String,
    pub(crate) slug: String,
    pub(crate) target: String,
    pub(crate) title: String,
    pub(crate) tags: Vec<String>,
}

#[derive(Default)]
pub(crate) struct LinkUpdate {
    pub(crate) target: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) enabled: Option<bool>,
    pub(crate) tags: Option<Vec<String>>,
}

#[derive(sqlx::FromRow)]
struct TaggedLink {
    #[sqlx(flatten)]
    link: Link,
    tag: Option<String>,
}

pub async fn open_database(path: &Path) -> Result<SqlitePool, Box<dyn Error + Send + Sync>> {
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
        .busy_timeout(DATABASE_TIMEOUT);
    let pool = SqlitePoolOptions::new()
        .min_connections(1)
        .max_connections(4)
        .acquire_timeout(DATABASE_TIMEOUT)
        .connect_with(opts)
        .await?;
    MIGRATIONS.run(&pool).await?;
    Ok(pool)
}

fn prepare_file(path: &Path) -> io::Result<()> {
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
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        directories.create(parent)?;
    }
    match file.open(path) {
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(error),
    }
}

pub(crate) async fn insert_link(
    pool: &SqlitePool,
    draft: &LinkDraft,
) -> Result<Option<Link>, sqlx::Error> {
    let mut transaction = pool.begin().await?;
    let rows = sqlx::query_as::<_, Link>(
        "INSERT INTO links (host, slug, target, title) VALUES (?, ?, ?, ?)
         ON CONFLICT(host, slug) DO NOTHING
         RETURNING id, host, slug, target, created_at, title, enabled",
    )
    .bind(&draft.host)
    .bind(&draft.slug)
    .bind(&draft.target)
    .bind(&draft.title)
    // The statement can fail at completion after SQLite has emitted the RETURNING row.
    .fetch_all(&mut *transaction)
    .await?;
    let link = rows.into_iter().next();
    let result = if let Some(mut link) = link {
        link.tags = insert_tags(&mut transaction, link.id, draft.tags.clone()).await?;
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
) -> Result<Option<Link>, sqlx::Error> {
    let mut transaction = pool.begin().await?;
    let mut query = QueryBuilder::new("UPDATE links SET target = COALESCE(");
    query
        .push_bind(update.target)
        .push(", target), title = COALESCE(")
        .push_bind(update.title)
        .push(", title), enabled = COALESCE(")
        .push_bind(update.enabled)
        .push(", enabled) WHERE id = ")
        .push_bind(id);
    push_host_scope(&mut query, hosts);
    query.push(" RETURNING id, host, slug, target, created_at, title, enabled");
    let rows = query
        .build_query_as::<Link>()
        .fetch_all(&mut *transaction)
        .await?;
    let link = rows.into_iter().next();
    let result = if let Some(mut link) = link {
        link.tags = if let Some(tags) = update.tags {
            sqlx::query("DELETE FROM link_tags WHERE link_id = ?")
                .bind(id)
                .execute(&mut *transaction)
                .await?;
            insert_tags(&mut transaction, id, tags).await?
        } else {
            sqlx::query_scalar("SELECT tag FROM link_tags WHERE link_id = ? ORDER BY tag")
                .bind(id)
                .fetch_all(&mut *transaction)
                .await?
        };
        Some(link)
    } else {
        None
    };
    transaction.commit().await?;
    Ok(result)
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
            SELECT id, host, slug, target, created_at, title, enabled FROM links WHERE id > ",
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

fn push_host_scope(query: &mut QueryBuilder<Sqlite>, hosts: &[String]) {
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
    async fn disabled_link_remains_listed_and_stops_resolving() {
        let directory = tempfile::tempdir().unwrap();
        let pool = open_database(&directory.path().join("links.sqlite"))
            .await
            .unwrap();
        let hosts = [HOST.to_owned()];
        let link = insert_link(&pool, &draft()).await.unwrap().unwrap();
        assert_eq!(link.tags, ["docs", "work"]);

        update_link(
            &pool,
            &hosts,
            link.id,
            LinkUpdate {
                target: None,
                title: Some("Archived docs".to_owned()),
                enabled: Some(false),
                tags: None,
            },
        )
        .await
        .unwrap()
        .unwrap();

        assert!(resolve(&pool, HOST, "Team/Docs").await.unwrap().is_none());
        let links = list(&pool, &hosts, 0).await.unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].id, link.id);
        assert!(!links[0].enabled);
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
        let link = insert_link(&pool, &draft()).await.unwrap().unwrap();
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
                enabled: None,
                tags: Some(vec!["reject".to_owned()]),
            },
        )
        .await;

        assert!(result.is_err());
        let links = list(&pool, &hosts, 0).await.unwrap();
        assert_eq!(links[0].target, TARGET);
        assert_eq!(links[0].tags, ["docs", "work"]);
    }

    #[tokio::test]
    async fn only_matching_hosts_can_delete_and_deleted_ids_stay_behind_cursors() {
        let directory = tempfile::tempdir().unwrap();
        let pool = open_database(&directory.path().join("links.sqlite"))
            .await
            .unwrap();
        let hosts = [HOST.to_owned()];
        let link = insert_link(&pool, &draft()).await.unwrap().unwrap();

        assert!(
            !remove_link(&pool, &["other.example".to_owned()], link.id)
                .await
                .unwrap()
        );
        assert!(remove_link(&pool, &hosts, link.id).await.unwrap());
        let new_link = insert_link(&pool, &draft()).await.unwrap().unwrap();
        let links = list(&pool, &hosts, link.id).await.unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].id, new_link.id);
    }
}
