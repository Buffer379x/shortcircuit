# Core foundation implementation plan

**Goal.** Create links through a locally protected API and issue redirects for explicitly configured hosts. Use permanent SQLite storage and separate public and admin listeners.

**Design.** Rust 1.100, Axum, Tokio, SQLx with bundled SQLite, Serde, and Tracing. Existing short URLs can be recreated by setting their original host and slug. [Architecture](../../ARCHITECTURE.md)

1. Add a versioned SQLite schema with strict tables and unique slugs per host. Verify the SQLite version before opening the persistent WAL. Use WAL, full synchronization, and a small pool. Exercise schema integrity in `tests/schema.py`.
2. Cover the HTTP contract in `tests/http.rs`. The public listener resolves canonical slugs on configured hosts. GET and HEAD return `302`, the stored `Location`, and `Cache-Control: no-store`. Incoming queries and extra path segments do not alter the destination. The admin listener provides health and readiness. Setting a bearer token file enables authenticated link creation and listing. Exercise persistence by reopening an isolated database.
3. Limit inputs and database reads, return consistent error codes, and support graceful shutdown. Pin the Rust toolchain and define a non-root container and CI. Run formatting, Clippy, Rust tests, and the container smoke test where the toolchain and daemon are available.
4. Keep the local start instructions and verification status current. Use the owner-approved public GitHub repository for source development. Use synthetic links for development and testing.
