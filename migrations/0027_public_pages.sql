-- Visitor-facing pages edited in Settings > Public pages. A missing row means the built-in page.
-- `structured` fills the built-in layout with plain-text copy, an optional accent, and an
-- optional raster logo. `custom` holds HTML that the server sanitized before saving; the server
-- sanitizes it again when it loads the row, so a row written by other means cannot add markup.
CREATE TABLE public_pages (
    page TEXT NOT NULL CHECK (page IN ('missing', 'unknown_host', 'password', 'once')),
    lang TEXT NOT NULL CHECK (lang IN ('en', 'de')),
    mode TEXT NOT NULL CHECK (mode IN ('structured', 'custom')),
    title TEXT CHECK (title IS NULL OR length(title) BETWEEN 1 AND 120),
    message TEXT CHECK (message IS NULL OR length(message) BETWEEN 1 AND 600),
    button TEXT CHECK (button IS NULL OR length(button) BETWEEN 1 AND 60),
    accent TEXT CHECK (
        accent IS NULL
        OR (length(accent) = 7 AND accent GLOB '#[0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f]')
    ),
    logo_type TEXT CHECK (logo_type IS NULL OR logo_type IN ('image/png', 'image/jpeg', 'image/webp')),
    logo BLOB CHECK (logo IS NULL OR length(logo) BETWEEN 1 AND 65536),
    html TEXT CHECK (html IS NULL OR length(CAST(html AS BLOB)) BETWEEN 1 AND 131072),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()) CHECK (updated_at >= 0),
    PRIMARY KEY (page, lang),
    CHECK ((logo IS NULL) = (logo_type IS NULL)),
    CHECK (
        (mode = 'structured' AND html IS NULL)
        OR (mode = 'custom' AND html IS NOT NULL AND title IS NULL AND message IS NULL
            AND button IS NULL AND accent IS NULL AND logo IS NULL)
    ),
    CHECK (page <> 'unknown_host' OR button IS NULL)
) STRICT;

-- Record public page changes in the administrative audit log. The action list keeps every
-- action that migration 0026 accepts.
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
        'public_pages_update', 'public_pages_reset', 'public_pages_import'
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
