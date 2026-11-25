# API Contract

## 0.1 Foundation

The admin listener exposes `GET /healthz` and `GET /readyz` with status-only responses. Setting `ADMIN_TOKEN_FILE` enables `POST /api/v1/links` and `GET /api/v1/links` on the admin listener. Both routes require `Authorization: Bearer …`. An operator supplies the lab token as exactly 64 hexadecimal characters. The process keeps a SHA-256 digest for authentication. User sign-in is planned.

| Operation | Request and response |
|---|---|
| `POST /api/v1/links` | Accepts JSON with `target` and optional `host` and `slug`. Returns `201` with `id`, `host`, `slug`, `target`, and `created_at`. Returns `409` if the slug is already in use on that host. Omitting `host` selects the first configured host. Omitting `slug` generates a ten-character slug. |
| `GET /api/v1/links` | Accepts the optional `after_id` cursor. Returns JSON with `items` and `next_after_id`, with up to 100 records per page across the configured hosts. |
| `GET /{slug}` or `HEAD /{slug}` | Served on the public listener. Returns `302` with the stored `Location` and `Cache-Control: no-store`. The redirect preserves the stored query and fragment and ignores the incoming query string. Slugs are case-sensitive and may span multiple path segments. |

Manual slugs use ASCII letters, digits, hyphens, and underscores. Separate segments with a single slash. Slugs can be up to 200 bytes long. The create endpoint caps request bodies at 32 KiB. The public listener caps the combined path and query at 8,192 bytes. Destinations must be absolute HTTP or HTTPS URIs up to 4,096 bytes long, with valid percent encoding and no embedded credentials. The API validates the format and preserves the supplied spelling. Link API errors use `application/problem+json` with `status`, `title`, and `code`. Request examples appear in the [README](../README.md).

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
