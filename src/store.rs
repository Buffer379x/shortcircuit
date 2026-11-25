use std::{
    error::Error,
    fs::{DirBuilder, OpenOptions},
    io,
    path::Path,
    time::Duration,
};

use serde::Serialize;
use sqlx::{
    Connection, SqliteConnection, SqlitePool,
    migrate::Migrator,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};

static MIGRATIONS: Migrator = sqlx::migrate!();
const MIN_SQLITE: [u32; 3] = [3, 51, 3];
const DATABASE_TIMEOUT: Duration = Duration::from_secs(2);
pub(crate) const PAGE_SIZE: usize = 100;

#[derive(Serialize)]
pub(crate) struct Link {
    pub(crate) id: i64,
    host: String,
    slug: String,
    target: String,
    created_at: String,
}

type LinkRow = (i64, String, String, String, String);

impl From<LinkRow> for Link {
    fn from((id, host, slug, target, created_at): LinkRow) -> Self {
        Self { id, host, slug, target, created_at }
    }
}

pub async fn open_database(path: &Path) -> Result<SqlitePool, Box<dyn Error + Send + Sync>> {
    let mut probe = SqliteConnection::connect_with(&SqliteConnectOptions::new().in_memory(true)).await?;
    let version: String = sqlx::query_scalar("SELECT sqlite_version()")
        .fetch_one(&mut probe)
        .await?;
    probe.close().await?;
    let parts = version.split('.').map(str::parse).collect::<Result<Vec<u32>, _>>()?;
    if parts.as_slice() < MIN_SQLITE.as_slice() {
        return Err(io::Error::other(format!("SQLite {version} does not meet the minimum version 3.51.3")).into());
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
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        directories.create(parent)?;
    }
    match file.open(path) {
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(error),
    }
}

pub(crate) async fn insert(
    pool: &SqlitePool,
    host: &str,
    slug: &str,
    target: &str,
) -> Result<Option<Link>, sqlx::Error> {
    sqlx::query_as::<_, LinkRow>(
        "INSERT INTO links (host, slug, target) VALUES (?, ?, ?)
         ON CONFLICT(host, slug) DO NOTHING
         RETURNING id, host, slug, target, created_at",
    )
    .bind(host)
    .bind(slug)
    .bind(target)
    // The statement can fail at completion after SQLite has emitted the RETURNING row.
    .fetch_all(pool)
    .await
    .map(|rows| rows.into_iter().next().map(Link::from))
}

pub(crate) async fn resolve(
    pool: &SqlitePool,
    host: &str,
    slug: &str,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar("SELECT target FROM links WHERE host = ? AND slug = ?")
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
    let mut query = sqlx::QueryBuilder::new(
        "SELECT id, host, slug, target, created_at FROM links WHERE id > ",
    );
    query.push_bind(after_id).push(" AND host IN (");
    {
        let mut placeholders = query.separated(", ");
        for host in hosts {
            placeholders.push_bind(host);
        }
    }
    query.push(") ORDER BY id LIMIT ").push_bind((PAGE_SIZE + 1) as i64);
    query
        .build_query_as::<LinkRow>()
        .fetch_all(pool)
        .await
        .map(|rows| rows.into_iter().map(Link::from).collect())
}
