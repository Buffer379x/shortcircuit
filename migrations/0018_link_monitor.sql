CREATE TABLE link_monitor (
    link_id INTEGER PRIMARY KEY REFERENCES links(id) ON DELETE CASCADE,
    revision TEXT NOT NULL DEFAULT (lower(hex(randomblob(16)))),
    priority INTEGER NOT NULL CHECK (priority IN (0, 1)),
    status TEXT NOT NULL DEFAULT 'not_checked'
        CHECK (status IN ('not_checked', 'healthy', 'redirected', 'warning', 'unreachable', 'broken', 'unknown')),
    last_checked INTEGER,
    next_check INTEGER NOT NULL DEFAULT 0,
    http_status INTEGER,
    elapsed_ms INTEGER,
    redirects INTEGER,
    failure TEXT,
    consecutive_failures INTEGER NOT NULL DEFAULT 0 CHECK (consecutive_failures >= 0)
) STRICT;

CREATE INDEX link_monitor_due ON link_monitor(next_check, link_id);

CREATE TRIGGER link_monitor_target_changed
AFTER UPDATE OF target ON links
WHEN old.target <> new.target
BEGIN
    DELETE FROM link_monitor WHERE link_id = new.id;
END;
