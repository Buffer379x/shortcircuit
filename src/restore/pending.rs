use std::io::Write;

use serde::Serialize;

use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RestoreStatus {
    None,
    PendingRestart,
    Applied,
    Failed,
    Interrupted,
    Unavailable,
}

struct RestorePaths {
    pending: PathBuf,
    applying: PathBuf,
    result: PathBuf,
}

impl RestorePaths {
    fn new(database: &Path) -> io::Result<Self> {
        Ok(Self {
            pending: sibling(database, ".restore-pending.sqlite")?,
            applying: sibling(database, ".restore-applying.sqlite")?,
            result: sibling(database, ".restore-result")?,
        })
    }

    fn ensure_can_stage(&self) -> io::Result<()> {
        if regular_metadata(&self.pending)?.is_some() || regular_metadata(&self.applying)?.is_some()
        {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "a browser restore is already pending",
            ));
        }
        Ok(())
    }

    fn finish(&self, result: &[u8]) -> io::Result<()> {
        let directory = parent(&self.result);
        let temporary = Stage::reserve(directory)?;
        let mut marker = File::options()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary.incoming)?;
        marker.write_all(result)?;
        marker.sync_all()?;
        fs::rename(&temporary.incoming, &self.result)?;
        let directory = File::open(directory)?;
        directory.sync_all()?;
        fs::remove_file(&self.applying)?;
        directory.sync_all()
    }
}

pub(crate) struct PendingUpload {
    paths: RestorePaths,
    stage: Stage,
}

impl PendingUpload {
    pub(crate) fn reserve(database: &Path) -> io::Result<Self> {
        let paths = RestorePaths::new(database)?;
        paths.ensure_can_stage()?;
        let stage = Stage::reserve(&fs::canonicalize(parent(database))?)?;
        File::options()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&stage.incoming)?;
        Ok(Self { paths, stage })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.stage.incoming
    }

    pub(crate) async fn validate(self) -> Result<ValidatedUpload, RestoreError> {
        let mut incoming = open_regular(self.path())?;
        let metadata = incoming.metadata()?;
        check_snapshot_header(&mut incoming, &metadata)?;
        let (schema, migrations) =
            canonical_schema(&self.stage.directory.join("canonical.sqlite")).await?;
        let (schema_version, created_at) =
            validate_snapshot(self.path(), &schema, &migrations).await?;
        Ok(ValidatedUpload {
            pending: self,
            schema_version,
            created_at,
        })
    }
}

pub(crate) struct ValidatedUpload {
    pending: PendingUpload,
    pub(crate) schema_version: i64,
    pub(crate) created_at: i64,
}

impl ValidatedUpload {
    pub(crate) fn publish(self) -> io::Result<()> {
        let pending = &self.pending;
        pending.paths.ensure_can_stage()?;
        let directory = File::open(parent(&pending.paths.pending))?;
        File::open(pending.path())?.sync_all()?;
        fs::hard_link(pending.path(), &pending.paths.pending)?;
        if let Err(error) = directory.sync_all() {
            let _ = fs::remove_file(&pending.paths.pending);
            let _ = directory.sync_all();
            return Err(error);
        }
        Ok(())
    }
}

pub(crate) fn has_pending_work(database: &Path) -> io::Result<bool> {
    let paths = RestorePaths::new(database)?;
    Ok(regular_metadata(&paths.pending)?.is_some() || regular_metadata(&paths.applying)?.is_some())
}

pub(crate) fn cancel_pending_restore(database: &Path) -> io::Result<()> {
    let paths = RestorePaths::new(database)?;
    if regular_metadata(&paths.pending)?.is_none() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "there is no pending browser restore",
        ));
    }
    let directory = File::open(parent(&paths.pending))?;
    fs::remove_file(&paths.pending)?;
    directory.sync_all()
}

pub(crate) fn restore_status(database: &Path) -> RestoreStatus {
    read_status(database).unwrap_or(RestoreStatus::Unavailable)
}

fn read_status(database: &Path) -> io::Result<RestoreStatus> {
    let paths = RestorePaths::new(database)?;
    if regular_metadata(&paths.applying)?.is_some() {
        return Ok(RestoreStatus::Interrupted);
    }
    if regular_metadata(&paths.pending)?.is_some() {
        return Ok(RestoreStatus::PendingRestart);
    }
    if regular_metadata(&paths.result)?.is_none() {
        return Ok(RestoreStatus::None);
    }
    let mut result = Vec::with_capacity(16);
    open_regular(&paths.result)?
        .take(16)
        .read_to_end(&mut result)?;
    Ok(match result.as_slice() {
        b"applied\n" => RestoreStatus::Applied,
        b"failed\n" => RestoreStatus::Failed,
        b"interrupted\n" => RestoreStatus::Interrupted,
        _ => RestoreStatus::Unavailable,
    })
}

/// Take the server's shared lock after processing a staged activation offline.
pub async fn apply_pending_restore(database: &Path) -> io::Result<(File, Option<bool>)> {
    let shared = data_lock(database, false)?;
    match has_pending_work(database) {
        Ok(true) => {}
        Ok(false) => return Ok((shared, None)),
        Err(_) => {
            tracing::warn!("could not inspect pending browser restore");
            return Ok((shared, None));
        }
    }

    drop(shared);
    let outcome = match data_lock(database, true) {
        Ok(exclusive) => match apply_offline(database, Arc::new(exclusive)).await {
            Ok(outcome) => outcome,
            Err(_) => {
                tracing::warn!("could not finish pending browser restore");
                Some(false)
            }
        },
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => None,
        Err(error) => return Err(error),
    };
    Ok((data_lock(database, false)?, outcome))
}

async fn apply_offline(database: &Path, lock: Arc<File>) -> io::Result<Option<bool>> {
    let paths = RestorePaths::new(database)?;
    if regular_metadata(&paths.applying)?.is_some() {
        paths.finish(b"interrupted\n")?;
        return Ok(Some(false));
    }
    if regular_metadata(&paths.pending)?.is_none() {
        return Ok(None);
    }

    // Claim durably before touching the database. A crash cannot replay this restore.
    let directory = File::open(parent(&paths.pending))?;
    fs::rename(&paths.pending, &paths.applying)?;
    directory.sync_all()?;
    // Keep the exclusive lock through result publication as well as activation.
    let (applied, reporter): (bool, &[u8]) =
        match restore_snapshot_locked(database, &paths.applying, Arc::clone(&lock)).await {
            Ok(_) => {
                tracing::info!("pending browser restore applied");
                (true, b"applied\n")
            }
            Err(_) => {
                tracing::warn!("pending browser restore failed");
                (false, b"failed\n")
            }
        };
    if paths.finish(reporter).is_err() {
        tracing::warn!("could not persist pending browser restore result");
    }
    Ok(Some(applied))
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn database(path: &Path, slug: &str) -> sqlx::SqlitePool {
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

    async fn slug(path: &Path) -> String {
        let pool = crate::open_database(path).await.unwrap();
        let result = sqlx::query_scalar("SELECT slug FROM links")
            .fetch_one(&pool)
            .await
            .unwrap();
        pool.close().await;
        result
    }

    #[tokio::test]
    async fn a_pending_snapshot_activates_only_once_while_offline() {
        let work = tempfile::tempdir().unwrap();
        let live = work.path().join("live.sqlite");
        let lock = data_lock(&live, false).unwrap();
        database(&live, "original").await.close().await;
        let source = database(&work.path().join("source.sqlite"), "candidate").await;
        let snapshot = crate::backup::create_snapshot(&source, &work.path().join("backups"))
            .await
            .unwrap();
        source.close().await;
        let pending = PendingUpload::reserve(&live).unwrap();
        fs::copy(snapshot, pending.path()).unwrap();
        pending.validate().await.unwrap().publish().unwrap();

        assert_eq!(
            PendingUpload::reserve(&live).err().unwrap().kind(),
            io::ErrorKind::AlreadyExists
        );

        assert_eq!(restore_status(&live), RestoreStatus::PendingRestart);
        let (shared, outcome) = apply_pending_restore(&live).await.unwrap();
        assert_eq!(outcome, None);
        drop(shared);
        assert_eq!(slug(&live).await, "original");
        drop(lock);

        let (shared, outcome) = apply_pending_restore(&live).await.unwrap();
        assert_eq!(outcome, Some(true));
        drop(shared);
        assert_eq!(restore_status(&live), RestoreStatus::Applied);
        assert_eq!(slug(&live).await, "candidate");
        let (shared, outcome) = apply_pending_restore(&live).await.unwrap();
        assert_eq!(outcome, None);
        drop(shared);
        assert_eq!(restore_status(&live), RestoreStatus::Applied);
        let rollbacks = fs::read_dir(work.path()).unwrap().filter(|entry| {
            entry
                .as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".rollback-")
        });
        assert_eq!(rollbacks.count(), 1);
    }

    #[tokio::test]
    async fn an_invalid_upload_cannot_be_armed() {
        let work = tempfile::tempdir().unwrap();
        let live = work.path().join("live.sqlite");
        database(&live, "original").await.close().await;
        let pending = PendingUpload::reserve(&live).unwrap();
        fs::write(pending.path(), b"invalid snapshot").unwrap();
        let temporary = pending.path().to_owned();

        assert!(pending.validate().await.is_err());
        assert!(!temporary.exists());
        assert_eq!(restore_status(&live), RestoreStatus::None);
        assert_eq!(slug(&live).await, "original");
    }

    #[tokio::test]
    async fn a_failed_activation_is_consumed_and_keeps_the_previous_database() {
        let work = tempfile::tempdir().unwrap();
        let live = work.path().join("live.sqlite");
        database(&live, "original").await.close().await;
        let paths = RestorePaths::new(&live).unwrap();
        fs::write(&paths.pending, b"invalid snapshot").unwrap();

        drop(apply_pending_restore(&live).await.unwrap());
        assert_eq!(restore_status(&live), RestoreStatus::Failed);
        assert!(!paths.pending.exists());
        assert!(!paths.applying.exists());
        drop(apply_pending_restore(&live).await.unwrap());
        assert_eq!(slug(&live).await, "original");
    }

    #[tokio::test]
    async fn an_interrupted_attempt_does_not_automatically_retry() {
        let work = tempfile::tempdir().unwrap();
        let live = work.path().join("live.sqlite");
        database(&live, "original").await.close().await;
        let paths = RestorePaths::new(&live).unwrap();
        fs::write(&paths.applying, b"previous attempt").unwrap();

        drop(apply_pending_restore(&live).await.unwrap());
        assert_eq!(restore_status(&live), RestoreStatus::Interrupted);
        assert!(!paths.applying.exists());
        assert_eq!(slug(&live).await, "original");
    }
}
