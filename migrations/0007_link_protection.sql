CREATE TABLE link_protection (
    link_id INTEGER PRIMARY KEY REFERENCES links(id) ON DELETE CASCADE,
    password_hash TEXT,
    one_time INTEGER NOT NULL DEFAULT 0 CHECK (one_time IN (0, 1)),
    consumed INTEGER NOT NULL DEFAULT 0 CHECK (consumed IN (0, 1)),
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision >= 1),
    CHECK (consumed = 0 OR one_time = 1)
) STRICT;
