# ShortCircuit feature matrix

Coverage as of November 25, 2026. **Current** means the current development implementation is present in source. **Planned** means work is scheduled in the roadmap. **Optional** identifies additions subject to demonstrated need. [Project status](PROJECT_STATUS.md) · [Roadmap](ROADMAP.md)

Priority **P0** covers production readiness and safe operation. **P1** covers the core 1.0 feature set. **P2** covers follow-up work and enhancements. **P3** and **P4** cover longer-term options.

## Redirects and link management

| Capability | Status | Priority | Scope |
|---|---|---|---|
| Create and list links | Current | P0 | The authenticated admin API accepts absolute HTTP or HTTPS destinations, preserves their stored URI spelling, and lists links with a cursor. |
| Custom and generated slugs | Current | P0 | Slugs are case-sensitive and can contain multiple segments. Custom segments use ASCII letters, digits, hyphens, and underscores. Generated slugs use 10 random characters. The database enforces uniqueness per host. |
| Host-bound redirects | Current | P0 | Configured hosts can reuse the same slug independently. Public GET and HEAD use `302` by default and include `Cache-Control: no-store`. Matching uses the normalized host and exact path with optional extra path forwarding. |
| Verify published addresses | Planned | P0 | Recreate required short URLs and check the configured host, exact slug, status, and destination through the intended proxy. Scan printed QR codes or compare their decoded URLs when applicable. |
| Edit links and destinations | Current | P0 | Update destinations, titles, tags, and enabled status. Published hosts and slugs stay fixed. |
| Activation windows and visit limits | Current | P0 | Evaluate time windows in UTC. Reserve limited visits atomically using a dedicated ledger with separate analytics counting. |
| Redirect status and forwarding options | Current | P0 | Add per-link status and explicit query forwarding and extra-path policies. Keep temporary redirects and `no-store` for protected and limited links. Verify encoded paths, query keys, and fragments. |
| Domain management and fallbacks | Current | P0/P1 | Manage allowed public hosts from the admin UI and configure validated fallback targets per domain. Test route collisions, errors, and redirect loops. |
| Ordered redirect rules | Current | P1 | The first matching rule wins. The administration interface edits and reorders rules. Conditions use language, device, operating system, browser, and query values. |
| Path matching extensions and aliases | Optional | P2 | Evaluate Unicode slugs, case-insensitive or trailing-slash matching, and explicit aliases against collision, encoding, and proxy requirements. |
| Search, filters, and sorting | Current | P1 | Search the full catalog by title, slug, domain, destination, or tag. Combine domain, status, and tag filters and sort paginated results. |
| Bulk actions | Current | P1 | Select up to 50 links, change their status or tags atomically, export native JSON or CSV, and confirm permanent deletion. |
| Slug generation settings | Planned | P1 | Allow a configurable generated length while preserving database uniqueness and bounded retries. |
| Tags | Current | P1 | View tag counts, rename or merge tags, and remove their assignments. Tag suggestions and colors are planned. |
| Crawler controls | Planned | P1 | Configure crawler directives and per-link crawlability. Check reserved route collisions before serving `robots.txt`. |
| Duplicate detection | Current | P2 | Lookup matches against the exact stored destination. The creation and edit forms offer copying an existing short URL. Approximate matches and merge are planned. |
| Protected links | Current | P2 | Optional password protection, explicit confirmation, and atomic consumption of one-time links. |
| Link history | Current | P2 | Compare and restore earlier metadata and redirect policies. History lists verified actors and protection changes and supports JSON export and deletion. |
| Rule editor and simulator | Planned | P2 | Preview the selected destination for representative inputs. Preserve rule order during edits and support keyboard reordering. |
| URL Cleaner | Current | P2 | Review known trackers and one-time exceptions locally. Use the proposed URL explicitly before saving. Leave recognized signed URLs unchanged. |
| Favorites and archives | Planned | P2 | Pin frequently used links and archive links from the active list. |
| Collections | Optional | P2 | Evaluate a lightweight collections model as an additional organization option. |
| Link Pages | Optional | P3 | Evaluate a bounded content model with templates, branding, and scheduled availability. |

## Analytics and privacy

| Capability | Status | Priority | Scope |
|---|---|---|---|
| Visit totals and time series | Current | P1 | Report totals by UTC date and top link using bounded ingestion and indexed aggregates. Tag and domain views remain planned. |
| Visit details and unmatched requests | Planned | P1 | Show retained details and classify requests to the site root, invalid short links, and other unmatched paths. Prevent sensitive URL values from entering analytics. |
| Referrers, devices, and bots | Planned | P1 | Store referrer domains and coarse browser, operating system, and device categories by default. Show unknown categories and allow filtering by bot classification. |
| Privacy settings, retention, and deletion | Current | P1 | Honor global and per-link opt-outs and Do Not Track, with configurable retention and a clear-all action. Analytics collection operates independently of visit limits. |
| Period comparisons and report controls | Current | P1 | Compare fixed 7-, 30-, 90-, and 365-day views against the preceding UTC period. Custom dates and additional visualization are planned. |
| Geolocation | Optional | P1 | Use an operator-provided local GeoIP dataset. Apply the configured granularity and handle missing data explicitly in both reports and rules. |
| Multiple segment comparisons | Optional | P2 | Compare selected links, tags, or domains when reporting needs justify the extra views. Use consistent time boundaries. |
| Advanced visualizations and estimates | Optional | P4 | Evaluate local maps, heatmaps, milestones, and unique visitor estimates against accessibility and privacy requirements. |

## Administration and operations

| Capability | Status | Priority | Scope |
|---|---|---|---|
| Separate listeners and local storage | Current | P0 | The Rust source defines independent public and admin routers, loopback bind defaults, and a versioned local SQLite schema with WAL and full synchronization. |
| Admin API authorization and errors | Current | P0 | Browser sessions and scoped bearer keys authorize the link API. Errors use structured problem responses. The public listener serves redirects independently. |
| Liveness and readiness | Current | P0 | The admin listener serves minimal status-only probes for process and database readiness. |
| Interactive setup and authentication | Current | P0 | A short-lived CLI setup token, revocable sessions, CSRF protection, and documented recovery are available. Keep the administration interface behind LAN or VPN access. |
| Scoped API credentials | Current | P0 | Issue keys with named scopes, optional domain restrictions and expiry, and revocation. The API reveals the secret only at creation. |
| OpenAPI and API diagnostics | Current | P1 | The implemented API has an OpenAPI 3.1 document served through the admin interface. Request IDs and dedicated credential rotation are planned. |
| Audit, diagnostics, and abuse controls | Planned | P0 | Add a dedicated audit trail for sensitive operations, rate-limit protected endpoints, and make detailed diagnostics available to authorized administrators. Bulk deletions currently produce a structured count in the application log. |
| Backup and restore | Current | P0 | The CLI creates a verified local snapshot and restores it offline with an independent rollback copy. An opt-in scheduler retains verified copies. Administration through the browser is planned. |
| Native data exchange | Current | P1 | Import and export bounded versioned JSON and CSV pages, preview conflicts, and apply transactionally. Field mapping is planned. |
| Maintenance CLI and installation | Planned | P1 | Provide setup, recovery, and operational commands. Verify a non-root Docker image, local storage, and the Unraid installation. |
| Cross-origin admin API access | Optional | P2 | Consider explicit origins for integrations that need browser access. |
| Additional authentication methods and roles | Optional | P3 | Consider TOTP, recovery codes, OIDC, and roles as operational requirements emerge. |
| Deployment extensions | Optional | P4 | Assess PostgreSQL, arm64 builds, and password-encrypted backups against measured needs. |

## Admin interface, QR, and integrations

| Capability | Status | Priority | Scope |
|---|---|---|---|
| Admin interface | Current | P1 | Account setup, links, domains, tags, analytics, and API key management are available in the responsive embedded interface. The design preview covers planned dashboard and monitoring screens. [Design system](DESIGN_SYSTEM.md) |
| Localization and themes | Current | P1 | Support English and German interface text, dark, light, and system themes, and locale-aware date and number display. |
| QR Studio | Current | P1 | Preview and export local SVG and PNG with colors, transparency, error correction, resolution, quiet zone, and an optional outline. Verify output with an independent decoder. |
| QR design templates | Current | P1 | Save, apply, and delete reusable render settings in the QR dialog. Templates are included in SQLite backups. |
| QR logo overlay | Current | P1 | Select a static PNG for the current dialog. The renderer checks the exact encoded URL before returning either format. |
| Destination title lookup | Optional | P1 | Enable title fetching explicitly through the shared outbound HTTP client, with validated targets and response limits. |
| Health Monitor | Planned | P2 | After explicit activation, run bounded, scheduled probes under the shared outbound request policy. Report status without deactivating links. |
| Webhooks | Planned | P2 | Provide a generic webhook using safe outbound policies. Enable destinations explicitly. |
| Notification connectors | Optional | P2 | Evaluate ntfy, Gotify, and other connectors when the generic webhook leaves a gap. |
| Quick Share and API examples | Optional | P3 | Provide examples for authenticated automation and Apple Shortcuts. Evaluate client libraries where needed, and assess a share target and browser integration on supported devices. |
| Installable admin interface | Optional | P2 | Evaluate PWA installation, cache behavior for authenticated sessions, and operation under an approved subpath. |

[API contract](API.md) · [Architecture](ARCHITECTURE.md) · [Release test plan](TESTING.md)
