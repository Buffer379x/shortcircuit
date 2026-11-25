CREATE TABLE link_redirect_policies (
    link_id INTEGER PRIMARY KEY REFERENCES links(id) ON DELETE CASCADE,
    status INTEGER NOT NULL CHECK (status IN (301, 302, 307, 308)),
    starts_at INTEGER,
    expires_at INTEGER,
    max_visits INTEGER CHECK (max_visits > 0),
    visits_used INTEGER NOT NULL DEFAULT 0 CHECK (visits_used >= 0),
    forward_query INTEGER NOT NULL CHECK (forward_query IN (0, 1)),
    forward_path INTEGER NOT NULL CHECK (forward_path IN (0, 1)),
    rules TEXT NOT NULL CHECK (json_valid(rules) AND json_type(rules) = 'array'),
    CHECK (starts_at IS NULL OR expires_at IS NULL OR starts_at < expires_at),
    CHECK (status IN (302, 307) OR (starts_at IS NULL AND expires_at IS NULL AND max_visits IS NULL))
) STRICT;
