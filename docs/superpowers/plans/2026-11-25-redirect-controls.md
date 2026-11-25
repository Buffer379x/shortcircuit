# Redirect controls implementation plan

**Goal.** Add editable domains and tags, deterministic redirect policies, durable visit limits, and local QR downloads while preserving existing short URLs.

1. Add versioned tables for link policies and domains. Preserve existing rows, exact destinations, and environment-configured hosts. Cover upgrades with the database tests.
2. Resolve an exact slug before trying path forwarding. Apply the stored time window, ordered rules, and optional query or path forwarding. Commit a reservation before returning a limited GET redirect. HEAD checks availability without consuming a visit.
3. Add admin domain and tag routes. Store only validated origins and fallback targets. Apply fallback redirects to the configured root and missing links on active hosts. Keep disabled hosts and blocked links unavailable.
4. Add scoped per-link policy and QR endpoints. Generate SVG and PNG locally from the configured public origin and the exact stored slug. Verify decoding independently.
5. Extend the embedded interface with domain and tag management, redirect policy editing, and QR preview. Keep the existing setup and link creation workflow operational.
6. Verify Rust, schema, frontend, Docker, and browser workflows through a GitHub pull request. Continue with analytics once redirect behavior and visit-limit tests are green.
