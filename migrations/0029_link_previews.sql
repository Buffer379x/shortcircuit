-- Preview images for saved destinations: the page's own og:image or twitter:image, otherwise its
-- icon. One row per link records the latest attempt. Only a re-encoded JPEG thumbnail is kept,
-- never the original file or its metadata, and only the origin of the image URL, which can
-- otherwise carry tokens.
CREATE TABLE link_previews (
    link_id INTEGER PRIMARY KEY REFERENCES links(id) ON DELETE CASCADE,
    revision TEXT NOT NULL DEFAULT (lower(hex(randomblob(16))))
        CHECK (length(revision) = 32 AND revision NOT GLOB '*[^0-9a-f]*'),
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'ready', 'none', 'failed', 'skipped')),
    failure TEXT CHECK (failure IS NULL OR failure IN (
        'blocked', 'busy', 'dns', 'timeout', 'connection', 'protocol', 'not_html',
        'too_large', 'unsupported', 'policy', 'restored'
    )),
    kind TEXT CHECK (kind IS NULL OR kind IN ('image', 'icon')),
    mime TEXT CHECK (mime IS NULL OR mime = 'image/jpeg'),
    bytes BLOB CHECK (bytes IS NULL OR length(bytes) BETWEEN 1 AND 49152),
    width INTEGER CHECK (width IS NULL OR width BETWEEN 1 AND 480),
    height INTEGER CHECK (height IS NULL OR height BETWEEN 1 AND 270),
    -- Scheme, host, and port of the image URL only.
    source_origin TEXT CHECK (
        source_origin IS NULL OR (
            length(source_origin) BETWEEN 8 AND 300
            AND (substr(source_origin, 1, 7) = 'http://' OR substr(source_origin, 1, 8) = 'https://')
            AND instr(substr(source_origin, 9), '/') = 0
            AND instr(source_origin, '?') = 0 AND instr(source_origin, '#') = 0
            AND instr(source_origin, '@') = 0
        )
    ),
    requested_at INTEGER NOT NULL DEFAULT (unixepoch()),
    fetched_at INTEGER,
    CHECK ((status = 'pending') = (fetched_at IS NULL)),
    CHECK ((status = 'ready') = (bytes IS NOT NULL)),
    CHECK ((status = 'ready') = (
        kind IS NOT NULL AND mime IS NOT NULL AND width IS NOT NULL AND height IS NOT NULL
        AND source_origin IS NOT NULL
    )),
    CHECK ((status IN ('failed', 'skipped')) = (failure IS NOT NULL)),
    CHECK (kind IS NOT 'icon' OR (width <= 64 AND height <= 64))
) STRICT;

CREATE INDEX link_previews_pending ON link_previews (status, requested_at, link_id);

-- A changed host or destination clears the old preview and queues a new fetch.
CREATE TRIGGER link_preview_target_changed
AFTER UPDATE OF host, target ON links
WHEN old.host IS NOT new.host OR old.target IS NOT new.target
BEGIN
    UPDATE link_previews SET
        revision = lower(hex(randomblob(16))), status = 'pending', failure = NULL, kind = NULL,
        mime = NULL, bytes = NULL, width = NULL, height = NULL, source_origin = NULL,
        requested_at = unixepoch(), fetched_at = NULL
    WHERE link_id = new.id;
END;

-- Password or one-time protection removes the preview and any pending fetch. Protected links
-- are never fetched.
CREATE TRIGGER link_preview_protected_insert
AFTER INSERT ON link_protection
WHEN new.one_time = 1 OR new.password_hash IS NOT NULL
BEGIN
    DELETE FROM link_previews WHERE link_id = new.link_id;
END;

CREATE TRIGGER link_preview_protected_update
AFTER UPDATE OF one_time, password_hash ON link_protection
WHEN new.one_time = 1 OR new.password_hash IS NOT NULL
BEGIN
    DELETE FROM link_previews WHERE link_id = new.link_id;
END;

-- The "Preview images" switch in Settings > Operations. NULL means not saved here: the
-- PREVIEW_IMAGES_ENABLED container variable applies, otherwise off. Previews also require
-- title lookup, because they come from the same page request.
ALTER TABLE runtime_settings ADD COLUMN preview_images_enabled INTEGER
    CHECK (preview_images_enabled IN (0, 1));
