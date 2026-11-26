# ShortCircuit handoff for Claude Code

Complete the outstanding implementation first. Review the resulting repository from the perspectives of a software developer, a product designer, a user, and a security engineer. Record the findings, write a prioritized improvement and cleanup plan, then implement and verify the in-scope items in that plan. Continue through all three stages in order. The owner's request authorizes routine, reversible source changes needed for this work.

## Starting point

- The public repository is `Buffer379x/shortcircuit`. The verified handoff baseline is `main` at `d7c56a4e8883c0c27da94ed8918f38c6bdfc4436`. Inspect the current checkout and preserve any work already present. In an exported workspace without Git metadata, use the source and verified commit as the reference and confirm repository state before using Git operations. [Baseline commit](https://github.com/Buffer379x/shortcircuit/commit/d7c56a4e8883c0c27da94ed8918f38c6bdfc4436)
- The published Unraid template installs `v0.6.0-preview.2` for `linux/amd64`. The current source contains additional functionality. Treat the published image and the source candidate as distinct installations when reproducing behavior. [Published preview](https://github.com/Buffer379x/shortcircuit/releases/tag/v0.6.0-preview.2)
- The verified source candidate passed 137 Rust tests, Clippy, Rustfmt, Svelte checks, the schema and OpenAPI checks, full npm and Rust dependency checks, and Docker and Chromium exercises. CI upgraded a disposable volume from the published preview and verified the existing account and the exact redirect destination. The image scan reported zero findings in its configured scope for fixable high and critical vulnerabilities. [CI evidence](https://github.com/Buffer379x/shortcircuit/actions/runs/45878618813)
- The CI benchmark used 1,024 loopback requests, eight concurrent workers, and one slug. It reported 2.175 ms P95 and 6.184 MiB in `docker stats` after the run. It is a smoke-test measurement. Collect separate evidence for process RSS, idle resources, representative data and traffic, and real Unraid and proxy behavior. [Benchmark protocol](PERFORMANCE.md)

Read [contributor guidance](../CLAUDE.md), [project status](PROJECT_STATUS.md), [feature matrix](FEATURE_MATRIX.md), [decisions](DECISIONS.md), and [testing](TESTING.md) first. Use [architecture](ARCHITECTURE.md), [database design](DATABASE.md), [API](API.md), [security](SECURITY.md), [threat model](THREAT_MODEL.md), [privacy](PRIVACY.md), [backup and restore](BACKUP_RESTORE.md), and [design system](DESIGN_SYSTEM.md) for the respective work. Verify each backlog item against the current code before implementing it. Reuse an existing implementation when it already meets the acceptance criteria and record the evidence.

## Project constraints

- Keep a modular Rust application with separate public and admin listeners, local SQLite, and embedded Svelte assets in one non-root Docker container. Keep the normal installation usable through the Unraid template alone.
- Preserve published hostnames, case-sensitive slugs, encoded paths, stored destinations, query strings, and fragments. Preserve transactional visit limits and one-time link consumption. Existing short links are recreated manually when needed. Keep the project documentation focused on ShortCircuit.
- Prioritize security, data integrity, reliability, usability, resource efficiency, maintainability, extensibility, and visual quality in that order. Preserve current safe defaults. Keep outbound jobs bounded and opt-in. Keep the public redirect path free of outbound requests.
- Write project documentation in English. Keep UI text centrally translated into English and German. Use functional copy throughout the UI, including sign-in. Preserve the design system, local assets, and accessible interaction patterns.
- Use synthetic fixtures and disposable data volumes. Keep passwords, tokens, visitor data, sensitive destinations, and production configuration out of Git, logs, test fixtures, and public artifacts. Run defensive verification against controlled targets.
- Release publication, image or template tag changes for deployment, license changes, paid resources, and operations on the user's Unraid, reverse proxy, domains, or live data require separate explicit owner approval. Continue source implementation and local or CI verification while an operator-dependent check is pending. Report that check with the evidence required to close it.

## Stage 1 · Complete the outstanding implementation

Work in small, independently verifiable changes. For each item, establish the current behavior, add focused regression coverage where needed, implement the change, and update the affected docs and API contract. Append versioned migrations and exercise upgrades from existing data. Capture the command and result for each acceptance check.

### 1. Analytics read permission

Start in [`src/auth.rs`](../src/auth.rs), [`src/admin/access.rs`](../src/admin/access.rs), [`src/admin/metrics.rs`](../src/admin/metrics.rs), and [`tests/admin.rs`](../tests/admin.rs). Extend the key permission model, scope storage validation, key UI and rotation, and [OpenAPI](openapi.json).

- [ ] Add a dedicated read-only analytics scope. Authorize the intended reporting endpoints explicitly. Preserve browser-session behavior and keep analytics settings and deletion restricted to browser sessions.
- [ ] Apply a host-bound key's restriction in the report and export queries, including totals, previous periods, rankings, and breakdowns. Review instance-wide metadata for cross-host disclosures.
- [ ] Prove that existing keys retain their grants, an ordinary link-read key cannot read analytics, expired and revoked keys are rejected, and a host-bound key cannot report on another host. Verify that rotation preserves the analytics scope.

### 2. Domain-root reporting

Start in [`src/http.rs`](../src/http.rs), [`src/analytics.rs`](../src/analytics.rs), [`src/admin/metrics.rs`](../src/admin/metrics.rs), and [`tests/analytics.rs`](../tests/analytics.rs). Make root requests distinguishable in the report, export, and analytics UI.

- [ ] Define how root requests with and without a fallback are counted. Keep them distinct from successful link redirects, blocked access, and missing slugs. Respect global analytics settings, Do Not Track, data retention, and the current request-method counting rules.
- [ ] Preserve the status and destination of the public root response. Prove that existing events and totals retain their meaning across migration, including when no row exists for the new category.

### 3. Destination policy and traffic visibility

Start in [`src/input.rs`](../src/input.rs), the handlers in [`src/admin`](../src/admin), and the final redirect resolution in [`src/redirect.rs`](../src/redirect.rs). Reuse [`src/analytics.rs`](../src/analytics.rs) and the Overview or Analytics UI for traffic visibility.

- [ ] Add an opt-in destination-domain allowlist and blocklist with normalized hostname matching and documented precedence. Map every destination write path, including rules, fallbacks, imports, duplication, and history restore. Define how the policy affects saved destinations when enabled and explain the effect in the UI and docs.
- [ ] Cover mixed case, IDNs, exact matches and explicit subdomain semantics, conflicts between allow and block rules, and existing links. Keep default behavior and stored URL spelling intact. When a policy prevents a protected redirect, preserve its visit allowance and one-time access state.
- [ ] Surface unusually high traffic using a bounded local count, a clear time window, and an understandable threshold or baseline. Separate the indicator from automated enforcement. Verify that the report respects host and analytics privacy boundaries.

### 4. Disk and CPU diagnostics

Extend [`src/admin/operations.rs`](../src/admin/operations.rs), [`frontend/src/Operations.svelte`](../frontend/src/Operations.svelte), the translation dictionary, and the system API schema.

- [ ] Report available space for the filesystem that contains the SQLite data. Provide CPU usage where a reliable reading is available and label the scope, units, and sampling window. Keep process RSS and container memory conceptually separate.
- [ ] Report unavailable or unsupported measurements explicitly. Verify healthy, unavailable, and low-space display states using controlled fixtures. Keep diagnostics authenticated and off the public listener. Reuse existing platform information and prefer measurements on demand.

### 5. Opt-in title retrieval

Extend the shared outbound policy in [`src/outbound.rs`](../src/outbound.rs) and reuse the existing link administration and background-work patterns.

- [ ] Fetch titles for eligible saved destinations after explicit activation. Keep fetching outside the redirect path. Use bounded parsing, validated connections, revalidation after any followed redirect, and bounded response bytes and time.
- [ ] Render extracted titles as text. Preserve user-written titles and drop a stale result after the hostname, target, enrollment, or title has changed. Verify disabled behavior, safe failure, and preservation of the exact saved destination.

### 6. Notification events and internal webhook connections

Start with [`src/monitor.rs`](../src/monitor.rs), [`src/outbound.rs`](../src/outbound.rs), [`src/backup_schedule.rs`](../src/backup_schedule.rs), and the existing configuration and operations code. Reuse the generic webhook transport.

- [ ] Reconcile the notification events in [the roadmap](ROADMAP.md) and the current feature matrix with the implementation. Cover upcoming and completed expiry, failed backups and other jobs, low storage, and relevant security events. Keep click milestones configurable. Define event identity, retry limits, backoff, duplicate suppression, and restart and restore behavior. Keep payloads minimal and provide actionable delivery status.
- [ ] Support internal destinations only through an explicitly configured, narrowly scoped trusted connector. Keep that policy separate from public probes and title retrieval. Pin allowed origins and address ranges, retain TLS verification, constrain credential use, and keep redirect following disabled for webhook delivery. Avoid any global override of outbound protections.
- [ ] Verify default-off behavior, connector isolation, rejected peers, repeated delivery failures, bounded work, and behavior after restart or restore with synthetic fixtures.

### Release preparation and backlog boundaries

Prepare the test and rollback procedure for the target Unraid configuration and Nginx Proxy Manager. Exercise installation, upgrade, backup, restore, and rollback in CI or a disposable environment. Verify the actual proxy trust and listener routing over IPv4 and IPv6 where available. Record the results for LAN benchmarks, exact published redirects, Mobile Safari, screen-reader use, and keyboard navigation. Check the core UI at 1920 × 1080, 1440 × 900, 1280 × 800, 1024 × 768, 768 × 1024, 390 × 844, and 320 × 568. Record the remaining operator-dependent work with an exact acceptance check. Preserve the older image and a verified pre-upgrade snapshot for rollback.

Prepare the source and CI changes for the stable artifact checks in [Testing](TESTING.md), including image pinning, SBOM and artifact verification. Record the release jobs and acceptance evidence for owner review.

The optional backlog includes local GeoIP, time and network redirect conditions, collections, Link Pages, Quick Share and PWA, TOTP and OIDC, additional roles, encrypted backups, and arm64. Keep their current roadmap status visible. Add an optional feature to this work only when the owner puts it in scope. See [the feature matrix](FEATURE_MATRIX.md) and [roadmap](ROADMAP.md).

Before moving to Stage 2, record each implementation item's status and evidence in [project status](PROJECT_STATUS.md). Continue the review when the remaining items require an owner decision or a target environment that is unavailable. Keep every such item visible in the final report.

## Stage 2 · Review the completed code from four perspectives

Create `docs/QUALITY_REVIEW.md`. Record the reviewed commit, environments, workflows exercised, tests actually run, and coverage limits. Inspect code and runtime behavior. Include the public and admin routers, persistence, analytics and jobs, API and permissions, frontend, packaging, documentation, and tests. Trace findings to code paths or observed behavior.

| Perspective | Questions to answer |
|---|---|
| Software developer | Are module boundaries clear? Where do repeated logic, unnecessary dependencies, or stale code make changes harder? Check query plans, migrations, concurrency, retries, startup, graceful shutdown, error handling, and configuration. Identify measurable resource costs and missing regression coverage. |
| Product designer | Are navigation, typography, spacing, forms, charts, interaction states, and status language consistent? Verify keyboard and focus behavior, contrast, touch targets, responsive layouts, Dark, Light, and System appearance, English and German, and accessible names. Exercise real UI flows against the WCAG 2.2 AA design target. Record gaps in screen-reader or browser coverage. |
| User and operator | Install from the template and exercise account setup, link creation, copy, QR, edit, bulk actions, analytics, monitor, backup, restore, and recovery with test data. Check confusing defaults, dead controls, surprises during updates, error recovery, documentation accuracy, and everyday friction. |
| Security engineer | Check listener and proxy boundaries, session and scoped-key authorization, CSRF, input and upload validation, outbound policy, transaction integrity, sensitive logging, retention, credential handling, restore, dependency and image findings, CI permissions, and deployment defaults. Use isolated defensive tests. Keep sensitive findings out of public artifacts and track them through the project's security process. |

Give findings stable IDs and rank them `P0` release blocker, `P1` high priority, `P2` medium, `P3` low, or `P4` optional. For each finding record the component, file or workflow, observed evidence, reproducibility, impact, proposed correction, estimated effort, expected benefit, and acceptance check. Mark assumptions and unavailable environments so readers can distinguish verified findings from hypotheses. Follow [the release test strategy](TESTING.md) and [threat model](THREAT_MODEL.md).

## Stage 3 · Plan, implement, and verify improvements

- [ ] Create `docs/IMPROVEMENT_PLAN.md`. Tie each task to a review finding, state the affected files and dependencies, and give an observable acceptance check. Include effort, risk, and a rollback consideration for changes that touch data, public behavior, or deployment.
- [ ] Implement the plan in priority order after writing it. Start with P0 and P1. Choose the smallest changes that address the findings and preserve supported behavior. Evaluate proposed cleanup against maintenance benefit and regression risk. Keep destructive operations and release actions within the approval boundaries above.
- [ ] Recheck each finding after its change. Rerun the full applicable test suite and record results against the final commit. Update the review and plan with implemented items, residual risks, and items deferred for a specific reason. Update the relevant project docs, feature matrix, OpenAPI, and changelog as appropriate.
- [ ] Report the commits and test evidence, meaningful behavior changes, and outstanding operator checks. Provide a release-readiness assessment grounded in the verified results and open risks. A release requires owner approval.

## Verification commands

Use the pinned toolchain in the checkout and build the frontend before compiling the embedded application. Use the repository's [Core checks workflow](../.github/workflows/ci.yaml) for Docker, Chromium, the disposable-volume upgrade, the benchmark, and the image scan. Confirm the checkout and Compose project are isolated before running container or restore tests.

```sh
npm ci --prefix frontend
npm run check --prefix frontend
npm run build --prefix frontend
node tests/url-cleaner.cjs
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
python3 tests/openapi.py
python3 tests/schema.py
python3 tests/analytics_schema.py
python3 tests/unraid_template.py
cargo deny --locked check advisories licenses sources
npm audit --audit-level=high --prefix frontend
```

Review both fixable and applicable unfixed findings before release. Report a check as passed only when it was actually executed successfully against the identified revision. If the local toolchain or Docker is unavailable, use CI with synthetic data and link the run. Record any check that remains unverified.
