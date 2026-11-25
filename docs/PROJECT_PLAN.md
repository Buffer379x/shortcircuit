# Project plan

ShortCircuit is a self-hosted link manager built around a Rust process, an embedded SQLite database, and a static admin interface. The architecture and design are approved for development. Existing links are recreated manually where needed. Production readiness requires separate verification and approval. [Core implementation plan](superpowers/plans/2026-11-25-core-foundation.md)

## Architecture and acceptance

| Area | Approach | Evidence required for production |
|---|---|---|
| Server and UI | Rust, Axum, Tokio, SQLx, SQLite, and static Svelte with TypeScript and Vite | Verify both listeners and representative redirects and errors through the intended reverse proxy |
| Data integrity | Local SSD pool, bounded WAL connection pool, transactional visit limits, and batched analytics | Verify concurrent requests, crash recovery, full-disk behavior, and consistent restore |
| Short URLs | Store the configured host, exact slug, and destination | Verify the status and location for recreated URLs. Scan printed QR codes or compare their decoded URLs when relevant. |
| Privacy | Minimal server-side data, optional local GeoIP, and local diagnostics | Exercise retention and deletion and check that logs omit sensitive fields |
| API | A native versioned API with scoped credentials | Test response contracts, authorization, and bounded pagination |
| Deployment | Non-root container, one persistent SQLite directory, and admin access through LAN or VPN | Verify backups, upgrades, proxy headers, IPv6, file permissions, and the bundled SQLite version on the target host |
| Design | Responsive admin interface, English by default, German, and dark, light, and system themes | Verify mobile workflows, keyboard interactions, and WCAG 2.2 AA requirements |

## Development and release

1. **Core development.** Verify SQLite, redirect behavior, the two listeners, and the local API with synthetic data.
2. **Administration and operations.** Test setup, session security, the admin UI, restore, and the hardened runtime.
3. **Recreate links as needed.** Copy the required host and slug into ShortCircuit and compare the expected redirect before switching routing. Use a printed QR code or its decoded URL when applicable.
4. **Production release.** Complete the release tests, recovery exercise, proxy verification, and performance benchmarks. Obtain the project owner's approval.
5. **Public source and release artifacts.** The owner approved publishing the development repository. The source uses the MIT license. Confirm release naming and bundled dependency notices before a separately approved release.

## Feasibility and scope

- The core system covers links, tags, domains, local QR generation, analytics, and private administration. One optional collection model is enough for additional organization.
- Health checks, metadata fetching, and webhooks share an outbound request policy. Each integration requires explicit activation and bounded work. Use a simple interval schedule and distinguish authentication and rate-limit responses from broken destinations.
- Large analytics histories need indexed queries, daily aggregates, configurable retention, and bounded background jobs. Bot and unique visitor estimates have limited precision. Geolocation depends on an optional local dataset.
- Link Pages, broader integrations, and additional user roles are demand-driven. The core release uses one process, one database, and static web assets.

## Milestone coverage

| Milestone | Scope |
|---|---|
| 0.1 | Core storage, domain routing, URL validation, basic admin API, and test container |
| 0.2–0.3 | Setup, sessions, link management, tags, domains, redirect options, visit limits, and basic QR codes |
| 0.4–0.5 | Analytics, privacy controls, backup and restore, and native CSV and JSON handling |
| 0.6–0.8 | QR design, link history, protected links, monitoring, smart rules, and selected integrations |
| 0.9–1.0 | Release verification, benchmarks, production hardening, and Unraid installation |

[Feature matrix](FEATURE_MATRIX.md) · [Roadmap](ROADMAP.md) · [Architecture](ARCHITECTURE.md) · [Design system](DESIGN_SYSTEM.md) · [Release testing](TESTING.md)
