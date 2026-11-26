ALTER TABLE links ADD COLUMN description TEXT NOT NULL DEFAULT ''
    CHECK (length(description) <= 500 AND instr(description, char(0)) = 0);
