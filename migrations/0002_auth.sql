CREATE TABLE admin_accounts (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    username TEXT NOT NULL CHECK (length(username) BETWEEN 1 AND 64),
    password_hash TEXT NOT NULL CHECK (substr(password_hash, 1, 10) = '$argon2id$')
) STRICT;

CREATE TABLE admin_bootstrap (
    kind TEXT PRIMARY KEY CHECK (kind IN ('setup', 'recovery')),
    token_hash BLOB NOT NULL CHECK (length(token_hash) = 32),
    expires_at INTEGER NOT NULL CHECK (expires_at > 0)
) STRICT;

CREATE TABLE admin_sessions (
    token_hash BLOB PRIMARY KEY CHECK (length(token_hash) = 32),
    account_id INTEGER NOT NULL REFERENCES admin_accounts(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL CHECK (last_seen_at >= created_at)
) STRICT;

CREATE TABLE admin_api_keys (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id INTEGER NOT NULL REFERENCES admin_accounts(id) ON DELETE CASCADE,
    token_hash BLOB NOT NULL UNIQUE CHECK (length(token_hash) = 32),
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 64),
    scopes INTEGER NOT NULL CHECK (scopes BETWEEN 1 AND 7),
    host TEXT CHECK (length(host) BETWEEN 1 AND 253),
    expires_at INTEGER CHECK (expires_at IS NULL OR expires_at >= created_at),
    created_at INTEGER NOT NULL
) STRICT;
