# Quality review

This review covers the ShortCircuit source after the Stage 1 work from the [Claude Code handoff](CLAUDE_CODE_HANDOFF.md), from four perspectives: software developer, product designer, user and operator, and security engineer. Each finding has a stable ID, a priority, evidence, and an acceptance check. [Improvement plan](IMPROVEMENT_PLAN.md) · [Project status](PROJECT_STATUS.md) · [Release verification](RELEASE_VERIFICATION.md)

Priorities: **P0** release blocker, **P1** high, **P2** medium, **P3** low, **P4** optional. Evidence is **Verified** when it was reproduced or read in code, and **Hypothesis** when it was inferred without reproduction. Effort: **S** under one hour, **M** about half a day, **L** more. The **Status** column records the result of the improvement work. See the plan for details.

## Reviewed state

| Item | Value |
|---|---|
| Source | Working tree on November 26, 2026: the handoff baseline `main` at `d7c56a4e8883c0c27da94ed8918f38c6bdfc4436` plus Stage 1 (schema 19 to 23). The exported workspace has no Git metadata, so no review commit exists. |
| Baseline CI | Core checks on `d7c56a4` succeeded (run 45879510730). The changes reviewed here have not run in CI. |
| Toolchain | Rust 1.100.0 and Node 24.22.3 in a local container, host Python 3.14 with SQLite 3.46.1, Docker 29.9.3, Playwright 1.65.1 Chromium, axe-core 4, Trivy 0.76.1, cargo-deny 0.22.1. |
| Host | x86-64, Intel Core i9-14900K, 8 visible CPUs, Linux. No Unraid host, Nginx Proxy Manager, real domains, TLS, Mobile Safari, or screen reader. |
| Runtime | Candidate image built locally (`shortcircuit:0.6.0-preview.2` tag, schema 23), started with `compose.yaml` (read-only root, all capabilities dropped). Published `ghcr.io/buffer379x/shortcircuit:0.6.0-preview.2` (`sha256:e27333a0…9f18`, schema 11) for upgrade and rollback. |
| Workflows exercised | Clean start, listener separation, non-root user, the Chromium admin workflow (`tests/admin-browser.cjs`), CLI backup, refused live restore, offline restore, upgrade from preview.2, rollback to preview.2 from the pre-upgrade snapshot, image scan, SBOM, loopback benchmark, 44 `/api/v1` routes without credentials and with three key types, template-style installation, account setup, recovery token, link creation and editing, bulk actions, analytics, import, backup and restore through the browser API, 77 screenshots at seven sizes with axe checks, and keyboard flows. |
| Tests run before improvements | 166 Rust tests (three cold runs), Clippy with `-D warnings`, Rustfmt, svelte-check, `tests/url-cleaner.cjs`, `tests/openapi.py`, `tests/schema.py`, `tests/analytics_schema.py`, `tests/unraid_template.py`, `cargo deny check advisories licenses sources`, `npm audit --audit-level=high`. |
| Coverage limits | Data volumes were synthetic, up to 20,001 links, 300,000 events, and 257,222 daily rows. Outbound features ran only against refused or unreachable targets: no real public title fetch, no real webhook receiver, and no private-CA connector. Query plans came from SQLite 3.46.1 on the host, not the bundled 3.54.1. Real screen readers, WebKit, Firefox, touch devices, zoom, forced colors, IPv6-only networks, and real proxy trust were not tested. |

## Correction to an earlier interpretation

The Stage 1 benchmark reported 448 dropped analytics events, which were first read as queue overflow during a burst. DEV-01 shows that they were failed write batches (7 × 64) caused by the missing SQLite temp directory. A controlled rerun confirmed it: 2,048 redirects with the image as shipped recorded 641 events and dropped 1,408. With `--tmpfs /tmp` the same load recorded all 2,049 events and dropped none.

## Developer

| ID | Priority | Finding | Evidence | Status |
|---|---|---|---|---|
| DEV-01 | P0 | The read-only container gives SQLite no writable temp directory. Statements that spill to a temp file fail with `SQLITE_IOERR_GETTEMPPATH` (6410): multi-row analytics batches, large sorts and GROUP BY in reports and catalog search, `VACUUM` in restore, and table rebuilds during upgrades with real data. | **Verified** with control runs (see above). Upgrade of a preview.2 volume with 300,000 events failed at migration 16. CLI restore of a 55 MB snapshot failed. Summary and text search returned 503 with 20,001 links. All pass with `--tmpfs /tmp`. Present since at least preview.2: `compose.yaml` and the Unraid template use a read-only root without tmpfs. | Fixed |
| DEV-02 | P1 | A failed migration leaves the database at an intermediate schema that neither the new nor the old image opens, and nothing takes a pre-upgrade snapshot. | **Verified**: the failed upgrade above stopped at schema 15. The old image reports `VersionMissing(12)`. | Fixed |
| DEV-03 | P2 | Analytics write failures are silent and counted as queue drops. | **Verified** in code: `run_writer` adds the batch size to `dropped` without logging. | Fixed |
| DEV-04 | P2 | Notification detection queries `LIMIT 100` without excluding keys already recorded, so candidates beyond the first 100 are never detected. Trimming the outbox to 1,000 rows can recreate trimmed keys that are still in a detection window. | **Verified** in code and with a 150-link test. | Fixed |
| DEV-05 | P2 | The CLI restore skips the notification and title-lookup reset that the staged restore performs, and notification `id` values repeat after a restore because they derive from database IDs. | **Verified** in code (`restore_snapshot` versus `record_startup_restore`). | Fixed |
| DEV-06 | P3 | Click-milestone detection scans `analytics_daily` once per milestone every minute: about 203 ms per milestone with 257,222 rows. | **Verified** with timing and `EXPLAIN QUERY PLAN`. | Fixed |
| DEV-07 | P3 | A pending title lookup still fetches a destination after password or one-time protection is added. Same as SEC-03. | **Verified** live. | Fixed |
| DEV-08 | P3 | A link-health status update and its outbox row are separate statements, so a notification can be lost. | **Verified** in code. | Fixed (one transaction; no failure-injection test) |
| DEV-09 | P3 | The notification task holds an `Analytics` clone, so on shutdown the writer can be aborted while the notifier delivers. | **Hypothesis**: an idle stop finished in 0.64 s. | Fixed |
| DEV-10 | P4 | `GET /api/v1/system` waits for a fixed 250 ms CPU sample and counts all raw events. | **Verified**. The sample window is documented and the endpoint is browser-only. | Fixed (background sampler every 15 s with sample age, raw count reused for 30 s) |
| DEV-11 | P4 | Hourly retention runs inside the analytics writer task, about 630 ms per run on the seeded volume. | **Verified**. Events queue during the run, bounded by the queue. | Fixed (own retention task, 512-row chunks with pauses; test writes an event mid-run) |
| DEV-12 | P4 | The public listener binds before migrations finish, so early connections wait. | **Verified** in code. | Fixed |
| OWN-1 | P1 | Migrations ran through the request pool, so its 2-second acquire timeout failed `open_database` under load (`PoolTimedOut` in 8 to 24 tests per cold run, and the same risk at startup on a busy host). | **Verified** in three cold runs before and after. | Fixed in Stage 1 |
| OWN-4 | P3 | Diagnostics reported `build_revision: null` in a locally built image. Cause: `compose.yaml` uses `pull_policy: build`, so `docker compose up` rebuilt the image without the variable. Not a product defect. | **Verified**: with the variable exported for `up`, the image reports it. | Hardened: `build.rs` passes the revision explicitly, `--version` prints it, and CI checks it |
| OWN-6 | P4 | `deny.toml` allows three licenses that no dependency uses. | **Verified**: cargo-deny warnings. | Fixed |
| OWN-7 | P4 | `tests/analytics_schema.py` needs a host SQLite whose JSON functions are innocuous under `trusted_schema=OFF`. Python with SQLite 3.40.1 fails, 3.46.1 passes. | **Verified**. CI uses the runner's Python. | Documented |

Query plans: the redirect lookup uses the `(host, slug)` index (about 0.05 ms). Report queries took 3 to 28 ms on the seeded volume. Migrations 19 to 23 preserve AUTOINCREMENT sequences, triggers, and indexes.

## Product designer

| ID | Priority | Finding | Evidence | Status |
|---|---|---|---|---|
| UX-01 | P1 | In the link dialog at 1440 × 900, a server error such as "slug in use" renders below the visible part of the dialog, the footer scrolls away, focus drops to `<body>`, and the error is not linked to the field. | **Verified** with a keyboard-only Playwright flow: error top 890 px, dialog bottom 888 px. | Fixed |
| UX-02 | P2 | Focus is lost to `<body>` after Delete link, Revoke key, Remove rule, and Check impact. | **Verified**. | Fixed |
| UX-03 | P2 | The fixed desktop rail cannot scroll, so items and the profile are cut off below about 720 px of height, for example at 125 % zoom on a 1366 × 768 laptop. | **Verified** at 1200 × 600 and 1366 × 650. | Fixed |
| UX-04 | P2 | Dark-theme input borders measure 2.85:1 on surfaces and 2.53:1 on raised surfaces, below the 3:1 non-text contrast minimum. | **Verified** by computing WCAG contrast from the tokens. | Fixed |
| UX-05 | P2 | A shared container query turns narrow-panel tables into unlabeled cards even at 1440 px (Overview, Analytics, Unusual traffic, Notifications). | **Verified** in screenshots. | Fixed |
| UX-06 | P2 | Destination policy and API key errors are not tied to the rule or fieldset that caused them. | **Verified**. | Fixed |
| UX-07 | P2 | Pages have no URL, so reload, Back, and bookmarks lose the location. The design system documents hash routes. | **Verified** in code. | Fixed |
| UX-08 | P2 | On phones all 11 navigation items wrap inline and take about 295 px at 320 × 568. The design system specifies a menu. | **Verified** in screenshots. | Fixed |
| UX-09 | P3 | Terminology varies within each language: Clicks, Redirects, and visits, and in German Klicks, Weiterleitungen, Aufrufe, and Zugriffe. German uses both Link-Monitor and Link-Überwachung. Title lookup reuses "Checks scheduled". Hours use a hard-coded unit. | **Verified** in the catalog. | Fixed in source after 0.8.0-preview.2: one count term per language (English "visits", German "Aufrufe" and "Aufruflimit"), and "Health"/"Zustand" for link checks in both languages |
| UX-10 | P3 | Native file inputs show browser-language text in the German UI. | **Verified**. | Fixed with Polar Signal (0.8.0-preview.1): file inputs are hidden behind localized buttons |
| UX-11 | P3 | `h3` has no type token, and the Unusual traffic header lays out differently from other panels. | **Verified**. | Fixed |
| UX-12 | P3 | Dialogs put initial focus on Close instead of the first field. | **Verified**. | Fixed |
| UX-13 | P3 | Row action buttons repeat the same accessible names on every row. | **Verified**. | Fixed with Polar Signal (0.8.0-preview.1): row actions name their link, for example "More actions for go.example/docs" |
| UX-14 | P4 | A few targets are below the 44 px project target but meet WCAG 2.5.8. Overview and Analytics share a navigation icon. | **Verified**. | Partly fixed |
| OWN-5 | P2 | The Destination policy enforce checkbox stretched across the row, the panel touched the domain list, and container memory read "of no limit". | **Verified** in screenshots. | Fixed during review |

Verified strengths: text tokens pass AA in both themes (lowest 4.93:1), focus outlines measure 8.9:1 and above, native dialogs trap focus and return it, the System theme follows the OS, the English and German catalogs are complete (702 keys each), axe found no violations in 42 scanned screens, and no screen overflows horizontally at any of the seven sizes.

## User and operator

| ID | Priority | Finding | Evidence | Status |
|---|---|---|---|---|
| OPS-01 | P1 | With the Unraid template defaults, the database, rollback copies, and every backup live in one named volume inside `docker.img`, scheduled backups are off, and API keys cannot pull a backup. Recreating `docker.img` loses everything. | **Verified** from the template and code. | Owner decision |
| OPS-02 | P2 | An admin address that differs from `ADMIN_ORIGIN` returns a bare 404, an unknown public host returns an empty 404, and nothing is logged. | **Verified**. | Fixed for the admin listener: the first mismatch is logged with the expected origin, the Unraid guide has a troubleshooting note, and non-API paths return a static English or German page that names no address. API paths keep their problem response. Unknown public hosts show the generic public page. |
| OPS-03 | P2 | The scheduled backup timer restarts on every container start, so frequent restarts can prevent any scheduled backup. | **Verified** in code. Not run with a real schedule. | Fixed |
| OPS-04 | P2 | There is no automatic snapshot before migrations, and a pre-upgrade backup cannot be restored by the newer image. The error does not say why. | **Verified**. Same root cause as DEV-02 for the first part. | Fixed (snapshot and message) |
| OPS-05 | P2 | The documented rollback command fails with a bare "Permission denied" when the snapshot was copied out with `docker cp`, and errors never name the path or UID 10001. | **Verified**. | Fixed |
| OPS-06 | P2 | The install guide lists settings that the published preview.2 image does not have, the template ReadMe link points to `main`, the upgrade text describes preview.1, and the candidate reports the same version as the published image. | **Verified**. | Partly fixed (docs). Version and template need owner approval. |
| OPS-07 | P2 | The template has no `TRUSTED_PROXIES` field, so behind Nginx Proxy Manager all clients share one rate-limit bucket. | **Verified**. | Owner decision |
| OPS-08 | P3 | An import with one invalid row fails with a generic `invalid_exchange` and no row number. | **Verified**. | Fixed (`row` and `field` problem extensions for JSON and CSV, shown in the UI) |
| OPS-09 | P3 | CLI and startup errors print Rust debug output such as `Error: Conflict`. | **Verified**. | Fixed |
| OPS-10 | P3 | The WebUI link and loopback short URLs assume the default ports. | **Verified**. | Documented |
| OPS-11 | P3 | Restore rollback copies accumulate unseen, and every browser backup has the same file name. | **Verified**. | Fixed (download names; read-only stored snapshot list in System, no stored-file download) |
| OPS-12 | P3 | Analytics loss is visible only as a bare counter, with no log line. | **Verified**. Mostly caused by DEV-01. | Fixed with DEV-03 |
| OPS-13 | P3 | The "Enabled" badge wraps mid-word at 1360 px. | **Verified**. | Fixed |
| OPS-14 | P4 | The notification acceptance check does not say that "Failed" takes about two hours. | **Verified**. | Fixed |
| OWN-3 | P2 | The runtime image is 154.6 MB uncompressed against a target below 80 MB. See SEC-07. | **Verified**. | Owner decision |
| OWN-8 | P3 | The unusual-traffic indicator counts all redirects, including detected bots and benchmarks, without saying so. | **Verified** after the benchmark. | Fixed |

## Security engineer

Security findings describe the class of problem and the affected path. None is in a published release except SEC-05, SEC-06, SEC-07, and SEC-10.

| ID | Priority | Finding | Evidence | Status |
|---|---|---|---|---|
| SEC-01 | P2 | The SBOM and the image scan see only Debian packages, not the Rust crates in the binary or the bundled frontend packages. | **Verified**: the SBOM has 91 Debian components and none of the 272 locked crates. | Fixed |
| SEC-02 | P2 | The release job runs the third-party scanner action with a `contents: write` token. | **Verified** in the workflow. Exploitability is a hypothesis. | Fixed |
| SEC-03 | P3 | The title worker does not recheck protection when it fetches, and monitor enrollment accepts protected links. | **Verified** live. | Fixed (title lookup). Monitor documented. |
| SEC-04 | P3 | Destination block rules for an IPv4 address miss its IPv4-mapped IPv6 spelling, and hosts with an empty label pass a block rule. Allow lists fail closed. | **Verified** in tests and browsers. Unreleased feature. | Fixed |
| SEC-05 | P3 | Protected-link password attempts share one per-address and one global budget across all links, the public 429 lacks `Retry-After`, and IPv6 clients are keyed by full address. | **Verified** live. | Fixed: `Retry-After`, IPv6 /64, and a per-link budget per client and across clients (`RATE_LIMIT_PUBLIC_PASSWORD_LINK`, default `5,30`). |
| SEC-06 | P3 | Restoring a snapshot restores the API keys and the administrator password that were valid when it was taken. | **Verified** in code. Documented behavior. | Documented, owner decision |
| SEC-07 | P3 | The Debian runtime base carries 39 unfixed HIGH or CRITICAL findings in packages the binary never loads (it links only libc, libm, and libgcc_s). | **Verified** with Trivy and `ldd`. | Evaluated on November 27, 2026: a `debian:trixie-slim` runtime base runs the image (readyz, CLI backup) but has 53 HIGH or CRITICAL findings with 1 fixable, against 39 with none fixable on bookworm. Keep bookworm; re-evaluate when trixie has fewer findings |
| SEC-08 | P4 | A host-bound write key can learn whether a link on another host has a destination that the policy refuses, through a 404 versus 422 difference. | **Verified** live. | Fixed |
| SEC-09 | P4 | HEAD on a visit-limited link returns its destination without counting a visit. This is documented but not stated in the editor. | **Verified**. | Documented |
| SEC-10 | P4 | The CI upgrade test pulls the previous image by mutable tag. | **Verified**. | Fixed |

Verified strengths: every `/api/v1` route behaved as documented without credentials and with host-bound, read-only, and analytics-only keys. Listener and Host separation, `TRUSTED_PROXIES` handling, CSRF and Origin checks, input and upload limits, transactional visit limits, one-time access, and restore validation held. Title lookup refused loopback, metadata, link-local, unique-local, shared, and private-by-DNS addresses. The notification connector applies only to webhook delivery and has no global override. Logs contain no secrets.

## Results after the improvements

The improvements in the [plan](IMPROVEMENT_PLAN.md) were implemented and verified on the same host on November 26, 2026. The final source has schema version 24.

| Check | Result |
|---|---|
| Rust | 172 tests pass (`cargo test --locked --all-targets`), Clippy with `-D warnings` reports nothing, Rustfmt is clean. |
| Frontend | `npm ci`, svelte-check with 0 errors and 0 warnings, `tests/url-cleaner.cjs` 5 of 5, Vite build. |
| Contracts | `tests/openapi.py`, `tests/schema.py`, `tests/analytics_schema.py`, and `tests/unraid_template.py` pass with host Python and SQLite 3.46.1. |
| Dependencies | `cargo deny check advisories licenses sources` passes without warnings. `npm audit --audit-level=high` reports 0 vulnerabilities. |
| Container, image as shipped (read-only root) | `--version` and diagnostics report the build revision. Listeners are separate, the user is 10001:10001, and `/data/tmp` is mode 0700. `tests/admin-browser.cjs` passes. A burst of 2,048 redirects at about 6,300 to 7,300 requests per second recorded 2,114 events (with warm-up) and dropped none, with no write failures. CLI backup, a refused live restore, and an offline restore work, and the redirect stays exact. |
| Upgrade and rollback | Published preview.2 (schema 11) with 5,000 extra links, 300,001 events, and 211,255 daily rows: the candidate wrote a verified pre-upgrade snapshot, migrated to schema 24, and was ready after 13 seconds. The preview account signs in and its link redirects exactly. The 30-day and 365-day summaries return 200 in 0.26 and 0.41 seconds, and text search returns 200 in 0.04 seconds. The pre-upgrade snapshot restores into a new volume with the preview.2 image, which then serves the seeded link. |
| Image | Trivy finds 0 fixable HIGH or CRITICAL findings and now also scans the Rust binary (0 findings). The image SBOM lists 191 Rust crates, for example `pkg:cargo/reqwest@0.13.5`. The frontend SBOM in the image lists 49 packages, including `svelte@5.57.3`. The image is 155.4 MB. The 39 unfixed Debian base findings remain (SEC-07). |
| Interface | Playwright acceptance checks for UX-01, UX-02 (four flows), UX-03, UX-05, UX-06, UX-07, UX-08, and UX-12 pass. 77 new screenshots at seven sizes in Dark and Light, English and German, show no horizontal overflow. axe-core reports no WCAG A or AA violations on 42 scanned screens. |
| Not rerun | The full four-perspective review was not repeated after the changes. CI has not run on this working tree. |

## Not covered

- The owner's Unraid host, Nginx Proxy Manager, DNS, TLS certificates, IPv6 routing, and real client networks. See [Release verification](RELEASE_VERIFICATION.md) for the exact checks.
- Real screen readers, Mobile Safari, Firefox, touch devices, zoom levels, forced colors, and text spacing.
- Outbound behavior against real public destinations, a real webhook receiver, and an internal receiver with a private CA.
- Long-running behavior: scheduled backups over days, 15-minute token expiry in real time, retention over months, and LAN load with representative data.
- Parser fuzzing, DNS rebinding with controlled DNS, and verification of pinned action SHAs against upstream.
