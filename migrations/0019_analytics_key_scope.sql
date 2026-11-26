-- Allow the analytics report scope (bit 8) next to read (1), write (2), and delete (4).
-- Existing keys keep their stored scope bits, IDs, secrets, host restrictions, and expiry.
CREATE TABLE admin_api_keys_next (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id INTEGER NOT NULL REFERENCES admin_accounts(id) ON DELETE CASCADE,
    token_hash BLOB NOT NULL UNIQUE CHECK (length(token_hash) = 32),
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 64),
    scopes INTEGER NOT NULL CHECK (scopes BETWEEN 1 AND 15),
    host TEXT CHECK (length(host) BETWEEN 1 AND 253),
    expires_at INTEGER CHECK (expires_at IS NULL OR expires_at >= created_at),
    created_at INTEGER NOT NULL
) STRICT;

INSERT INTO admin_api_keys_next (id, account_id, token_hash, name, scopes, host, expires_at, created_at)
SELECT id, account_id, token_hash, name, scopes, host, expires_at, created_at FROM admin_api_keys;

-- Carry the highest issued ID forward so a revoked key ID is never issued again.
DELETE FROM sqlite_sequence WHERE name = 'admin_api_keys_next';
INSERT INTO sqlite_sequence (name, seq)
SELECT 'admin_api_keys_next', seq FROM sqlite_sequence WHERE name = 'admin_api_keys';

DROP TABLE admin_api_keys;
ALTER TABLE admin_api_keys_next RENAME TO admin_api_keys;
