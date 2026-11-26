# Resource and Performance Plan

## Loopback smoke measurement

The container workflow measures one existing short link in its synthetic test data after the restore exercise. It warms the app with 64 requests, then times 1,024 requests across eight HTTP/1.1 connections. The runner records architecture, virtual CPU count, time percentiles, throughput, errors, and a Docker memory snapshot. Analytics uses its default settings. This loopback result describes the test runner and the small fixture.

The first verified run used an x86-64 runner with four virtual CPUs. All 1,024 requests returned the expected status. P50, P95, and P99 were 0.702 ms, 1.184 ms, and 1.829 ms. Docker reported 4.453 MiB immediately after traffic. Record idle process RSS and memory accounting separately on the target hardware. [Verified CI measurement](https://github.com/Buffer379x/shortcircuit/actions/runs/45766453106)

The measurement ends at the short link response. Repeat `--slug` to visit multiple known short paths. Use a separate client on the target LAN for the production benchmark. Record the CPU model, database size, number of links, active features, and proxy layout alongside the result.

```sh
python3 tests/redirect_benchmark.py --base-url http://localhost:8080 --slug example --warmup 200 --requests 10000 --concurrency 16
```


The figures below are project targets and planning estimates. Release builds and benchmark results are pending. The reference environment is an amd64 Linux homelab with an Intel N100 (four cores) or equivalent processor, 8 GiB of RAM, a local SSD pool, and one ShortCircuit container. Start with four SQLite connections and two Tokio worker threads. The reference dataset has 10,000 links across four domains, 1,000,000 historical visits, 20 tags, and 3% limited links. Report monitoring and GeoIP overhead separately. Measure latency through completion of the redirect response and record its actual status. Disable navigation to the destination.

| Metric | Project target or definition | Planning estimate |
|---|---|---|
| Idle RSS | Ideally below 40 MB. Measure process RSS in MiB and convert to MB for comparison. | About 20–45 MiB after warm-up, including the application-side SQLite cache and base frontend assets. Measure separately with an active sign-in and with GeoIP enabled. |
| Container memory | Measure cgroup `memory.current` in bytes and use `memory.stat` to explain the breakdown. | About 30–90 MiB before the OS file cache fully warms up. Warm file caching can raise this substantially. Set a container memory cap using container measurements and workload requirements. |
| Idle CPU | Report median and P95 utilization as a percentage of one core, with job activity identified. | Target below 0.5% of one core over 10 minutes without network or maintenance activity. |
| Image | Ideally below 80 MB. Report compressed transfer size and uncompressed local size separately for the same platform image. | The runtime currently uses Debian Slim. The CI container build records the size returned by Docker image inspect. Report the Docker image store and measurement command, and measure registry transfer size from a published image digest. |
| Redirects | LAN P95 below 10 ms under representative load. Measure directly and through the local TLS proxy. | Plan for 20–100 requests/s in the mixed workload. Report limited and protected links and concurrent jobs separately. |
| Startup | Prefer under 3 seconds from process start to readiness with a consistent database. | Budget 0.3–2 seconds for warm startup. Measure cold startup, restore, a full WAL, and schema upgrades separately. |
| Storage | Test with 1 million detailed visits. | Roughly 150–500 MiB with core indexes and representative field lengths. Referrer cardinality, rollups, and SQLite page utilization affect size. Account for WAL, staging, assets, and backups separately. |

Avoid double-counting overlapping memory components. Read RSS, PSS, and composition from `/proc/<pid>/smaps_rollup`. Read `memory.current`, `memory.stat` (`anon`, `file`, and `shmem`), `memory.peak`, `memory.events`, and `cpu.stat` from the container cgroup. Measure the kernel's file cache separately from the SQLite pager cache bounded by SQL settings. Docker CLI and kernel figures can differ in their cache accounting. When relevant, measure MaxMind file size, warm mapped pages, and browser or image generation peaks separately. [Linux Kernel Documentation on cgroup v2](https://docs.kernel.org/admin-guide/cgroup-v2.html)

## Reproducible Benchmark Protocol

1. Record the image version and digest, Git commit, CPU frequency profile, RAM, kernel, Docker version, filesystem, SSD or pool model, TLS and proxy settings, and network latency. Use the same hardware, representative dataset, protection, and tracking settings when comparing runs. Run each configuration in isolation. Put the benchmark client on the same LAN and test keep-alive and cold TLS connections separately.
2. Save a deterministic seed such as `20261125`. Generate ten months of visits, multiple domains, relevant slug types, representative referrer cardinality, and hot and cold link sets. Use synthetic data and keep requests within the benchmark environment.
3. Use an open-loop load generator with redirect following disabled. After a 60-second warm-up, run each scenario five times for two minutes. Measure the baseline at 20 and 100 requests/s with up to 16 and 32 concurrent requests, respectively. Stress-test at 250 requests/s with up to 64 concurrent requests. Record the generator and version and check sampled status codes and `Location` values against expectations. A sample workload is 90% successful regular redirects, 5% missing links, 3% limited links, and 2% `HEAD` requests. Reset limited-link fixtures before each run. Measure limit boundaries and the post-exhaustion response mix separately.
4. Run scenarios with analytics off, with standard settings, with admin reporting or retention active, and with backup or health jobs active. Use local test servers and an isolated network policy for monitoring. Measure cold and warm caches and UI or dashboard queries. Report each safety and durability configuration separately.
5. Publish P50, P95, and P99 latency, error and timeout rates, throughput, events dropped when the queue is full, SQLite wait and writer time, WAL size, database and index bytes, CPU deltas, RSS, and cgroup memory for each scenario. Report which release paths were exercised and distinguish direct-listener measurements from LAN measurements through the TLS proxy. After warm-up, measure 10 minutes of idle operation both with and without scheduled background work.
6. Measure startup in five runs each with an empty and a representative database. Log container start, process start, and the first successful readiness check separately. Document warm starts and cold filesystem cache runs on a dedicated benchmark host. Label image sizes with architecture, compression, and measurement tool. Retain query plans for expensive SQL.

Keep sensitive destination URLs out of profiling logs. Refine performance gates once the first measurements establish a baseline, accounting for the reference hardware when setting P95 targets. Publish the methodology with the results.

## Budgets and Overload Behavior

| Component | Initial budget | Behavior under saturation |
|---|---|---|
| Public redirects | One indexed read with no network request. Bound connections and deadlines. Give redirects priority within the connection budget by limiting admin and job concurrency. | A failed read returns a generic `503` and bypasses fallback handling. Readable unlimited links continue to resolve if the analytics writer fails. |
| Limited redirects | Short SQLite write with a conditional update and a durable commit. | Return `503` without `Location` on a lock, full disk, or expired deadline. Use the configured response or fallback when a link has expired or its limit is exhausted. |
| Analytics | Queue at most 4,096 events with field length caps and a 4 MiB byte limit. Write a batch of 128 events or flush after up to 250 ms from the first event. | Drop new events when the queue is full and increment a loss counter. Retry transient database failures within a short budget. Release writer resources on repeated failure. |
| Admin and SQLite | Four connections and a default busy timeout of two seconds. Measure contention before lowering the configured timeout. Bound admin report queries, use keyset pagination, and cap page sizes. | Run large exports and imports asynchronously in batches. Cancel long read transactions and analytical scans to limit WAL growth and protect redirect capacity. |
| Jobs | At most one backup or import maintenance task at a time. One monitor probe executes at a time. The idle schedule uses the next due check and a bounded recheck window. | Apply backoff and jitter after failures or external throttling. Pause jobs when disk space is low or redirect or database load is high. |
| Disk | Before backup, import, or restore, check capacity for the WAL and expected snapshots or staging data. Budget storage using database size and backup retention. | Return an explicit error for new mutations or imports. Protect the existing database and valid backups, and leave cleanup to the defined retention process. |
| Frontend | Build and precompress cacheable assets. The initial JavaScript budget is below 120 KiB gzip. Lazy-load analytics, QR, and import modules. | Publish a release bundle report. Justify increases or defer modules. Limit fonts and third-party requests to those the interface requires. |

The initial design relies on indexed reads and bounded SQLite statement and page caches. Consider a redirect cache only if measurements show a need and changes, imports, deactivations, and restores can invalidate affected entries. See [Database Design](DATABASE.md) for WAL, checkpoints, schemas, and backups.

## Shutdown and Observability

The source allows up to ten seconds for HTTP shutdown, two seconds for analytics, five seconds for scheduled backup handling, then two seconds to close the database pool. The Compose configuration allows 25 seconds.

The target shutdown sequence marks the app unready on `SIGTERM` and closes both listeners to new requests. Let in-flight requests finish within a deadline. Block new jobs, stop or checkpoint active jobs, and drain analytics within a bounded period. Report remaining events as lost where possible, then close database handles. A container stop grace period of 30 seconds is one way to accommodate an application shutdown of up to 20 seconds. A host or process crash can lose buffered analytics events. Limits are committed durably before the response.

Authorized administrators can see system and job status, free disk space, SQLite and WAL sizes, queue usage, and dropped-event counts. Label RSS and cgroup readings separately and keep optional metrics internal. Structured logs and built-in system status provide diagnostics.
