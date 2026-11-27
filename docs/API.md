# API Contract

The [OpenAPI document](openapi.json) covers the implemented administrative endpoints. Signed-in administrators can also download it from `/api/v1/openapi.json`. The build checks the set of paths and methods against the application. [OpenAPI 3.1](https://spec.openapis.org/oas/v3.1.0.html)

## Current API

The admin listener exposes `GET /healthz` and `GET /readyz` with status-only responses. The admin interface and its API use the configured `ADMIN_ORIGIN`. Any other Host or port returns `404`: `/api/` routes keep the `application/problem+json` body with code `not_found`, unknown `/api/` paths stay empty, and other paths get a small static HTML page in English or German that says to open the configured administration address without naming it. HEAD responses have no body. The probes answer on any address. The public listener resolves enabled short links.

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
| `GET /api/v1/links/{id}` | Returns one link with the same fields as a catalog search item. Requires a browser session or a key with the `read` scope. A link outside a key's host restriction returns `404`. Without the `analytics` scope, a key receives no traffic fields. |
| `PATCH /api/v1/links/{id}` | Accepts any of `target`, `title`, `description`, `enabled`, `tags`, and `analytics_enabled`. A supplied tag list replaces the previous list. The published host and slug remain fixed. |
| `DELETE /api/v1/links/{id}` | Deletes a link and returns `204`. |
| `GET` and `POST /api/v1/keys` | Require a browser session. Listing returns key metadata. Creation accepts `name`, `scopes`, optional `host`, and optional `expires_at` in Unix seconds. The response contains `key` and the one-time `token`. |
| `DELETE /api/v1/keys/{id}` | Revokes the key and returns `204`. |
| `GET` and `POST /api/v1/catalog/domains` | Browser-session catalog. GET returns `items`. POST sets `host`, `origin`, `enabled`, and optional `root_target`, `missing_target`, and `block_crawlers`. Omitted crawler preferences on existing domains preserve the saved value. Domain origins contain the same host and an optional port. |
| `GET /api/v1/catalog/domains/deletion` and `DELETE /api/v1/catalog/domains` | Browser-session preview and deletion of a domain. See [Deleting a domain](#deleting-a-domain). |
| `GET`, `PATCH`, and `DELETE /api/v1/catalog/tags` | Browser-session catalog. GET returns tag names, link counts, and any saved color or description. PATCH renames or merges using `name` and `new_name`. DELETE uses `name` and removes the tag and its assignments. Mutations return `204`. |
| `PATCH /api/v1/catalog/tags/details` | Replaces saved details for an existing tag. The body contains `name`, optional `color` as a six-digit hex color with a leading `#`, and an optional plain-text `description` of up to 500 Unicode characters. Use null or omit the color to clear it. Omit the description or send an empty string to clear it. Returns `204`. |
| `GET` and `PATCH /api/v1/links/{id}/policy` | Reads or replaces the redirect settings. Writable keys are `status`, `starts_at`, `expires_at`, `max_visits`, `forward_query`, `forward_path`, `noindex`, and `rules`. Read responses include `visits_used`. Omitting `noindex` preserves its saved value. Other omitted PATCH fields take their defaults. |
| `GET` and `PATCH /api/v1/links/{id}/protection` | Returns `password_required`, `one_time`, and `consumed`. PATCH takes `one_time` and optional `password` or `remove_password: true`. Omit both password fields to keep the current password. A permanent redirect or an attempt to remove a consumed one-time restriction returns `409`. |
| `GET` and `DELETE /api/v1/links/{id}/history` | Browser-session access. GET returns current editable settings and up to 25 recent revisions. DELETE clears the link's history and returns `204`. |
| `POST /api/v1/links/{id}/policy/simulate` | Evaluates draft rules and forwarding against sample request fields. Returns the destination, configured status, and one-based rule number or null for the default destination. Requires a browser session with CSRF or a scoped write key. |
| `POST /api/v1/links/{id}/history/{revision}/restore` | Browser-session access with CSRF protection. Restores one metadata or redirect-policy revision and returns `204`. Informational protection entries return `409`. |
| `GET /api/v1/links/{id}/qr` | Renders the configured public short URL as SVG or PNG. Supports image size, colors, transparency, error correction, quiet zone, and an optional outline. Requires a read scope or browser session. |
| `POST /api/v1/links/{id}/qr/logo` | Renders the same short URL with a centered logo from a raw PNG request body. Requires a browser session, the exact admin Origin, and CSRF protection. |
| `GET` and `POST /api/v1/qr/templates` | Browser-session access. GET returns `items`, newest first. POST creates a QR design template and returns it with an `id`. |
| `DELETE /api/v1/qr/templates/{id}` | Removes a saved template and returns `204`. Requires a browser session. |
| `GET` or `HEAD /robots.txt` | Returns the crawler preference and headers for an active public host. |
| `GET` or `HEAD /{slug}` | Resolves the link's redirect policy. The default is `302` with the stored `Location` and `Cache-Control: no-store`. A protected link returns a confirmation page for GET or an empty response for HEAD. |
| `POST /{slug}` | Accepts a protected-link confirmation from the exact public origin. A successful confirmation returns `303` to continue with GET. |

Link objects contain `id`, `host`, `slug`, `target`, `created_at`, `title`, `description`, `enabled`, `analytics_enabled`, and `tags`. Slugs use case-sensitive ASCII letters, digits, hyphens, underscores, and segment separators. Their maximum is 200 bytes. With no custom slug, the server uses its configured generated length (10 by default). Destinations are absolute HTTP or HTTPS URIs up to 4,096 bytes, with valid percent encoding and credentials excluded. A link can carry up to 20 tags. Titles and tags use limits of 160 and 40 Unicode characters respectively. Descriptions support up to 500 Unicode characters and line feeds after trimming. Send an empty string to clear a description. Link creation accepts optional `enabled` and `analytics_enabled` flags that default to true.

An API key uses `Authorization: Bearer <token>`. A key can hold `read` for list operations, `write` for creation and edits, `delete` for link deletion, and `analytics` for read-only analytics reports. Link scopes do not include analytics access, and the analytics scope grants no link access. An optional host restriction applies to reads, changes, and analytics reports. The browser supplies an exact `Origin` and `X-CSRF-Token` for session-authenticated changes. Errors use `application/problem+json` with `status`, `title`, and `code`. Import problems can add `row` and `field` to locate the first failing item. [Security design](SECURITY.md)

### Tags

Tag details keep a tag in the catalog even when its link count is zero. A merge keeps the destination's existing color and description and fills any empty fields from the source. Native link exchange keeps tag names. A full SQLite backup preserves the saved tag details.

### Catalog search

The search body accepts optional `query`, `host`, `enabled`, `state`, `view`, `tags`, `tag_match`, `sort`, and `after` fields. The query is a case-insensitive Unicode substring up to 4,096 bytes and matches titles, descriptions, slugs, domains, short URLs, destinations, and tag names. The domain, enabled, state, view, and tag conditions all apply together.

The `state` filter takes one of `active`, `scheduled`, `expired`, `disabled`, or `exhausted` and keeps the links whose item `state` (below) has that value when the page is read. Any other value returns `422` with code `invalid_request`.

Use `tags` for up to 20 exact tag names. The `tag_match` value is `all` by default. Use `any` to require at least one of the selected tags. The legacy `tag` field still accepts one tag. Supply either `tag` or a nonempty `tags` array.

The `view` value is `all` by default. The browser uses `unarchived` for its main list. Use `archived` for the archive or `favorites` for unarchived favorites. Search items include the `favorite` and `archived` flags and a `selection_key`.

Search items also report `recorded_redirects` from retained daily analytics, `last_redirect_date` as a UTC date, and `last_redirect_at` in Unix seconds if detail is available for that date. A link with no recorded redirects has a count of zero and null dates. Tracking preferences, retention, and deletion apply to these values.

Each item also describes the link's live behavior and recent traffic:

| Field | Meaning |
|---|---|
| `state` | `active`, `scheduled`, `expired`, `disabled`, or `exhausted`, using the same classification as the Overview. A disabled link or domain is `disabled`. A consumed one-time link or a reached visit limit is `exhausted`. Archiving is reported by `archived` and does not change the state. |
| `starts_at`, `expires_at` | The policy window in Unix seconds, or null. |
| `max_visits`, `visits_used` | The visit limit or null, and the visits counted against it (zero without a policy). |
| `protection` | `password` when a password is set, even if access is also one-time, `one_time` for one-time access without a password, otherwise null. |
| `rule_count` | Number of dynamic redirect rules. |
| `visits_30d` | Successful redirects of every visitor class in the last 30 UTC days, including today. Null when analytics is off for the link or the installation. |
| `trend` | 14 daily redirect counts for the last 14 UTC days, oldest first and ending today. Empty when analytics is off for the link or the installation. |
| `monitor` | `status`, `priority`, `last_checked`, `http_status`, `elapsed_ms`, and `failure` from the last health check, or null when the link is not enrolled. |

Each search page reads these values with a fixed number of indexed queries, independent of the number of items.

`PATCH /api/v1/links/{id}/organization` accepts a browser administrator session, the `key` from the search item's `selection_key`, and one or both Boolean flags. The response returns both flags. A missing or replaced row returns `409`. Archived links continue to resolve according to their current redirect policy. A full SQLite backup preserves the organization flags.

Supported sort values are `newest` (default), `oldest`, `title_asc`, `title_desc`, `slug_asc`, `slug_desc`, `host_asc`, `host_desc`, `target_asc`, `target_desc`, `enabled_first`, `disabled_first`, `visits_desc`, `visits_asc`, `last_visit_desc`, and `last_visit_asc`. Each response includes up to 100 `items` and `next_cursor`. Send the returned cursor as `after` with the same filters and sort to continue. A null cursor means the search is complete. The cursor carries its sort key and the link ID, so deleting the anchor link does not prevent paging. Changing filters or sort starts a new search. A cursor whose `sort` differs from the request, or whose key is unusable, returns `422` with code `invalid_link_cursor`.

The traffic sorts order by `visits_30d`, or by the last recorded redirect: `last_redirect_at`, or the start of `last_redirect_date` when that day's detail is no longer retained. Equal values are ordered by link ID in the same direction. Links without a value, meaning untracked links for visits and links with no recorded redirect for the last visit, come last in both directions and are ordered by ID among themselves. Their cursor key is a decimal integer; treat it as opaque. Values are read when each page is requested, so a link whose traffic changes between pages can move past the cursor. Traffic sorts read analytics, which only browser sessions may do here: API keys are refused with `401` on this endpoint whatever their scopes. Each traffic-sorted page reads one indexed value per matching link, so its cost grows with the catalog size like the other sorts.

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

The summary and report download accept a browser administrator session or an API key with the `analytics` scope. Settings, visit details, and deletion require a browser administrator session. Changes require the standard CSRF token and Origin header.

A host-bound analytics key always reports on its own host. Omitting `host` selects that host, and naming another host returns `403`. Totals, the previous period, the daily series, rankings, and breakdowns all use the same restriction. The process-wide `dropped` count has no host and is `null` for a host-bound key.

The summary and report download accept `link=<id>`. The link must exist within the report's host, which is the key's host restriction or the requested `host`. An unknown link, or one on another host, returns `404`. A per-link report includes the link's blocked requests, has no root path requests or missing paths, and keeps the process-wide `dropped` count.

| Operation | Request and response |
|---|---|
| `GET /api/v1/analytics/summary?days=30` | Accepts 1 to 365 UTC days. Returns successful redirects, blocked links, missing links, root path requests with and without a root destination, visitor classes for redirects, the preceding period's redirect count, a daily series, up to ten top links, and a process-local dropped-events count. The optional `link` parameter restricts every part of the report to one link ID and is echoed as `link`. |
| `GET` and `PATCH /api/v1/analytics/settings` | Reads or replaces the settings `enabled`, `referrers`, `classify`, `honor_dnt`, `detail_days`, and `aggregate_days`. Retention is between 1 and 3650 days. The aggregate period must cover at least the detail period. |
| `GET /api/v1/analytics/events` | Lists up to 100 retained events newest first. Accepts the optional `before_id` cursor. Returns `items` and `next_before_id`. |
| `DELETE /api/v1/analytics/events/{id}` | Deletes one retained event and adjusts its daily count atomically. Returns `204`. |
| `DELETE /api/v1/analytics/data` | Deletes details, every UTC-day rollup, and queued events from the previous generation. Returns `204`. |

### Deleting a domain

Both calls require a browser session. API keys are refused. `GET /api/v1/catalog/domains/deletion?host=…[&target=…]` changes nothing. It returns `links` (every link on the host, archived and disabled ones included), `managed` (saved settings exist), `environment` (the host is in `PUBLIC_HOSTS`), `api_keys` (unexpired keys restricted to the host), and `targets` (other active domains). With `target` it adds `move` with the `conflicts` total and up to 20 `conflicting_slugs`, in slug order.

`DELETE /api/v1/catalog/domains` takes `host`, `links` (`move`, `delete`, or `none`), `expected_links` from the preview, `target` for `move`, and `confirm` for `delete`. `confirm` must be the host name. `none` requires `expected_links: 0`. One `BEGIN IMMEDIATE` transaction rechecks the count and the conflicts, changes the links, removes the saved settings, and writes a `domain_delete` audit entry whose `details` hold `host`, `links`, `target`, `count`, and `environment`. The response reports `moved`, `deleted`, and `environment`.

- **Move** changes only the links' host. IDs, tags, history, protection, redirect policies and rules, visit counters, and health checks stay. A pending or settled title lookup is cleared, as with any host change. Recorded analytics keep the host they were recorded under, so domain totals for the old host remain historical. The target must be another active domain. Destinations do not change, so the destination policy has nothing new to check and is still enforced on redirects. Short URLs and printed QR codes for the old host stop working.
- **Delete** removes the links exactly as bulk delete does: their tags, history, protection, policies, health checks, title lookups, visit events, and daily totals go with them. Domain-level analytics without a link and settled notifications stay.
- **`PUBLIC_HOSTS`**: such a host can be deleted. Its saved settings and links are removed, and it stays served with default settings (enabled, `https` origin, no fallbacks) until it is removed from the environment. Disable it instead to stop serving it immediately. Keys restricted to it keep working on it.
- A host outside `PUBLIC_HOSTS` with unexpired restricted API keys returns `409` `domain_has_api_keys`. Revoke the keys first, so that adding the same host later does not reactivate them. The admin listener does not depend on public domains, so the admin origin's host needs no special rule.

Errors: `404` for an unknown host. `409` `domain_deletion_changed` when the count differs from `expected_links`, and `409` `domain_move_conflict` when slugs exist on the target. `422` `invalid_host`, `invalid_domain_deletion`, `invalid_move_target`, or `domain_delete_confirmation`. Nothing changes on any error.

### Destination policy

The destination policy is off by default and has no rules. `GET /api/v1/destination-policy` returns `enabled`, the normalized `rules`, and the `impact` on saved destinations. `PATCH` replaces the whole policy in one transaction and records an audit entry. `POST /api/v1/destination-policy/preview` normalizes a draft and reports its impact without saving it. All three require a browser administrator session. Changes require Origin and CSRF. Bodies can use up to 64 KiB.

A rule has a `host`, an `action` of `allow` or `block`, and an optional `subdomains` flag. The server trims the host, converts Unicode names with IDNA, lowercases it, and removes one trailing dot. Wildcards, schemes, ports, and paths are rejected. `subdomains: true` matches the host and every name below it, and applies only to hostnames. IP addresses match exactly. A host can appear once with the same `subdomains` value. The policy holds up to 200 rules.

Evaluation uses the host that the URL parser derives from the stored destination, including percent-decoding and numeric IPv4 forms. The stored destination text is never changed.

1. A matching `block` rule refuses the destination, regardless of any matching `allow` rule.
2. Otherwise, if at least one `allow` rule exists, the destination must match an `allow` rule.
3. Otherwise the destination is permitted. An enabled policy without rules permits every destination.

While the policy is enabled, writes that introduce a refused destination return `422` with code `destination_not_allowed`. This covers link creation and edits, redirect rules, domain root and missing-path destinations, native JSON and CSV imports, duplicates created through the link form, and history restores. A write that keeps an already saved destination is accepted, so unrelated edits to an affected link still work. The public listener checks the destination it selects, including rule destinations and domain fallbacks, before it shows a confirmation form or reserves a visit. A refused redirect returns `404`, counts as `blocked` in analytics, and leaves visit counters and one-time access unchanged. A refused fallback behaves like a missing fallback. The redirect simulator reports the result as `permitted`.

### Public pages

Settings > Public pages replaces the missing-link page (which also serves inactive links), the unknown or disabled host page, the protected-link password page, and the one-time confirmation page, separately for English and German. A page and language without an entry uses the built-in page. The admin listener's own address notice is not configurable. Every route below requires a browser administrator session; a request with an `Authorization` header is refused with `401`, so API keys can neither read nor change public pages. Changes require Origin and CSRF. Bodies can use up to 2 MiB.

- `GET /api/v1/settings/public-pages` returns the configured `pages`, the `builtin` copy of every page and language, and the `limits`.
- `PATCH /api/v1/settings/public-pages` saves one draft and records `public_pages_update`. The response holds the stored page and the `removed` categories.
- `POST /api/v1/settings/public-pages/reset` takes `{"page", "lang"}` or `{"all": true}`, returns the affected pages to the built-in page, and records `public_pages_reset`.
- `POST /api/v1/settings/public-pages/preview` renders a draft without saving it and returns a `url`. `GET` on that URL serves the page for ten minutes with the public status, markup, and Content Security Policy, except `frame-ancestors 'self'` and `X-Frame-Options: SAMEORIGIN`. Previews use the first public host, a sample root destination, and a placeholder challenge that cannot confirm a link.
- `POST /api/v1/settings/public-pages/import/validate` checks a template file, renders a preview of every entry, and stores nothing. `POST /api/v1/settings/public-pages/import` replaces all pages with the file in one transaction and records `public_pages_import`. Pages the file does not contain return to the built-in page.
- `GET /api/v1/settings/public-pages/export` downloads the template file. `GET /api/v1/settings/public-pages/presets` lists the starter templates "Polar Signal" (structured) and "Minimal" (custom HTML).

A draft has `page` (`missing`, `unknown_host`, `password`, or `once`), `lang` (`en` or `de`), and `mode`.

- **`structured`** keeps the built-in layout and stylesheet. Optional plain-text `title` (120 characters), `message` (600, line breaks kept), and `button` (60, not for `unknown_host`) replace the built-in copy; empty fields keep it. The missing page's button appears only while the root destination redirects. `accent` is `#rrggbb`; the page uses dark or white button text, whichever contrasts more, and refuses colors where neither reaches 4.5:1. `logo` is `{"type", "data"}` with a PNG, JPEG, or WebP file of at most 64 KiB in base64. Its signature must match the type. SVG is refused. The logo replaces the mark as a `data:` URI.
- **`custom`** stores `html` of at most 64 KiB after an allowlist sanitizer rebuilt it. The sanitizer keeps common text and layout elements, `class`, `id`, `title`, `lang`, `dir`, `role`, and a few ARIA attributes, `href` with HTTP, HTTPS, or a relative URL, `img` sources that are inline PNG, JPEG, or WebP `data:` URIs, and `<style>` elements. It removes scripts, event handlers, `style` attributes, other URLs, iframes, objects, embeds, forms and inputs, `meta`, `link`, `base`, SVG, and comments, and reports what it removed. A style sheet with `@import`, a `url()` other than an inline raster image, a CSS escape, or a `<` is refused with `unsafe_css`. The first `<title>` becomes the document title.
- Placeholders in custom HTML: `{{host}}` (the request host), `{{lang}}`, `{{root_link}}` (the built-in link to the domain root, empty unless the root destination redirects), and `{{form}}`. Values are HTML-escaped. `{{form}}` and `{{root_link}}` are allowed only in text. Password and one-time pages require `{{form}}` exactly once (`form_required`, `form_duplicate`); other pages refuse it (`form_not_supported`). The server inserts its own form there, with the signed challenge, password field, error message, and submit button, so the CSRF binding, rate limits, and status codes stay server-controlled. No placeholder yields the request path or slug.

The template file is `{"format": "shortcircuit-public-pages", "version": 1, "pages": [...]}` with up to eight drafts, each page and language at most once. Imports pass the same validation and sanitizer as single saves. Errors: `422` with `invalid_format`, `unsupported_version`, `too_many_pages`, or, with `row` and `field`, `duplicate_page` or any draft code.

Custom pages are served with `Content-Security-Policy: default-src 'none'; style-src <sha256 of each style element>; img-src data:; form-action 'self'; base-uri 'none'; frame-ancestors 'none'`. Structured pages use the same policy with the built-in stylesheet's hash and, with an accent or logo, a second hashed style element. Built-in pages keep their previous policy. Status codes, other headers, HEAD responses, and `Accept-Language` selection are unchanged, and a missing path and an inactive link still receive byte-identical responses. The public listener compiles the pages into memory and refreshes them after a change, so a `404` does not read the database while the pages are unchanged. A stored entry that no longer validates, for example one written around the API, falls back to the built-in page.

### Unusual traffic

`GET /api/v1/analytics/traffic` lists up to ten links whose successful redirects on the current UTC day reach at least 100 and at least three times their daily average over the previous seven UTC days. The report reads retained daily rollups, so analytics settings, per-link opt-outs, Do Not Track, and retention apply. It accepts an optional `host`. Keys with the `analytics` scope can read it, and a host-bound key reports only on its host. The indicator is informational and never disables, blocks, or changes a link.

### Title lookup

Title lookup is off until it is turned on in Settings › Operations or by `TITLE_LOOKUP_ENABLED=true` (see [Operations settings](#operations-settings)). `GET /api/v1/title-lookups` reports whether it is enabled, the number of pending lookups, and the 50 most recent results. `POST /api/v1/title-lookups` queues up to 50 links with an empty title, and `POST /api/v1/links/{id}/title-lookup` queues one link. Both require a browser session with Origin and CSRF and return `202`. A disabled feature returns `409 title_lookup_disabled`. A link with a title returns `409 title_present`, and a link with password or one-time protection returns `409 title_lookup_protected`.

One background worker fetches one destination at a time, two seconds apart, through the public outbound policy. It sends GET to the stored destination without its fragment, follows at most three redirects with each hop revalidated, requires an uncompressed `text/html` or `application/xhtml+xml` response, and reads at most 128 KiB within eight seconds. The first `<title>` element becomes plain text: entities are decoded, whitespace is collapsed, control characters are removed, and the result is limited to 160 characters. The worker skips destinations that the destination policy refuses. It saves the title only while the link still has an empty title and the same host and destination, records a metadata history entry for the requesting administrator, and otherwise drops the result. A changed destination or a title written by a person cancels a pending lookup. Lookups pending for more than 24 hours expire, and lookups pending in a restored snapshot are canceled at startup. The stored destination is never changed.

### Notifications

`GET /api/v1/notifications` reports whether notifications are configured, the webhook origin without its path or query, whether the internal connector is used, the selected event kinds, delivery counts, and the 25 most recent outbox rows with attempts, the last error code, and the next attempt time. `POST /api/v1/notifications/test` queues a `test` event and returns `202`, or `409 notifications_disabled`. Both require a browser session. The settings are changed with [Operations settings](#operations-settings); container variables are only start values.

| Event | Trigger and identity | Payload fields |
|---|---|---|
| `link_health` | A confirmed change to `broken` or `unreachable`, or back to `healthy` or `redirected`, for an enrolled link | `link_id`, `status` |
| `link_expiring` | An enabled link reaches the expiry notice window, once per link and expiry time | `link_id`, `expires_at` |
| `link_expired` | A link's expiry time has passed, within seven days, once per link and expiry time | `link_id`, `expires_at` |
| `click_milestone` | Today's retained redirects lift a link's total across a configured milestone, once per link and milestone | `link_id`, `milestone` |
| `backup_failed` | A scheduled backup could not be created or verified | `source` |
| `job_failed` | The analytics queue dropped events, at most once per UTC day | `job`, `dropped_events` |
| `storage_low` | Free space on the data volume is below the diagnostics threshold, at most once per UTC day | `available_bytes`, `low_threshold_bytes` |
| `security` | A recovery, password change, key change, restore step, destination policy change, or operations settings change, or any rejected or failed audited action except sign-in | `audit_id`, `action`, `outcome` |

Every delivery also contains `version` (1), a stable string `id`, `event`, and `occurred_at` in UTC Unix seconds. History from before notifications were first activated, or before a restore, is not sent.

### Operations settings

`GET /api/v1/settings/operations` returns the effective health check, title lookup, scheduled backup, and notification settings. Each value is an object with `value`, `source`, and `container`: `source` is `saved` (set in Settings › Operations), `container` (from the container variable), or `default` (built in), and `container` is the value a reset returns to, or `null`. The effective value is the saved value if one exists, otherwise the container value, otherwise the default. Nothing is saved after an upgrade, so existing installations keep their container values.

| Field | Container variable | Range | Default |
|---|---|---|---|
| `monitor_enabled` | `MONITOR_ENABLED` | `true` or `false` | `false` |
| `monitor_interval_hours` | `MONITOR_INTERVAL_HOURS` | 1–720 | 24 |
| `monitor_priority_hours` | `MONITOR_PRIORITY_HOURS` | 1 to the stable interval | 6, or the stable interval if shorter |
| `title_lookup_enabled` | `TITLE_LOOKUP_ENABLED` | `true` or `false` | `false` |
| `backup_interval_hours` | `BACKUP_INTERVAL_HOURS` | 0–720, 0 is off | 24 |
| `backup_keep` | `BACKUP_KEEP` | 1–365 | 7 |
| `notification_webhook_url` | `NOTIFICATION_WEBHOOK_URL`, `NOTIFICATION_WEBHOOK_URL_FILE`, or the `MONITOR_WEBHOOK_URL` alias | HTTPS URL | none, notifications off |
| `notification_authorization` | `NOTIFICATION_WEBHOOK_AUTHORIZATION_FILE` | header value | none |
| `notification_events` | `NOTIFICATION_EVENTS` | at least one kind | every kind except `click_milestone` unless milestones are set; only `link_health` for the alias |
| `notification_click_milestones` | `NOTIFICATION_CLICK_MILESTONES` | up to 10 counts | none |
| `notification_expiry_notice_hours` | `NOTIFICATION_EXPIRY_NOTICE_HOURS` | 1–720 | 24 |

The webhook URL and the authorization value are write-only. The response reports them as `notification_webhook` and `notification_authorization` with `configured`, `source`, and `container_configured`; the webhook also reports its `origin`, the scheme, host, and port only. `notifications_active` is true while a valid webhook is effective, and `connector` reports whether the internal connector is configured. `GET /api/v1/system` includes the same object as `operations_settings`.

`PATCH /api/v1/settings/operations` saves the fields it contains and deletes the saved values listed in `reset`, for example `{"monitor_enabled": true, "reset": ["backup_keep"]}`. A field cannot be both set and reset. An empty `notification_webhook_url` turns notifications off, and an empty `notification_authorization` sends no header. The whole effective combination is validated before anything is written: `invalid_monitor_intervals`, `invalid_backup_interval`, `invalid_backup_keep`, `invalid_notification_webhook` (HTTPS on port 443, or the connector origin), `notification_webhook_not_public` (a literal non-public address without a connector), `invalid_notification_authorization`, `invalid_notification_events`, `click_milestones_required`, `invalid_click_milestones`, `invalid_expiry_notice`, or `invalid_reset`, each with `422`. Changes apply without a restart and return the new settings. Each change writes an `operations_settings_update` audit entry whose `details.changed` lists the changed setting names, never their values.

Setting or resetting the webhook URL or the authorization needs a fresh password confirmation: `POST /api/v1/settings/operations/authorize` with `{"password": "…"}` returns `204` and grants one such change for two minutes, bound to the browser session. It shares the `RATE_LIMIT_SENSITIVE` budget with the backup and restore confirmations and is audited as `operations_settings_authorize`. Without a grant, the change fails with `403 operation_authorization_required`; a change that fails validation keeps the grant. A changed webhook discards pending deliveries, and only events after the change are generated. These routes require a browser session with Origin and CSRF. API keys are refused with `401`.

Connector variables (`NOTIFICATION_CONNECTOR_ORIGIN`, `NOTIFICATION_CONNECTOR_NETWORKS`, `NOTIFICATION_CONNECTOR_CA_FILE`), public hosts, the admin origin, trusted proxies, rate limits, listener addresses, and the log level stay environment-only.

### Native link exchange

`GET /api/v1/exchange/links` returns version 1, a bounded page of items, and `next_after_id`. Use that value as `after_id` to continue. The exported link fields are `host`, `slug`, `target`, `title`, `tags`, `enabled`, and `analytics_enabled`. A nonempty `description` is included as an optional extension to version 1. Import treats an absent description as empty. Native JSON and CSV exports mark protected links as disabled. Restore their protection settings from a database backup or configure protection on the imported copy before enabling it.

`POST /api/v1/exchange/links` accepts `version: 1`, a Boolean `preview`, and up to 100 `items` within the request size limit. Existing domains must be active. The server returns per-link `create`, `unchanged`, or `conflict` results. A rejected item returns `422` with code `invalid_exchange`, or `destination_not_allowed` for a refused destination, and the problem extensions `row` and `field`: `row` is the 1-based position of the first failing item and `field` the exchange field that failed (`version`, `host`, `slug`, `target`, `title`, `description`, `tags`, `enabled`, or `analytics_enabled`). A repeated host and slug is reported at its second occurrence with field `slug`. For CSV, `row` counts data rows after the header, mapped files report the exchange field instead of the column name, and a malformed record has `row` without `field`. Problems with the whole document, such as an unknown version or too many items, have neither. Submitted values are never echoed. An apply rechecks conflicts in the same transaction as the insert. If any item conflicts, the response is `409` and all existing links remain intact. This format transfers basic link data. A full snapshot includes settings, policies, and visit history.

`GET /api/v1/exchange/links.csv` downloads one CSV page. Send `after_id` with the value from the previous response's `X-Next-After-Id` header to continue. The same authenticated endpoint accepts POST with `Content-Type: text/csv` and defaults to preview. Pass `preview=false` to apply an import. Responses to POST use the same JSON report as the native JSON endpoint.

CSV uses the columns `version,host,slug,target,title,tags,enabled,analytics_enabled`. It accepts a final `description` column and exports it on pages containing descriptions. Each row uses version 1, a JSON array in the tags column, and lowercase Boolean values. The export quotes fields and prefixes spreadsheet-sensitive values. Use the original export for an exact round trip and use JSON for data exchange through tools that resave spreadsheets. [OWASP CSV injection guidance](https://community.owasp.org/attacks/CSV_Injection)

### Public redirect behavior

An exact path takes precedence. When the exact path is missing, the longest stored prefix that allows extra paths can handle the request. Disabled or out-of-window links and exhausted visit limits return `404`. Unknown paths and the root path can use their configured domain fallback and otherwise return `404`. Analytics counts a GET for the root path as `root_fallback` or `root_missing` according to the response it received. Neither outcome changes the redirect, blocked, or missing totals, the visitor classes, or the rankings, which describe short-link paths. Disabled domains return `404` and remain editable on the admin listener. A public `404` to GET or POST carries a small HTML page. An unknown path and an inactive link (disabled, outside its window, used up, or refused by policy) get the same page, so the response does not reveal whether a slug exists. Unknown and disabled hosts get a separate generic page. The missing-link page links to the domain root only while the root destination redirects. The page uses German when the first preferred `Accept-Language` tag is `de` and English otherwise, contains no scripts or external resources, and allows its one inline stylesheet through a `sha256` hash in its Content Security Policy. HEAD responses keep no body. Administrators can replace these pages and the password and one-time pages per language. See [Public pages](#public-pages).

A policy supports `301`, `302`, `307`, or `308`. Start and expiration are inclusive and exclusive respectively, expressed as UTC Unix seconds. Time windows and visit limits require a temporary status. Browsers or intermediaries can retain permanent redirects. A limited link increments its counter atomically for each successful GET while the limit is active. HEAD checks the same conditions without incrementing it and returns the same `Location`. A visit limit counts uses and is not access control. Use password or one-time protection to restrict access. Editing or pausing a limit preserves its counter.

A rule contains a stored `target` and 1 to 8 `conditions`. The first of up to 16 rules whose conditions all match supplies the destination. Without a match, the link uses its stored target. Condition types are `language`, `device`, `os`, `browser`, and `query`. All have a `value`. Query conditions also have a `key`. Client characteristics are estimates from request headers. Language uses the preferred valid language from `Accept-Language`.

Query and extra path forwarding are both off by default. Query forwarding appends the incoming raw query before the stored fragment. Extra path forwarding appends validated path segments before the stored query and fragment. Leave forwarding off for destinations whose parameters or path are signed.

### Redirect simulator and crawler preferences

Use `POST /api/v1/links/{id}/policy/simulate` with the draft `policy` and optional `query`, `accept_language`, `user_agent`, and `extra_path` fields. Submit the query without the leading question mark. The simulator accepts up to 44 KiB, including the draft and sample data. An extra path requires the draft to enable extra path forwarding and the combined short path to fit within 200 characters. The simulator reads the saved default destination and uses the same first-matching-rule logic as public GET and HEAD. It returns JSON. Live requests also evaluate route priority, availability, limits, and access protection.

The public listener serves `robots.txt` for active configured hosts. A domain with `block_crawlers` enabled requests that crawlers skip all paths. The file contains no individual shortcodes. A redirect with `noindex` enabled sends `X-Robots-Tag: noindex, nofollow`. Crawler adherence varies. Crawlers need to fetch a response to see its header, and indexing behavior can vary for redirects. Use access protection for sensitive redirects. [RFC 9309](https://www.rfc-editor.org/rfc/rfc9309.html) · [Google Search Central](https://developers.google.com/search/docs/crawling-indexing/robots-meta-tag)

### Protected links

Protected links require a temporary redirect policy and an HTTPS public origin, with an HTTP loopback exception for development. Passwords require 12 or more characters and at most 1,024 UTF-8 bytes. The confirmation is valid for five minutes and bound to the public origin, path, query, and protection revision. Password checks are rate-limited per client and per process, and per link for each client and across all clients (`RATE_LIMIT_PUBLIC_PASSWORD` and `RATE_LIMIT_PUBLIC_PASSWORD_LINK` in [Security](SECURITY.md#client-addresses-and-request-limits)). A refused attempt returns `429` with `Retry-After: 60`. After one confirmed access, a one-time link returns `404`; create a new link to issue another one-time access. A protected short link controls access through ShortCircuit. The destination is responsible for its own access control.

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
- `GET /api/v1/system` reads storage and memory on request. `storage` reports available and total bytes of the filesystem that holds the SQLite data and is `low` below 512 MiB or three times the database size, whichever is larger. `cpu` is the latest sample of a background sampler that starts with the server, reads the process counters in `/proc/self/stat` and the cgroup v2 `cpu.stat` every 15 seconds (1 second for the first sample), and stops on shutdown. It reports each scope as a percentage of one core over `window_ms`, with `sampled_at` (UTC Unix seconds) and `age_seconds`. Before the first sample, or when the latest sample is older than 60 seconds, both scopes are `unavailable`, `window_ms` is 0, and the two time fields are null. `raw_event_count` is counted at most once every 30 seconds. `container_memory` reads cgroup v2 `memory.current` and `memory.max`. It includes page cache and is separate from `process_rss_bytes`. Each reading states `unavailable` instead of guessing when its source is missing.


## Reports and operations

`GET /api/v1/monitoring` lists enrolled links in pages of 50 with `next_after`. Each item carries the link's `id`, `host`, `slug`, `title`, and `target` with its last check (`status`, `priority`, `last_checked`, `http_status`, `elapsed_ms`, `redirects`, `failure`, `consecutive_failures`), so a client needs no request per link.

`GET /api/v1/overview` requires a browser session. Besides the lifecycle counts, active domains, recent links, and monitor counts, it returns `attention`: up to eight unarchived links with `id`, `host`, `slug`, `title`, `reason`, `expires_at`, `max_visits`, and `visits_used`. Each link appears once, with the first reason that applies. Links whose health check reports `unreachable` or `broken` come first with reason `unreachable`. Links that expire within the next 14 days follow with reason `expiring`, soonest first. Links that have used at least 80 percent of their visit limit come last with reason `limit`, highest share first.

The authenticated analytics summary accepts `days` or a paired `start` and `end` using inclusive UTC dates. Add `host` to restrict the report to a configured public host. `GET /api/v1/analytics/export` downloads the matching versioned JSON report.

Key rotation requires the administrator password and returns a replacement token once. A single SQLite transaction inserts the replacement, revokes the previous key, and records the operation in the audit log.

System diagnostics, audit records, enrollment, domain probes, and backup actions require a browser session. `GET /api/v1/system/backups` lists up to 100 stored files, newest first, with `kind` (`manual`, `scheduled`, `pre_upgrade`, or `rollback`), `file_name`, `size_bytes`, `created_at`, and `schema_version`, plus `truncated`. It returns file names only, skips symbolic links and unexpected names, and reads at most 1,000 entries per location. Stored files are not downloadable through the API; copy them from the data volume. Backup and restore each start with a password-confirmed, single-use grant. A restore upload is a raw SQLite snapshot. After validation the snapshot is staged and takes effect on a controlled restart. The `201` response of `POST /api/v1/system/restore` contains `schema_version` and `created_at` of the snapshot, plus `target_schema_version` and `upgrade_required` (API v1, additive). A snapshot from an older supported schema, back to schema 6, is accepted and upgraded to `target_schema_version` when the restart applies it, and validation first tests that upgrade on a temporary copy. A refused upload returns `422` with one of these codes: `snapshot_schema_newer` for a snapshot from a later release, `snapshot_schema_unsupported` for a schema before 6, `snapshot_migrations_mismatch` when the migration versions, descriptions, or checksums differ, `snapshot_upgrade_failed` when the upgrade test fails, and `invalid_snapshot` for any other verification failure. See the native [OpenAPI specification](openapi.json) for path, media type, and status details. [Backup and restore](BACKUP_RESTORE.md)

To map a generic CSV, POST the UTF-8 body as `text/csv` to `/api/v1/exchange/links.csv/columns`. Apply the selected zero-based column positions to both preview and import. A configured fixed host can supply the domain for all rows. Duplicate labels remain distinguishable by position. Request parameter details and import limits are in [OpenAPI](openapi.json).
