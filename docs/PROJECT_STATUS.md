# Project status

**November 26, 2026 · Development after 0.6.0-preview.2.** Architecture and design are approved for implementation. Existing links are recreated manually.

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
| Analytics | UTC trends, custom dates, previous-period comparison, JSON report export, top links, domains and tags, and browser, OS, device and traffic-source breakdowns. Local GeoIP remains optional. |
| Backup and restore | The CLI creates verified SQLite snapshots and can restore them offline. The browser downloads a verified snapshot and can validate and stage a restore for the next controlled restart. Optional scheduled backups have independent retention. |
| Monitoring and operations | Browser-only monitoring of explicitly enrolled destinations and configured public domains, plus opt-in link health webhooks, an administrative audit log, and basic system diagnostics. |
| Data exchange | Bounded native JSON and CSV exchange, configurable CSV column mapping, preview, conflict detection, and atomic apply. |
| Authentication | A CLI-issued account claim, Argon2id passwords, revocable sessions, account recovery, CSRF protection, scoped API keys and transactional rotation. Configurable trusted proxies and rate budgets are available. |
| Database verification | The schema tests cover compatibility, the redirect index, and exact stored destinations. The authentication and lifecycle migrations have additional transactional regression tests. |
| Build and container verification | GitHub Actions checks the Rust integration tests, Clippy, Rustfmt, frontend types and assets, and a non-root container with a Chromium smoke test. [CI results](https://github.com/Buffer379x/shortcircuit/actions) |
| GitHub | The owner approved public source development and container previews. The repository uses MIT. [Repository](https://github.com/Buffer379x/shortcircuit) · [Preview](https://github.com/Buffer379x/shortcircuit/releases/tag/v0.6.0-preview.2) |
| UI and branding | The embedded Svelte admin interface includes account setup, authentication, links, and API keys with English and German UI text and dark, light, or system appearance. The separate design preview covers additional planned screens. |
| Browser verification | CI exercises the integrated administration workflow in Chromium. The [capture instructions](../README.md) cover the separate design preview. |
| Unraid | The project owner confirmed that preview.1 starts through Unraid and displays the Docker icon and browser favicon. The amd64 template supports a Docker-managed data volume and private administration through a reverse proxy. [Installation](UNRAID_INSTALLATION.md) |
| Performance and release | The benchmark method and resource estimates are documented. Production measurements and operating approval are pending. |

## Open work

- **Reporting.** The Overview and Analytics pages provide operational counts, trends, and UTC report ranges. Date and time labels outside UTC reports support a browser-specific display timezone. GeoIP can be evaluated with an operator-supplied dataset and an approved distribution policy.
- **Monitoring and automation.** Opt-in saved-link and configured public-domain probes, adaptive intervals, and a generic HTTPS webhook are implemented as development code. Title lookup, notification events beyond link health, and private connectors remain open.
- **Administration.** System diagnostics report uptime, process RSS where available, SQLite and data sizes, and recent backup and monitor status. CPU and filesystem telemetry, destination policies, and release security acceptance remain open.
- **Production verification.** Exercise backup, upgrades and rollback on the intended Unraid storage and reverse proxy. Measure memory, image size, startup, and latency under representative load. Complete responsive and accessibility checks, review advisory and dependency scan results, and obtain owner acceptance.

Collections, Link Pages, Quick Share and Apple Shortcuts, PWA installation, additional notification connectors, TOTP, OIDC, user roles, encrypted backups, and arm64 are optional roadmap items. [Feature matrix](FEATURE_MATRIX.md) · [Roadmap](ROADMAP.md)
