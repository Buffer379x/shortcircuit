-- Opt-in destination host policy. Disabled by default with no rules.
CREATE TABLE destination_policy (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    enabled INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0, 1)),
    generation INTEGER NOT NULL DEFAULT 0 CHECK (generation >= 0)
) STRICT;

INSERT INTO destination_policy (id) VALUES (1);

-- Hosts are stored in their normalized ASCII form: lowercase, IDNA-encoded, no trailing dot.
-- subdomains = 1 matches the host itself and every name below it.
CREATE TABLE destination_rules (
    host TEXT NOT NULL CHECK (
        length(host) BETWEEN 1 AND 255
        AND host = lower(host)
        AND instr(host, char(0)) = 0
        AND substr(host, -1) <> '.'
    ),
    subdomains INTEGER NOT NULL CHECK (subdomains IN (0, 1)),
    action TEXT NOT NULL CHECK (action IN ('allow', 'block')),
    PRIMARY KEY (host, subdomains)
) STRICT, WITHOUT ROWID;

-- Record destination policy changes in the administrative audit log.
DROP TRIGGER admin_audit_trim;

CREATE TABLE admin_audit_next (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    action TEXT NOT NULL CHECK (action IN (
        'setup_token_issued', 'recovery_token_issued', 'account_setup', 'login', 'account_recovery',
        'password_change', 'key_create', 'key_rotate', 'key_revoke', 'backup_authorize',
        'backup_download', 'restore_authorize', 'restore_validate', 'restore_stage',
        'restore_cancel', 'restore_apply', 'monitoring_update', 'destination_policy_update'
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
