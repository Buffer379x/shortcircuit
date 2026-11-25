# Local backup and restore

The first backup implementation uses the ShortCircuit CLI and a private directory below the data root. It can create a compact, consistent SQLite snapshot while the service is running. The snapshot retains links, reports, redirect rules, domain settings, and authentication records required to use the restored installation. Browser sessions and one-time setup and recovery claims are removed from the copy. [SQLite VACUUM INTO](https://www.sqlite.org/lang_vacuum.html)

Restore is an offline operation with an explicit CLI confirmation. The running server and administrative maintenance commands take a shared data-directory lock. Restore takes the exclusive lock. It validates the backup format, schema, migration checksums, database consistency, and foreign keys in a bounded private staging file. It keeps an independent rollback snapshot and activates the replacement atomically on the same filesystem. [Rust file locking](https://doc.rust-lang.org/std/fs/struct.File.html#method.try_lock)

## Acceptance checks

- Take a snapshot of an active WAL database and verify a transaction acknowledged by the service is present in the snapshot.
- Verify the source's active session, tokens, and data remain intact after the backup.
- Verify a staged backup is rejected with a mismatch in its version, schema, or migration checksums.
- Confirm offline restore requires the exclusive lock and preserves the database when it cannot acquire the lock.
- Confirm a restored link resolves to its original destination after restarting the same non-root container.
- Confirm sessions created before the restore are revoked and the recorded rollback file is usable.
- Document the initial format and the CLI restore exercise. Add scheduled backups and administrative downloads once offline recovery is verified.
