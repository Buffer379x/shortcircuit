# Project status

**November 25, 2026 · 0.1.0 in development.** Architecture and design are approved for implementation. Existing links are recreated manually.

| Area | Status |
|---|---|
| Rust foundation | Source is present for host-bound redirects, separate public and admin listeners, SQLite WAL with full synchronization, versioned schema, health probes, and optional authenticated link creation and listing. |
| Link creation | Supports custom case-sensitive slugs with optional segments and randomly generated 10-character slugs. The public listener reads destinations from stored links. |
| Development security | Loopback defaults, a bearer token for the development admin API, and body and URI limits. Production hardening and interactive sign-in are pending. |
| Database verification | Five schema tests passed with system SQLite and with SQLite 3.54.1 built from the bundled source package. |
| Build and container verification | GitHub Actions runs the Rust integration tests, Clippy, Rustfmt, and the container smoke test. Dependency versions are recorded in `Cargo.lock`, and the Docker build uses it. Docker Compose accepted the local configuration. [CI results](https://github.com/Buffer379x/shortcircuit/actions) |
| GitHub | [The repository](https://github.com/Buffer379x/shortcircuit) exists. The owner approved public source development in this repository. The owner set the MIT license in the initial commit. Release artifacts still require approval. |
| UI and branding | Nine preview routes, including sign-in, with English and German UI text, dark and light themes, and four static design renders. The production admin interface is planned for a later milestone. |
| Browser verification | Chromium failed to start in the development sandbox. The [capture instructions](../README.md) describe how to record screenshots and exercise the responsive layouts in a local browser. |
| Performance and release | The benchmark method and resource estimates are documented. Production measurements and operating approval are pending. |

[Core implementation plan](superpowers/plans/2026-11-25-core-foundation.md) · [Roadmap](ROADMAP.md) · [Decisions](DECISIONS.md)
