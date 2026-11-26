# Changelog

## Unreleased

- Expand analytics with custom UTC dates, host filters, client and traffic breakdowns, domain and tag rankings, and a JSON report download.
- Download verified SQLite snapshots and prepare offline restore from the administration interface.
- Add API key rotation, an administrative audit trail, configurable rate limits, and an explicit trusted-proxy policy.
- Add opt-in checks for stored targets and configured public domains, with bounded retries and optional HTTPS health notifications.

- Add domain crawler preferences and per-link indexing hints.
- Simulate the current redirect policy draft using sample request details.

- Add optional tag colors and descriptions, preserve details during tag merges, and suggest existing tags in the link form.

- Add link descriptions to the catalog, revision history, and native JSON and CSV exchange.
- Create disabled duplicates from the catalog with editable metadata and a fresh shortcode.
- Show recorded redirects and the last recorded use in the catalog.

- Add favorites and an archive view to the link catalog. Archiving preserves the public redirect and its access settings.
- Filter the link catalog by up to 20 tags, matching all or any of them.
- Set the length of automatically generated shortcodes with `SHORTCODE_LENGTH`.

## 0.6.0-preview.2

- Apply status or tag changes to selected links, export the selection as JSON or CSV, or confirm atomic deletion.
- Search, filter, and sort all links in the catalog with stable cursor pagination and keep search terms in the request body.
- Add optional PNG logos to the QR dialog with verified output in PNG and SVG.
- Add an optional QR outline outside the quiet zone. The preview, PNG, SVG, and saved templates support the same setting.
- Save and reuse named QR design templates across links. Include templates in SQLite backups.
- Add QR Studio color, transparency, error correction, and quiet-zone controls with contrast validation and optional PNG clipboard copy.
- Add link history with comparison, category-specific restoration, JSON export, and per-link deletion.
- Add password-protected links and confirmed, atomically consumed one-time links. Native link exports keep protected entries disabled when imported.

## 0.6.0-preview.1

- Add an Unraid Docker Manager template that pulls the versioned preview image from GitHub Container Registry and initializes a persistent Docker volume.

- Document the implemented management endpoints in OpenAPI 3.1 and serve the specification on the admin listener.

- Add native CSV exchange with bounded pages, spreadsheet-oriented escaping, and transactional import.

- Add optional scheduled backups with independent retention.

- Add exact destination lookup and a local preview for removing tracking parameters.

- Add verified local backups and offline restore with a rollback database.

- Add UTC analytics reports, coarse visitor classification, privacy preferences, and data retention and deletion.

- Add managed domains and fallback URLs, the tag catalog, redirect rules and visit limits, and local QR exports.

- Add one-time CLI setup and recovery, Argon2id passwords, revocable sessions, and scoped API keys.
- Add the embedded Svelte admin interface with English and German copy, theme settings, and local assets.
- Add link editing, enable and disable, deletion, tags, and JSON export of displayed results.
- Preserve existing links during database schema upgrades.

- Add the Rust foundation with Axum, SQLite, separate public and admin listeners, and an optional token-protected link API.
- Add a non-root Dockerfile, a local Compose profile, and a CI workflow.
- Record the proposed architecture, data model, threat model, and benchmark plan.
- Add the local design preview and initial branding assets.
