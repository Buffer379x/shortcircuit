CREATE TABLE domains (
    host TEXT PRIMARY KEY CHECK (length(host) BETWEEN 1 AND 253 AND host = lower(host)),
    origin TEXT NOT NULL CHECK (length(origin) BETWEEN 1 AND 4096),
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    root_target TEXT CHECK (length(root_target) BETWEEN 1 AND 4096),
    missing_target TEXT CHECK (length(missing_target) BETWEEN 1 AND 4096)
) STRICT, WITHOUT ROWID;

CREATE INDEX link_tags_by_tag ON link_tags (tag, link_id);
