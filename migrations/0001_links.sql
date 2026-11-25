CREATE TABLE links (
    id INTEGER PRIMARY KEY,
    host TEXT NOT NULL CHECK (length(host) BETWEEN 1 AND 253),
    slug TEXT NOT NULL COLLATE BINARY CHECK (length(slug) BETWEEN 1 AND 200),
    target TEXT NOT NULL CHECK (length(target) BETWEEN 1 AND 4096),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE (host, slug)
) STRICT;
