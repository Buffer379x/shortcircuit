# Project status

**November 26, 2026 · 0.6.0 in development.** Architecture and design are approved for implementation. Existing links are recreated manually.

| Area | Status |
|---|---|
| Public foundation | Host-bound redirects, a versioned SQLite database with WAL and full synchronization, and separate public and admin listeners. |
| Link management | Create, list, edit, enable, disable, tag, and delete links. The admin interface can export the displayed links as JSON. |
| Link tools | The link forms can find exact destination matches or preview the removal of known tracking parameters. |
| Redirect controls | Managed domains and fallbacks, time windows, atomic visit limits, ordered rules, and opt-in query and path forwarding. |
| Protected links | Optional passwords and confirmed one-time access. The public form verifies a short-lived signature and the configured origin before redirecting. SQLite enforces consumption independently of analytics. |
| Link history | Record individual link edits with actor and timestamp. Compare and restore metadata or redirect policy through the admin dialog, download JSON, or clear a link's history. |
| QR Studio | Preview and export local PNG and SVG with colors, transparency, error correction, size, and quiet-zone controls. Copy PNG when the browser supports image clipboard access. |
| Tags | The admin catalog shows tag counts and supports rename, merge and delete. |
| Analytics | UTC trends, period comparison, top links, DNT, referrer hostname controls, per-link opt-out, retention, and clear-all. Browser, OS, device, and geolocation breakdowns are pending. |
| Backup and restore | The CLI creates consistent, verified SQLite backups and supports an offline restore with a rollback copy. Optional scheduled backups have independent retention and use the same verified snapshot format. Admin downloads are planned. |
| Data exchange | Bounded native JSON and CSV exchange, preview, exact collision checks, and atomic apply. Bulk field mapping is planned. |
| Authentication | A CLI-issued account claim, Argon2id passwords, revocable sessions, account recovery, CSRF protection, and scoped API keys. |
| Database verification | The schema tests cover compatibility, the redirect index, and exact stored destinations. The authentication and lifecycle migrations have additional transactional regression tests. |
| Build and container verification | GitHub Actions checks the Rust integration tests, Clippy, Rustfmt, frontend types and assets, and a non-root container with a Chromium smoke test. [CI results](https://github.com/Buffer379x/shortcircuit/actions) |
| GitHub | The owner approved public source development and the first container preview. The repository uses MIT. [Repository](https://github.com/Buffer379x/shortcircuit) · [Preview](https://github.com/Buffer379x/shortcircuit/releases/tag/v0.6.0-preview.1) |
| UI and branding | The embedded Svelte admin interface includes account setup, authentication, links, and API keys with English and German UI text and dark, light, or system appearance. The separate design preview covers additional planned screens. |
| Browser verification | CI exercises the integrated administration workflow in Chromium. The [capture instructions](../README.md) cover the separate design preview. |
| Unraid | The project owner confirmed that the amd64 preview starts through Unraid and displays the Docker icon and browser favicon. The template supports a Docker-managed data volume and private administration through a reverse proxy. [Installation](UNRAID_INSTALLATION.md) |
| Performance and release | The benchmark method and resource estimates are documented. Production measurements and operating approval are pending. |

[Core implementation plan](superpowers/plans/2026-11-25-core-foundation.md) · [Roadmap](ROADMAP.md) · [Decisions](DECISIONS.md)
