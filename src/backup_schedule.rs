use std::{
    ffi::OsStr,
    fs::{self, File},
    io,
    path::{Path, PathBuf},
    time::Duration,
};

use sqlx::SqlitePool;
use tokio::{
    sync::watch,
    task,
    time::{self, Instant, MissedTickBehavior},
};

pub struct BackupSchedule {
    interval: Duration,
    keep: usize,
}

impl BackupSchedule {
    pub fn new(interval_hours: u64, keep: usize) -> Result<Option<Self>, &'static str> {
        if interval_hours == 0 {
            return Ok(None);
        }
        if interval_hours > 720 {
            return Err("scheduled backup interval must be between 1 and 720 hours");
        }
        if !(1..=365).contains(&keep) {
            return Err("scheduled backup retention must be between 1 and 365 snapshots");
        }
        Ok(Some(Self {
            interval: Duration::from_secs(interval_hours * 3600),
            keep,
        }))
    }

    pub async fn run(
        self,
        pool: SqlitePool,
        backup_dir: PathBuf,
        mut shutdown: watch::Receiver<bool>,
    ) {
        let scheduled = backup_dir.join("scheduled");
        let mut timer = time::interval_at(Instant::now() + self.interval, self.interval);
        timer.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                biased;
                _ = shutdown.wait_for(|stopped| *stopped) => return,
                _ = timer.tick() => {}
            }
            let result = crate::backup::create_snapshot(&pool, &scheduled).await;
            crate::backup::record_backup_result(&backup_dir, result.is_ok()).await;
            let completed = match result {
                Ok(path) => {
                    tracing::info!("scheduled backup verified");
                    path
                }
                Err(_) => {
                    tracing::warn!("scheduled backup creation failed");
                    continue;
                }
            };
            let keep = self.keep;
            if !matches!(
                task::spawn_blocking(move || prune(&completed, keep)).await,
                Ok(Ok(()))
            ) {
                tracing::warn!("scheduled backup retention failed");
            }
        }
    }
}

fn backup_timestamp(name: &OsStr) -> Option<u64> {
    let name = name.to_str()?.strip_prefix("shortcircuit-")?;
    let (timestamp, nonce) = name.strip_suffix(".sqlite")?.split_once('-')?;
    if timestamp.is_empty()
        || (timestamp.len() > 1 && timestamp.starts_with('0'))
        || !timestamp.bytes().all(|byte| byte.is_ascii_digit())
        || nonce.len() != 32
        || !nonce
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return None;
    }
    timestamp.parse().ok()
}

fn prune(completed: &Path, keep: usize) -> io::Result<()> {
    let directory = completed
        .parent()
        .filter(|parent| parent.file_name() == Some(OsStr::new("scheduled")))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid schedule directory"))?;
    let protected = completed
        .file_name()
        .filter(|name| backup_timestamp(name).is_some())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid snapshot name"))?;
    if !fs::symlink_metadata(directory)?.is_dir() || !fs::symlink_metadata(completed)?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid completed snapshot",
        ));
    }

    let mut prior = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name();
        if name.as_os_str() == protected {
            continue;
        }
        let Some(timestamp) = backup_timestamp(&name) else {
            continue;
        };
        if entry.file_type()?.is_file() {
            prior.push((timestamp, name));
        }
    }
    prior.sort_unstable();
    let remove = prior.len().saturating_sub(keep - 1);
    if remove == 0 {
        return Ok(());
    }

    for (_, name) in prior.into_iter().take(remove) {
        let candidate = directory.join(name);
        let removed = match fs::symlink_metadata(&candidate) {
            Ok(metadata) if metadata.is_file() => fs::remove_file(candidate),
            Ok(_) => Ok(()),
            Err(error) => Err(error),
        };
        if let Err(error) = removed
            && error.kind() != io::ErrorKind::NotFound
        {
            return Err(error);
        }
    }
    File::open(directory)?.sync_all()
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;

    use super::*;

    fn backup_name(seconds: u64) -> String {
        format!("shortcircuit-{seconds}-0123456789abcdef0123456789abcdef.sqlite")
    }

    #[tokio::test]
    async fn scheduled_job_publishes_a_snapshot_and_stops_on_signal() {
        let work = tempfile::tempdir().unwrap();
        let pool = crate::open_database(&work.path().join("test.db"))
            .await
            .unwrap();
        let target = work.path().join("backups");
        let (stop, signal) = watch::channel(false);
        let worker = tokio::spawn(
            BackupSchedule {
                interval: Duration::from_millis(400),
                keep: 2,
            }
            .run(pool, target.clone(), signal),
        );
        let scheduled = target.join("scheduled");
        assert!(!scheduled.exists());
        time::timeout(Duration::from_secs(5), async {
            loop {
                if fs::read_dir(&scheduled).is_ok_and(|mut names| {
                    names.any(|entry| {
                        entry.is_ok_and(|entry| backup_timestamp(&entry.file_name()).is_some())
                    })
                }) {
                    break;
                }
                time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .unwrap();
        stop.send_replace(true);
        time::timeout(Duration::from_secs(2), worker)
            .await
            .unwrap()
            .unwrap();
    }

    #[test]
    fn validates_enabled_hours_and_retention_count() {
        assert!(matches!(BackupSchedule::new(0, 0), Ok(None)));
        assert!(matches!(BackupSchedule::new(0, 366), Ok(None)));
        for (hours, keep) in [(1, 1), (720, 365)] {
            let schedule = BackupSchedule::new(hours, keep).unwrap().unwrap();
            assert_eq!(schedule.interval, Duration::from_secs(hours * 3600));
            assert_eq!(schedule.keep, keep);
        }
        assert!(BackupSchedule::new(721, 7).is_err());
        assert!(BackupSchedule::new(1, 0).is_err());
        assert!(BackupSchedule::new(1, 366).is_err());
    }

    #[test]
    fn recognizes_only_canonical_owned_names() {
        for seconds in [0, 1, u64::MAX] {
            let name = backup_name(seconds);
            assert_eq!(backup_timestamp(OsStr::new(&name)), Some(seconds));
        }
        for filename in [
            "shortcircuit-00-0123456789abcdef0123456789abcdef.sqlite",
            "shortcircuit-+1-0123456789abcdef0123456789abcdef.sqlite",
            "shortcircuit-18446744073709551616-0123456789abcdef0123456789abcdef.sqlite",
            "shortcircuit-2-0123456789ABCDEF0123456789ABCDEF.sqlite",
            "shortcircuit-2-0123456789abcdef.sqlite",
            "shortcircuit-2-0123456789abcdef0123456789abcdef.sqlite-wal",
            "manual-2-0123456789abcdef0123456789abcdef.sqlite",
        ] {
            assert_eq!(backup_timestamp(OsStr::new(filename)), None, "{filename}");
        }
    }

    #[test]
    fn retention_pins_the_verified_snapshot_despite_clock_skew() {
        let root = tempfile::tempdir().unwrap();
        let scheduled = root.path().join("scheduled");
        fs::create_dir(&scheduled).unwrap();
        let created = scheduled.join(backup_name(9));
        let old = scheduled.join(backup_name(99));
        let other_newest = scheduled.join(backup_name(101));
        let manual = root.path().join(backup_name(9));
        let unrelated = scheduled.join("personal-export.sqlite");
        for file in [&created, &old, &other_newest, &manual, &unrelated] {
            fs::write(file, b"test file").unwrap();
        }
        let linked = scheduled.join(backup_name(1));
        symlink(&manual, &linked).unwrap();
        let directory = scheduled.join(backup_name(2));
        fs::create_dir(&directory).unwrap();

        prune(&created, 2).unwrap();
        assert!(created.is_file());
        assert!(other_newest.is_file());
        assert!(!old.exists());
        assert!(unrelated.is_file());
        assert!(directory.is_dir());
        assert!(fs::symlink_metadata(&linked).unwrap().is_symlink());
        assert_eq!(fs::read(&manual).unwrap(), b"test file");
        prune(&created, 1).unwrap();
        assert!(created.is_file());
        assert!(!other_newest.exists());
    }

    #[test]
    fn retention_keeps_priors_if_the_newly_verified_file_is_missing() {
        let root = tempfile::tempdir().unwrap();
        let scheduled = root.path().join("scheduled");
        fs::create_dir(&scheduled).unwrap();
        let prior = scheduled.join(backup_name(99));
        fs::write(&prior, b"prior snapshot").unwrap();

        assert!(prune(&scheduled.join(backup_name(102)), 1).is_err());
        assert_eq!(fs::read(&prior).unwrap(), b"prior snapshot");
    }
}
