# API Contract

The [OpenAPI document](openapi.json) covers the implemented administrative endpoints. Signed-in administrators can also download it from `/api/v1/openapi.json`. The build checks the set of paths and methods against the application. [OpenAPI 3.1](https://spec.openapis.org/oas/v3.1.0.html)

## Current API

The admin listener exposes `GET /healthz` and `GET /readyz` with status-only responses. The admin interface and its API use the configured `ADMIN_ORIGIN`. The public listener resolves enabled short links.

| Operation | Request and response |
|---|---|
| `GET /api/v1/auth/session` | Returns `setup_required` and `user`. An active session receives the username and CSRF token. |
| `POST /api/v1/auth/setup` | Accepts `token`, `username`, and `password`. Consumes the CLI claim and returns `201`, the username, CSRF token, and a session cookie. |
| `POST /api/v1/auth/login` | Accepts `username` and `password` and returns an active session. |
| `POST /api/v1/auth/recover` | Accepts the CLI recovery `token` and a new `password`. Revokes existing sessions and returns a replacement session. |
| `POST /api/v1/auth/password` and `POST /api/v1/auth/logout` | Require a browser session and CSRF token. Password change accepts `current_password` and `password`. Logout revokes the cookie and returns `204`. |
| `GET /api/v1/domains` | Returns active `hosts`, an `origins` map for configured hosts visible to the caller, and the local public port. |
| `POST /api/v1/links/duplicates` | Admin session lookup for an exact stored destination. Accepts `target` and an optional positive `exclude_id`. Returns up to ten items newest first and `has_more`. |
| `GET /api/v1/links` | Accepts the optional `after_id` cursor. Returns `items` and `next_after_id` with up to 100 records per page. |
| `POST /api/v1/links/search` | Searches, filters, and sorts all links in the administrator's catalog. Requires a browser session, Origin and CSRF. Search terms and cursors are sent in the JSON body. |
| `POST /api/v1/links/bulk` | Applies one action atomically to 1 to 50 selected links. Requires a browser session, Origin, and CSRF. |
| `POST /api/v1/links/bulk/export` | Downloads the selected links as native JSON or CSV. Requires a browser session, Origin, and CSRF. |
| `POST /api/v1/links` | Accepts `target` and optional `host`, `slug`, `title`, `description`, `tags`, `enabled`, `analytics_enabled`, and `protection`. The protection object takes `one_time` and optional `password`. Both the link and protection commit together. Returns `201`, or `409` for a duplicate slug on the same host. |
| `PATCH /api/v1/links/{id}` | Accepts any of `target`, `title`, `description`, `enabled`, `tags`, and `analytics_enabled`. A supplied tag list replaces the previous list. The published host and slug remain fixed. |
| `DELETE /api/v1/links/{id}` | Deletes a link and returns `204`. |
| `GET` and `POST /api/v1/keys` | Require a browser session. Listing returns key metadata. Creation accepts `name`, `scopes`, optional `host`, and optional `expires_at` in Unix seconds. The response contains `key` and the one-time `token`. |
| `DELETE /api/v1/keys/{id}` | Revokes the key and returns `204`. |
| `GET` and `POST /api/v1/catalog/domains` | Browser-session catalog. GET returns `items`. POST sets `host`, `origin`, `enabled`, and optional `root_target` and `missing_target`. Domain origins contain the same host and an optional port. |
| `GET`, `PATCH`, and `DELETE /api/v1/catalog/tags` | Browser-session catalog. GET returns tag names and link counts. PATCH renames or merges using `name` and `new_name`. DELETE uses `name` and removes the tag from its assigned links. Mutations return `204`. |
| `GET` and `PATCH /api/v1/links/{id}/policy` | Reads or replaces the redirect settings. Writable keys are `status`, `starts_at`, `expires_at`, `max_visits`, `forward_query`, `forward_path`, and `rules`. Read responses include `visits_used`. Omitted PATCH fields take their defaults. |
| `GET` and `PATCH /api/v1/links/{id}/protection` | Returns `password_required`, `one_time`, and `consumed`. PATCH takes `one_time` and optional `password` or `remove_password: true`. Omit both password fields to keep the current password. A permanent redirect or an attempt to remove a consumed one-time restriction returns `409`. |
| `GET` and `DELETE /api/v1/links/{id}/history` | Browser-session access. GET returns current editable settings and up to 25 recent revisions. DELETE clears the link's history and returns `204`. |
| `POST /api/v1/links/{id}/history/{revision}/restore` | Browser-session access with CSRF protection. Restores one metadata or redirect-policy revision and returns `204`. Informational protection entries return `409`. |
| `GET /api/v1/links/{id}/qr` | Renders the configured public short URL as SVG or PNG. Supports image size, colors, transparency, error correction, quiet zone, and an optional outline. Requires a read scope or browser session. |
| `POST /api/v1/links/{id}/qr/logo` | Renders the same short URL with a centered logo from a raw PNG request body. Requires a browser session, the exact admin Origin, and CSRF protection. |
| `GET` and `POST /api/v1/qr/templates` | Browser-session access. GET returns `items`, newest first. POST creates a QR design template and returns it with an `id`. |
| `DELETE /api/v1/qr/templates/{id}` | Removes a saved template and returns `204`. Requires a browser session. |
| `GET` or `HEAD /{slug}` | Resolves the link's redirect policy. The default is `302` with the stored `Location` and `Cache-Control: no-store`. A protected link returns a confirmation page for GET or an empty response for HEAD. |
| `POST /{slug}` | Accepts a protected-link confirmation from the exact public origin. A successful confirmation returns `303` to continue with GET. |

Link objects contain `id`, `host`, `slug`, `target`, `created_at`, `title`, `description`, `enabled`, `analytics_enabled`, and `tags`. Slugs use case-sensitive ASCII letters, digits, hyphens, underscores, and segment separators. Their maximum is 200 bytes. With no custom slug, the server uses its configured generated length (10 by default). Destinations are absolute HTTP or HTTPS URIs up to 4,096 bytes, with valid percent encoding and credentials excluded. A link can carry up to 20 tags. Titles and tags use limits of 160 and 40 Unicode characters respectively. Descriptions support up to 500 Unicode characters and line feeds after trimming. Send an empty string to clear a description. Link creation accepts optional `enabled` and `analytics_enabled` flags that default to true.

An API key uses `Authorization: Bearer <token>`. A key can hold `read` for list operations, `write` for creation and edits, and `delete` for link deletion. An optional host restriction applies to both reads and changes. The browser supplies an exact `Origin` and `X-CSRF-Token` for session-authenticated changes. Errors use `application/problem+json` with `status`, `title`, and `code`. [Security design](SECURITY.md)

### Catalog search

The search body accepts optional `query`, `host`, `enabled`, `view`, `tags`, `tag_match`, `sort`, and `after` fields. The query is a case-insensitive Unicode substring up to 4,096 bytes and matches titles, descriptions, slugs, domains, short URLs, destinations, and tag names. The domain, enabled, view, and tag conditions all apply together.

Use `tags` for up to 20 exact tag names. The `tag_match` value is `all` by default. Use `any` to require at least one of the selected tags. The legacy `tag` field still accepts one tag. Supply either `tag` or a nonempty `tags` array.

The `view` value is `all` by default. The browser uses `unarchived` for its main list. Use `archived` for the archive or `favorites` for unarchived favorites. Search items include the `favorite` and `archived` flags and a `selection_key`.

Search items also report `recorded_redirects` from retained daily analytics, `last_redirect_date` as a UTC date, and `last_redirect_at` in Unix seconds if detail is available for that date. A link with no recorded redirects has a count of zero and null dates. Tracking preferences, retention, and deletion apply to these values.

`PATCH /api/v1/links/{id}/organization` accepts a browser administrator session, the `key` from the search item's `selection_key`, and one or both Boolean flags. The response returns both flags. A missing or replaced row returns `409`. Archived links continue to resolve according to their current redirect policy. A full SQLite backup preserves the organization flags.

Supported sort values are `newest` (default), `oldest`, `title_asc`, `title_desc`, `slug_asc`, `slug_desc`, `host_asc`, `host_desc`, `target_asc`, `target_desc`, `enabled_first`, and `disabled_first`. Each response includes up to 100 `items` and `next_cursor`. Send the returned cursor as `after` with the same filters and sort to continue. A null cursor means the search is complete. The cursor carries its sort key and the link ID, so deleting the anchor link does not prevent paging. Changing filters or sort starts a new search.

### Bulk actions

Use `links: [{ "id": 1, "key": "<selection_key>" }]` to address selected rows. Copy each key from the search result. A batch accepts 1 to 50 different IDs. A missing, replaced, or out-of-catalog row returns `409`. The server returns `422` if the request is malformed at the field level, a link would exceed 20 tags, or an export would exceed the import size limit.

The `change` object supports `{"action":"set_enabled","enabled":true}`, `{"action":"add_tags","tags":["docs"]}`, `{"action":"remove_tags","tags":["docs"]}`, and `{"action":"delete","confirmed":true}`. A successful edit returns the selected links in `items`. A successful deletion returns their IDs in `deleted_ids`. Changes to existing metadata are recorded in link history. The service logs the number of links deleted after the transaction commits.

The export body uses `links` and `format` (`json` or `csv`). Exports use the native import format. Protected links are exported as disabled, and CSV fields use the same spreadsheet escaping as other native exports.

### Link history

Each revision includes an actor, a UTC timestamp in Unix seconds, an action, and the previous configuration for that category. The actor identifies the administrator username or the numeric ID of a scoped API key. Direct link edits record metadata, tags, and redirect policy changes. Protection changes record an informational entry. Catalog-wide tag operations apply to the catalog. The administration interface exports the fetched history as JSON.

A restore applies only the selected category. Metadata includes destination, title, description, tags, enabled state, and analytics preference. An older revision with no description field preserves the current description. A policy restore keeps the current visit counter and revalidates the configuration against current protection. A successful restore that changes settings records the previous state. The short URL and access protection persist across these operations.

### Styled QR codes

The `/api/v1/links/{id}/qr` endpoint accepts the following query options. The QR code contains the configured public origin and the exact stored slug.

| Option | Values | Default |
|---|---|---|
| `format` | `svg` or `png` | `svg` |
| `size` | 128 to 1,024 pixels per side | `384` |
| `foreground` | Six hexadecimal sRGB digits | `000000` |
| `background` | Six hexadecimal sRGB digits | `ffffff` |
| `transparent` | `true` or `false` | `false` |
| `correction` | `L`, `M`, `Q`, or `H` | `M` |
| `quiet_zone` | 4 to 12 modules on each side | `4` |
| `frame` | `none` or `outline` | `none` |

ShortCircuit requires dark modules and a contrast ratio of at least 4.5:1 as a scan-readability guard. A transparent export uses the background color as the contrast reference and leaves its light cells and quiet zone transparent. Place it on a background with the same or better contrast. A larger correction level or quiet zone can require a larger image for the same link. Check the final printed size and background with the intended scanner. [QR quiet-zone guidance](https://www.qrcode.com/en/howto/code.html) · [QR error correction](https://www.qrcode.com/en/about/error_correction.html) · [Relative luminance](https://www.w3.org/WAI/WCAG21/Understanding/contrast-minimum.html)

The outline uses the foreground color. It reserves three modules per side for the line and clear space around it, in addition to the configured quiet zone. The image retains its requested dimensions. An outline can reduce the size available for each QR module, so verify the final output at the intended display or print size.

The logo endpoint uses the same query options and defaults to error correction `H`. It accepts a single static PNG up to 256 KiB and 512 pixels on either side as `image/png`. The renderer fits the image on a light center patch, preserves its aspect ratio and checks that the resulting pixels decode to the exact link URL. It returns `422` if it cannot verify the result. The SVG uses generated color rectangles, and PNG and SVG share the same geometry. The server discards the upload after the request. The browser retains the selection for the current dialog.

To save a QR template, send a JSON object with `name`, `size`, `foreground`, `background`, `transparent`, `correction`, `quiet_zone`, and optionally `frame`. Templates default to `none` for the frame. The same render setting limits apply, and saved colors use lowercase hexadecimal. Names are trimmed, case-sensitive, and limited to 64 characters. A duplicate name or a full catalog of 32 templates returns `409`. Applying a template in the QR dialog sets its style on the current link and uses that link's public short URL. Resolution requirements still depend on the encoded URL.

### Analytics

Analytics endpoints require a browser administrator session. Changes require the standard CSRF token and Origin header.

| Operation | Request and response |
|---|---|
| `GET /api/v1/analytics/summary?days=30` | Accepts 1 to 365 UTC days. Returns successful redirects, blocked links, missing links, visitor classes for redirects, the preceding period's redirect count, a daily series, up to ten top links, and a process-local dropped-events count. |
| `GET` and `PATCH /api/v1/analytics/settings` | Reads or replaces the settings `enabled`, `referrers`, `classify`, `honor_dnt`, `detail_days`, and `aggregate_days`. Retention is between 1 and 3650 days. The aggregate period must cover at least the detail period. |
| `GET /api/v1/analytics/events` | Lists up to 100 retained events newest first. Accepts the optional `before_id` cursor. Returns `items` and `next_before_id`. |
| `DELETE /api/v1/analytics/events/{id}` | Deletes one retained event and adjusts its daily count atomically. Returns `204`. |
| `DELETE /api/v1/analytics/data` | Deletes details, every UTC-day rollup, and queued events from the previous generation. Returns `204`. |

### Native link exchange

`GET /api/v1/exchange/links` returns version 1, a bounded page of items, and `next_after_id`. Use that value as `after_id` to continue. The exported link fields are `host`, `slug`, `target`, `title`, `tags`, `enabled`, and `analytics_enabled`. A nonempty `description` is included as an optional extension to version 1. Import treats an absent description as empty. Native JSON and CSV exports mark protected links as disabled. Restore their protection settings from a database backup or configure protection on the imported copy before enabling it.

`POST /api/v1/exchange/links` accepts `version: 1`, a Boolean `preview`, and up to 100 `items` within the request size limit. Existing domains must be active. The server returns per-link `create`, `unchanged`, or `conflict` results. An apply rechecks conflicts in the same transaction as the insert. If any item conflicts, the response is `409` and all existing links remain intact. This format transfers basic link data. A full snapshot includes settings, policies, and visit history.

`GET /api/v1/exchange/links.csv` downloads one CSV page. Send `after_id` with the value from the previous response's `X-Next-After-Id` header to continue. The same authenticated endpoint accepts POST with `Content-Type: text/csv` and defaults to preview. Pass `preview=false` to apply an import. Responses to POST use the same JSON report as the native JSON endpoint.

CSV uses the columns `version,host,slug,target,title,tags,enabled,analytics_enabled`. It accepts a final `description` column and exports it on pages containing descriptions. Each row uses version 1, a JSON array in the tags column, and lowercase Boolean values. The export quotes fields and prefixes spreadsheet-sensitive values. Use the original export for an exact round trip and use JSON for data exchange through tools that resave spreadsheets. [OWASP CSV injection guidance](https://community.owasp.org/attacks/CSV_Injection)

### Public redirect behavior

An exact path takes precedence. When the exact path is missing, the longest stored prefix that allows extra paths can handle the request. Disabled or out-of-window links and exhausted visit limits return `404`. Unknown paths and the root path can use their configured domain fallback and otherwise return `404`. Disabled domains return `404` and remain editable on the admin listener.

A policy supports `301`, `302`, `307`, or `308`. Start and expiration are inclusive and exclusive respectively, expressed as UTC Unix seconds. Rules, time windows, and visit limits require a temporary status. Browsers or intermediaries can retain permanent redirects. A limited link increments its counter atomically for each successful GET while the limit is active. HEAD checks the same conditions without incrementing it. Editing or pausing a limit preserves its counter.

A rule contains a stored `target` and 1 to 8 `conditions`. The first of up to 16 rules whose conditions all match supplies the destination. Without a match, the link uses its stored target. Condition types are `language`, `device`, `os`, `browser`, and `query`. All have a `value`. Query conditions also have a `key`. Client characteristics are estimates from request headers. Language uses the preferred valid language from `Accept-Language`.

Query and extra path forwarding are both off by default. Query forwarding appends the incoming raw query before the stored fragment. Extra path forwarding appends validated path segments before the stored query and fragment. Leave forwarding off for destinations whose parameters or path are signed.

Protected links require a temporary redirect policy and an HTTPS public origin, with an HTTP loopback exception for development. Passwords require 12 or more characters and at most 1,024 UTF-8 bytes. The confirmation is valid for five minutes and bound to the public origin, path, query, and protection revision. Password checks are rate-limited. After one confirmed access, a one-time link returns `404`; create a new link to issue another one-time access. A protected short link controls access through ShortCircuit. The destination is responsible for its own access control.

## Planned API

The JSON API under `/api/v1` runs on the admin listener. Same-origin browser clients use revocable sessions with CSRF protection for unsafe methods. Automation uses bearer keys with explicitly assigned permissions. A trusted reverse proxy terminates TLS. New operations require updates to the versioned contract. Swagger UI is an optional authenticated static interface.

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
