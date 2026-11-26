# Backup and Restore Design

## Current CLI

Create a backup while the container is running.

```sh
docker compose exec -T shortcircuit shortcircuit backup
```

The command prints the backup path under `/data/backups`. The database-only format includes local settings, links, link history, redirect rules, tags, analytics, the administrator account, and API credentials. The copied browser sessions and setup or recovery claims are removed. Keep external environment settings and proxy configuration with your operational backup. Store the snapshot securely because it includes confidential destinations and authentication material.

To restore a snapshot from the same schema version during scheduled downtime, stop the service and use a one-off container with the same app-data volume. Use the path printed by the backup command.

```sh
docker compose stop shortcircuit
docker compose run --rm --no-deps shortcircuit restore /data/backups/SHORTCIRCUIT-SNAPSHOT.sqlite --confirm
docker compose up --detach --wait
```

The restore refuses a busy data directory, accepts standalone snapshots up to 8 GiB, and validates a private staging copy. It prints a restorable rollback snapshot when the previous database can be verified. If the previous database cannot be snapshotted, it preserves a private database file for manual recovery. Sign in again after restart. The snapshot restores API credential state at the time of capture. Revoke any previously revoked credentials that were active when the snapshot was created. Record an independent backup and exercise recovery in a disposable target before production use.

Take a verified snapshot with the current image before applying an upgrade. To roll back a schema upgrade, use the previous image to restore its pre-upgrade snapshot into a separate data volume. Verify the restored instance before routing traffic to it. Restoring a snapshot returns one-time links to their state at capture time. Disable any one-time links that were consumed after the snapshot before reopening public traffic.

History entries present when a snapshot was created can return after restore. Reapply any required history purges before opening access to the restored administration interface.

## Automatic backups

Set `BACKUP_INTERVAL_HOURS` to an interval from 1 to 720 hours and restart the container. The default `0` disables the scheduler. The first backup starts after one interval. The scheduler creates at most one backup at a time and skips missed intervals.

Set `BACKUP_KEEP` to retain 1 to 365 scheduled backups. The default is 7. Verified copies are stored in `/data/backups/scheduled`, and a failed attempt leaves existing backups intact. Retention applies to the scheduled directory. On shutdown the process allows up to five seconds for an in-progress scheduled job to finish after HTTP and analytics wind down.

Browser downloads and browser restore are planned.

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
