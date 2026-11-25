# Project status

**November 25, 2026 · 0.5.0 in development.** Architecture and design are approved for implementation. Existing links are recreated manually.

| Area | Status |
|---|---|
| Public foundation | Host-bound redirects, a versioned SQLite database with WAL and full synchronization, and separate public and admin listeners. |
| Link management | Create, list, edit, enable, disable, tag, and delete links. The admin interface can export the displayed links as JSON. |
| Redirect controls | Managed domains and fallbacks, time windows, atomic visit limits, ordered rules, and opt-in query and path forwarding. |
| QR and tags | Local PNG and SVG export for the configured public short URL. The admin catalog shows tag counts and supports rename, merge and delete. |
| Analytics | UTC trends, period comparison, top links, DNT, referrer hostname controls, per-link opt-out, retention, and clear-all. Browser, OS, device, and geolocation breakdowns are pending. |
| Backup and restore | The CLI creates consistent, verified SQLite backups and supports an offline restore with a rollback copy. Scheduled backups and admin downloads are planned. |
| Authentication | A CLI-issued account claim, Argon2id passwords, revocable sessions, account recovery, CSRF protection, and scoped API keys. |
| Database verification | The schema tests cover compatibility, the redirect index, and exact stored destinations. The authentication and lifecycle migrations have additional transactional regression tests. |
| Build and container verification | GitHub Actions checks the Rust integration tests, Clippy, Rustfmt, frontend types and assets, and a non-root container with a Chromium smoke test. [CI results](https://github.com/Buffer379x/shortcircuit/actions) |
| GitHub | [The repository](https://github.com/Buffer379x/shortcircuit) exists. The owner approved public source development in this repository. The owner set the MIT license in the initial commit. Release artifacts still require approval. |
| UI and branding | The embedded Svelte admin interface includes account setup, authentication, links, and API keys with English and German UI text and dark, light, or system appearance. The separate design preview covers additional planned screens. |
| Browser verification | CI exercises the integrated administration workflow in Chromium. The [capture instructions](../README.md) cover the separate design preview. |
| Performance and release | The benchmark method and resource estimates are documented. Production measurements and operating approval are pending. |

[Core implementation plan](superpowers/plans/2026-11-25-core-foundation.md) · [Roadmap](ROADMAP.md) · [Decisions](DECISIONS.md)
