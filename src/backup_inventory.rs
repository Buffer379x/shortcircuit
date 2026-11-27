//! A read-only inventory of the snapshots and rollback copies stored on the data volume.
//!
//! The listing names files only. It never returns a path, follows no symbolic links, reads a
//! bounded number of directory entries, and lists at most `MAX_LISTED` files.

use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    time::{Duration, UNIX_EPOCH},
};

use serde::Serialize;
use sqlx::{ConnectOptions, Connection, sqlite::SqliteConnectOptions};

use crate::backup_schedule::backup_timestamp;

/// The most files one listing returns, newest first.
pub(crate) const MAX_LISTED: usize = 100;
/// The most directory entries read from each location.
const MAX_SCANNED: usize = 1000;
/// The time allowed to read the schema version of one file.
const SCHEMA_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StoredKind {
    /// Written by the `backup` command to `backups/`.
    Manual,
    /// Written by the backup schedule to `backups/scheduled/`.
    Scheduled,
    /// Written before migrations to `backups/pre-upgrade/`.
    PreUpgrade,
    /// The previous database kept next to the live database by a restore.
    Rollback,
}

#[derive(Debug, Serialize)]
pub(crate) struct StoredBackup {
    pub(crate) kind: StoredKind,
    pub(crate) file_name: String,
    pub(crate) size_bytes: u64,
    /// UTC Unix seconds from the snapshot name, or the file time for rollback copies.
    pub(crate) created_at: Option<u64>,
    /// The schema version recorded in the file, when it can be read.
    pub(crate) schema_version: Option<i64>,
}

#[derive(Debug, Serialize)]
pub(crate) struct StoredBackups {
    pub(crate) items: Vec<StoredBackup>,
    /// True when more files exist than were scanned or listed.
    pub(crate) truncated: bool,
}

struct Candidate {
    kind: StoredKind,
    path: PathBuf,
    file_name: String,
    size_bytes: u64,
    created_at: Option<u64>,
}

/// Lists stored snapshots and rollback copies for `database`. Blocking file reads run on the
/// blocking pool; schema versions are read from at most `MAX_LISTED` files.
pub(crate) async fn list(database: &Path) -> std::io::Result<StoredBackups> {
    let database = database.to_owned();
    let (mut candidates, truncated) = tokio::task::spawn_blocking(move || scan(&database))
        .await
        .map_err(std::io::Error::other)?;
    let mut items = Vec::with_capacity(candidates.len());
    for candidate in candidates.drain(..) {
        let schema_version = tokio::time::timeout(SCHEMA_TIMEOUT, schema_version(&candidate.path))
            .await
            .ok()
            .flatten();
        items.push(StoredBackup {
            kind: candidate.kind,
            file_name: candidate.file_name,
            size_bytes: candidate.size_bytes,
            created_at: candidate.created_at,
            schema_version,
        });
    }
    Ok(StoredBackups { items, truncated })
}

fn scan(database: &Path) -> (Vec<Candidate>, bool) {
    let backups = database.with_file_name("backups");
    let data = database
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let rollback_prefix = database
        .file_name()
        .map(|name| format!("{}.rollback-", name.to_string_lossy()));
    let mut truncated = false;
    let mut candidates = Vec::new();
    for (kind, directory) in [
        (StoredKind::Manual, backups.clone()),
        (StoredKind::Scheduled, backups.join("scheduled")),
        (StoredKind::PreUpgrade, backups.join("pre-upgrade")),
        (StoredKind::Rollback, data.to_owned()),
    ] {
        let matches = |name: &OsStr| match kind {
            StoredKind::Rollback => rollback_prefix
                .as_deref()
                .is_some_and(|prefix| is_rollback_name(name, prefix)),
            _ => backup_timestamp(name).is_some(),
        };
        truncated |= scan_directory(kind, &directory, matches, &mut candidates);
    }
    // Newest first; files without a time sort last.
    candidates.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| a.file_name.cmp(&b.file_name))
    });
    if candidates.len() > MAX_LISTED {
        candidates.truncate(MAX_LISTED);
        truncated = true;
    }
    (candidates, truncated)
}

/// Adds matching regular files of `directory` and reports whether entries were left unread.
fn scan_directory(
    kind: StoredKind,
    directory: &Path,
    matches: impl Fn(&OsStr) -> bool,
    candidates: &mut Vec<Candidate>,
) -> bool {
    if !fs::symlink_metadata(directory).is_ok_and(|metadata| metadata.is_dir()) {
        return false;
    }
    let Ok(entries) = fs::read_dir(directory) else {
        return false;
    };
    let mut entries = entries.filter_map(Result::ok);
    for entry in entries.by_ref().take(MAX_SCANNED) {
        let name = entry.file_name();
        if !matches(&name) {
            continue;
        }
        let path = entry.path();
        // Symbolic links and other special files are never listed.
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        let created_at = match kind {
            StoredKind::Rollback => metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|elapsed| elapsed.as_secs()),
            _ => backup_timestamp(&name),
        };
        let Some(file_name) = name.to_str() else {
            continue;
        };
        candidates.push(Candidate {
            kind,
            path,
            file_name: file_name.to_owned(),
            size_bytes: metadata.len(),
            created_at,
        });
    }
    entries.next().is_some()
}

fn is_rollback_name(name: &OsStr, prefix: &str) -> bool {
    name.to_str()
        .and_then(|name| name.strip_prefix(prefix))
        .and_then(|rest| rest.strip_suffix(".sqlite"))
        .is_some_and(|nonce| {
            nonce.len() == 32
                && nonce
                    .bytes()
                    .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        })
}

/// Reads the schema version from the backup metadata, or from the migration table of a raw
/// rollback copy. The file is opened read-only and immutable, without trusting its schema.
async fn schema_version(path: &Path) -> Option<i64> {
    let mut connection = SqliteConnectOptions::new()
        .filename(path)
        .read_only(true)
        .immutable(true)
        .pragma("trusted_schema", "OFF")
        .disable_statement_logging()
        .connect()
        .await
        .ok()?;
    let recorded: Option<i64> =
        sqlx::query_scalar("SELECT schema_version FROM shortcircuit_backup_metadata WHERE id = 1")
            .fetch_optional(&mut connection)
            .await
            .ok()
            .flatten();
    let version = match recorded {
        Some(version) => Some(version),
        None => sqlx::query_scalar("SELECT max(version) FROM _sqlx_migrations WHERE success = 1")
            .fetch_one(&mut connection)
            .await
            .ok()
            .flatten(),
    };
    let _ = connection.close().await;
    version.filter(|version| *version > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[tokio::test]
    async fn lists_each_kind_by_name_without_paths_links_or_foreign_files() {
        let work = tempfile::tempdir().unwrap();
        let database = work.path().join("shortcircuit.db");
        let pool = crate::open_database(&database).await.unwrap();
        let backups = database.with_file_name("backups");
        let manual = crate::backup::create_snapshot(&pool, &backups)
            .await
            .unwrap();
        let scheduled = crate::backup::create_snapshot(&pool, &backups.join("scheduled"))
            .await
            .unwrap();
        let pre_upgrade = crate::backup::create_snapshot(&pool, &backups.join("pre-upgrade"))
            .await
            .unwrap();
        let schema: i64 =
            sqlx::query_scalar("SELECT max(version) FROM _sqlx_migrations WHERE success = 1")
                .fetch_one(&pool)
                .await
                .unwrap();
        pool.close().await;
        // A raw rollback copy carries no backup metadata, only the migration table.
        let rollback = work.path().join(format!(
            "shortcircuit.db.rollback-{}.sqlite",
            "a".repeat(32)
        ));
        fs::copy(&database, &rollback).unwrap();
        // Files that must not be listed: wrong names, a symbolic link, and staging directories.
        fs::write(backups.join("notes.txt"), "synthetic").unwrap();
        fs::write(backups.join("shortcircuit-12-short.sqlite"), "synthetic").unwrap();
        std::os::unix::fs::symlink(
            &manual,
            backups.join(format!("shortcircuit-1-{}.sqlite", "b".repeat(32))),
        )
        .unwrap();
        fs::create_dir(backups.join(format!("shortcircuit-2-{}.sqlite", "c".repeat(32)))).unwrap();
        fs::write(work.path().join("other.db.rollback-x.sqlite"), "synthetic").unwrap();

        let listing = list(&database).await.unwrap();
        assert!(!listing.truncated);
        let mut kinds: Vec<_> = listing
            .items
            .iter()
            .map(|item| (item.kind, item.file_name.clone()))
            .collect();
        kinds.sort_by_key(|(_, name)| name.clone());
        let name = |path: &Path| path.file_name().unwrap().to_str().unwrap().to_owned();
        let mut expected = vec![
            (StoredKind::Manual, name(&manual)),
            (StoredKind::Scheduled, name(&scheduled)),
            (StoredKind::PreUpgrade, name(&pre_upgrade)),
            (StoredKind::Rollback, name(&rollback)),
        ];
        expected.sort_by_key(|(_, name)| name.clone());
        assert_eq!(kinds, expected);
        for item in &listing.items {
            assert_eq!(item.schema_version, Some(schema), "{}", item.file_name);
            assert!(item.size_bytes > 0);
            assert!(item.created_at.unwrap() > 0);
            assert!(!item.file_name.contains('/'));
        }
        let json = serde_json::to_string(&listing).unwrap();
        assert!(!json.contains(&work.path().display().to_string()));
    }

    #[tokio::test]
    async fn the_listing_is_bounded_and_newest_first() {
        let work = tempfile::tempdir().unwrap();
        let database = work.path().join("shortcircuit.db");
        let scheduled = database.with_file_name("backups").join("scheduled");
        fs::create_dir_all(&scheduled).unwrap();
        for index in 0..(MAX_LISTED + 5) {
            fs::write(
                scheduled.join(format!(
                    "shortcircuit-{}-{:032x}.sqlite",
                    1000 + index,
                    index
                )),
                "not a database",
            )
            .unwrap();
        }
        let listing = list(&database).await.unwrap();
        assert!(listing.truncated);
        assert_eq!(listing.items.len(), MAX_LISTED);
        assert_eq!(
            listing.items[0].created_at,
            Some(1000 + MAX_LISTED as u64 + 4)
        );
        assert!(
            listing
                .items
                .iter()
                .all(|item| item.schema_version.is_none())
        );
        // A missing data directory lists nothing.
        let missing = list(&work.path().join("missing").join("shortcircuit.db"))
            .await
            .unwrap();
        assert!(missing.items.is_empty() && !missing.truncated);
    }
}
