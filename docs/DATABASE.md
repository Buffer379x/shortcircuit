# Database Design

## 0.1 Foundation

The versioned SQL migration creates a strict `links` table with an integer `id`, `host`, case-sensitive `slug`, `target`, and a UTC `created_at` timestamp. The unique index on `(host, slug)` also supports redirect lookups. The database stores the validated destination string as supplied so redirects preserve its encoding, query, and fragment. The source checks for SQLite 3.51.3 or newer at startup and configures WAL, foreign keys, `synchronous=FULL`, a pool of up to four connections, and two-second busy and pool-acquisition timeouts.

Schema version 11 assigns a unique 128-bit selection key to each existing link and to rows on insert. Bulk requests verify both the numeric ID and the key inside the write transaction. A reused SQLite row ID represents a new selection.

Schema version 12 adds `favorite` and `archived` flags to `links`, both defaulting to false. A catalog organization write matches the link ID, selection key, and configured host in one statement. Redirect lookup ignores both flags. SQLite backups include them.

Schema version 13 adds the optional plain-text link description with an empty-string default for existing links. An older history revision without a description leaves the current description intact when restored. Catalog usage reads the retained daily analytics and the latest available detailed event for the selected page. It requires no separate visit ledger.

Schema version 14 adds `tag_details` for an optional color and plain-text description. The tag name remains the exact identity already used in link assignments. A tag with saved details remains available at zero assignments. Rename, merge, and deletion update assignments and details in one transaction. A merge uses existing destination values and fills empty fields from the source. SQLite backups include the details. Native link exchange transfers tag names.

Schema version 15 adds `domains.block_crawlers` and `link_redirect_policies.noindex`. Both start disabled. Existing policy revisions omit the new option and preserve its current value when restored. The redirect simulator reads link metadata and applies the same ordered rule evaluation as the public listener.

Schema version 19 rebuilds `admin_api_keys` so the scope bit mask accepts the analytics report scope (bit 8, values 1 through 15). The migration copies every key unchanged and carries the AUTOINCREMENT sequence forward, so a revoked key ID is not issued again.

Schema version 20 rebuilds `analytics_events` and `analytics_daily` to accept the `root_fallback` and `root_missing` outcomes, which require a null or zero link ID. Existing rows keep their outcome, so earlier totals keep their meaning, and a period without root rows reports zero. The migration preserves event IDs, the event ID sequence, the indexes, and the link deletion trigger. The rebuild temporarily needs free space comparable to the analytics tables.

Schema version 21 adds `destination_policy`, a single row with `enabled` (off by default) and a `generation` counter, and `destination_rules` keyed by normalized host and subdomain flag. Every policy change increments the generation. The redirect lookup reads the generation in the same statement as the link, so the public listener reloads its in-memory copy only after a change. The migration also rebuilds `admin_audit` to accept the `destination_policy_update` action and carries its ID sequence forward.

Schema version 22 adds `link_title_lookups` with one row per link: a request revision, the host and destination at request time, the requesting administrator, status, failure code, and the extracted title. Triggers delete the row when the link's host or destination changes and delete a pending row when its title changes, so a late result cannot overwrite a newer state.

Schema version 23 adds `notification_state`, which holds the activation or restore time and the audit cursor, and `notification_outbox`, keyed by a unique deterministic event key with status, attempts, next attempt time, and the last error code. A trigger keeps at most 1,000 settled rows. Pending rows are never trimmed.

Schema version 24 adds a random notification epoch, keeps settled outbox rows for at least eight days before trimming, and adds triggers that cancel a pending title lookup when password or one-time protection is added.

Before pending migrations run on an existing database, startup writes a verified snapshot to `backups/pre-upgrade` and keeps the two newest. Migrations run on a dedicated connection with a 60-second acquisition deadline, separate from the request pool's two-second deadline.

## Protected links

Migration `0007` adds `link_protection` with the link ID, optional Argon2id password hash, one-time and consumed flags, and a protection revision. A deleted link cascades to its protection record. An existing link has no protection by default. Changes to protection increment its revision. Confirmation checks that revision before consuming the one-time flag and any existing visit limit in the same write transaction. Signed confirmation tokens expire in memory and require no challenge rows.

## Link history

Migration `0008` adds `link_history`. Direct link edits write a typed previous configuration and the verified actor in the same transaction as the mutation. Protection events store only actor, time, and action. History retains up to 25 entries per link. Rows older than 30 days are excluded from the API immediately and cleaned up in bounded batches. Deleting the link cascades to its history. A browser administrator can purge its history separately.

## Planned Entities and Relationships

The target storage is SQLite on a local persistent volume. Use named projections for list and dashboard queries. Consider an abstraction for multiple database engines when measured requirements justify it.

| Table | Keys and important fields | Integrity and access |
|---|---|---|
| `domains` | `id`, normalized `authority`, public base URL, default flag, status, and fallbacks | Keep each authority unique. A partial index permits at most one default domain. An application transaction enforces exactly one. Resolve links using the actual request host. |
| `links` | `id`, `domain_id`, `slug`, `wire_url`, title, description, status, analytics and crawling flags, validity period, `max_uses`, redirect and routing settings, creation and update times, and revision | Enforce uniqueness on `(domain_id, slug COLLATE BINARY)` and use `CHECK` constraints for ranges and time windows. Add lookup keys for case-insensitive domains only after checking for collisions and ensuring existing case variants remain addressable. Give aliases or path prefixes their own lookup table only if a concrete need emerges. |
| `link_limits` | `link_id` primary and foreign key, `baseline_uses`, `local_uses`, baseline provenance, and policy version | Keep a durable enforcement counter independent of analytics. Require a verified baseline if an operator chooses to count prior uses when activating a limit. |
| `redirect_rules` | `id`, `link_id`, unique `position`, destination, schema version, and validated predicates | Enforce a unique order per link. Use a stable rule ID for analytics and a revision for audits. Store predicates as a versioned JSON syntax tree with bounded depth and size. Reject scripts. |
| `tags`, `link_tags` | Tag ID, name, optional color and description, and join on `(link_id, tag_id)` | Enforce name uniqueness according to the product's matching rules. Add a reverse index on `(tag_id, link_id)`. Use transactions for operations involving multiple tags, including renames and merges. |
| `collections`, `collection_links`, `favorites` | Collection and join records. Favorites link a user to a link or a collection through separate foreign-key relationships. | Use collections as the additional organization tool. Retain composable tag filters. |
| `users`, `sessions` | User ID, account status, password hash in PHC format, session ID, `token_hash`, expiry, rotation, and revocation state | Index token digests. Support revocation and deletion by user, with idle and absolute expiry. Store only token digests. |
| `api_keys`, `api_key_domains` | Public lookup ID, `token_hash`, name, permissions, expiry, revocation, and allowed domains | Check each permission against the domain and object. Issue keys through the application. |
| `visits` | Monotonic event ID, UTC time, nullable link ID, domain ID, outcome, category, optional referrer domain, optional truncated IP prefix, optional geo and device fields, and optional rule ID | Orphan events have a null link ID. A deletion job may also clear the link ID. By default, exclude full IP addresses from storage. Index `(link_id, occurred_at, id)`, `(domain_id, occurred_at, id)`, and `(occurred_at, id)`. |
| `visit_daily`, `visit_daily_breakdowns` | UTC day, typed grouping by link, domain, outcome, and class, plus an optional dimension and rollup code | Materialize only combinations used by queries. Cap high-cardinality referrers and report grouped values separately. Use a UTC projection for date ranges. Use raw or sufficiently granular data for local calendar days and daylight saving transitions, or show the precision limit. |
| `audit_events` | Audit event type, actor ID, time, and object ID | Exclude destination URLs and secrets from audit events. Restrict access to the event log. |
| `settings`, `setup_state` | Typed, versioned settings and active setup token digest with a time limit | Complete setup in a one-time transaction. Require recent authentication for protected settings. Limit deployment overrides to documented ports and paths. |
| `import_runs`, `source_mappings` | Native format and version where applicable, file identifier, status, validation results, batch cursor, and mappings from record identifiers to link IDs | Allow one active run per source file identity. Keep artifacts in staging and credentials out of manifests. Use stable record IDs where present or a documented file identity and record position for retries. |
| `job_runs`, `health_targets`, `health_results` | Last status and next due time, destination reference, probe profile, result, timestamp, and duration | Bound retention and the number of stored results. Claim jobs atomically and track restart state. Deduplicate health checks by destination and policy. |
| `notification_endpoints`, `notification_outbox` | Access-controlled connector settings, event ID, delivery status, and next retry time | Deduplicate deliveries and cap retention and retry counts. Keep secrets out of ordinary configuration exports. |
| `qr_templates` | `id`, unique `name`, resolution, two colors, transparency, error correction, quiet zone, and frame | Schema version 9 adds a strict table. Version 10 adds the frame column with a default of `none` for existing templates. The insert statement checks the installation limit of 32 templates atomically. The QR renderer validates style settings during creation. |
| `link_pages`, `page_items`, `assets` | Validated content fields, asset ID, type, storage path, digest, and length | Planned for later phases. Public link pages share the host and path namespace and collision checks with short links. Immutable asset keys simplify backups. |

Start with additional indexes on `links(domain_id, created_at, id)` and `links(status, valid_until, id)`. Measure whether exact destination duplicate detection needs an index. Verify compound indexes against real filters with `EXPLAIN QUERY PLAN`. Use keyset pagination with the sort value and ID for stable lists and exports. Bound filter sizes and query cost. Fetch tags with the list query or in a batch to avoid N+1 lookups. Evaluate full-text search when measured search workloads justify it, with explicit URL tokenization and deletion behavior.

The target model uses integer UTC Unix timestamps with a documented unit, preferably milliseconds. The API returns UTC timestamps in RFC 3339 format. Keep unknown historical fields as `NULL`. If the initial analytics view groups past visits by current tags, label it clearly. Historical tag membership requires a separate data contract. Aggregates must reflect their data sources when later edits or deletions affect results.

## Write Protocols

- **Counters and concurrency.** A confirmed use updates the enforcement counter with a conditional atomic operation. The transaction checks existence, link revision, protection, status, time window, and the sum of the baseline and local counters. When the revision changes, reevaluate the request under the new configuration or deny it. Build the redirect response after commit. A network failure after commit can consume a use. The counter measures authorized redirects regardless of whether the destination ultimately receives the request.
- **What counts as a use.** Regular `GET` requests and explicit confirmation via `POST` for protected or one-time links can consume the configured allowance. Exclude `HEAD`, recognized previews, and monitoring requests from one-time consumption.
- **Adding a limit to an existing link.** The default unlimited link has no synchronous use counter. The editor must offer a limit starting at activation or starting from a verified exact baseline. An exact count since creation requires independent verification because analytics is asynchronous. Deleting analytics leaves active enforcement counters intact.
- **Analytics.** Write detail events and rollups atomically in batches. Make writes retryable using stable batch or event IDs until commit. A manual raw-data deletion can reduce existing rollups transactionally while enough detail remains. Routine retention removes detail and keeps the specified rollups. Full deletion removes both. For already aggregated records, allow deletion of the relevant aggregate ranges.
- **Mutations.** Write links, tags, rules, audit events, and applicable revisions together. Use optimistic revision checks for UI edits. Process large bulk actions in bounded batches, document partial success, and audit privileged operations.
- **Native imports.** Validate CSV or JSON in isolated staging and record the format, file identity, and validation result. Write validated records through application-controlled prepared statements. Keep the input immutable. Make duplicate detection, conflict resolution, and retry behavior explicit before applying changes.

## SQLite and Storage

- Use `journal_mode=WAL`, `foreign_keys=ON` on each connection, and `synchronous=FULL`. The target starts with four connections, a configurable 250 ms busy timeout, and separate bounded request and pool-acquisition deadlines. Reserve longer write budgets for defined maintenance jobs. Keep transactions short and network I/O outside transactions. A connection or storage failure on a limited link returns a controlled `503`. [SQLite WAL](https://sqlite.org/wal.html) · [SQLite PRAGMA](https://www.sqlite.org/pragma.html)
- Bundle SQLite and verify the actual engine version at startup and in CI. The uniform minimum is 3.51.3. SQLite documents a potential WAL corruption bug in 3.51.2 and older affected versions when checkpoints and writes overlap, with specific backports available. Use `sqlite_version()` to verify the engine in use. [SQLite WAL-Reset Bug](https://sqlite.org/wal.html#the_wal_reset_bug) · [SQLx SQLite](https://docs.rs/sqlx/latest/sqlx/sqlite/)
- Set `trusted_schema=OFF` and disable extension loading. For staged database snapshots, use only application-controlled queries against a known schema. Open untrusted files read-only with resource limits and verify file size, schema, and integrity before activation. [SQLite Security](https://www.sqlite.org/security.html)
- Keep the database, WAL, and SHM files together on local storage with reliable locks and `fsync`. The preferred Unraid bind mount is `/mnt/<Pool>/appdata/shortcircuit` to `/data` on a local SSD, with active app data excluded from Mover. Use `/mnt/user` only when the actual path and exclusive local placement are verified. Use one path consistently for the share. Keep active WAL files off SMB and NFS. [Unraid Exclusive Shares](https://docs.unraid.net/unraid-os/release-notes/6.12.0/#exclusive-shares)
- Version schema changes with SQLx migrations. Compare schemas and run startup tests against fixtures from supported prior versions. Before a potentially irreversible upgrade, verify a backup and confirm adequate free space. Use passive checkpoints during regular operation and monitor WAL size. Bound long-lived reads, reports, imports, and backup work by time and data volume.

## Backup and Restore Consistency

Prefer the SQLite Online Backup API with reviewed Rust bindings and a shared `libsqlite3-sys` version. `VACUUM INTO` is a consistent alternative if tests show acceptable operational impact and the output file is explicitly synced. Collect the database snapshot and referenced assets from the same logical view. Use a versioned manifest with schema and digest information. Limit and isolate uploads and publish a backup only after verification. [SQLite Backup API](https://www.sqlite.org/backup.html) · [VACUUM INTO](https://www.sqlite.org/lang_vacuum.html)

Verify restore uploads in staging before the maintenance window. Put the application in maintenance mode, block writes and new redirects, drain analytics, and close the connection pool. Activate validated data atomically on the same filesystem. Retain the previous dataset as a separate generation for rollback and switch the database and assets together. Once assets are present, prefer an atomic pointer to a self-contained dataset. Keep WAL files with their original generation. Check readiness and schema after startup and revoke prior sessions. See [Backup and Restore](BACKUP_RESTORE.md) for the workflow and limits.


## Administration and monitoring records

The latest schema adds coarse analytics dimensions, a bounded administrative audit log, and explicit link-monitor enrollment with an enrollment revision. A link target change removes its monitoring row. The revision prevents a delayed result from updating a replacement enrollment. Public redirect visits and atomic access limits do not depend on the monitoring table.

Runtime connections default to a SQLite busy timeout of 2,000 milliseconds. Set `SQLITE_BUSY_TIMEOUT_MS` to a value from 1 to 10,000 if the local storage and workload warrant a change. The separate pool acquisition timeout remains two seconds.
