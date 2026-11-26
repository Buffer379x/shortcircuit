ALTER TABLE links ADD COLUMN selection_key TEXT NOT NULL DEFAULT ''
    CHECK (selection_key = '' OR (length(selection_key) = 32 AND selection_key NOT GLOB '*[^0-9a-f]*'));

UPDATE links SET selection_key = lower(hex(randomblob(16)));
CREATE UNIQUE INDEX links_selection_key ON links(selection_key);

CREATE TRIGGER links_assign_selection_key
AFTER INSERT ON links
BEGIN
    UPDATE links SET selection_key = lower(hex(randomblob(16))) WHERE id = NEW.id;
END;
