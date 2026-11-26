-- Durable notification outbox. Rows are created only while a notification webhook is configured.
CREATE TABLE notification_state (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    -- Events whose natural time is earlier are not generated, for example after a restore.
    not_before INTEGER,
    -- The last administrative audit entry that was considered for security notifications.
    audit_cursor INTEGER NOT NULL DEFAULT 0 CHECK (audit_cursor >= 0)
) STRICT;

INSERT INTO notification_state (id) VALUES (1);

CREATE TABLE notification_outbox (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    -- A deterministic identity. Repeated detection of the same event is ignored.
    event_key TEXT NOT NULL UNIQUE CHECK (length(event_key) BETWEEN 1 AND 200),
    event TEXT NOT NULL CHECK (event IN (
        'test', 'link_health', 'link_expiring', 'link_expired', 'click_milestone',
        'backup_failed', 'job_failed', 'storage_low', 'security'
    )),
    payload TEXT NOT NULL CHECK (
        length(payload) <= 2048 AND json_valid(payload) AND json_type(payload) = 'object'
    ),
    occurred_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'delivered', 'failed', 'discarded')),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 100),
    next_attempt_at INTEGER NOT NULL DEFAULT (unixepoch()),
    last_error TEXT CHECK (last_error IS NULL OR last_error IN (
        'blocked', 'busy', 'dns', 'timeout', 'connection', 'protocol', 'restored', 'stale'
    )),
    delivered_at INTEGER,
    CHECK ((status = 'delivered') = (delivered_at IS NOT NULL))
) STRICT;

CREATE INDEX notification_outbox_due ON notification_outbox (status, next_attempt_at, id);

-- Keep at most 1,000 settled rows. Pending rows are never trimmed.
CREATE TRIGGER notification_outbox_trim AFTER INSERT ON notification_outbox
BEGIN
    DELETE FROM notification_outbox
    WHERE status <> 'pending'
      AND id <= (SELECT id FROM notification_outbox ORDER BY id DESC LIMIT 1 OFFSET 1000);
END;
