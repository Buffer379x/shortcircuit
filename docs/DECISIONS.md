# Decisions and open questions

Architecture and design are approved for implementation. Existing links are recreated manually as needed. The owner approved container previews. Production verification and stable release approval are pending.

| ID | Status | Area | Decision or open question |
|---|---|---|---|
| D01 | Proposed | Process model | Run a modular Rust application in one non-root container with separate public and admin listeners. Terminate TLS at the reverse proxy. |
| D02 | Proposed | Storage | Use a local SQLite database in WAL mode with full synchronization, online backups, and a database build with the WAL-reset fix. |
| D03 | Proposed | Redirect defaults | Use `302` redirects. Disable query and path forwarding by default. Preserve the configured host, exact slug, and stored destination. |
| D04 | Decided | Existing links | Recreate the links the operator still needs. Analytics begins when links are served by ShortCircuit. |
| D05 | Proposed | Frontend | Build static assets in Svelte, TypeScript, and Vite. Use CSS tokens, local graphics, and system fonts. A static browser preview is available. |
| D06 | Proposed | Data defaults | Minimize visit data, honor Do Not Track, and retain details for 90 days and aggregates for 24 months. Disable geolocation and server-side fetching by default. |
| D07 | Proposed | Private administration | Use LAN or VPN access, secure session cookies, short-lived CLI setup tokens, and explicitly provisioned API keys. Setup requires an operator-selected password. Registration is closed. |
| D10 | Open | Production routing | Before switching proxy routes, verify every required public host, exact slug, and expected redirect for the recreated links. |
| D11 | Decided for source development | Name and license | The owner approved the public ShortCircuit repository and selected the MIT license. Release naming clearance remains open. |
| D12 | Open | Design | The proposed direction uses a circuit with a lightning bolt, dark and light palettes, and compact navigation. Review against the graphics in the separate ShortCircuit chat is pending. |
| D13 | Open | Availability | Measure load and agree on acceptable downtime. The initial design uses one instance and requires a maintenance window for some operations. |
| D15 | Open | Extended scope | Prioritize password-encrypted backups, TOTP, roles, Link Pages, and additional connectors against actual needs. |

## Technical risks and follow-up

The 0.1 foundation is present as source code. The checks below cover the planned product and any manual production switch.

| Priority | Risk | Mitigation or verification |
|---|---|---|
| P0 | A recreated short URL resolves differently | Check real hosts, exact slugs, and expected redirect responses before switching the proxy. Scan published QR codes when relevant. |
| P0 | Concurrent requests grant more redirects than a link allows | Reserve visits atomically using a dedicated limit ledger. Test exhaustion, retries, and the remaining budget for any recreated link with a limit. |
| P0 | Outbound requests reach protected addresses, admin routes become public, or a restore fails | Use a shared outbound request policy, separate listeners, strict trust boundaries, and a verified restore exercise. |
| P1 | Synchronous visit limits contend with analytics and backup | Keep priority transactions short, bound batch sizes, and measure p95 and p99 under mixed load. |
| P1 | Stored analytics exceed the intended privacy policy | Test retention, deletion, and opt-outs. Document which fields each setting collects. |
| P1 | The public name could be confused with an existing name in the same category | Resolve the naming decision before public distribution. |
| P2 | Integration and PWA work expands beyond user needs | Start with generic webhooks, the native API, and Apple Shortcuts examples. Evaluate additional integrations as needs emerge. |
| P2 | The production UI diverges from the preview | Reuse design tokens and review screen reader behavior, localization, and real success and error states. |

[Architecture](ARCHITECTURE.md) · [Security](SECURITY.md) · [Name and licensing](NAME_AND_LICENSE.md) · [Roadmap](ROADMAP.md)
