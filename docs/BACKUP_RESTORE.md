# Backup and Restore Design

## Current CLI

Create a backup while the container is running.

```sh
docker compose exec -T shortcircuit shortcircuit backup
```

The command prints the backup path under `/data/backups`. The database-only format includes local settings, links, link history, redirect rules, tags, QR templates, analytics, the administrator account, and API credentials. The copied browser sessions and setup or recovery claims are removed. Keep external environment settings and proxy configuration with your operational backup. Store the snapshot securely because it includes confidential destinations and authentication material.

To restore a snapshot from this release or an older supported one during scheduled downtime, stop the service and use a one-off container with the same app-data volume. Use the path printed by the backup command.

```sh
docker compose stop shortcircuit
docker compose run --rm --no-deps shortcircuit restore /data/backups/SHORTCIRCUIT-SNAPSHOT.sqlite --confirm
docker compose up --detach --wait
```

The restore refuses a busy data directory, accepts standalone snapshots up to 8 GiB, and validates a private staging copy. It prints a restorable rollback snapshot when the previous database can be verified. If the previous database cannot be snapshotted, it preserves a private database file for manual recovery. Sign in again after restart. The snapshot restores API credential state at the time of capture. Revoke any previously revoked credentials that were active when the snapshot was created. Record an independent backup and exercise recovery in a disposable target before production use.

Take a verified snapshot with the current image before applying an upgrade. To roll back a schema upgrade, use the previous image to restore its pre-upgrade snapshot into a separate data volume, as described in [Rolling back an upgrade](#rolling-back-an-upgrade). Verify the restored instance before routing traffic to it. Restoring a snapshot returns one-time links to their state at capture time. Disable any one-time links that were consumed after the snapshot before reopening public traffic.

History entries present when a snapshot was created can return after restore. Reapply any required history purges before opening access to the restored administration interface.

## Pre-upgrade snapshots

When a new release finds pending migrations in an existing database, it first writes a verified snapshot to `/data/backups/pre-upgrade` and logs its path. Startup stops with a message naming the directory if the snapshot cannot be written, so no upgrade runs without a rollback point. The two newest pre-upgrade snapshots are kept. The previous release can restore such a snapshot, and so can every later release, as described below.

## Restoring older backups

A release restores snapshots from its own schema and from every older schema down to schema 6, which 0.6.0-preview.1, the first release with verified backups, wrote. This covers browser downloads, command-line and scheduled backups, and pre-upgrade snapshots. Validation of an older snapshot is as strict as for a current one:

- Its migration history must be an exact prefix of the release's migrations, with the same versions, descriptions, results, and checksums.
- Its tables, indexes, and triggers must match what those migrations produce, apart from the backup format marker.
- It must pass the format check, SQLite `quick_check`, and `foreign_key_check`.

The release refuses a snapshot from a newer schema, from a schema before 6, or with a different migration history, and names the reason and the schema versions involved.

The restore upgrades a private staged copy of an older snapshot with the same migrations that startup applies to a live database. It checks the result against the current schema and only then removes sessions and replaces the live database in one rename. If a migration fails, the live database, its rollback state, and the source file stay unchanged. The CLI reports the error, and a browser restore records **Last staged restore failed** while the service starts on the previous database. The service never starts on a half-migrated restore. A successful restore keeps the previous database as its rollback copy, and the next startup has nothing left to migrate.

The browser upload also tests the upgrade on a temporary copy in the private staging directory, then deletes the copy. A backup that cannot be upgraded is refused before it is staged, and the staged file keeps its original schema until the restart. The restore dialog states the backup's schema and the schema it will have after the restart, and the CLI prints both. An upgrade needs free space for one more copy of the snapshot plus the migration's temporary space.

## Rolling back an upgrade

An older image cannot open a volume that a newer image migrated, and it refuses backups from the newer schema. Changing **Repository** back to the old image is therefore not a rollback after a schema change. This applies to 0.9.0, which adds migrations after schema 24, so 0.8 images refuse its volume and its backups.

Roll back with the pre-upgrade snapshot instead. The first start of 0.9.0 writes a schema 24 snapshot to `/data/backups/pre-upgrade`. Copy it, or the backup you downloaded before the upgrade, and restore it with the 0.8 image into a separate data volume. Then start the 0.8 image on that volume and verify it. Links, analytics, and settings changed after the upgrade exist only in the upgraded volume, so keep that volume until the cause is understood. [Release verification](RELEASE_VERIFICATION.md#rollback)

Restore and temporary files share the data volume. The image sets `SQLITE_TMPDIR=/data/tmp`, because the root filesystem is read-only and SQLite needs a writable directory for sorts, multi-row writes, and `VACUUM`. Keep free space of at least three times the database size. The diagnostics page warns below that.

A restore discards notifications and title lookups that were pending in the snapshot, starts a new notification ID epoch, and generates only events that happen after the restore.

Verified snapshots do not contain a notification webhook URL or Authorization header saved in Settings › Operations, nor the saved switches for health checks and title lookup. A webhook URL can carry its secret in the path, and a restored installation must not start outgoing requests that its own administrator did not turn on. After a restore these four settings follow the container variables of the restoring installation, otherwise their defaults, which are off. Turn them on and enter the webhook again in Settings › Operations if needed. Intervals, backup retention, and the event selection are restored. It restores the administrator password and the API keys that were valid when the snapshot was taken, including keys revoked later. Revoke such keys again and change the password if the snapshot predates a credential change.

If a copied snapshot is not readable by UID 10001, the restore names the file and suggests `chmod 644`. Browser downloads are named `shortcircuit-schema<version>-<unix-time>.sqlite`.

## Automatic backups

Set the interval and retention in **Settings › Operations › Scheduled backups**. Changes apply without a restart. The interval is 1 to 720 hours, and 0 turns scheduled backups off. Since 0.9.0 the built-in default is a daily backup, as the Unraid template already set; before, scheduled backups were off unless `BACKUP_INTERVAL_HOURS` was set. `BACKUP_INTERVAL_HOURS` and `BACKUP_KEEP` remain accepted as start values that apply while nothing is saved in the interface, and the page shows which source each value comes from. The schedule continues from the newest scheduled snapshot, so restarts and setting changes do not postpone it. Without one, the first backup starts one interval after the schedule became active. An overdue backup runs about one minute after startup or after the interval is shortened. A failed attempt waits a full interval before the next one. The scheduler creates at most one backup at a time and skips missed intervals.

Keep 1 to 365 scheduled backups. The default is 7. A changed count applies after the next scheduled backup. Verified copies are stored in `/data/backups/scheduled`, and a failed attempt leaves existing backups intact. Retention applies to the scheduled directory. On shutdown the process allows up to five seconds for an in-progress scheduled job to finish after HTTP and analytics wind down.


## Consistent Snapshots

Prefer the SQLite Online Backup API. Verify that the Rust backup bindings use the same SQLite version as SQLx. Consider `VACUUM INTO` as an alternative if it simplifies maintenance and measurements show acceptable locking and load. Create snapshots separately from the live WAL on suitable local storage. Abort if space runs low. Copy large databases in bounded steps with a deadline that prioritizes redirects. [SQLite Online Backup API](https://www.sqlite.org/backup.html) · [VACUUM INTO](https://www.sqlite.org/lang_vacuum.html)

A versioned bundle contains a database snapshot, assets required by that snapshot, and the corresponding configuration. The manifest records each file's size, type, and SHA-256 digest. The digest verifies transfer integrity. If the bundle is encrypted, use an authenticated scheme and define key management. For assets that can change, use immutable blobs listed in the manifest from the same logical snapshot. Invalidate sessions, setup tokens, and any temporary operation credentials during restore. Define separate handling for external integration secrets.

## Operations

- The initial retention proposal allows manual backups and optional daily backups, with seven successful backups retained and at least one copy stored independently. Preserve the last successfully verified backup when changing retention.
- Write to a temporary file with restrictive permissions. Verify file size and SQLite consistency, sync the file and directory, and then publish atomically. Show the time of the last verification and independent copy.
- Require admin reauthentication for downloads and restores, and audit both operations. Warn administrators that the bundle contains destination URLs, hashes, settings, and any retained history.
- Store backups under `/data/backups`. Completed bundles can be copied to external storage. Use a consistent SQLite snapshot or stop the application before taking a host-level backup of live data.

## Restore and Failure Scenarios

1. Upload to isolated staging with limits on compressed and decompressed size, file count, and processing time. Accept explicit manifest paths only. Reject links, device files, and archive paths outside staging. Treat archives and databases as untrusted.
2. Validate the snapshot with SQLite `quick_check` or `integrity_check` plus `foreign_key_check`. Verify the format, schema, expected tables, assets, and digests. Reject incompatible newer schemas. Test schema upgrades only in staging.
3. Enter maintenance mode, pause jobs and new redirects, drain analytics within its deadline, and close database connections. Preserve an emergency backup and keep staging and app data on the same filesystem. Activate the validated data generation using an atomic switch or a logged switch with explicit startup recovery.
4. Open and validate the new database and assets, invalidate sessions, and restore readiness. On failure, switch back to the previous generation. Keep the database, WAL, and SHM files with their own generation.
5. Before release, simulate process termination at each switch boundary. Test temporary locks, low disk space, missing files, and interrupted uploads.

Measure snapshot consistency, recovery point objective, and recovery time objective in the lab.

## Browser administration

Open **System and backups** and confirm the current administrator password for each download, upload, or discard action. A successful confirmation authorizes one action for that session for two minutes. A browser download streams a newly verified snapshot. Store it alongside other confidential operational backups.

**Stored snapshots** on the same tab lists, newest first, the snapshots on the data volume: command-line backups in `/data/backups`, scheduled backups in `/data/backups/scheduled`, pre-upgrade snapshots in `/data/backups/pre-upgrade`, and the rollback copies a restore keeps next to the database as `/data/shortcircuit.db.rollback-<id>.sqlite`. Each row shows the type, creation time (the file time for rollback copies), schema version, and size. The list shows up to 100 files and cannot download or delete them. Copy the files you need from the volume, and delete rollback copies there after you have verified the restored instance, because nothing removes them automatically.

The restore screen accepts the same standalone SQLite snapshot format as the CLI. The application limits the upload to 8 GiB and 30 minutes and validates the snapshot before staging it privately in the app-data directory. Check reverse proxy limits for file size and read duration when using large databases.

A staged restore appears as **Pending restart** while the current database continues serving requests. Discard it before the restart if you selected the wrong file. Restart ShortCircuit during the maintenance window to activate the staged restore. Startup acquires the data-directory lock before changing the active database and retains a rollback copy. Review the status and sign in again after activation. A backup from an older release is upgraded during that restart, before activation. Follow the CLI recovery procedure above if startup or validation fails.

A process interrupted after claiming a restore records **Interrupted** on startup and holds the previous state or the recovery copy for operator inspection. Treat the active generation as uncertain until you verify links and API credentials. Before reopening public traffic, review consumed one-time links, restored credentials, and enrolled link monitoring. Prevent outbound activity during a recovery exercise by starting the test installation without `MONITOR_ENABLED`, `TITLE_LOOKUP_ENABLED`, and a notification webhook. Snapshots carry none of these switches, so health checks, title lookup, and notifications stay off until you turn them on.
