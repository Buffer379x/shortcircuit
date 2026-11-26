use std::{
    error::Error,
    fs::{self, DirBuilder, File, Metadata, TryLockError},
    io::{self, Read, Seek, SeekFrom},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    time::Duration,
};

use sqlx::{
    Connection, SqliteConnection, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};

use crate::backup::MAX_SNAPSHOT_BYTES;
const MARKER_NAME: &str = "shortcircuit_backup_metadata";
const MARKER_SQL: &str = "CREATE TABLE shortcircuit_backup_metadata (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    format_version INTEGER NOT NULL CHECK (format_version > 0),
    schema_version INTEGER NOT NULL CHECK (schema_version > 0),
    created_at INTEGER NOT NULL CHECK (created_at >= 0)
) STRICT";
const SIDECARS: [&str; 3] = ["-wal", "-shm", "-journal"];

type RestoreError = Box<dyn Error + Send + Sync>;
type SchemaRow = (String, String, String, Option<String>);
type MigrationRow = (i64, String, i64, Vec<u8>);

#[derive(Debug)]
pub struct RestoreOutcome {
    pub rollback: Option<PathBuf>,
    pub rollback_is_snapshot: bool,
    pub schema_version: i64,
    pub created_at: i64,
}

/// Hold this file for as long as the process uses the database.
pub fn data_lock(database: &Path, exclusive: bool) -> io::Result<File> {
    let path = sibling(database, ".lock")?;
    DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(parent(&path))?;
    regular_metadata(&path)?;

    let lock = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(&path)?;
    let metadata = checked_metadata(&lock, &path)?;
    if metadata.nlink() != 1 || metadata.permissions().mode() & 0o777 != 0o600 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "database lock must be a private file with mode 0600",
        ));
    }

    let result = if exclusive {
        lock.try_lock()
    } else {
        lock.try_lock_shared()
    };
    result.map_err(|error| match error {
        TryLockError::WouldBlock => io::Error::new(
            io::ErrorKind::WouldBlock,
            "database is in use by another ShortCircuit process",
        ),
        TryLockError::Error(error) => error,
    })?;
    checked_metadata(&lock, &path)?;
    Ok(lock)
}

/// Restore a snapshot while the ShortCircuit server is stopped.
pub async fn restore_snapshot(
    database: &Path,
    source: &Path,
) -> Result<RestoreOutcome, RestoreError> {
    let lock = data_lock(database, true)?;
    let directory = fs::canonicalize(parent(database))?;
    let filename = database
        .file_name()
        .ok_or_else(|| invalid_input("database path needs a filename"))?;
    let database = directory.join(filename);
    checked_metadata(&lock, &sibling(&database, ".lock")?)?;
    if let (Some(live), Some(backup)) = (regular_metadata(&database)?, regular_metadata(source)?)
        && same_file(&live, &backup)
    {
        return Err(invalid_input("restore source must differ from the live database").into());
    }
    let source = source.to_owned();
    let staged =
        tokio::task::spawn_blocking(move || Stage::copy_source(&directory, &source)).await??;

    let (schema, migrations) = canonical_schema(&staged.directory.join("canonical.sqlite")).await?;
    let (schema_version, created_at) =
        validate_snapshot(&staged.incoming, &schema, &migrations).await?;
    scrub_snapshot(&staged.incoming).await?;
    checkpoint_live(&database).await?;

    let previous = regular_metadata(&database)?;
    let candidate = if previous.is_some() {
        rollback_snapshot(&database, &staged.directory, &schema, &migrations)
            .await
            .ok()
    } else {
        None
    };
    let rollback_is_snapshot = candidate.is_some();

    // Keep the offline lock until the blocking activation has finished, even on cancellation.
    let rollback = tokio::task::spawn_blocking(move || {
        let _lock = lock;
        staged.activate(&database, previous.as_ref(), candidate.as_deref())
    })
    .await??;
    Ok(RestoreOutcome {
        rollback,
        rollback_is_snapshot,
        schema_version,
        created_at,
    })
}

struct Stage {
    directory: PathBuf,
    incoming: PathBuf,
    nonce: u128,
}

impl Stage {
    fn copy_source(directory: &Path, source: &Path) -> io::Result<Self> {
        if sidecars_present(source)? {
            return Err(invalid_data("snapshot must be a standalone SQLite backup"));
        }
        let mut source_file = open_regular(source)?;
        let metadata = source_file.metadata()?;
        if !(100..=MAX_SNAPSHOT_BYTES).contains(&metadata.len()) {
            return Err(invalid_data(
                "snapshot size is outside the supported bounds",
            ));
        }
        let mut header = [0; 100];
        source_file.read_exact(&mut header)?;
        if &header[..16] != b"SQLite format 3\0" || header[18..20] != [1, 1] {
            return Err(invalid_data("snapshot is not a standalone SQLite backup"));
        }
        source_file.seek(SeekFrom::Start(0))?;

        let mut bytes = [0; 16];
        getrandom::fill(&mut bytes)
            .map_err(|_| io::Error::other("restore staging name generation failed"))?;
        let nonce = u128::from_ne_bytes(bytes);
        let directory = directory.join(format!(".restore-{nonce:032x}"));
        DirBuilder::new().mode(0o700).create(&directory)?;
        let stage = Self {
            incoming: directory.join("incoming.sqlite"),
            directory,
            nonce,
        };
        copy_private(&mut source_file, &metadata, &stage.incoming)?;
        if sidecars_present(source)? {
            return Err(invalid_data(
                "snapshot acquired journal files during restore",
            ));
        }
        Ok(stage)
    }

    fn activate(
        self,
        database: &Path,
        previous: Option<&Metadata>,
        rollback_snapshot: Option<&Path>,
    ) -> io::Result<Option<PathBuf>> {
        if sidecars_present(database)? || sidecars_present(&self.incoming)? {
            return Err(invalid_data("database still has SQLite journal files"));
        }
        let directory = File::open(parent(database))?;
        let staging_directory = File::open(&self.directory)?;
        File::open(&self.incoming)?.sync_all()?;

        let preserved = self.directory.join("previous.sqlite");
        let rollback = match previous {
            Some(metadata) => {
                let mut original = open_regular(database)?;
                if !unchanged(metadata, &original.metadata()?) {
                    return Err(invalid_data("live database changed during restore"));
                }
                original.sync_all()?;
                fs::hard_link(database, &preserved)?;
                if !same_file(metadata, &checked_metadata(&original, &preserved)?) {
                    return Err(invalid_data("live database changed during restore"));
                }

                let rollback = sibling(database, &format!(".rollback-{:032x}.sqlite", self.nonce))?;
                if let Some(snapshot) = rollback_snapshot {
                    fs::hard_link(snapshot, &rollback)?;
                } else {
                    let copy = self.directory.join("rollback.sqlite");
                    copy_private(&mut original, metadata, &copy)?;
                    fs::hard_link(&copy, &rollback)?;
                }
                directory.sync_all()?;
                Some(rollback)
            }
            None => None,
        };
        staging_directory.sync_all()?;

        let live_unchanged = match (previous, regular_metadata(database)?.as_ref()) {
            (Some(before), Some(now)) => unchanged(before, now),
            (None, None) => true,
            _ => false,
        };
        if !live_unchanged || sidecars_present(database)? {
            return Err(invalid_data("live database changed during restore"));
        }
        fs::rename(&self.incoming, database)?;
        if let Err(error) = directory.sync_all() {
            let revert = if rollback.is_some() {
                fs::rename(&preserved, database)
            } else {
                fs::remove_file(database)
            };
            if let Err(revert_error) = revert.and_then(|()| directory.sync_all()) {
                return Err(io::Error::other(format!(
                    "restore activation could not be synced ({error}). \
                     Automatic rollback could not be confirmed ({revert_error}). \
                     Saved previous database: {rollback:?}"
                )));
            }
            return Err(error);
        }
        Ok(rollback)
    }
}

async fn rollback_snapshot(
    database: &Path,
    staging_directory: &Path,
    schema: &[SchemaRow],
    migrations: &[MigrationRow],
) -> Result<PathBuf, RestoreError> {
    let source = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(database)
                .read_only(true)
                .immutable(true)
                .pragma("trusted_schema", "OFF"),
        )
        .await?;
    let candidate =
        crate::backup::create_snapshot(&source, &staging_directory.join("rollback")).await;
    source.close().await;
    let candidate = candidate?;
    if fs::metadata(&candidate)?.len() > MAX_SNAPSHOT_BYTES {
        return Err(invalid_data("rollback snapshot exceeds the supported size").into());
    }
    validate_snapshot(&candidate, schema, migrations).await?;
    Ok(candidate)
}

impl Drop for Stage {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

async fn canonical_schema(
    path: &Path,
) -> Result<(Vec<SchemaRow>, Vec<MigrationRow>), RestoreError> {
    let pool = crate::open_database(path).await?;
    let result = read_canonical(&pool).await;
    pool.close().await;
    Ok(result?)
}

async fn read_canonical(
    pool: &SqlitePool,
) -> Result<(Vec<SchemaRow>, Vec<MigrationRow>), sqlx::Error> {
    let schema =
        sqlx::query_as("SELECT type, name, tbl_name, sql FROM sqlite_schema ORDER BY type, name")
            .fetch_all(pool)
            .await?;
    let migrations = sqlx::query_as(
        "SELECT version, description, CASE WHEN success = 1 THEN 1 ELSE 0 END, checksum
         FROM _sqlx_migrations ORDER BY version",
    )
    .fetch_all(pool)
    .await?;
    Ok((schema, migrations))
}

async fn validate_snapshot(
    incoming: &Path,
    schema: &[SchemaRow],
    migrations: &[MigrationRow],
) -> Result<(i64, i64), RestoreError> {
    let mut snapshot = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(incoming)
            .read_only(true)
            .immutable(true)
            .pragma("trusted_schema", "OFF"),
    )
    .await?;
    let mut actual_schema = sqlx::query_as::<_, SchemaRow>(
        "SELECT type, name, tbl_name, sql FROM sqlite_schema ORDER BY type, name LIMIT ?",
    )
    .bind((schema.len() + 2) as i64)
    .fetch_all(&mut snapshot)
    .await?;
    let marker = actual_schema
        .iter()
        .position(|(kind, name, table, sql)| {
            kind == "table"
                && name == MARKER_NAME
                && table == MARKER_NAME
                && sql
                    .as_deref()
                    .is_some_and(|ddl| ddl.split_whitespace().eq(MARKER_SQL.split_whitespace()))
        })
        .ok_or_else(|| invalid_data("snapshot format marker is missing or invalid"))?;
    actual_schema.remove(marker);
    if actual_schema.as_slice() != schema {
        return Err(
            invalid_data("snapshot schema does not match this ShortCircuit version").into(),
        );
    }

    let actual_migrations = sqlx::query_as::<_, MigrationRow>(
        "SELECT version, description, CASE WHEN success = 1 THEN 1 ELSE 0 END, checksum
         FROM _sqlx_migrations ORDER BY version LIMIT ?",
    )
    .bind((migrations.len() + 1) as i64)
    .fetch_all(&mut snapshot)
    .await?;
    if actual_migrations.as_slice() != migrations {
        return Err(invalid_data("snapshot migration checksums or versions do not match").into());
    }
    let current_version = migrations
        .last()
        .ok_or_else(|| invalid_data("application schema has no migrations"))?
        .0;
    let versions: Vec<(i64, i64, i64, i64)> = sqlx::query_as(
        "SELECT id, format_version, schema_version, created_at
         FROM shortcircuit_backup_metadata LIMIT 2",
    )
    .fetch_all(&mut snapshot)
    .await?;
    let [(1, 1, schema_version, created_at)] = versions.as_slice() else {
        return Err(invalid_data("snapshot metadata does not describe a supported format").into());
    };
    if *schema_version != current_version || *created_at < 0 {
        return Err(invalid_data("snapshot metadata is incompatible with this schema").into());
    }

    check_database(&mut snapshot).await?;
    snapshot.close().await?;
    Ok((*schema_version, *created_at))
}

async fn scrub_snapshot(path: &Path) -> Result<(), RestoreError> {
    let mut snapshot = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(path)
            .journal_mode(SqliteJournalMode::Delete)
            .synchronous(SqliteSynchronous::Full)
            .foreign_keys(true)
            .pragma("trusted_schema", "OFF")
            .pragma("secure_delete", "ON"),
    )
    .await?;
    let mut transaction = snapshot.begin().await?;
    sqlx::raw_sql(
        "DELETE FROM admin_sessions;
         DELETE FROM admin_bootstrap;
         DROP TABLE shortcircuit_backup_metadata;",
    )
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    // Rebuild the file to discard free pages belonging to older temporary credentials.
    sqlx::query("VACUUM").execute(&mut snapshot).await?;
    check_database(&mut snapshot).await?;
    snapshot.close().await?;
    Ok(())
}

async fn check_database(connection: &mut SqliteConnection) -> Result<(), RestoreError> {
    let result: String = sqlx::query_scalar("PRAGMA quick_check(1)")
        .fetch_one(&mut *connection)
        .await?;
    if result != "ok" {
        return Err(invalid_data("snapshot failed SQLite quick check").into());
    }
    if sqlx::query("PRAGMA foreign_key_check")
        .fetch_optional(connection)
        .await?
        .is_some()
    {
        return Err(invalid_data("snapshot failed SQLite foreign-key check").into());
    }
    Ok(())
}

async fn checkpoint_live(database: &Path) -> Result<(), RestoreError> {
    let Some(_) = regular_metadata(database)? else {
        if sidecars_present(database)? {
            return Err(invalid_data("journal files exist without a live database").into());
        }
        return Ok(());
    };
    if !sidecars_present(database)? {
        return Ok(());
    }

    let mut live = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(database)
            .busy_timeout(Duration::from_secs(2))
            .pragma("trusted_schema", "OFF"),
    )
    .await?;
    let (busy, frames, checkpointed): (i64, i64, i64) =
        sqlx::query_as("PRAGMA wal_checkpoint(TRUNCATE)")
            .fetch_one(&mut live)
            .await?;
    live.close().await?;
    if busy != 0 || frames != checkpointed || sidecars_present(database)? {
        return Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "could not checkpoint and close the live SQLite journal",
        )
        .into());
    }
    Ok(())
}

fn copy_private(source: &mut File, metadata: &Metadata, target: &Path) -> io::Result<()> {
    let mut target = File::options()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(target)?;
    let mut bounded = (&mut *source).take(metadata.len().saturating_add(1));
    let count = io::copy(&mut bounded, &mut target)?;
    if count != metadata.len() || !unchanged(metadata, &source.metadata()?) {
        return Err(invalid_data("database changed while copying"));
    }
    target.sync_all()
}

fn sidecars_present(path: &Path) -> io::Result<bool> {
    let mut found = false;
    for suffix in SIDECARS {
        found |= regular_metadata(&sibling(path, suffix)?)?.is_some();
    }
    Ok(found)
}

fn regular_metadata(path: &Path) -> io::Result<Option<Metadata>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(Some(metadata)),
        Ok(_) => Err(invalid_input(
            "database and journal paths must be regular files",
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn open_regular(path: &Path) -> io::Result<File> {
    let before = regular_metadata(path)?
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "database file is missing"))?;
    let file = File::open(path)?;
    if !same_file(&before, &checked_metadata(&file, path)?) {
        return Err(invalid_data("database file changed while opening"));
    }
    Ok(file)
}

fn checked_metadata(file: &File, path: &Path) -> io::Result<Metadata> {
    let metadata = file.metadata()?;
    if regular_metadata(path)?.is_some_and(|current| same_file(&metadata, &current)) {
        return Ok(metadata);
    }
    Err(invalid_data("database file changed while opening"))
}

fn same_file(first: &Metadata, second: &Metadata) -> bool {
    first.dev() == second.dev() && first.ino() == second.ino()
}

fn unchanged(first: &Metadata, second: &Metadata) -> bool {
    same_file(first, second)
        && first.len() == second.len()
        && first.mtime() == second.mtime()
        && first.mtime_nsec() == second.mtime_nsec()
}

fn sibling(path: &Path, suffix: &str) -> io::Result<PathBuf> {
    let mut filename = path
        .file_name()
        .ok_or_else(|| invalid_input("database path needs a filename"))?
        .to_os_string();
    filename.push(suffix);
    Ok(path.with_file_name(filename))
}

fn parent(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn invalid_input(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn database(path: &Path, slug: &str) -> SqlitePool {
        let pool = crate::open_database(path).await.unwrap();
        sqlx::query(
            "INSERT INTO links (host, slug, target) VALUES ('go.example', ?, 'https://target.example')",
        )
        .bind(slug)
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    async fn link_slug(path: &Path) -> String {
        let pool = crate::open_database(path).await.unwrap();
        let result = sqlx::query_scalar("SELECT slug FROM links")
            .fetch_one(&pool)
            .await
            .unwrap();
        pool.close().await;
        result
    }

    #[tokio::test]
    async fn holding_a_shared_lock_prevents_an_offline_restore() {
        let directory = tempfile::tempdir().unwrap();
        let live = directory.path().join("live.sqlite");
        database(&live, "live").await.close().await;
        let original = fs::read(&live).unwrap();
        let lock = data_lock(&live, false).unwrap();
        assert_eq!(lock.metadata().unwrap().permissions().mode() & 0o777, 0o600);
        let likewise = data_lock(&live, false).unwrap();

        let error = restore_snapshot(&live, &directory.path().join("absent.sqlite"))
            .await
            .unwrap_err();
        assert_eq!(
            error.downcast_ref::<io::Error>().unwrap().kind(),
            io::ErrorKind::WouldBlock
        );
        assert_eq!(fs::read(&live).unwrap(), original);
        drop((likewise, lock));
    }

    #[tokio::test]
    async fn a_valid_restore_replaces_the_database_and_preserves_a_private_rollback() {
        let directory = tempfile::tempdir().unwrap();
        let live = directory.path().join("live.sqlite");
        let unclean_source = directory.path().join("unclean-source.sqlite");
        let old = database(&unclean_source, "original").await;
        // Copy the main file and committed journal before the normal close checkpoints them.
        fs::copy(&unclean_source, &live).unwrap();
        fs::copy(
            sibling(&unclean_source, "-wal").unwrap(),
            sibling(&live, "-wal").unwrap(),
        )
        .unwrap();
        old.close().await;
        let source_pool = database(&directory.path().join("source.sqlite"), "restored").await;
        sqlx::query(
            "INSERT INTO qr_templates (name, size, foreground, background, transparent, correction, quiet_zone, frame)
             VALUES ('Print', 512, '002040', 'fff4e0', 1, 'H', 8, 'outline')",
        )
        .execute(&source_pool)
        .await
        .unwrap();
        let snapshot =
            crate::backup::create_snapshot(&source_pool, &directory.path().join("backups"))
                .await
                .unwrap();
        source_pool.close().await;
        let mut source =
            SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&snapshot))
                .await
                .unwrap();
        sqlx::raw_sql(
            "INSERT INTO admin_accounts (username, password_hash) VALUES ('owner', '$argon2id$test-hash');
             INSERT INTO admin_sessions (token_hash, account_id, created_at, last_seen_at)
                 VALUES (zeroblob(32), 1, unixepoch(), unixepoch());
             INSERT INTO admin_bootstrap (kind, token_hash, expires_at)
                 VALUES ('setup', zeroblob(32), unixepoch() + 7200);",
        )
        .execute(&mut source)
        .await
        .unwrap();
        source.close().await.unwrap();

        let outcome = restore_snapshot(&live, &snapshot).await.unwrap();
        assert!(outcome.schema_version > 0);
        assert!(outcome.created_at > 0);
        assert!(outcome.rollback_is_snapshot);
        let rollback = outcome.rollback.unwrap();
        assert_eq!(
            fs::metadata(&rollback).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let rollback_check = directory.path().join("rollback-check.sqlite");
        let checked = restore_snapshot(&rollback_check, &rollback).await.unwrap();
        assert!(checked.rollback.is_none());
        assert_eq!(link_slug(&rollback_check).await, "original");
        assert_eq!(link_slug(&live).await, "restored");

        let pool = crate::open_database(&live).await.unwrap();
        let counts: (i64, i64, i64) = sqlx::query_as(
            "SELECT (SELECT count(*) FROM admin_sessions),
                    (SELECT count(*) FROM admin_bootstrap),
                    (SELECT count(*) FROM sqlite_schema WHERE name = 'shortcircuit_backup_metadata')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(counts, (0, 0, 0));
        let template: (String, i64, String, String, bool, String, i64, String) = sqlx::query_as(
            "SELECT name, size, foreground, background, transparent, correction, quiet_zone, frame FROM qr_templates",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            template,
            (
                "Print".to_owned(),
                512,
                "002040".to_owned(),
                "fff4e0".to_owned(),
                true,
                "H".to_owned(),
                8,
                "outline".to_owned(),
            )
        );
    }

    #[tokio::test]
    async fn damaged_live_data_is_preserved_as_a_raw_rollback() {
        let directory = tempfile::tempdir().unwrap();
        let live = directory.path().join("live.sqlite");
        let original = b"damaged database fixture";
        fs::write(&live, original).unwrap();
        let source_pool = database(&directory.path().join("source.sqlite"), "restored").await;
        let snapshot =
            crate::backup::create_snapshot(&source_pool, &directory.path().join("backups"))
                .await
                .unwrap();

        let outcome = restore_snapshot(&live, &snapshot).await.unwrap();
        assert!(!outcome.rollback_is_snapshot);
        assert_eq!(fs::read(outcome.rollback.unwrap()).unwrap(), original);
        assert_eq!(link_slug(&live).await, "restored");
    }

    #[tokio::test]
    async fn an_extra_schema_object_leaves_live_bytes_and_rollback_state_unchanged() {
        let directory = tempfile::tempdir().unwrap();
        let live = directory.path().join("live.sqlite");
        database(&live, "live").await.close().await;
        let original = fs::read(&live).unwrap();
        let source_pool = database(&directory.path().join("source.sqlite"), "candidate").await;
        let snapshot =
            crate::backup::create_snapshot(&source_pool, &directory.path().join("backups"))
                .await
                .unwrap();
        let mut source =
            SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&snapshot))
                .await
                .unwrap();
        sqlx::query("CREATE TABLE unexpected (id INTEGER PRIMARY KEY)")
            .execute(&mut source)
            .await
            .unwrap();
        source.close().await.unwrap();

        assert!(restore_snapshot(&live, &snapshot).await.is_err());
        assert_eq!(fs::read(&live).unwrap(), original);
        assert!(!fs::read_dir(directory.path()).unwrap().any(|entry| {
            let name = entry.unwrap().file_name();
            name.to_string_lossy().contains(".rollback-")
        }));
    }
}
