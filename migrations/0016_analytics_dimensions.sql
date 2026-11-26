ALTER TABLE analytics_events ADD COLUMN browser TEXT NOT NULL DEFAULT 'unknown'
    CHECK (browser IN ('chrome', 'edge', 'firefox', 'opera', 'safari', 'unknown'));
ALTER TABLE analytics_events ADD COLUMN os TEXT NOT NULL DEFAULT 'unknown'
    CHECK (os IN ('android', 'chromeos', 'ios', 'linux', 'macos', 'windows', 'unknown'));
ALTER TABLE analytics_events ADD COLUMN device TEXT NOT NULL DEFAULT 'unknown'
    CHECK (device IN ('desktop', 'mobile', 'tablet', 'unknown'));
ALTER TABLE analytics_events ADD COLUMN traffic_source TEXT NOT NULL DEFAULT 'unknown'
    CHECK (traffic_source IN ('automated', 'direct', 'internal', 'preview', 'referral', 'unknown'));

DROP TRIGGER delete_link_analytics_daily;
ALTER TABLE analytics_daily RENAME TO previous_analytics_daily;

CREATE TABLE analytics_daily (
    day INTEGER NOT NULL CHECK (day >= 0),
    host TEXT NOT NULL CHECK (length(host) BETWEEN 1 AND 253),
    link_id INTEGER NOT NULL CHECK (link_id >= 0),
    kind TEXT NOT NULL CHECK (kind IN ('redirect', 'blocked', 'missing')),
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
    PRIMARY KEY (day, host, link_id, kind, visit_class, browser, os, device, traffic_source)
) STRICT, WITHOUT ROWID;

-- Older detail events share the same unknown dimensions as their daily counts.
INSERT INTO analytics_daily (day, host, link_id, kind, visit_class, total)
SELECT day, host, link_id, kind, visit_class, total FROM previous_analytics_daily;

DROP TABLE previous_analytics_daily;
CREATE INDEX analytics_daily_by_host ON analytics_daily (host, day);
CREATE INDEX analytics_daily_by_link ON analytics_daily (link_id, day);

CREATE TRIGGER delete_link_analytics_daily
AFTER DELETE ON links
WHEN OLD.id > 0
BEGIN
    DELETE FROM analytics_daily WHERE link_id = OLD.id;
END;
