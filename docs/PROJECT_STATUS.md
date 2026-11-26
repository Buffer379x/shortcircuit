# Project status

**November 26, 2026 · 0.7.0-preview.1, schema version 24.** Architecture and design are approved for implementation. Existing links are recreated manually. The handoff's Stage 1 items are implemented in source and verified locally. Target-host acceptance and release approval are pending. [Quality review](QUALITY_REVIEW.md) · [Improvement plan](IMPROVEMENT_PLAN.md) · [Release verification](RELEASE_VERIFICATION.md)

| Area | Status |
|---|---|
| Public foundation | Host-bound redirects, a versioned SQLite database with WAL and full synchronization, and separate public and admin listeners. |
| Link management | Create, edit, enable, disable, tag, and delete links. Add descriptions and create disabled duplicates for review. Favorites and an archive view organize the catalog. Search, combine tag filters with all or any matching, and sort with cursor pagination. Set the generated shortcode length at startup. Apply status and tag changes, export native JSON or CSV, and confirm deletion for selections of up to 50 links. |
| Link tools | The link forms can find exact destination matches or preview the removal of known tracking parameters. |
| Redirect controls | Managed domains and fallbacks, time windows, atomic visit limits, ordered rules, a draft rule simulator, crawler preferences, and opt-in query and path forwarding. |
| Protected links | Optional passwords and confirmed one-time access. The public form verifies a short-lived signature and the configured origin before redirecting. SQLite enforces consumption independently of analytics. |
| Link history | Record individual link edits with actor and timestamp. Compare and restore metadata or redirect policy through the admin dialog, download JSON, or clear a link's history. |
| QR Studio | Preview and export local PNG and SVG with colors, transparency, error correction, size, quiet zone, an optional outline, and a verified PNG logo for the current dialog. Copy PNG when the browser supports image clipboard access. Save reusable style templates in SQLite. |
| Tags | The admin catalog shows tag counts and supports colors, descriptions, rename, merge, and delete. The link form suggests existing tags. Tags with saved details remain available without assignments. |
| Analytics | UTC trends, custom dates, previous-period comparison, JSON report export, top links, domains and tags, and browser, OS, device and traffic-source breakdowns. Domain root requests are reported separately. A read-only `analytics` API key scope honors host restrictions. An informational indicator flags unusually high traffic. Local GeoIP remains optional. |
| Backup and restore | The CLI creates verified SQLite snapshots and can restore them offline. The browser downloads a verified snapshot and can validate and stage a restore for the next controlled restart. Optional scheduled backups have independent retention. |
| Monitoring and operations | Browser-only monitoring of explicitly enrolled destinations and configured public domains, opt-in title lookup, opt-in webhook notifications with a durable outbox and an optional pinned internal connector, an administrative audit log, and system diagnostics with storage, CPU, and container memory. |
| Data exchange | Bounded native JSON and CSV exchange, configurable CSV column mapping, preview, conflict detection, and atomic apply. |
| Authentication | A CLI-issued account claim, Argon2id passwords, revocable sessions, account recovery, CSRF protection, scoped API keys and transactional rotation. Configurable trusted proxies and rate budgets are available. |
| Destination policy | Opt-in allow and block rules for destination hosts, enforced on every destination write and on each redirect before visits or one-time access are consumed. |
| Database verification | The schema tests cover compatibility, the redirect index, and exact stored destinations. The authentication and lifecycle migrations have additional transactional regression tests. |
| Build and container verification | GitHub Actions checks the Rust integration tests, Clippy, Rustfmt, frontend types and assets, and a non-root container with a Chromium smoke test. [CI results](https://github.com/Buffer379x/shortcircuit/actions) |
| GitHub | The owner approved public source development and container previews. The repository uses MIT. [Repository](https://github.com/Buffer379x/shortcircuit) · [Preview](https://github.com/Buffer379x/shortcircuit/releases/tag/v0.6.0-preview.2) |
| UI and branding | The embedded Svelte admin interface includes account setup, authentication, links, and API keys with English and German UI text and dark, light, or system appearance. The separate design preview covers additional planned screens. |
| Browser verification | CI exercises the integrated administration workflow in Chromium. The [capture instructions](../README.md) cover the separate design preview. |
| Unraid | The project owner confirmed that preview.1 starts through Unraid and displays the Docker icon and browser favicon. The amd64 template supports a Docker-managed data volume and private administration through a reverse proxy. [Installation](UNRAID_INSTALLATION.md) |
| Performance and release | The benchmark method and resource estimates are documented. Production measurements and operating approval are pending. |

## Stage 1 implementation status

Evidence refers to the local verification on November 26, 2026 against the working tree described in the [quality review](QUALITY_REVIEW.md#reviewed-state). The workspace has no Git metadata, so there are no commits. CI has not run on these changes. Stage 3 later added schema 24 and the fixes listed in the [improvement plan](IMPROVEMENT_PLAN.md).

| Item | Status | Evidence |
|---|---|---|
| 1. Analytics read permission | Implemented | Migration 19 widens the scope mask and keeps key IDs monotonic. `report_access` admits browser sessions or keys with `analytics`. Host-bound keys are forced to their host in totals, previous period, daily series, rankings, breakdowns, export, and the traffic report. `dropped` is null for them. Tests: `auth::upgrade_keeps_existing_key_grants_and_never_reuses_a_key_id`, `auth::rotation_preserves_the_analytics_scope`, `tests/analytics_access.rs` (link-read key refused, expired and revoked keys refused, other host refused, settings, details, and deletion remain browser-only). |
| 2. Domain-root reporting | Implemented | Migration 20 adds `root_fallback` and `root_missing`. GET on `/` is counted by its actual response under the same settings, Do Not Track, and retention. Status and `Location` are unchanged. Tests: `tests/analytics.rs::domain_root_requests_are_counted_separately_without_changing_the_response`, `analytics::upgrade_keeps_existing_outcomes_and_reports_no_root_requests`, `tests/analytics_schema.py::test_root_categories_upgrade_keeps_existing_outcomes`. |
| 3. Destination policy and traffic visibility | Implemented | Migration 21, `src/destinations.rs`, write checks in every destination write path, redirect-time enforcement before challenges and reservations, impact preview, audit entry, and `/api/v1/analytics/traffic`. Tests: `destinations::*`, `redirect::a_refused_protected_destination_keeps_its_one_time_access_and_allowance`, `tests/destination_policy.rs`, `metrics::traffic_flags_only_links_far_above_their_recent_average`. |
| 4. Disk and CPU diagnostics | Implemented | `src/resources.rs` reads `statvfs` for the data directory, `/proc/self/stat`, and cgroup v2 files on demand over a 250 ms window. States are healthy, low, or unavailable. Tests: `resources::*` fixtures and `operations::browser_backup_round_trips_through_checked_staging_and_discard`. |
| 5. Opt-in title retrieval | Implemented | Migration 22 and `src/titles.rs`. Fetching uses the public outbound policy with redirect revalidation, a 128 KiB and eight-second bound, and plain-text extraction. Results apply only to an unchanged, untitled link. Tests: `titles::*`, `outbound::titles_are_bounded_plain_text_from_the_first_title_element`. A fetch from a real public destination was not exercised. |
| 6. Notifications and internal connector | Implemented | Migration 23 and `src/notifications.rs` with deterministic event keys, eight capped attempts, stale expiry, restore discard, and status in Operations. The connector pins one HTTPS origin and CIDR ranges and is used only for webhook delivery. Tests: `notifications::*`, `outbound::connectors_pin_one_https_origin_to_private_ranges`, `outbound::connector_delivery_refuses_addresses_outside_the_pinned_ranges`. A delivery to a real internal TLS receiver was not exercised. |
| Release preparation | Prepared | [Release verification](RELEASE_VERIFICATION.md) defines the target-host procedure, acceptance checks, and rollback. CI now reports unfixed findings and keeps an SBOM. The release workflow attaches an SBOM and the image digest. Base images are pinned by digest. |

## Review and improvements

The [quality review](QUALITY_REVIEW.md) found one P0 issue: the read-only container gave SQLite no temporary directory, so analytics batches, larger reports, search, restore, and upgrades with real data failed. That issue dates back to at least preview.2. The review also found four P1 issues: no pre-upgrade snapshot, out-of-view dialog errors, all Unraid data in one Docker volume, and the migration pool timeout. All P0 and P1 code issues are fixed and verified. The [improvement plan](IMPROVEMENT_PLAN.md) lists every task with its result, and the review records the final verification.

## Release readiness

**Not ready for a stable release.** On November 26, 2026 the owner approved publishing the improvements as `0.7.0-preview.1`, a development preview. Template decision: keep the Docker volume, which works with the template alone, and enable daily backups and the Trusted proxies field. The source passes every local check, including the container lifecycle with the image as shipped and an upgrade from the published preview with 300,000 analytics events. What remains open:

- **CI has not run on this working tree**, and the workspace has no Git metadata, so no commits exist. Commit the changes in focused pieces and let Core checks run before any release step.
- **Owner decisions:**
  - Moving `/data` out of `docker.img` by default needs an ownership step that the template alone cannot perform. Backups still share the volume until they are copied elsewhere (OPS-01).
  - The runtime base image: distroless or a newer Debian, given that the Unraid console needs a shell (SEC-07, OWN-3).
  - Whether a restore should revoke credentials created after the snapshot (SEC-06).
- **Target-host acceptance** as listed in [release verification](RELEASE_VERIFICATION.md#target-host-verification):
  - pre-upgrade snapshot and upgrade on the Unraid host
  - exact published redirects over IPv4 and IPv6
  - proxy trust with Nginx Proxy Manager
  - backup and restore on the target storage
  - LAN benchmark
  - Mobile Safari
  - a screen reader with keyboard-only use
  - notifications against a real receiver, if they are used
- **Deferred items** with their reasons in the plan: UX-10, UX-13, OPS-08, DEV-10, DEV-11, a per-link password budget (SEC-05), and listing rollback copies (OPS-11).

## Open work

- **Reporting.** The Overview and Analytics pages provide operational counts, trends, and UTC report ranges. Date and time labels outside UTC reports support a browser-specific display timezone. GeoIP can be evaluated with an operator-supplied dataset and an approved distribution policy.
- **Monitoring and automation.** Opt-in probes, title lookup, notification events, and one pinned internal connector are implemented as development code. Their delivery against real receivers and destinations is unverified.
- **Administration.** Diagnostics now include storage, CPU, and container memory, and the destination policy is available. Release security acceptance remains open.
- **Production verification.** Exercise backup, upgrades and rollback on the intended Unraid storage and reverse proxy. Measure memory, image size, startup, and latency under representative load. Complete responsive and accessibility checks, review advisory and dependency scan results, and obtain owner acceptance.

Collections, Link Pages, Quick Share and Apple Shortcuts, PWA installation, additional notification connectors, TOTP, OIDC, user roles, encrypted backups, and arm64 are optional roadmap items. [Feature matrix](FEATURE_MATRIX.md) · [Roadmap](ROADMAP.md)
