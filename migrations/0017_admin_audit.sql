CREATE TABLE admin_audit (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    action TEXT NOT NULL CHECK (action IN (
        'setup_token_issued', 'recovery_token_issued', 'account_setup', 'login', 'account_recovery',
        'password_change', 'key_create', 'key_rotate', 'key_revoke', 'backup_authorize',
        'backup_download', 'restore_authorize', 'restore_validate', 'restore_stage',
        'restore_cancel', 'restore_apply', 'monitoring_update'
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

CREATE TRIGGER admin_audit_trim AFTER INSERT ON admin_audit
BEGIN
    DELETE FROM admin_audit
    WHERE id <= (SELECT id FROM admin_audit ORDER BY id DESC LIMIT 1 OFFSET 1000);
END;
