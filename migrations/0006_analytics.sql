ALTER TABLE links ADD COLUMN analytics_enabled INTEGER NOT NULL DEFAULT 1
    CHECK (analytics_enabled IN (0, 1));

CREATE TABLE analytics_settings (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    referrers INTEGER NOT NULL DEFAULT 1 CHECK (referrers IN (0, 1)),
    classify INTEGER NOT NULL DEFAULT 1 CHECK (classify IN (0, 1)),
    honor_dnt INTEGER NOT NULL DEFAULT 1 CHECK (honor_dnt IN (0, 1)),
    detail_days INTEGER NOT NULL DEFAULT 90 CHECK (detail_days BETWEEN 1 AND 3650),
    aggregate_days INTEGER NOT NULL DEFAULT 730 CHECK (aggregate_days BETWEEN 1 AND 3650),
    generation INTEGER NOT NULL DEFAULT 0 CHECK (generation >= 0)
) STRICT;

INSERT INTO analytics_settings (id) VALUES (1);

CREATE TABLE analytics_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    occurred_at INTEGER NOT NULL CHECK (occurred_at >= 0),
    link_id INTEGER REFERENCES links(id) ON DELETE CASCADE CHECK (link_id > 0),
    host TEXT NOT NULL CHECK (length(host) BETWEEN 1 AND 253),
    kind TEXT NOT NULL CHECK (kind IN ('redirect', 'blocked', 'missing')),
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
    )
) STRICT;

CREATE INDEX analytics_events_by_time ON analytics_events (occurred_at, id);
CREATE INDEX analytics_events_by_host ON analytics_events (host, occurred_at, id);
CREATE INDEX analytics_events_by_link ON analytics_events (link_id, occurred_at, id);

-- The zero link ID groups outcomes with no associated link.
CREATE TABLE analytics_daily (
    day INTEGER NOT NULL CHECK (day >= 0),
    host TEXT NOT NULL CHECK (length(host) BETWEEN 1 AND 253),
    link_id INTEGER NOT NULL CHECK (link_id >= 0),
    kind TEXT NOT NULL CHECK (kind IN ('redirect', 'blocked', 'missing')),
    visit_class TEXT NOT NULL CHECK (visit_class IN ('human', 'bot', 'unknown')),
    total INTEGER NOT NULL CHECK (total > 0),
    PRIMARY KEY (day, host, link_id, kind, visit_class)
) STRICT, WITHOUT ROWID;

CREATE INDEX analytics_daily_by_host ON analytics_daily (host, day);
CREATE INDEX analytics_daily_by_link ON analytics_daily (link_id, day);

CREATE TRIGGER delete_link_analytics_daily
AFTER DELETE ON links
WHEN OLD.id > 0
BEGIN
    DELETE FROM analytics_daily WHERE link_id = OLD.id;
END;
