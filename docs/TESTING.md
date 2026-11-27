# Test and Release Strategy

GitHub Actions runs the core test suite on pull requests and on `main`. The design preview has its own verification status. See [Project Status](PROJECT_STATUS.md).

| When | Checks | Evidence |
|---|---|---|
| Each relevant change | Rust formatting, Clippy, and tests. Frontend type checks and linting, OpenAPI contract checks, SQLite schema upgrade chains, and focused UI and redirect fixtures as those features land. | Deterministic seeds and synthetic data. |
| Each module | Slug, domain, proxy, parsing, fallback, and rule fixtures. Sessions and scopes, outbound network policy in isolated fixtures, import and restore failures, accessibility, and input states. | Targeted coverage of boundary and failure cases. |
| Each release | Redirects through the intended proxy configuration, a rehearsal for any planned traffic switch, failure and recovery cases, Docker on amd64, end-to-end tests, dependency and license checks, a container scan, and the Unraid installation workflow. | Archive a test report with versions, environment, and reproducible results. |
| Extended coverage | Multiple architectures, longer load profiles, fuzzing or property-based tests, and independent security review. | Prioritize changes to outbound fetching, authentication, limit enforcement, backup, or atomic restore. |

## P0 Cases

- Compare redirects by host, exact slug, method, status, `Location`, and other headers. Cover case, IDNA, multiple path segments, encoding, query strings and fragments, extra paths, unknown hosts, fallbacks, expiry, deactivation, dynamic rule order, and route collisions. Verify QR payloads and deployed QR image URLs separately where relevant.
- Check permissions for each object and action. Verify that setup works only within its intended trust boundary. Test CSRF, CORS, rate limits, proxy headers, session rotation, and revocation. Verify that outbound connections can reach only approved addresses.
- Test preset remaining allowances under concurrent `GET`, `HEAD`, and preview requests. Cover analytics loss, retries, races, a full database, shutdown, and rollback.
- Test repeated, interrupted, and canceled native CSV and JSON imports, conflict handling, and the counting of new visits. Verify exact host, slug, and destination when manually recreating a short URL intended to stay in service.
- Test backup and restore with low disk space, corrupted content, incompatible versions, concurrent operations, and process termination. Verify recovery to the previous database generation.
- Run the artifact on the target system and verify the non-root user, separate listeners, health checks, restart, and stop. Check signatures or digests and the SBOM for stable artifacts. Review dependencies and licenses with `cargo audit` and `cargo deny` or equivalent tools.

## UI and Release Report

Test core workflows in Dark, Light, and System themes, in English and German, at all seven target resolutions. Cover keyboard input, focus, form errors, loading and empty states, and races. Measure contrast and manually review WCAG 2.2 AA conformance with at least one screen reader and Mobile Safari. Check asset requests and common browsers. Scan real QR PNG and SVG output at different sizes and on different backgrounds. [WCAG 2.2](https://www.w3.org/TR/WCAG22/)

`npm run check --prefix frontend` runs svelte-check, the design lint (`frontend/tools/lint-design.py`: no color literals outside the tokens, no literal font sizes, radii, or z-indexes, no page restyling a shared component), the WCAG contrast check of every token pair in both themes (`frontend/tools/contrast.py`), and the catalog check (`tests/i18n_catalog.py`: identical English and German keys, unique keys across area files, no empty strings). The browser smoke test also runs axe-core in both themes and fails on a Content Security Policy violation. [Design system](DESIGN_SYSTEM.md) · [UI guidelines](UI_GUIDELINES.md)

The release review report records severity from P0 through P4, component, impact, evidence and reproduction steps, resolution, effort, and benefit. Track required 1.0 features and any deviations by version in the test report. Unresolved critical or high risks, an unverified planned traffic switch, or an untested restore block release. Stable version tags and `latest` follow approval.

## CI and Versioning

After core checks succeed for a push to `main`, the container preview workflow checks that the Unraid template references the approved preview tag. For a new release, it builds the amd64 image from that commit and runs the template defaults in Docker. For preview.2 it also upgrades a disposable data volume from the published preview.1 image, signs in with the existing account, verifies the exact redirect, and checks a bulk write. The workflow pushes the image to GHCR. A clean runner pulls the tagged image anonymously and verifies its digest before the workflow posts the GitHub prerelease with the XML and image digest. If the anonymous pull fails because the package is private, set the package visibility to Public and rerun the failed jobs. [GitHub package visibility](https://docs.github.com/en/packages/learn-github-packages/configuring-a-packages-access-control-and-visibility#configuring-visibility-of-packages-for-your-personal-account)

The Git workflow checks changes before they merge into the main branch. Preview CI builds amd64 images with a pinned Rust toolchain, OCI labels, and verifiable digests. Base-image digest pinning, reproducible builds, dependency and container scans, and SBOM generation are stable release tasks. Future CI caches should use toolchain, architecture, and lockfile keys. Review dependency updates as individual changes. Tag release candidate images with a SemVer prerelease tag. Apply stable tags and `latest` only after stable release authorization. Document the schema version, pre-upgrade backup, and supported rollback path. Add arm64 when the target system requires it and dedicated release checks pass.


## Development candidate checks

The GitHub Actions core workflow type-checks and builds the admin interface, runs the SQLite and Rust contract suites, and starts the container as its non-root user. An independent job checks RustSec advisories, licenses, and dependency sources with cargo-deny and checks frontend dependencies and build tools with npm audit. The container job scans the local candidate image with Trivy for fixed high or critical findings. During release review, inspect unfixed vulnerabilities for applicability and document any accepted residual risk.

The hosted loopback benchmark uses the smoke-test database. It validates the measurement script and reports the observed latency, local image size, and container memory. Complete the representative LAN workload in [Performance](PERFORMANCE.md) before a stable release.

## Container checks added after preview.2

The core workflow fails if the read-only container logs lost analytics events after the redirect burst, which catches a missing writable SQLite temp directory. `tests/analytics_schema.py` needs a Python build whose SQLite treats the JSON functions as innocuous with `trusted_schema=OFF` (3.46 passes, 3.40 fails). Run it with the runner's Python. Upgrade tests in Rust use `store::open_database_at_version` to stop at an earlier schema before seeding data.
