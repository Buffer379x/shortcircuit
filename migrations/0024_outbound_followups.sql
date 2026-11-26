-- A random epoch distinguishes notification IDs after a restore, when database IDs can repeat.
ALTER TABLE notification_state ADD COLUMN epoch TEXT NOT NULL
    DEFAULT '00000000000000000000000000000000'
    CHECK (length(epoch) = 32 AND epoch NOT GLOB '*[^0-9a-f]*');
UPDATE notification_state SET epoch = lower(hex(randomblob(16)));

-- Keep settled rows for at least eight days, longer than any detection window, so a trimmed
-- event key cannot be detected again. Pending rows are never trimmed.
DROP TRIGGER notification_outbox_trim;
CREATE TRIGGER notification_outbox_trim AFTER INSERT ON notification_outbox
BEGIN
    DELETE FROM notification_outbox
    WHERE status <> 'pending'
      AND created_at < unixepoch() - 8 * 86400
      AND id <= (SELECT id FROM notification_outbox ORDER BY id DESC LIMIT 1 OFFSET 1000);
END;

-- Adding password or one-time protection cancels a pending title lookup before any fetch.
CREATE TRIGGER link_title_lookup_protected_insert
AFTER INSERT ON link_protection
WHEN new.one_time = 1 OR new.password_hash IS NOT NULL
BEGIN
    DELETE FROM link_title_lookups WHERE link_id = new.link_id AND status = 'pending';
END;

CREATE TRIGGER link_title_lookup_protected_update
AFTER UPDATE OF one_time, password_hash ON link_protection
WHEN new.one_time = 1 OR new.password_hash IS NOT NULL
BEGIN
    DELETE FROM link_title_lookups WHERE link_id = new.link_id AND status = 'pending';
END;
