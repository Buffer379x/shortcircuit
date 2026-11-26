ALTER TABLE qr_templates ADD COLUMN frame TEXT NOT NULL DEFAULT 'none'
    CHECK (frame IN ('none', 'outline'));
