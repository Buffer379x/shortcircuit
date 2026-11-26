use std::{
    error::Error,
    fs::{self, DirBuilder, File, OpenOptions},
    io,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use sqlx::{
    Connection, SqliteConnection, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous},
};

pub(crate) const MAX_SNAPSHOT_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const BACKUP_FORMAT_VERSION: i64 = 1;

/// Create a local backup in the server-configured directory.
pub async fn create_snapshot(
    pool: &SqlitePool,
    backups_dir: &Path,
) -> Result<PathBuf, Box<dyn Error + Send + Sync>> {
    let staged = verified_snapshot(pool, backups_dir).await?;
    Ok(tokio::task::spawn_blocking(move || staged.publish()).await??)
}

pub(crate) async fn verified_snapshot(
    pool: &SqlitePool,
    backups_dir: &Path,
) -> Result<StagedSnapshot, Box<dyn Error + Send + Sync>> {
    let backups_dir = backups_dir.to_owned();
    let staged =
        tokio::task::spawn_blocking(move || StagedSnapshot::reserve(&backups_dir)).await??;
    let filename = staged.database.to_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "backup path is not valid UTF-8",
        )
    })?;
    sqlx::query("VACUUM main INTO ?")
        .bind(filename)
        .execute(pool)
        .await?;

    staged.checked_size()?;
    let mut snapshot = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(&staged.database)
            .journal_mode(SqliteJournalMode::Delete)
            .synchronous(SqliteSynchronous::Full)
            .foreign_keys(true)
            .pragma("trusted_schema", "OFF")
            .pragma("secure_delete", "ON"),
    )
    .await?;
    annotate_snapshot(&mut snapshot).await?;
    verify_snapshot(&mut snapshot).await?;
    snapshot.close().await?;

    staged.checked_size()?;
    Ok(staged)
}

async fn annotate_snapshot(
    snapshot: &mut SqliteConnection,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut transaction = snapshot.begin().await?;
    let schema_version: i64 = sqlx::query_scalar(
        "SELECT version FROM _sqlx_migrations WHERE success = 1 ORDER BY version DESC LIMIT 1",
    )
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "snapshot lacks a schema version",
        )
    })?;

    sqlx::raw_sql(
        "DELETE FROM admin_sessions;
         DELETE FROM admin_bootstrap;
         CREATE TABLE IF NOT EXISTS shortcircuit_backup_metadata (
             id INTEGER PRIMARY KEY CHECK (id = 1),
             format_version INTEGER NOT NULL CHECK (format_version > 0),
             schema_version INTEGER NOT NULL CHECK (schema_version > 0),
             created_at INTEGER NOT NULL CHECK (created_at >= 0)
         ) STRICT;",
    )
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "INSERT INTO shortcircuit_backup_metadata
             (id, format_version, schema_version, created_at)
         VALUES (1, ?, ?, unixepoch())
         ON CONFLICT (id) DO UPDATE SET
             format_version = excluded.format_version,
             schema_version = excluded.schema_version,
             created_at = excluded.created_at",
    )
    .bind(BACKUP_FORMAT_VERSION)
    .bind(schema_version)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(())
}

async fn verify_snapshot(
    snapshot: &mut SqliteConnection,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let quick_check: String = sqlx::query_scalar("PRAGMA quick_check(1)")
        .fetch_one(&mut *snapshot)
        .await?;
    if quick_check != "ok" {
        return Err(
            io::Error::new(io::ErrorKind::InvalidData, "snapshot failed quick_check").into(),
        );
    }
    if sqlx::query("PRAGMA foreign_key_check")
        .fetch_optional(snapshot)
        .await?
        .is_some()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "snapshot failed foreign_key_check",
        )
        .into());
    }
    Ok(())
}

pub(crate) struct StagedSnapshot {
    directory: PathBuf,
    database: PathBuf,
    target: PathBuf,
}

impl StagedSnapshot {
    fn checked_size(&self) -> io::Result<u64> {
        let bytes = fs::metadata(&self.database)?.len();
        if bytes > MAX_SNAPSHOT_BYTES {
            return Err(io::Error::other("backup exceeds the supported snapshot size"));
        }
        Ok(bytes)
    }

    pub(crate) fn open(&self) -> io::Result<(File, u64)> {
        Ok((File::open(&self.database)?, self.checked_size()?))
    }

    fn reserve(backups_dir: &Path) -> io::Result<Self> {
        let backups_dir = prepare_directory(backups_dir)?;
        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_secs();
        let mut random = [0; 16];
        getrandom::fill(&mut random)
            .map_err(|_| io::Error::other("backup name generation failed"))?;
        let nonce = u128::from_ne_bytes(random);
        let directory = backups_dir.join(format!(".snapshot-{nonce:032x}"));
        DirBuilder::new().mode(0o700).create(&directory)?;
        let staged = Self {
            database: directory.join("database.sqlite"),
            target: backups_dir.join(format!("shortcircuit-{created_at}-{nonce:032x}.sqlite")),
            directory,
        };
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&staged.database)?;
        Ok(staged)
    }

    fn publish(self) -> io::Result<PathBuf> {
        let parent = self.target.parent().expect("backup file has a parent");
        let directory = File::open(parent)?;
        File::open(&self.database)?.sync_all()?;
        fs::hard_link(&self.database, &self.target)?;
        // Persist both publication and removal of the private staging name.
        if let Err(error) = fs::remove_dir_all(&self.directory).and_then(|()| directory.sync_all())
        {
            let _ = fs::remove_file(&self.target);
            let _ = directory.sync_all();
            return Err(error);
        }
        Ok(self.target.clone())
    }
}

impl Drop for StagedSnapshot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn prepare_directory(path: &Path) -> io::Result<PathBuf> {
    let path = std::path::absolute(path)?;
    if path.components().any(|part| part == Component::ParentDir) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "backup directory must not contain a parent path component",
        ));
    }

    let mut checked = PathBuf::new();
    for component in path.components() {
        checked.push(component.as_os_str());
        let metadata = match fs::symlink_metadata(&checked) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                match DirBuilder::new().mode(0o700).create(&checked) {
                    Ok(()) => {
                        if let Some(parent) = checked.parent() {
                            File::open(parent)?.sync_all()?;
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(error),
                }
                fs::symlink_metadata(&checked)?
            }
            Err(error) => return Err(error),
        };
        let mode = metadata.permissions().mode();
        if !metadata.is_dir() || (mode & 0o022 != 0 && mode & 0o1000 == 0) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "backup path requires real directories without shared writes",
            ));
        }
    }
    if fs::symlink_metadata(&path)?.permissions().mode() & 0o022 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "backup directory must not permit shared writes",
        ));
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;

    use super::*;

    #[tokio::test]
    async fn backup_copies_live_rows_and_scrubs_only_the_snapshot() {
        let work = tempfile::tempdir().unwrap();
        let pool = crate::open_database(&work.path().join("live.sqlite"))
            .await
            .unwrap();
        sqlx::raw_sql(
            "INSERT INTO links (host, slug, target)
             VALUES ('go.example', 'Team/Docs', 'https://example.test/?Signature=XL%2f');
             INSERT INTO admin_accounts (username, password_hash)
             VALUES ('owner', '$argon2id$synthetic');
             INSERT INTO admin_api_keys (account_id, token_hash, name, scopes, created_at)
             VALUES (1, randomblob(32), 'synthetic key', 1, unixepoch());
             INSERT INTO admin_bootstrap (kind, token_hash, expires_at)
             VALUES ('setup', randomblob(32), unixepoch() - 1),
                    ('recovery', randomblob(32), unixepoch() + 900);",
        )
        .execute(&pool)
        .await
        .unwrap();
        let active_hash = [0x97_u8; 32];
        let idle_hash = [0xa5_u8; 32];
        let absolute_hash = [0x5b_u8; 32];
        for (digest, age, idle) in [
            (active_hash, 60_i64, 10_i64),
            (idle_hash, 7200, 3600),
            (absolute_hash, 86_400, 100),
        ] {
            sqlx::query(
                "INSERT INTO admin_sessions (token_hash, account_id, created_at, last_seen_at)
                 VALUES (?, 1, unixepoch() - ?, unixepoch() - ?)",
            )
            .bind(digest.as_slice())
            .bind(age)
            .bind(idle)
            .execute(&pool)
            .await
            .unwrap();
        }

        let backups = work.path().join("backups");
        let snapshot_path = create_snapshot(&pool, &backups).await.unwrap();
        assert_eq!(snapshot_path.parent(), Some(backups.as_path()));
        assert_eq!(fs::read_dir(&backups).unwrap().count(), 1);
        assert_eq!(
            fs::metadata(&snapshot_path).unwrap().permissions().mode() & 0o077,
            0
        );
        let snapshot_bytes = fs::read(&snapshot_path).unwrap();
        for digest in [active_hash, idle_hash, absolute_hash] {
            assert!(
                !snapshot_bytes
                    .windows(digest.len())
                    .any(|data| data == digest.as_slice())
            );
        }

        let mut snapshot = SqliteConnection::connect_with(
            &SqliteConnectOptions::new()
                .filename(&snapshot_path)
                .read_only(true),
        )
        .await
        .unwrap();
        let saved: (String, String) = sqlx::query_as("SELECT slug, target FROM links")
            .fetch_one(&mut snapshot)
            .await
            .unwrap();
        assert_eq!(
            saved,
            (
                "Team/Docs".to_owned(),
                "https://example.test/?Signature=XL%2f".to_owned()
            )
        );
        let versions: (i64, i64) = sqlx::query_as(
            "SELECT format_version, schema_version FROM shortcircuit_backup_metadata WHERE id = 1",
        )
        .fetch_one(&mut snapshot)
        .await
        .unwrap();
        let live_version: i64 = sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(versions, (1, live_version));
        let saved_auth: (i64, i64) = sqlx::query_as(
            "SELECT (SELECT COUNT(*) FROM admin_sessions), (SELECT COUNT(*) FROM admin_bootstrap)",
        )
        .fetch_one(&mut snapshot)
        .await
        .unwrap();
        assert_eq!(saved_auth, (0, 0));
        let retained_auth: (String, String) = sqlx::query_as(
            "SELECT account.username, key.name FROM admin_accounts AS account
             JOIN admin_api_keys AS key ON key.account_id = account.id",
        )
        .fetch_one(&mut snapshot)
        .await
        .unwrap();
        assert_eq!(
            retained_auth,
            ("owner".to_owned(), "synthetic key".to_owned())
        );
        let live_auth: (i64, i64) = sqlx::query_as(
            "SELECT (SELECT COUNT(*) FROM admin_sessions), (SELECT COUNT(*) FROM admin_bootstrap)",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(live_auth, (3, 2));
        let live_marker: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM sqlite_schema WHERE name = 'shortcircuit_backup_metadata'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(live_marker, 0);
    }

    #[tokio::test]
    async fn a_foreign_key_violation_prevents_publication_and_removes_staging() {
        let work = tempfile::tempdir().unwrap();
        let pool = crate::open_database(&work.path().join("live.sqlite"))
            .await
            .unwrap();
        let mut live = pool.acquire().await.unwrap();
        sqlx::raw_sql(
            "PRAGMA foreign_keys = OFF;
             INSERT INTO link_tags (link_id, tag) VALUES (99, 'orphan');
             PRAGMA foreign_keys = ON;",
        )
        .execute(&mut *live)
        .await
        .unwrap();
        drop(live);

        let backups = work.path().join("backups");
        let error = create_snapshot(&pool, &backups).await.unwrap_err();
        assert_eq!(
            error.downcast_ref::<io::Error>().unwrap().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(fs::read_dir(&backups).unwrap().count(), 0);
    }

    #[test]
    fn publication_keeps_an_existing_destination_and_removes_staging() {
        let work = tempfile::tempdir().unwrap();
        let pending = StagedSnapshot::reserve(work.path()).unwrap();
        let existing = pending.target.clone();
        fs::write(&existing, b"previous backup").unwrap();
        fs::write(&pending.database, b"verified snapshot").unwrap();

        let error = pending.publish().unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&existing).unwrap(), b"previous backup");
        assert_eq!(fs::read_dir(work.path()).unwrap().count(), 1);
    }

    #[test]
    fn backup_directory_rejects_symlinked_ancestors() {
        let work = tempfile::tempdir().unwrap();
        let real = work.path().join("real");
        fs::create_dir(&real).unwrap();
        let alias = work.path().join("alias");
        symlink(&real, &alias).unwrap();

        assert!(StagedSnapshot::reserve(&alias.join("backups")).is_err());
        assert!(!real.join("backups").exists());
    }
}
