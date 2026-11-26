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
| Analytics | UTC trends, period comparison, top links, catalog click counts and last use, DNT, referrer hostname controls, per-link opt-out, retention, and clear-all. Browser, OS, device, and geolocation breakdowns are pending. |
| Backup and restore | The CLI creates consistent, verified SQLite backups and supports an offline restore with a rollback copy. Optional scheduled backups have independent retention and use the same verified snapshot format. Admin downloads are planned. |
| Data exchange | Bounded native JSON and CSV exchange, preview, exact collision checks, and atomic apply. Bulk field mapping is planned. |
| Authentication | A CLI-issued account claim, Argon2id passwords, revocable sessions, account recovery, CSRF protection, and scoped API keys. |
| Database verification | The schema tests cover compatibility, the redirect index, and exact stored destinations. The authentication and lifecycle migrations have additional transactional regression tests. |
| Build and container verification | GitHub Actions checks the Rust integration tests, Clippy, Rustfmt, frontend types and assets, and a non-root container with a Chromium smoke test. [CI results](https://github.com/Buffer379x/shortcircuit/actions) |
| GitHub | The owner approved public source development and container previews. The repository uses MIT. [Repository](https://github.com/Buffer379x/shortcircuit) · [Preview](https://github.com/Buffer379x/shortcircuit/releases/tag/v0.6.0-preview.2) |
| UI and branding | The embedded Svelte admin interface includes account setup, authentication, links, and API keys with English and German UI text and dark, light, or system appearance. The separate design preview covers additional planned screens. |
| Browser verification | CI exercises the integrated administration workflow in Chromium. The [capture instructions](../README.md) cover the separate design preview. |
| Unraid | The project owner confirmed that preview.1 starts through Unraid and displays the Docker icon and browser favicon. The amd64 template supports a Docker-managed data volume and private administration through a reverse proxy. [Installation](UNRAID_INSTALLATION.md) |
| Performance and release | The benchmark method and resource estimates are documented. Production measurements and operating approval are pending. |

## Open work

- **Link management and redirects.** Domain DNS and health checks depend on the health monitoring work.
- **Analytics and dashboard.** Top tags and domains, browser, OS, and device breakdowns, more detailed bot and preview filters, custom dates, richer dashboard views, and analytics export. Evaluate local GeoIP with a suitable dataset license.
- **Monitoring and automation.** An opt-in outbound request policy shared by the Health Monitor, automatic title lookup, and webhooks. Add bounded adaptive checks, per-link settings, and notifications after that policy is verified.
- **Administration and abuse controls.** Browser backup downloads and validated restore, a durable audit log, system diagnostics, API key rotation, configured trusted proxies for client address attribution, and broader rate limits. Native CSV and JSON work with the ShortCircuit format. Import field mapping remains open.
- **Production verification.** Exercise backup, upgrades and rollback on the intended Unraid storage and reverse proxy. Measure memory, image size, startup, and latency under representative load. Complete accessibility and responsive checks, dependency and container scans, release verification, and owner acceptance.

Collections, Link Pages, Quick Share and Apple Shortcuts, PWA installation, additional notification connectors, TOTP, OIDC, user roles, encrypted backups, and arm64 are optional roadmap items. [Feature matrix](FEATURE_MATRIX.md) · [Roadmap](ROADMAP.md)

[Core implementation plan](superpowers/plans/2026-11-25-core-foundation.md) · [Roadmap](ROADMAP.md) · [Decisions](DECISIONS.md)
