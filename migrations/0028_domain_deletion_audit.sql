-- Record domain deletions in the administrative audit log. The action list keeps every action
-- that earlier migrations accept. The new nullable `details` column holds a small JSON object
-- with the deleted host, the link action, the move target, and the affected link count.
-- Existing rows keep their IDs and values and have no details.
DROP TRIGGER admin_audit_trim;

CREATE TABLE admin_audit_next (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    action TEXT NOT NULL CHECK (action IN (
        'setup_token_issued', 'recovery_token_issued', 'account_setup', 'login', 'account_recovery',
        'password_change', 'key_create', 'key_rotate', 'key_revoke', 'backup_authorize',
        'backup_download', 'restore_authorize', 'restore_validate', 'restore_stage',
        'restore_cancel', 'restore_apply', 'monitoring_update', 'destination_policy_update',
        'operations_settings_authorize', 'operations_settings_update',
        'public_pages_update', 'public_pages_reset', 'public_pages_import',
        'domain_delete'
    )),
    actor TEXT NOT NULL CHECK (actor IN ('admin', 'system', 'api_key')),
    actor_id INTEGER,
    object_id INTEGER CHECK (object_id > 0),
    outcome TEXT NOT NULL CHECK (outcome IN ('success', 'rejected', 'failed')),
    details TEXT CHECK (
        details IS NULL OR (
            length(details) <= 1024 AND json_valid(details) AND json_type(details) = 'object'
        )
    ),
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
