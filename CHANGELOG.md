# Changelog

## Unreleased

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
