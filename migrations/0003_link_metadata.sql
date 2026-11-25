-- Preserve existing link IDs and never reuse a deleted ID in a pagination cursor.
CREATE TABLE updated_links (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    host TEXT NOT NULL CHECK (length(host) BETWEEN 1 AND 253),
    slug TEXT NOT NULL COLLATE BINARY CHECK (length(slug) BETWEEN 1 AND 200),
    target TEXT NOT NULL CHECK (length(target) BETWEEN 1 AND 4096),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    title TEXT NOT NULL DEFAULT '',
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    UNIQUE (host, slug)
) STRICT;

INSERT INTO updated_links (id, host, slug, target, created_at)
SELECT id, host, slug, target, created_at FROM links;

DROP TABLE links;
ALTER TABLE updated_links RENAME TO links;

CREATE TABLE link_tags (
    link_id INTEGER NOT NULL REFERENCES links(id) ON DELETE CASCADE,
    tag TEXT NOT NULL CHECK (length(tag) > 0),
    PRIMARY KEY (link_id, tag)
) STRICT, WITHOUT ROWID;
