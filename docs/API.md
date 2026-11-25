# API Contract

## 0.2 Secure Administration

The admin listener exposes `GET /healthz` and `GET /readyz` with status-only responses. The admin interface and its API use the configured `ADMIN_ORIGIN`. The public listener resolves enabled short links.

| Operation | Request and response |
|---|---|
| `GET /api/v1/auth/session` | Returns `setup_required` and `user`. An active session receives the username and CSRF token. |
| `POST /api/v1/auth/setup` | Accepts `token`, `username`, and `password`. Consumes the CLI claim and returns `201`, the username, CSRF token, and a session cookie. |
| `POST /api/v1/auth/login` | Accepts `username` and `password` and returns an active session. |
| `POST /api/v1/auth/recover` | Accepts the CLI recovery `token` and a new `password`. Revokes existing sessions and returns a replacement session. |
| `POST /api/v1/auth/password` and `POST /api/v1/auth/logout` | Require a browser session and CSRF token. Password change accepts `current_password` and `password`. Logout revokes the cookie and returns `204`. |
| `GET /api/v1/domains` | Returns the configured `hosts` visible to the caller and the local public port. |
| `GET /api/v1/links` | Accepts the optional `after_id` cursor. Returns `items` and `next_after_id` with up to 100 records per page. |
| `POST /api/v1/links` | Accepts `target` and optional `host`, `slug`, `title`, and `tags`. Returns `201` with the created link. A duplicate slug on the same host returns `409`. |
| `PATCH /api/v1/links/{id}` | Accepts any of `target`, `title`, `enabled`, and `tags`. A supplied tag list replaces the previous list. The published host and slug remain fixed. |
| `DELETE /api/v1/links/{id}` | Deletes a link and returns `204`. |
| `GET` and `POST /api/v1/keys` | Require a browser session. Listing returns key metadata. Creation accepts `name`, `scopes`, optional `host`, and optional `expires_at` in Unix seconds. The response contains `key` and the one-time `token`. |
| `DELETE /api/v1/keys/{id}` | Revokes the key and returns `204`. |
| `GET` or `HEAD /{slug}` | Returns `302` with the stored `Location` and `Cache-Control: no-store` for an enabled public link. The destination retains its stored query string and fragment. |

Link objects contain `id`, `host`, `slug`, `target`, `created_at`, `title`, `enabled`, and `tags`. Slugs use case-sensitive ASCII letters, digits, hyphens, underscores, and segment separators. Their maximum is 200 bytes. Destinations are absolute HTTP or HTTPS URIs up to 4,096 bytes, with valid percent encoding and credentials excluded. A link can carry up to 20 tags. Titles and tags use limits of 160 and 40 Unicode characters respectively.

An API key uses `Authorization: Bearer <token>`. A key can hold `read` for list operations, `write` for creation and edits, and `delete` for link deletion. An optional host restriction applies to both reads and changes. The browser supplies an exact `Origin` and `X-CSRF-Token` for session-authenticated changes. Errors use `application/problem+json` with `status`, `title`, and `code`. [Security design](SECURITY.md)

## Planned API

The JSON API under `/api/v1` runs on the admin listener. Same-origin browser clients use revocable sessions with CSRF protection for unsafe methods. Automation uses bearer keys with explicitly assigned permissions. A trusted reverse proxy terminates TLS. The versioned contract generates OpenAPI, and CI checks for unintended breaking changes. Swagger UI is an optional authenticated static interface.

| Resource | Planned operations |
|---|---|
| `/auth/setup`, `/auth/login`, `/auth/logout`, `/auth/session`, `/auth/password` | Setup authorization, sign-in, session status, revocation, and password changes |
| `/links`, `/links/{id}`, `/links/{id}/rules`, `/links/{id}/history` | Listing, filtering, search, preview, create, read, update, delete, rules, history, and revision restoration |
| `/links/bulk`, `/tags`, `/collections`, `/domains` | Bounded bulk actions, tag merges, organization, domain settings, and fallbacks |
| `/analytics/summary`, `/analytics/visits`, `/analytics/export` | Periods, dimensions, filtering, export, and controlled deletion |
| `/qr`, `/qr/templates` | Reproducible PNG and SVG generation and templates |
| `/monitoring/jobs`, `/monitoring/results`, `/link-pages` | Check jobs, result review, and optional link pages |
| `/import/jobs`, `/export/jobs`, `/backup/jobs`, `/backup/{id}/download`, `/backup/restores` | Native CSV and JSON import and export, backup and restore jobs, and downloads that recheck permissions |
| `/keys`, `/settings`, `/system` | Named scopes, one-time key display, rotation, settings, and diagnostics |

## Conventions

- Use stable cursor pagination with the ID as a tiebreaker and a capped page size. Define cursor reversal and invalidation for each sort order. Return UTC timestamps in RFC 3339 format.
- Enforce request schemas, field limits, and allowed values. Check field-level permissions before processing. API key scopes distinguish read, write, delete, and analytics access, with optional domain restrictions. Grant full access explicitly.
- Return `application/problem+json` with a typed problem code, status, correlation ID, and optional field paths. Exclude tokens, raw URLs, and internal stack traces. Use `201` with `Location` for creation, `202` with a job ID for queued work, `401` or `403` when access is denied, `409` for conflicts, `412` for stale revisions, `422` for semantically invalid input, and `429` with retry guidance when throttled.
- Support `Idempotency-Key` for operations such as creation and job submission that clients may retry. Bind each key to the caller, route, request hash, and retention window. Return `409` when a retry reuses a key with a different payload. Updates use a revision or the `If-Match` header.
- Rate-limit the admin API, with separate limits for sign-in and sensitive operations. Use an explicit CORS allowlist. Keep credentials out of query strings and browser local storage.
- Expose feature routes when their contracts are implemented. The public listener serves redirects and explicitly public link features. Keep internal health endpoints and OpenAPI on the admin listener.
- System diagnostics report the build, Git commit, start time, uptime, local database and visit data sizes, free capacity, and job and backup status. Where available, memory and CPU readings come from the process or cgroup. The admin UI labels the source and units. Metrics are optional and internal.
