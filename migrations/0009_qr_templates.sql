CREATE TABLE qr_templates (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE CHECK (length(name) BETWEEN 1 AND 64 AND name = trim(name)),
    size INTEGER NOT NULL CHECK (size BETWEEN 128 AND 1024),
    foreground TEXT NOT NULL CHECK (length(foreground) = 6 AND foreground NOT GLOB '*[^0-9a-f]*'),
    background TEXT NOT NULL CHECK (length(background) = 6 AND background NOT GLOB '*[^0-9a-f]*'),
    transparent INTEGER NOT NULL CHECK (transparent IN (0, 1)),
    correction TEXT NOT NULL CHECK (correction IN ('L', 'M', 'Q', 'H')),
    quiet_zone INTEGER NOT NULL CHECK (quiet_zone BETWEEN 4 AND 12)
) STRICT;
