# ShortCircuit feature matrix

Coverage reflects the current development branch. **Current** means the implementation is present in source. **Planned** means work is scheduled in the roadmap. **Optional** identifies additions subject to demonstrated need. [Project status](PROJECT_STATUS.md) · [Roadmap](ROADMAP.md)

Priority **P0** covers production readiness and safe operation. **P1** covers the core 1.0 feature set. **P2** covers follow-up work and enhancements. **P3** and **P4** cover longer-term options.

## Redirects and link management

| Capability | Status | Priority | Scope |
|---|---|---|---|
| Create and list links | Current | P0 | The authenticated admin API accepts absolute HTTP or HTTPS destinations, preserves their stored URI spelling, and lists links with a cursor. |
| Custom and generated slugs | Current | P0 | Slugs are case-sensitive and can contain multiple segments. Custom segments use ASCII letters, digits, hyphens, and underscores. Generated slugs use 10 random characters by default. The database enforces uniqueness per host. |
| Host-bound redirects | Current | P0 | Configured hosts can reuse the same slug independently. Public GET and HEAD use `302` by default and include `Cache-Control: no-store`. Matching uses the normalized host and exact path with optional extra path forwarding. |
| Verify published addresses | Planned | P0 | Recreate required short URLs and check the configured host, exact slug, status, and destination through the intended proxy. Scan printed QR codes or compare their decoded URLs when applicable. |
| Edit links and destinations | Current | P0 | Update destinations, titles, descriptions, tags, and enabled status. Published hosts and slugs stay fixed. |
| Activation windows and visit limits | Current | P0 | Evaluate time windows in UTC. Reserve limited visits atomically using a dedicated ledger with separate analytics counting. |
| Redirect status and forwarding options | Current | P0 | Add per-link status and explicit query forwarding and extra-path policies. Keep temporary redirects and `no-store` for protected and limited links. Verify encoded paths, query keys, and fragments. |
| Domain management and fallbacks | Current | P0/P1 | Manage allowed public hosts from the admin UI and configure validated fallback targets per domain. Test route collisions, errors, and redirect loops. |
| Ordered redirect rules | Current | P1 | The first matching rule wins. The administration interface edits and reorders rules. Conditions use language, device, operating system, browser, and query values. |
| Path matching extensions and aliases | Optional | P2 | Evaluate Unicode slugs, case-insensitive or trailing-slash matching, and explicit aliases against collision, encoding, and proxy requirements. |
| Search, filters, and sorting | Current | P1 | Search the full catalog by title, slug, domain, destination, or tag. Combine domain and status filters with all-tag or any-tag matching and sort paginated results. |
| Bulk actions | Current | P1 | Select up to 50 links, change their status or tags atomically, export native JSON or CSV, and confirm permanent deletion. |
| Slug generation settings | Current | P1 | Set the generated length to 8 through 32 with `SHORTCODE_LENGTH`. Existing and custom slugs preserve their spelling. |
| Tags | Current | P1 | View tag counts, edit colors and descriptions, rename or merge tags, and remove their assignments. The link form suggests existing tags as the operator types. |
| Destination policy | Current | P1 | Opt-in allow and block rules for destination hosts with explicit subdomain matching and IDNA normalization. Block rules take precedence. Enforced on every destination write and on each redirect without changing saved destinations or consuming visit limits. |
| Crawler controls | Current | P1 | Set the domain-wide crawling preference in `robots.txt`. Individual links can request search exclusion through an HTTP header. |
| Duplicate detection | Current | P2 | Lookup matches against the exact stored destination. The creation and edit forms offer copying an existing short URL. Approximate matches and merge are planned. |
| Duplicate a link | Current | P2 | Open a prefilled form for the destination, title, description, tags, and analytics preference. Save the copy disabled and review its redirect and access settings before enabling it. |
| Protected links | Current | P2 | Optional password protection, explicit confirmation, and atomic consumption of one-time links. |
| Link history | Current | P2 | Compare and restore earlier metadata and redirect policies. History lists verified actors and protection changes and supports JSON export and deletion. |
| Rule simulation | Current | P2 | Preview the status and destination selected by a draft policy using sample language, user agent, query, and path information. |
| URL Cleaner | Current | P2 | Review known trackers and one-time exceptions locally. Use the proposed URL explicitly before saving. Leave recognized signed URLs unchanged. |
| Favorites and archives | Current | P2 | Mark links as favorites or archive them from the default view. Archived links continue to use their current redirect and access settings. |
| Collections | Optional | P2 | Evaluate a lightweight collections model as an additional organization option. |
| Link Pages | Optional | P3 | Evaluate a bounded content model with templates, branding, and scheduled availability. |

## Analytics and privacy

| Capability | Status | Priority | Scope |
|---|---|---|---|
| Visit totals and time series | Current | P1 | Report totals by UTC date and rank links, domains, and current tag assignments using bounded ingestion and indexed aggregates. |
| Catalog usage | Current | P1 | Show successful redirects kept in analytics and the last recorded use, with a UTC date when the exact time has aged out. |
| Visit details and unmatched requests | Current | P1 | Show retained details for redirected, blocked, and unmatched short-link requests. Report domain root requests separately as redirected by the root destination or without a destination. |
| Referrers, devices, and bots | Current | P1 | Referrer capture can store hostnames. Reports group traffic by estimated visit class, browser, OS, device and traffic source, including detected previews. |
| Privacy settings, retention, and deletion | Current | P1 | Honor global and per-link opt-outs and Do Not Track, with configurable retention and a clear-all action. Analytics collection operates independently of visit limits. |
| Unusual traffic indicator | Current | P1 | Flag up to ten links whose redirects today (UTC) reach at least 100 and three times their seven-day daily average. Informational only, bounded to daily rollups, and limited by key host restrictions. |
| Period comparisons and report controls | Current | P1 | Compare a preset or inclusive custom UTC period with its predecessor, filter by host, and download a versioned report. |
| Geolocation | Optional | P1 | Use an operator-provided local GeoIP dataset. Apply the configured granularity and handle missing data explicitly in both reports and rules. |
| Multiple segment comparisons | Optional | P2 | Compare selected links, tags, or domains when reporting needs justify the extra views. Use consistent time boundaries. |
| Advanced visualizations and estimates | Optional | P4 | Evaluate local maps, heatmaps, milestones, and unique visitor estimates against accessibility and privacy requirements. |

## Administration and operations

| Capability | Status | Priority | Scope |
|---|---|---|---|
| Separate listeners and local storage | Current | P0 | The Rust source defines independent public and admin routers, loopback bind defaults, and a versioned local SQLite schema with WAL and full synchronization. |
| Admin API authorization and errors | Current | P0 | Browser sessions and scoped bearer keys authorize the link API. Errors use structured problem responses. The public listener serves redirects independently. |
| Resource diagnostics | Current | P1 | Report free space on the data filesystem with a low-space state, process and container CPU from a background sample every 15 seconds with its window and age, process RSS, and container memory separately. Unavailable readings are labeled. Browser session only. |
| Liveness and readiness | Current | P0 | The admin listener serves minimal status-only probes for process and database readiness. |
| Interactive setup and authentication | Current | P0 | A short-lived CLI setup token, revocable sessions, CSRF protection, and documented recovery are available. Keep the administration interface behind LAN or VPN access. |
| Scoped API credentials | Current | P0 | Issue keys with `read`, `write`, `delete`, or the read-only `analytics` report scope, optional domain restrictions and expiry, and revocation. A domain restriction also limits analytics reports. Password-confirmed rotation preserves the key metadata and discloses the new secret once. |
| OpenAPI and API diagnostics | Current | P1 | The implemented API has an OpenAPI 3.1 document served through the admin interface. A browser-accessible system endpoint reports database and restore state. |
| Audit, diagnostics, and abuse controls | Current | P0 | The administrative audit log records sensitive account, credential, and recovery actions. Separate configurable budgets limit login, admin API, and protected-link requests. The default client address is the transport peer. |
| Backup and restore | Current | P0 | The CLI creates and restores verified SQLite snapshots. An opt-in scheduler retains verified copies and continues its schedule across restarts. Startup writes a verified pre-upgrade snapshot before migrations. The browser can download a snapshot and stage a restore for the next controlled restart. |
| Native data exchange | Current | P1 | Import and export bounded versioned JSON and CSV pages, map generic CSV columns in the browser, preview conflicts, and apply transactionally. |
| Maintenance CLI and installation | Current | P1 | CLI commands handle setup, recovery, backups, restore, and readiness. The Unraid preview template uses the versioned amd64 non-root image. Production validation on the intended storage and proxy remains open. |
| Cross-origin admin API access | Optional | P2 | Consider explicit origins for integrations that need browser access. |
| Additional authentication methods and roles | Optional | P3 | Consider TOTP, recovery codes, OIDC, and roles as operational requirements emerge. |
| Deployment extensions | Optional | P4 | Assess PostgreSQL, arm64 builds, and password-encrypted backups against measured needs. |

## Admin interface, QR, and integrations

| Capability | Status | Priority | Scope |
|---|---|---|---|
| Admin interface | Current | P1 | Account setup, links, domains, tags, analytics, and API key management are available in the responsive embedded interface. Overview, monitoring, system diagnostics, backups and the audit log are built into the admin interface. [Design system](DESIGN_SYSTEM.md) |
| Localization and themes | Current | P1 | Support English and German interface text, dark, light, and system themes, locale-aware numbers and dates, and a display timezone preference. |
| QR Studio | Current | P1 | Preview and export local SVG and PNG with colors, transparency, error correction, resolution, quiet zone, and an optional outline. Verify output with an independent decoder. |
| QR design templates | Current | P1 | Save, apply, and delete reusable render settings in the QR dialog. Templates are included in SQLite backups. |
| QR logo overlay | Current | P1 | Select a static PNG for the current dialog. The renderer checks the exact encoded URL before returning either format. |
| Destination title lookup | Current | P1 | After `TITLE_LOOKUP_ENABLED=true`, fetch page titles for untitled, unprotected links in one bounded background worker through the shared outbound policy. Titles written by people are never replaced, and stale results are dropped. |
| Destination preview images | Current | P2 | After Preview images and title lookup are on, read the page's own `og:image`, `twitter:image`, or icon from the title lookup response, fetch it through the shared outbound policy, and store only a re-encoded JPEG thumbnail of at most 48 KiB. No screenshots or headless browser. Shown in the links list and on the link detail page. |
| Health Monitor | Current | P2 | After activation and per-link enrollment, run adaptive, bounded probes under the outbound address policy. Support manual checks, a bounded batch queue and configured public-domain probes. |
| Webhooks and notification events | Current | P2 | An explicitly configured HTTPS webhook receives link health, upcoming and completed expiry, click milestones, failed scheduled backups and analytics drops, low storage, and security events through a durable, deduplicated outbox with bounded retries. One pinned internal connector can reach a private receiver. |
| Notification connectors | Optional | P2 | Evaluate ntfy, Gotify, and other connectors when the generic webhook leaves a gap. |
| Quick Share and API examples | Optional | P3 | Provide examples for authenticated automation and Apple Shortcuts. Evaluate client libraries where needed, and assess a share target and browser integration on supported devices. |
| Installable admin interface | Optional | P2 | Evaluate PWA installation, cache behavior for authenticated sessions, and operation under an approved subpath. |

[API contract](API.md) · [Architecture](ARCHITECTURE.md) · [Release test plan](TESTING.md)
