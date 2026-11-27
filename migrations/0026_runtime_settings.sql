-- Operational settings saved in Settings > Operations. A NULL column is not saved here: the
-- container variable applies, otherwise the built-in default. Saving a value makes it
-- authoritative until it is reset, which sets the column back to NULL.
CREATE TABLE runtime_settings (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    monitor_enabled INTEGER CHECK (monitor_enabled IN (0, 1)),
    monitor_interval_hours INTEGER CHECK (monitor_interval_hours BETWEEN 1 AND 720),
    monitor_priority_hours INTEGER CHECK (monitor_priority_hours BETWEEN 1 AND 720),
    title_lookup_enabled INTEGER CHECK (title_lookup_enabled IN (0, 1)),
    -- 0 turns scheduled backups off.
    backup_interval_hours INTEGER CHECK (backup_interval_hours BETWEEN 0 AND 720),
    backup_keep INTEGER CHECK (backup_keep BETWEEN 1 AND 365),
    -- '' saves "no webhook", which turns notifications off. Secret: never returned by the API
    -- and removed from verified snapshots.
    notification_webhook_url TEXT CHECK (
        notification_webhook_url = ''
        OR (length(notification_webhook_url) BETWEEN 9 AND 4096
            AND substr(notification_webhook_url, 1, 8) = 'https://')
    ),
    -- '' saves "no Authorization header". Secret, like the webhook URL.
    notification_authorization TEXT CHECK (length(notification_authorization) <= 4096),
    -- Comma-separated event kinds.
    notification_events TEXT CHECK (
        length(notification_events) BETWEEN 1 AND 200
        AND notification_events NOT GLOB '*[^a-z_,]*'
    ),
    -- Comma-separated redirect counts; '' saves an empty list.
    notification_click_milestones TEXT CHECK (
        length(notification_click_milestones) <= 200
        AND notification_click_milestones NOT GLOB '*[^0-9,]*'
    ),
    notification_expiry_notice_hours INTEGER CHECK (
        notification_expiry_notice_hours BETWEEN 1 AND 720
    ),
    updated_at INTEGER CHECK (updated_at >= 0),
    CHECK (
        monitor_interval_hours IS NULL OR monitor_priority_hours IS NULL
        OR monitor_priority_hours <= monitor_interval_hours
    )
) STRICT;

INSERT INTO runtime_settings (id) VALUES (1);

-- Record operational settings changes and their password confirmations in the audit log.
DROP TRIGGER admin_audit_trim;

CREATE TABLE admin_audit_next (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    action TEXT NOT NULL CHECK (action IN (
        'setup_token_issued', 'recovery_token_issued', 'account_setup', 'login', 'account_recovery',
        'password_change', 'key_create', 'key_rotate', 'key_revoke', 'backup_authorize',
        'backup_download', 'restore_authorize', 'restore_validate', 'restore_stage',
        'restore_cancel', 'restore_apply', 'monitoring_update', 'destination_policy_update',
        'operations_settings_authorize', 'operations_settings_update'
    )),
    actor TEXT NOT NULL CHECK (actor IN ('admin', 'system', 'api_key')),
    actor_id INTEGER,
    object_id INTEGER CHECK (object_id > 0),
    outcome TEXT NOT NULL CHECK (outcome IN ('success', 'rejected', 'failed')),
    CHECK (
        (actor = 'system' AND actor_id IS NULL) OR
        (actor = 'admin' AND actor_id IS 1) OR
        (actor = 'api_key' AND actor_id IS NOT NULL AND actor_id > 0)
    )
) STRICT;

INSERT INTO admin_audit_next (id, created_at, action, actor, actor_id, object_id, outcome)
SELECT id, created_at, action, actor, actor_id, object_id, outcome FROM admin_audit;

DELETE FROM sqlite_sequence WHERE name = 'admin_audit_next';
INSERT INTO sqlite_sequence (name, seq)
SELECT 'admin_audit_next', seq FROM sqlite_sequence WHERE name = 'admin_audit';

DROP TABLE admin_audit;
ALTER TABLE admin_audit_next RENAME TO admin_audit;

CREATE TRIGGER admin_audit_trim AFTER INSERT ON admin_audit
BEGIN
    DELETE FROM admin_audit
    WHERE id <= (SELECT id FROM admin_audit ORDER BY id DESC LIMIT 1 OFFSET 1000);
END;
