-- Opt-in title lookups for saved destinations. One row per link records the latest request.
-- A lookup applies a title only while the link still has no title and the same host and target.
CREATE TABLE link_title_lookups (
    link_id INTEGER PRIMARY KEY REFERENCES links(id) ON DELETE CASCADE,
    revision TEXT NOT NULL DEFAULT (lower(hex(randomblob(16))))
        CHECK (length(revision) = 32 AND revision NOT GLOB '*[^0-9a-f]*'),
    host TEXT NOT NULL CHECK (length(host) BETWEEN 1 AND 253),
    target TEXT NOT NULL CHECK (length(target) BETWEEN 1 AND 4096),
    requested_by TEXT NOT NULL CHECK (length(requested_by) BETWEEN 1 AND 160),
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'applied', 'no_title', 'failed', 'skipped')),
    failure TEXT CHECK (failure IS NULL OR failure IN (
        'blocked', 'busy', 'dns', 'timeout', 'connection', 'protocol', 'not_html',
        'policy', 'expired', 'restored'
    )),
    title TEXT CHECK (title IS NULL OR (length(title) BETWEEN 1 AND 160 AND instr(title, char(0)) = 0)),
    requested_at INTEGER NOT NULL DEFAULT (unixepoch()),
    completed_at INTEGER,
    CHECK ((status = 'pending') = (completed_at IS NULL)),
    CHECK ((status = 'applied') = (title IS NOT NULL))
) STRICT;

CREATE INDEX link_title_lookups_pending ON link_title_lookups (status, requested_at, link_id);

-- A changed host or destination invalidates any lookup for the previous destination.
CREATE TRIGGER link_title_lookup_target_changed
AFTER UPDATE OF host, target ON links
WHEN old.host IS NOT new.host OR old.target IS NOT new.target
BEGIN
    DELETE FROM link_title_lookups WHERE link_id = new.id;
END;

-- A title written by an administrator cancels a pending lookup.
CREATE TRIGGER link_title_lookup_title_changed
AFTER UPDATE OF title ON links
WHEN old.title IS NOT new.title
BEGIN
    DELETE FROM link_title_lookups WHERE link_id = new.id AND status = 'pending';
END;
