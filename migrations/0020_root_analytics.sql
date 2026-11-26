-- Count requests for a domain root path separately from short-link outcomes.
-- root_fallback: the domain's configured root destination answered with a redirect.
-- root_missing: the root path had no usable destination and returned 404.
-- Existing rows keep their kind, so redirect, blocked, and missing totals are unchanged.
DROP TRIGGER delete_link_analytics_daily;

CREATE TABLE analytics_events_next (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    occurred_at INTEGER NOT NULL CHECK (occurred_at >= 0),
    link_id INTEGER REFERENCES links(id) ON DELETE CASCADE CHECK (link_id > 0),
    host TEXT NOT NULL CHECK (length(host) BETWEEN 1 AND 253),
    kind TEXT NOT NULL CHECK (kind IN ('redirect', 'blocked', 'missing', 'root_fallback', 'root_missing')),
    visit_class TEXT NOT NULL CHECK (visit_class IN ('human', 'bot', 'unknown')),
    source TEXT NOT NULL CHECK (
        length(source) BETWEEN 1 AND 40
        AND instr(source, char(0)) = 0
        AND source NOT GLOB '*[^a-z0-9_-]*'
    ),
    referrer TEXT CHECK (
        length(referrer) BETWEEN 1 AND 253
        AND instr(referrer, char(0)) = 0
        AND (
            referrer NOT GLOB '*[^a-z0-9.-]*'
            OR (
                substr(referrer, 1, 1) = '['
                AND substr(referrer, -1) = ']'
                AND instr(referrer, ':') > 0
                AND substr(referrer, 2, length(referrer) - 2) NOT GLOB '*[^0-9a-f:.]*'
            )
        )
    ),
    browser TEXT NOT NULL DEFAULT 'unknown'
        CHECK (browser IN ('chrome', 'edge', 'firefox', 'opera', 'safari', 'unknown')),
    os TEXT NOT NULL DEFAULT 'unknown'
        CHECK (os IN ('android', 'chromeos', 'ios', 'linux', 'macos', 'windows', 'unknown')),
    device TEXT NOT NULL DEFAULT 'unknown'
        CHECK (device IN ('desktop', 'mobile', 'tablet', 'unknown')),
    traffic_source TEXT NOT NULL DEFAULT 'unknown'
        CHECK (traffic_source IN ('automated', 'direct', 'internal', 'preview', 'referral', 'unknown')),
    -- Root requests never belong to a link.
    CHECK (kind NOT IN ('root_fallback', 'root_missing') OR link_id IS NULL)
) STRICT;

INSERT INTO analytics_events_next (id, occurred_at, link_id, host, kind, visit_class, source,
                                   referrer, browser, os, device, traffic_source)
SELECT id, occurred_at, link_id, host, kind, visit_class, source,
       referrer, browser, os, device, traffic_source
FROM analytics_events;

-- Keep event IDs monotonic for cursors and deletion requests.
DELETE FROM sqlite_sequence WHERE name = 'analytics_events_next';
INSERT INTO sqlite_sequence (name, seq)
SELECT 'analytics_events_next', seq FROM sqlite_sequence WHERE name = 'analytics_events';

DROP TABLE analytics_events;
ALTER TABLE analytics_events_next RENAME TO analytics_events;
CREATE INDEX analytics_events_by_time ON analytics_events (occurred_at, id);
CREATE INDEX analytics_events_by_host ON analytics_events (host, occurred_at, id);
CREATE INDEX analytics_events_by_link ON analytics_events (link_id, occurred_at, id);

CREATE TABLE analytics_daily_next (
    day INTEGER NOT NULL CHECK (day >= 0),
    host TEXT NOT NULL CHECK (length(host) BETWEEN 1 AND 253),
    link_id INTEGER NOT NULL CHECK (link_id >= 0),
    kind TEXT NOT NULL CHECK (kind IN ('redirect', 'blocked', 'missing', 'root_fallback', 'root_missing')),
    visit_class TEXT NOT NULL CHECK (visit_class IN ('human', 'bot', 'unknown')),
    total INTEGER NOT NULL CHECK (total > 0),
    browser TEXT NOT NULL DEFAULT 'unknown'
        CHECK (browser IN ('chrome', 'edge', 'firefox', 'opera', 'safari', 'unknown')),
    os TEXT NOT NULL DEFAULT 'unknown'
        CHECK (os IN ('android', 'chromeos', 'ios', 'linux', 'macos', 'windows', 'unknown')),
    device TEXT NOT NULL DEFAULT 'unknown'
        CHECK (device IN ('desktop', 'mobile', 'tablet', 'unknown')),
    traffic_source TEXT NOT NULL DEFAULT 'unknown'
        CHECK (traffic_source IN ('automated', 'direct', 'internal', 'preview', 'referral', 'unknown')),
    CHECK (kind NOT IN ('root_fallback', 'root_missing') OR link_id = 0),
    PRIMARY KEY (day, host, link_id, kind, visit_class, browser, os, device, traffic_source)
) STRICT, WITHOUT ROWID;

INSERT INTO analytics_daily_next (day, host, link_id, kind, visit_class, total,
                                  browser, os, device, traffic_source)
SELECT day, host, link_id, kind, visit_class, total, browser, os, device, traffic_source
FROM analytics_daily;

DROP TABLE analytics_daily;
ALTER TABLE analytics_daily_next RENAME TO analytics_daily;
CREATE INDEX analytics_daily_by_host ON analytics_daily (host, day);
CREATE INDEX analytics_daily_by_link ON analytics_daily (link_id, day);

CREATE TRIGGER delete_link_analytics_daily
AFTER DELETE ON links
WHEN OLD.id > 0
BEGIN
    DELETE FROM analytics_daily WHERE link_id = OLD.id;
END;
