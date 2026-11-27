# Changelog

## Unreleased

## 0.9.0-preview.1

- After navigation, focus moves to the new page's heading only if it has not moved since: a quick click or keystroke on the new page keeps its focus.
- Schema version 29. The first start writes a verified pre-upgrade snapshot and migrates schema 24 to 29.
- Preview images: links can show the image that their destination page offers for sharing, as messengers do: `og:image`, then `twitter:image`, otherwise the page icon or `/favicon.ico`, with a placeholder when there is none. No screenshots and no headless browser. Off by default: turn on **Fetch preview images** in Settings › Operations, or set `PREVIEW_IMAGES_ENABLED=true`; it also needs title lookup, because the image URL comes from the same page request. Each image request goes through the public outbound policy and the destination policy, is limited to 2 MiB and eight seconds, and is decoded only as PNG, JPEG, WebP, or GIF (first frame) by pure-Rust decoders with dimension and allocation limits; SVG is refused. Only a re-encoded JPEG thumbnail of at most 48 KiB (480 × 270, icons 64 × 64) and the image URL's origin are stored, never the original file, its metadata, or the full URL. Protected links are never fetched, a changed destination clears the preview and queues a new one, and verified snapshots drop the switch. The links list shows a small thumbnail in each row and card, and the link detail page a preview card with source, fetch time, and **Refresh preview**. API v1, additive: `GET /api/v1/links/{id}/preview` (browser sessions and `read` keys within their host) serves the thumbnail with `nosniff`, `Cache-Control: private`, and the admin CSP; browser-only `POST /api/v1/links/{id}/preview/refresh`; a `preview` field on catalog and detail items; and `preview_images_enabled` and `preview_images_active` in the operations settings. Schema version 29 adds `link_previews` and the `runtime_settings.preview_images_enabled` column. The `image` crate (0.25.10, four decoders only) is a new dependency. [API](docs/API.md#preview-images) · [Security](docs/SECURITY.md#implemented-preview-image-policy) · [Privacy](docs/PRIVACY.md)
- Settings > Public pages: preview and edit the pages visitors see for a missing or inactive link, an unknown or disabled domain, a password-protected link, and a one-time link, separately in English and German. Fields mode (default) changes the title, message, button label, accent color, and logo inside the built-in layout. HTML mode (expert) serves your own HTML and CSS after an allowlist sanitizer removes scripts, event handlers, unsafe URLs, external resources, frames, plugins, forms, and `meta` refreshes; password and one-time pages require `{{form}}`, where the server inserts its own form. Customized pages use a strict hash-based CSP without scripts. Drafts preview in a sandboxed frame rendered by the server. Export and import a template file, import a single `.html` file into one page, or start from the "Polar Signal" or "Minimal" templates, each with a preview before anything changes. Reset to the built-in page at any time. Missing and inactive links still get byte-identical responses, and an invalid stored page falls back to the built-in one. API v1, additive and browser-session only: `/api/v1/settings/public-pages` with `reset`, `preview`, `import/validate`, `import`, `export`, and `presets`. The admin CSP adds `frame-src 'self'` for the preview. Schema version 27 adds `public_pages` and the audit actions `public_pages_update`, `public_pages_reset`, and `public_pages_import`. [API](docs/API.md#public-pages) · [Security](docs/SECURITY.md#public-pages)

- The browser tab shows only "ShortCircuit" instead of the current page name. The page heading still names the view and receives focus after navigation.
- Domains: delete a domain from its row menu. When links still use it, choose to move them to another active domain, with slug conflicts listed before anything changes, or to delete them after typing the domain name, with a pointer to Import and export and backups. Moved links keep their IDs, history, protection, rules, and health checks; recorded analytics keep the old host. A host from `PUBLIC_HOSTS` loses its saved settings and links but stays served with defaults until it is removed from the environment. Deletion of a host outside `PUBLIC_HOSTS` is refused while API keys are restricted to it, and every deletion is recorded in the audit log with host, action, target, and count. API v1, additive: `GET /api/v1/catalog/domains/deletion`, `DELETE /api/v1/catalog/domains`, and `details` on audit entries. Schema version 28 rebuilds `admin_audit` for this. [API](docs/API.md#deleting-a-domain)
- Settings > Operations: turn health checks and title lookup on or off, set the health check intervals, the scheduled backup interval and retention, and the notification webhook, Authorization header, events, click milestones, and expiry notice in the web interface. Changes apply without a restart. A value saved there takes precedence over the container variable, and **Reset to container value** deletes it again. Settings > Operations and System show each value's source: saved here, container variable, or default. Nothing is saved on upgrade, so existing installations keep their container values. The webhook URL and the header are write-only, changing either one needs the current password, a changed webhook discards pending deliveries, and every change is audited by setting name. Health, Overview, the link Health tab, and System point to Settings › Operations instead of naming `MONITOR_ENABLED` and the other variables. Connector variables, hosts, proxies, rate limits, listeners, and the log level stay environment-only. API v1, additive and browser-session only: `GET` and `PATCH /api/v1/settings/operations`, `POST /api/v1/settings/operations/authorize`, and `operations_settings` in `GET /api/v1/system`. Schema version 26 adds `runtime_settings` and the audit actions `operations_settings_update` and `operations_settings_authorize`. [API](docs/API.md#operations-settings) · [Security](docs/SECURITY.md#notifications-and-internal-connectors)
- Scheduled backups run daily and keep 7 copies by default, the values the Unraid template used to set. Before, they were off unless `BACKUP_INTERVAL_HOURS` was set. The Unraid template no longer sets `BACKUP_INTERVAL_HOURS` and `BACKUP_KEEP`; both are still accepted as start values, and an existing container keeps the values it has. `compose.yaml` still passes `BACKUP_INTERVAL_HOURS=0` unless you set it.
- Verified snapshots no longer contain a saved webhook URL or Authorization header, or the saved health check and title lookup switches. After a restore they follow the restoring installation's container values until saved again. [Backup and restore](docs/BACKUP_RESTORE.md)
- Notification variables other than the webhook, such as `NOTIFICATION_EVENTS`, no longer stop the start when no webhook variable is set, because the webhook can now come from the interface.
- Restore accepts backups from older releases back to 0.6.0-preview.1 (schema 6): browser downloads, command-line and scheduled backups, and pre-upgrade snapshots. Their migration history must match this release's migrations exactly up to their schema. The restore upgrades a private staged copy with the normal migrations and activates it only after it matches the current schema, so a failed upgrade leaves the current database and its rollback state untouched. Newer, older unsupported, and mismatched backups are refused with their own problem codes, and the browser upload tests the upgrade before staging. The restore dialog says which schema a backup is from and that it is upgraded on restart. API v1, additive: the restore upload response adds `target_schema_version` and `upgrade_required`. [Backup and restore](docs/BACKUP_RESTORE.md#restoring-older-backups)
- 0.9.0 changes the schema, so going back to 0.8 by changing the image is not possible. Roll back by restoring the pre-upgrade snapshot with the 0.8 image into a separate volume. [Rollback](docs/RELEASE_VERIFICATION.md#rollback)
- Sorting links by 30-day visits reads each link's total from the new covering index `analytics_daily_by_link_kind` (migration 25). With 20,000 links and 600,000 daily rows the full catalog page takes about 60–85 ms instead of 75–110 ms, and catalog pages in other sorts load their visit columns about a third faster. The index adds 3–8 percent to the database size.

## 0.8.0-preview.3

- Dialogs no longer take focus back to their first field after it has moved. Typing right after a dialog opens, for example into a tag's description, could land in the name field and rename the tag.
- The image applies Debian security updates at build time on a refreshed bookworm-slim base, which fixes the newly published `libpam` finding (CVE-2025-6020).
- API v1, additive: monitoring list items include the link `title` and `target`. Health no longer requests each link separately, which kept it within the admin request budget.
- Links: filter the list by live status (active, scheduled, expired, disabled, limit reached) and sort it by 30-day visits or last visit from the column headers. API v1, additive: `POST /api/v1/links/search` accepts `state` and the sorts `visits_desc`, `visits_asc`, `last_visit_desc`, and `last_visit_asc`; links without a value come last. [API](docs/API.md#catalog-search)
- Protected-link passwords also have a budget per link: 5 attempts per client and 30 across all clients per minute by default (`RATE_LIMIT_PUBLIC_PASSWORD_LINK`). Guessing one link's password no longer exhausts the attempts for every other link. [Security](docs/SECURITY.md#client-addresses-and-request-limits)
- The admin listener answers another address with a static English or German page that says to open the configured administration address without naming it. The status stays `404`, and API paths, HEAD, and the probes are unchanged.
- `GET /api/v1/system` no longer waits 250 ms for a CPU sample. A background sampler reads the CPU counters every 15 seconds from startup until shutdown, and `cpu` adds `sampled_at` and `age_seconds` (API v1, additive). Before the first sample, and after 60 seconds without one, CPU use is reported as unavailable. `raw_event_count` is counted at most once every 30 seconds.
- Analytics retention runs in its own task beside the event writer and deletes at most 512 rows per short transaction with a pause between chunks, so a long cleanup no longer holds up event ingestion. Retention periods and the hourly schedule are unchanged.
- System, Backups: a read-only **Stored snapshots** table lists command-line, scheduled, and pre-upgrade snapshots and restore rollback copies with type, time, schema, and size, from the new browser-only `GET /api/v1/system/backups`. It returns file names only and lists up to 100 files. Stored files are not downloadable; Download backup still creates a new verified snapshot.
- Sorting the link list by 30-day visits reads the daily totals in one pass instead of once per link: with 20,000 links and 600,000 daily rows the query takes about 70–100 ms instead of 175–215 ms. The schema stays at version 24, so existing backups stay restorable.
- Imports name the first failing row: a rejected JSON or CSV import adds `row` and, when known, `field` to the `invalid_exchange` or `destination_not_allowed` problem (API v1, additive), and Import and export shows them next to the error.

## 0.8.0-preview.2

- Health: on phones and narrow containers each monitored link is a card with its result, last check, response time, and priority switch.

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
