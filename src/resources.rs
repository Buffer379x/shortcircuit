//! On-demand platform readings for authenticated diagnostics.
//!
//! Every reading reports its own availability. Process figures come from `/proc/self`, container
//! figures from the cgroup v2 files of the process's own cgroup, and storage from the filesystem
//! that holds the SQLite data. Nothing here runs in the background.

use std::{
    path::Path,
    time::{Duration, Instant},
};

use serde::Serialize;

/// The interval between the two CPU counter readings of one sample.
pub(crate) const CPU_WINDOW: Duration = Duration::from_millis(250);
/// Storage is low below this many bytes, or below three times the database size if larger.
pub(crate) const MIN_FREE_BYTES: u64 = 512 * 1024 * 1024;
const CGROUP: &str = "/sys/fs/cgroup";
const MAX_READ: u64 = 16 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Availability {
    Measured,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StorageStatus {
    Healthy,
    Low,
    Unavailable,
}

/// Free and total space of the filesystem that contains the database.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Storage {
    pub(crate) status: StorageStatus,
    /// Bytes available to the unprivileged service user.
    pub(crate) available_bytes: Option<u64>,
    pub(crate) total_bytes: Option<u64>,
    pub(crate) low_threshold_bytes: u64,
}

pub(crate) fn classify_storage(space: Option<(u64, u64)>, database_bytes: u64) -> Storage {
    let low_threshold_bytes = MIN_FREE_BYTES.max(database_bytes.saturating_mul(3));
    match space {
        Some((available, total)) => Storage {
            status: if available < low_threshold_bytes {
                StorageStatus::Low
            } else {
                StorageStatus::Healthy
            },
            available_bytes: Some(available),
            total_bytes: Some(total),
            low_threshold_bytes,
        },
        None => Storage {
            status: StorageStatus::Unavailable,
            available_bytes: None,
            total_bytes: None,
            low_threshold_bytes,
        },
    }
}

/// Reads the filesystem that holds `directory`. Blocking; call from a blocking task.
pub(crate) fn storage(directory: &Path, database_bytes: u64) -> Storage {
    classify_storage(filesystem_space(directory), database_bytes)
}

#[cfg(unix)]
fn filesystem_space(directory: &Path) -> Option<(u64, u64)> {
    let stat = rustix::fs::statvfs(directory).ok()?;
    let available = stat.f_bavail.checked_mul(stat.f_frsize)?;
    let total = stat.f_blocks.checked_mul(stat.f_frsize)?;
    Some((available, total))
}

#[cfg(not(unix))]
fn filesystem_space(_: &Path) -> Option<(u64, u64)> {
    None
}

/// CPU use over one sampling window as a percentage of one core.
/// Values above 100 mean that more than one core was busy.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct Cpu {
    pub(crate) window_ms: u64,
    pub(crate) available_cores: Option<usize>,
    pub(crate) process: Availability,
    pub(crate) process_percent: Option<f64>,
    pub(crate) container: Availability,
    pub(crate) container_percent: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CpuCounters {
    /// Process user and system time in clock ticks.
    process_ticks: Option<u64>,
    /// cgroup usage in microseconds.
    container_usec: Option<u64>,
}

fn read_counters() -> CpuCounters {
    CpuCounters {
        process_ticks: read_small("/proc/self/stat")
            .as_deref()
            .and_then(parse_process_ticks),
        container_usec: read_small(&format!("{CGROUP}/cpu.stat"))
            .as_deref()
            .and_then(parse_cgroup_usage),
    }
}

fn percent(delta: Option<u64>, unit_per_second: Option<u64>, elapsed: Duration) -> Option<f64> {
    let seconds = elapsed.as_secs_f64();
    let unit = unit_per_second.filter(|unit| *unit > 0)? as f64;
    let delta = delta? as f64;
    (seconds > 0.0).then(|| delta / unit / seconds * 100.0)
}

fn compare(
    before: CpuCounters,
    after: CpuCounters,
    elapsed: Duration,
    ticks_per_second: Option<u64>,
    available_cores: Option<usize>,
) -> Cpu {
    let delta = |start: Option<u64>, end: Option<u64>| end?.checked_sub(start?);
    let process_percent = percent(
        delta(before.process_ticks, after.process_ticks),
        ticks_per_second,
        elapsed,
    );
    let container_percent = percent(
        delta(before.container_usec, after.container_usec),
        Some(1_000_000),
        elapsed,
    );
    let availability = |value: Option<f64>| {
        if value.is_some() {
            Availability::Measured
        } else {
            Availability::Unavailable
        }
    };
    Cpu {
        window_ms: elapsed.as_millis() as u64,
        available_cores,
        process: availability(process_percent),
        process_percent: process_percent.map(round),
        container: availability(container_percent),
        container_percent: container_percent.map(round),
    }
}

fn round(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

/// Samples CPU counters twice, `CPU_WINDOW` apart.
pub(crate) async fn sample_cpu() -> Cpu {
    let before = tokio::task::spawn_blocking(read_counters)
        .await
        .unwrap_or(CpuCounters {
            process_ticks: None,
            container_usec: None,
        });
    let started = Instant::now();
    tokio::time::sleep(CPU_WINDOW).await;
    let after = tokio::task::spawn_blocking(read_counters)
        .await
        .unwrap_or(CpuCounters {
            process_ticks: None,
            container_usec: None,
        });
    compare(
        before,
        after,
        started.elapsed(),
        ticks_per_second(),
        std::thread::available_parallelism().ok().map(usize::from),
    )
}

#[cfg(target_os = "linux")]
fn ticks_per_second() -> Option<u64> {
    Some(rustix::param::clock_ticks_per_second())
}

#[cfg(not(target_os = "linux"))]
fn ticks_per_second() -> Option<u64> {
    None
}

/// Memory charged to the container's cgroup. This differs from the process resident set size
/// because it includes page cache and other processes in the same cgroup.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct ContainerMemory {
    pub(crate) status: Availability,
    pub(crate) current_bytes: Option<u64>,
    /// None when the cgroup has no limit or the limit is unavailable.
    pub(crate) limit_bytes: Option<u64>,
}

/// Blocking; call from a blocking task.
pub(crate) fn container_memory() -> ContainerMemory {
    memory_from(
        read_small(&format!("{CGROUP}/memory.current")).as_deref(),
        read_small(&format!("{CGROUP}/memory.max")).as_deref(),
    )
}

fn memory_from(current: Option<&str>, limit: Option<&str>) -> ContainerMemory {
    let current_bytes = current.and_then(|value| value.trim().parse().ok());
    let limit_bytes = limit
        .map(str::trim)
        .filter(|value| *value != "max")
        .and_then(|value| value.parse().ok());
    ContainerMemory {
        status: if current_bytes.is_some() {
            Availability::Measured
        } else {
            Availability::Unavailable
        },
        current_bytes,
        limit_bytes: current_bytes.and(limit_bytes),
    }
}

fn read_small(path: &str) -> Option<String> {
    use std::io::Read;
    let mut value = String::new();
    std::fs::File::open(path)
        .ok()?
        .take(MAX_READ)
        .read_to_string(&mut value)
        .ok()?;
    Some(value)
}

/// Sums utime and stime (fields 14 and 15) of `/proc/self/stat`.
fn parse_process_ticks(stat: &str) -> Option<u64> {
    // The command name can contain spaces and parentheses, so start after the last ')'.
    let fields: Vec<&str> = stat.rsplit_once(')')?.1.split_ascii_whitespace().collect();
    let user: u64 = fields.get(11)?.parse().ok()?;
    let system: u64 = fields.get(12)?.parse().ok()?;
    user.checked_add(system)
}

fn parse_cgroup_usage(stat: &str) -> Option<u64> {
    stat.lines()
        .find_map(|line| line.strip_prefix("usage_usec "))?
        .trim()
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1024 * 1024 * 1024;

    #[test]
    fn storage_reports_healthy_low_and_unavailable_states() {
        let healthy = classify_storage(Some((20 * GIB, 100 * GIB)), 10 * 1024 * 1024);
        assert_eq!(healthy.status, StorageStatus::Healthy);
        assert_eq!(healthy.available_bytes, Some(20 * GIB));
        assert_eq!(healthy.low_threshold_bytes, MIN_FREE_BYTES);

        let small_disk = classify_storage(Some((100 * 1024 * 1024, GIB)), 1024);
        assert_eq!(small_disk.status, StorageStatus::Low);

        // A large database raises the threshold to three copies.
        let large_database = classify_storage(Some((5 * GIB, 100 * GIB)), 2 * GIB);
        assert_eq!(large_database.low_threshold_bytes, 6 * GIB);
        assert_eq!(large_database.status, StorageStatus::Low);

        let unknown = classify_storage(None, 0);
        assert_eq!(unknown.status, StorageStatus::Unavailable);
        assert_eq!(unknown.available_bytes, None);
        assert_eq!(unknown.total_bytes, None);
    }

    #[cfg(unix)]
    #[test]
    fn the_data_directory_filesystem_can_be_read() {
        let directory = tempfile::tempdir().unwrap();
        let reading = storage(directory.path(), 0);
        assert_ne!(reading.status, StorageStatus::Unavailable);
        assert!(reading.available_bytes <= reading.total_bytes);
        let missing = storage(&directory.path().join("missing"), 0);
        assert_eq!(missing.status, StorageStatus::Unavailable);
    }

    #[test]
    fn process_and_container_counters_parse_only_valid_values() {
        let stat = "42 (short (circuit) x) S 1 42 42 0 -1 4194560 100 0 0 0 250 50 0 0 20 0 3 0";
        assert_eq!(parse_process_ticks(stat), Some(300));
        assert_eq!(parse_process_ticks("42 (truncated) S 1"), None);
        assert_eq!(parse_process_ticks("no parentheses"), None);
        assert_eq!(
            parse_cgroup_usage("usage_usec 1500000\nuser_usec 1000000\nsystem_usec 500000\n"),
            Some(1_500_000)
        );
        assert_eq!(parse_cgroup_usage("user_usec 1\n"), None);
    }

    #[test]
    fn cpu_percentages_are_labeled_per_scope() {
        let before = CpuCounters {
            process_ticks: Some(1000),
            container_usec: Some(5_000_000),
        };
        let after = CpuCounters {
            process_ticks: Some(1050),
            container_usec: Some(5_750_000),
        };
        let cpu = compare(before, after, Duration::from_secs(1), Some(100), Some(4));
        assert_eq!(cpu.process, Availability::Measured);
        assert_eq!(cpu.process_percent, Some(50.0));
        assert_eq!(cpu.container_percent, Some(75.0));
        assert_eq!(cpu.window_ms, 1000);
        assert_eq!(cpu.available_cores, Some(4));

        let unavailable = CpuCounters {
            process_ticks: None,
            container_usec: None,
        };
        let cpu = compare(
            unavailable,
            unavailable,
            Duration::from_millis(250),
            Some(100),
            None,
        );
        assert_eq!(cpu.process, Availability::Unavailable);
        assert_eq!(cpu.container, Availability::Unavailable);
        assert_eq!(cpu.process_percent, None);
        // A counter that moves backwards, for example after a cgroup change, is not reported.
        let reset = CpuCounters {
            process_ticks: Some(10),
            container_usec: Some(1),
        };
        assert_eq!(
            compare(before, reset, Duration::from_secs(1), Some(100), None).process_percent,
            None
        );
    }

    #[test]
    fn container_memory_distinguishes_missing_files_and_unlimited_cgroups() {
        let limited = memory_from(Some("104857600\n"), Some("536870912\n"));
        assert_eq!(limited.status, Availability::Measured);
        assert_eq!(limited.current_bytes, Some(104_857_600));
        assert_eq!(limited.limit_bytes, Some(536_870_912));
        let unlimited = memory_from(Some("1024"), Some("max\n"));
        assert_eq!(unlimited.limit_bytes, None);
        let missing = memory_from(None, Some("max"));
        assert_eq!(missing.status, Availability::Unavailable);
        assert_eq!(missing.limit_bytes, None);
    }
}
