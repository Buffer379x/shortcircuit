# Changelog

## Unreleased

## 0.8.0-preview.1

- Admin interface redesigned with Polar Signal:
  - Near-monochrome dark and light themes with pink as the signal color, local Geist fonts, and Lucide icons.
  - A grouped sidebar with a command palette (Ctrl/Cmd+K), and a link details page with a route map, rules, access, health, and history.
  - One link form for creating and editing, charts in Analytics and Overview, QR Studio, and Settings with privacy and analytics controls.
  - The new logo and brand package (favicon, app and touch icons, web manifest, Unraid icon, README banner).
  - Old hash addresses redirect to the new pages. No function was removed. [Design system](docs/DESIGN_SYSTEM.md)
- Public missing, inactive, and unknown-domain responses and the password and one-time pages show small branded pages in English or German. Inactive links answer exactly like missing ones. Status codes and headers are unchanged.
- The admin listener serves its script and stylesheet with Brotli or gzip when the browser accepts it, and serves fonts, icons, and a web manifest. The admin Content Security Policy adds `font-src 'self'` and `manifest-src 'self'`.
- API v1, additive: catalog search items and the new `GET /api/v1/links/{id}` report the live `state`, policy window and visit limit, `protection`, `rule_count`, 30-day redirects, a 14-day trend, and the last health check. Keys without the `analytics` scope receive no traffic fields. The analytics summary and download accept `link=<id>`, and the Overview lists up to eight links that need attention.

## 0.7.0-preview.1

- The Unraid template enables daily scheduled backups, adds a Trusted proxies field, and links the installation guide for this release.
- Fix analytics writes, reports, search, restore, and upgrades with real data in the read-only container by giving SQLite a temporary directory on the data volume (`SQLITE_TMPDIR=/data/tmp`).
- Write a verified pre-upgrade snapshot before migrations and run migrations outside the request pool's two-second deadline.
- Continue scheduled backups from the newest scheduled snapshot after a restart.
- Log analytics losses at most once a minute, readable CLI and startup errors with paths, and the first request that uses an address other than `ADMIN_ORIGIN`.
- Notifications: detect every candidate, avoid repeats after trimming, use epoch-prefixed IDs after a restore, and reset outbound work in every restore path.
- Destination policy: compare IPv4-mapped IPv6 literals as IPv4 and refuse hosts with empty labels. Title lookup skips links that gained protection.
- Public `429` responses include `Retry-After`, and IPv6 clients share rate budgets per /64.
- Admin interface: hash routes, a menu on small screens, scrollable dialogs with field errors and initial focus, focus after deletions, stronger field borders, a scrollable rail, and readable report tables.
- Build the binary with `cargo auditable`, ship a frontend SBOM, generate release SBOMs without write permissions, and pin the CI upgrade image by digest.
- Name browser backup downloads with the schema version and time.

- Add opt-in webhook notifications for link health, upcoming and completed expiry, click milestones, failed backups and jobs, low storage, and security events with a durable, deduplicated outbox, bounded retries, and delivery status in Operations. `MONITOR_WEBHOOK_URL` becomes an alias. Link health deliveries now use the `link_health` event format.
- Allow one explicitly configured internal HTTPS connector with pinned address ranges and an optional private CA for notification delivery only.
- Add opt-in page title lookup for untitled links with `TITLE_LOOKUP_ENABLED`. Lookups are bounded, revalidate redirects, skip protected links, and never replace a written title.
- Report free space on the data volume, process and container CPU use, and container memory in the system diagnostics, with explicit unavailable and low-space states.
- Add an opt-in destination host policy with allow and block rules, subdomain and IDN handling, an impact preview, and enforcement on writes and redirects. Refused redirects keep visit allowances and one-time access.
- Show links with unusually high traffic today compared with their seven-day average on the Overview and Analytics pages. The indicator never changes a link.
- Report GET requests for a domain root path separately as redirected by the root destination or without a destination. Existing totals keep their meaning.
- Add the `analytics` API key scope for read-only analytics reports and exports. Link scopes do not grant analytics access. A host-bound key reports only on its own host.

- Add an Overview with link lifecycle counts, recent links, traffic, and monitor status.
- Add browser field mapping for generic CSV files while retaining the native format.
- Expand analytics with custom UTC dates, host filters, client and traffic breakdowns, domain and tag rankings, and a JSON report download.
- Download verified SQLite snapshots and prepare offline restore from the administration interface.
- Add API key rotation, an administrative audit trail, configurable rate limits, and an explicit trusted-proxy policy.
- Add opt-in checks for stored targets and configured public domains, with bounded retries and optional HTTPS health notifications.

- Add domain crawler preferences and per-link indexing hints.
- Simulate the current redirect policy draft using sample request details.

- Add optional tag colors and descriptions, preserve details during tag merges, and suggest existing tags in the link form.

- Add link descriptions to the catalog, revision history, and native JSON and CSV exchange.
- Create disabled duplicates from the catalog with editable metadata and a fresh shortcode.
- Show recorded redirects and the last recorded use in the catalog.

- Add favorites and an archive view to the link catalog. Archiving preserves the public redirect and its access settings.
- Filter the link catalog by up to 20 tags, matching all or any of them.
- Set the length of automatically generated shortcodes with `SHORTCODE_LENGTH`.

## 0.6.0-preview.2

- Apply status or tag changes to selected links, export the selection as JSON or CSV, or confirm atomic deletion.
- Search, filter, and sort all links in the catalog with stable cursor pagination and keep search terms in the request body.
- Add optional PNG logos to the QR dialog with verified output in PNG and SVG.
- Add an optional QR outline outside the quiet zone. The preview, PNG, SVG, and saved templates support the same setting.
- Save and reuse named QR design templates across links. Include templates in SQLite backups.
- Add QR Studio color, transparency, error correction, and quiet-zone controls with contrast validation and optional PNG clipboard copy.
- Add link history with comparison, category-specific restoration, JSON export, and per-link deletion.
- Add password-protected links and confirmed, atomically consumed one-time links. Native link exports keep protected entries disabled when imported.

## 0.6.0-preview.1

- Add an Unraid Docker Manager template that pulls the versioned preview image from GitHub Container Registry and initializes a persistent Docker volume.

- Document the implemented management endpoints in OpenAPI 3.1 and serve the specification on the admin listener.

- Add native CSV exchange with bounded pages, spreadsheet-oriented escaping, and transactional import.

- Add optional scheduled backups with independent retention.

- Add exact destination lookup and a local preview for removing tracking parameters.

- Add verified local backups and offline restore with a rollback database.

- Add UTC analytics reports, coarse visitor classification, privacy preferences, and data retention and deletion.

- Add managed domains and fallback URLs, the tag catalog, redirect rules and visit limits, and local QR exports.

- Add one-time CLI setup and recovery, Argon2id passwords, revocable sessions, and scoped API keys.
- Add the embedded Svelte admin interface with English and German copy, theme settings, and local assets.
- Add link editing, enable and disable, deletion, tags, and JSON export of displayed results.
- Preserve existing links during database schema upgrades.

- Add the Rust foundation with Axum, SQLite, separate public and admin listeners, and an optional token-protected link API.
- Add a non-root Dockerfile, a local Compose profile, and a CI workflow.
- Record the proposed architecture, data model, threat model, and benchmark plan.
- Add the local design preview and initial branding assets.
