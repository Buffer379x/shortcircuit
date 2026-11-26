# Roadmap and effort

Estimates are in person-weeks for one experienced developer. They include implementation, tests, documentation, and review. Time for approvals and Unraid maintenance windows varies. Recreate existing links manually where needed.

| Phase | Scope | Acceptance criteria | Person-weeks |
|---|---|---|---:|
| 0 | Product research, architecture, data model, security, design study, and implementation plan | Architecture and design approved for implementation | Complete |
| 0.1 | Development foundation with two listeners, local SQLite, host routing, URL contract, redirects, and CI | Host and path fixtures, a non-root container, and database startup on a local pool | 2–3 |
| 0.2 | Setup, sessions, recovery, API scopes, admin UI, localization, links, tags, and initial export | Secure first sign-in, an end-to-end link workflow, and isolation of admin routes from the public listener | 3–5 |
| 0.3 | Domain and tag management, redirect rules and fallbacks, visit limits, and basic QR generation | Defined redirect behavior, reliable QR scanning, and atomic visit limits | 3–5 |
| 0.4 | Analytics, retention, opt-outs, visit types, referrer and client categories, and optional geolocation | Verified counts, targeted deletion, mixed-load tests, and reports with period comparisons | 3–4 |
| 0.5 | Backup, restore, and native CSV and JSON import and export | Consistent snapshots, a verified restore, and tested data exchange | 2–4 |
| 0.6 | Complete link management and add QR Studio, link history, protected links, duplicate detection, and URL Cleaner | Tested history and restore, confirmation for sensitive limit changes, and QR scanning across export variants | 3–5 |
| 0.7 | Health Monitor, shared outbound HTTP client with request validation, adaptive intervals, rule simulator, and webhooks | Bounded outbound work, validated connections, deterministic rules, and health checks that report without deactivating links | 3–5 |
| 0.8 | Favorites and archives, with demand-driven integrations, collections, and enhancements | Verified organization workflows, demonstrated need for optional features, and regression coverage for new integrations | 1–3 |
| 0.9 / 1.0 | Unraid template, benchmarks, restore exercise, documentation, and release review | Completed release test plan, verification of recreated URLs before any routing change, and operator approval. [Test plan](TESTING.md) | 3–5 |

Estimated effort after phase 0 is **23–39 person-weeks**. Schedule the first restore exercise in phase 0.5.

## Feature priorities

| Feature group | Priority | Effort | Scope |
|---|---|---|---|
| Redirects, exact hosts and paths, URLs embedded in QR codes, and atomic limits | P0 | High | Verify these before switching production routing |
| Secure administration, API scopes, recovery, audit, and abuse controls | P0 | High | Establish the security foundation before enabling outbound features |
| Backup, restore, and manual transition | P0 | High | Exercise restore and verify any published addresses that need to keep working |
| Link management, search, sorting, bulk actions, tags, and domains | P1 | Medium | Deliver the core workflows for 1.0. Collections are an optional addition. |
| CSV and JSON, Docker and Unraid installation, and documentation | P1 | Medium | Define native data formats and safe import and export defaults |
| Analytics, segment and period comparisons, privacy, and bot classification | P1 | High | Reuse chart and table components. An interactive map is optional. |
| Local QR generation, templates, colors, SVG and PNG, and scan verification | P1 | Medium | Start with basic generation and verify scanning before adding frames, logos, and templates |
| Ordered redirect rules and simulation | P1 for core rules, P2 for simulation and extensions | High | Implement rule ordering and common conditions first. Expand network, region, and time conditions as needed. |
| Exact duplicates, favorites, and archives | P2 | Medium | Start with exact matches. Approximate matching, manual merge, and collections are optional follow-up work. |
| Link history and protected links | P2 | High | Define retention and deletion with the schema. Implement confirmed one-time access and atomic limits first. |
| Health Monitor and adaptive intervals | P2 | High | Opt in and bound concurrency. Group identical destinations only when their probe policy and access context match. |
| URL Cleaner | P2 | Medium | Preview changes locally and apply them after confirmation |
| Webhooks and optional notifications | P2 | Medium | Start with a generic webhook. Evaluate ntfy and Gotify when needed. |
| Quick Share, API examples, and Apple Shortcuts | P3 | Low to medium | Start with iOS and macOS examples and keyboard shortcuts. Assess PWA Share Target and browser integration on the target devices. |
| Link Pages with templates, branding, and scheduled availability | P3 | High | Evaluate after the core release using a bounded content model |
| OIDC, TOTP, recovery codes, and user roles | P3 | Medium to high | Consider TOTP first. Evaluate OIDC and roles when multi-user operation warrants them. |
| Maps, heatmaps, milestones, and unique visitor estimates | P4 | Medium to high | Require a demonstrated need, local processing, and suitable privacy controls |
| PostgreSQL, arm64, and password-encrypted backups | P4 | Medium to high | Evaluate against measured operational needs |

[Feature matrix](FEATURE_MATRIX.md) · [Project status](PROJECT_STATUS.md)


## Acceptance gates after preview.2

The development branch groups the Overview, analytics reports, generic CSV mapping, browser backups, system diagnostics, administrative auditing, API key rotation, and opt-in link monitoring. The candidate lifecycle covers a clean Docker start, the embedded admin UI, a backup and restore cycle, and upgrade from preview.2 on a disposable volume.

The intended Unraid host still needs its own verification of local pool storage, actual public and admin routes, proxy header behavior, and recovery. Confirm the original hostnames and slugs that the operator intends to preserve. Collect the representative LAN benchmark and review the final release after that verification.

The use of tags and favorites covers routine organization. Collections remain elective. GeoIP, title lookup, notification providers beyond the generic health webhook, and Link Pages have separate deployment and privacy costs. Evaluate them against actual operating needs after the core acceptance run.
