CREATE TABLE link_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    link_id INTEGER NOT NULL REFERENCES links(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    actor_kind TEXT NOT NULL CHECK (actor_kind IN ('admin', 'api_key')),
    actor_label TEXT NOT NULL CHECK (length(actor_label) BETWEEN 1 AND 160),
    action TEXT NOT NULL CHECK (action IN ('metadata', 'policy', 'protection', 'restore')),
    previous TEXT CHECK (previous IS NULL OR (json_valid(previous) AND json_type(previous) = 'object')),
    CHECK ((action = 'protection') = (previous IS NULL))
) STRICT;

CREATE INDEX link_history_by_link ON link_history (link_id, id DESC);
CREATE INDEX link_history_by_age ON link_history (created_at);
