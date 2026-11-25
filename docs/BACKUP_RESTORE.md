# Backup and Restore Design

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
