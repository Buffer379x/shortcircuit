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
    time::{self, Instant},
};

use crate::{notifications::Notifier, runtime_settings::RuntimeSettings};

/// The scheduled backup job. The interval and retention come from the effective settings and
/// apply without a restart; an interval of 0 turns the schedule off.
pub struct BackupSchedule {
    settings: RuntimeSettings,
    notifier: Notifier,
    /// The length of one interval hour, shorter only in tests.
    hour: Duration,
}

impl BackupSchedule {
    pub fn new(settings: &RuntimeSettings, notifier: Notifier) -> Self {
        Self {
            settings: settings.clone(),
            notifier,
            hour: Duration::from_secs(3600),
        }
    }

    /// The interval, or `None` while scheduled backups are off, and the retention count.
    fn current(&self) -> (Option<Duration>, usize) {
        let settings = self.settings.current();
        let hours = u32::from(settings.backup_interval_hours.value);
        (
            (hours > 0).then(|| self.hour * hours),
            usize::from(settings.backup_keep.value),
        )
    }

    pub async fn run(
        self,
        pool: SqlitePool,
        backup_dir: PathBuf,
        mut shutdown: watch::Receiver<bool>,
    ) {
        let scheduled = backup_dir.join("scheduled");
        let mut settings = self.settings.subscribe();
        // When the schedule became active, and when this process last attempted a backup.
        let mut active_since: Option<Instant> = None;
        let mut last_attempt: Option<Instant> = None;
        loop {
            if *shutdown.borrow() {
                return;
            }
            let (Some(interval), _) = self.current() else {
                active_since = None;
                tokio::select! {
                    biased;
                    _ = shutdown.wait_for(|stopped| *stopped) => return,
                    Ok(()) = settings.changed() => {}
                }
                continue;
            };
            let since = *active_since.get_or_insert_with(Instant::now);
            // Continue the schedule from the newest scheduled snapshot, so restarts and setting
            // changes cannot postpone backups indefinitely. An overdue backup runs shortly after
            // startup. A failed attempt waits a full interval before the next one.
            let newest = {
                let scheduled = scheduled.clone();
                task::spawn_blocking(move || newest_snapshot(&scheduled))
                    .await
                    .ok()
                    .flatten()
            };
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_secs());
            let delay = next_delay(
                newest,
                now,
                interval,
                since.elapsed(),
                last_attempt.map(|attempt| attempt.elapsed()),
            );
            tokio::select! {
                biased;
                _ = shutdown.wait_for(|stopped| *stopped) => return,
                Ok(()) = settings.changed() => continue,
                _ = time::sleep(delay) => {}
            }
            last_attempt = Some(Instant::now());
            let result = crate::backup::create_snapshot(&pool, &scheduled).await;
            crate::backup::record_backup_result(&backup_dir, result.is_ok()).await;
            let completed = match result {
                Ok(path) => {
                    tracing::info!("scheduled backup verified");
                    path
                }
                Err(_) => {
                    tracing::warn!("scheduled backup creation failed");
                    let at = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |elapsed| elapsed.as_secs() as i64);
                    let event = crate::notifications::Event::BackupFailed { at };
                    if self.notifier.record(&pool, event).await.is_err() {
                        tracing::warn!("could not record a backup failure notification");
                    }
                    continue;
                }
            };
            // Retention reads the current setting, so a changed count applies to the next run.
            let keep = self.current().1;
            if !matches!(
                task::spawn_blocking(move || prune(&completed, keep)).await,
                Ok(Ok(()))
            ) {
                tracing::warn!("scheduled backup retention failed");
            }
        }
    }
}

/// Delay before the next scheduled backup. Without any scheduled snapshot, the first one runs
/// one interval after the schedule became active.
fn next_delay(
    newest: Option<u64>,
    now: u64,
    interval: Duration,
    active_for: Duration,
    since_attempt: Option<Duration>,
) -> Duration {
    let scheduled = match newest {
        Some(_) => first_delay(newest, now, interval),
        None => interval.saturating_sub(active_for),
    };
    let retry = since_attempt.map_or(Duration::ZERO, |elapsed| interval.saturating_sub(elapsed));
    scheduled.max(retry)
}

/// Delay before the first scheduled backup after startup.
const STARTUP_DELAY: Duration = Duration::from_secs(60);

fn first_delay(newest: Option<u64>, now: u64, interval: Duration) -> Duration {
    match newest {
        Some(created) => {
            let due = created.saturating_add(interval.as_secs());
            Duration::from_secs(due.saturating_sub(now)).max(STARTUP_DELAY)
        }
        None => interval,
    }
}

fn newest_snapshot(directory: &Path) -> Option<u64> {
    fs::read_dir(directory)
        .ok()?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter_map(|entry| backup_timestamp(&entry.file_name()))
        .max()
}

pub(crate) fn backup_timestamp(name: &OsStr) -> Option<u64> {
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

/// Keeps the `keep` newest snapshots in the scheduled or pre-upgrade directory of `completed`.
pub(crate) fn prune(completed: &Path, keep: usize) -> io::Result<()> {
    let directory = completed
        .parent()
        .filter(|parent| {
            [OsStr::new("scheduled"), OsStr::new("pre-upgrade")]
                .contains(&parent.file_name().unwrap_or_default())
        })
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

    fn snapshots(directory: &Path) -> usize {
        fs::read_dir(directory).map_or(0, |names| {
            names
                .filter(|entry| {
                    entry
                        .as_ref()
                        .is_ok_and(|entry| backup_timestamp(&entry.file_name()).is_some())
                })
                .count()
        })
    }

    #[tokio::test]
    async fn a_saved_interval_starts_the_schedule_without_a_restart() {
        let work = tempfile::tempdir().unwrap();
        let pool = crate::open_database(&work.path().join("test.db"))
            .await
            .unwrap();
        let container = crate::runtime_settings::OperationsEnvironment::from_env(|name| {
            (name == "BACKUP_INTERVAL_HOURS").then(|| "0".to_owned())
        })
        .unwrap();
        let settings = RuntimeSettings::load(&pool, container).await.unwrap();
        let target = work.path().join("backups");
        let (stop, signal) = watch::channel(false);
        let mut schedule = BackupSchedule::new(&settings, Notifier::new(&settings));
        schedule.hour = Duration::from_millis(200);
        let worker = tokio::spawn(schedule.run(pool.clone(), target.clone(), signal));
        let scheduled = target.join("scheduled");
        // The container turned scheduled backups off: nothing is written.
        time::sleep(Duration::from_millis(600)).await;
        assert_eq!(snapshots(&scheduled), 0);

        // Saving an interval applies at once; the first backup follows one interval later.
        settings
            .save(
                &pool,
                &serde_json::from_value(serde_json::json!({"backup_interval_hours": 1})).unwrap(),
                || true,
            )
            .await
            .unwrap();
        time::timeout(Duration::from_secs(5), async {
            while snapshots(&scheduled) == 0 {
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
    fn failed_attempts_wait_a_full_interval_and_changes_keep_the_schedule() {
        let hour = Duration::from_secs(3600);
        let now = 1_800_000_000;
        // No snapshot: one interval after the schedule became active, not after each change.
        assert_eq!(next_delay(None, now, hour, Duration::ZERO, None), hour);
        assert_eq!(
            next_delay(None, now, hour, Duration::from_secs(600), None),
            Duration::from_secs(3000)
        );
        // An overdue snapshot runs soon, unless this process just failed an attempt.
        assert_eq!(
            next_delay(Some(now - 7200), now, hour, Duration::ZERO, None),
            STARTUP_DELAY
        );
        assert_eq!(
            next_delay(
                Some(now - 7200),
                now,
                hour,
                Duration::ZERO,
                Some(Duration::ZERO)
            ),
            hour
        );
    }

    #[test]
    fn restarts_continue_the_schedule_and_run_overdue_backups_soon() {
        let day = Duration::from_secs(24 * 3600);
        let now = 1_800_000_000;
        // No scheduled snapshot yet: wait one interval.
        assert_eq!(first_delay(None, now, day), day);
        // The last scheduled snapshot is 25 hours old: run after the startup delay.
        assert_eq!(first_delay(Some(now - 25 * 3600), now, day), STARTUP_DELAY);
        // One hour old: continue the existing schedule instead of starting over.
        assert_eq!(
            first_delay(Some(now - 3600), now, day),
            Duration::from_secs(23 * 3600)
        );
        let directory = tempfile::tempdir().unwrap();
        for name in [
            "shortcircuit-1700000000-0123456789abcdef0123456789abcdef.sqlite",
            "shortcircuit-1700000100-0123456789abcdef0123456789abcdef.sqlite",
            "notes.txt",
        ] {
            fs::write(directory.path().join(name), b"").unwrap();
        }
        assert_eq!(newest_snapshot(directory.path()), Some(1_700_000_100));
        assert_eq!(newest_snapshot(&directory.path().join("missing")), None);
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
