ALTER TABLE domains ADD COLUMN block_crawlers INTEGER NOT NULL DEFAULT 0
    CHECK (block_crawlers IN (0, 1));

ALTER TABLE link_redirect_policies ADD COLUMN noindex INTEGER NOT NULL DEFAULT 0
    CHECK (noindex IN (0, 1));
