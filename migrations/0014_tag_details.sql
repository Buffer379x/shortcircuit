CREATE TABLE tag_details (
    name TEXT PRIMARY KEY CHECK (
        length(name) BETWEEN 1 AND 40
        AND instr(name, ',') = 0 AND instr(name, char(0)) = 0
    ),
    color TEXT CHECK (
        color IS NULL OR (
            length(color) = 7 AND substr(color, 1, 1) = '#' AND instr(color, char(0)) = 0
            AND substr(color, 2) NOT GLOB '*[^0-9a-f]*'
        )
    ),
    description TEXT NOT NULL DEFAULT ''
        CHECK (length(description) <= 500 AND instr(description, char(0)) = 0),
    CHECK (color IS NOT NULL OR description <> '')
) STRICT, WITHOUT ROWID;
