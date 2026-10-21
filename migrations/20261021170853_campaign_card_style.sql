-- Dashboard card personality, chosen by the GM: a border colour from a fixed palette ('' = the
-- default border) and an optional uploaded banner image.
ALTER TABLE campaigns ADD COLUMN accent TEXT NOT NULL DEFAULT ''
    CHECK (accent IN ('', 'red', 'orange', 'amber', 'green', 'teal', 'blue', 'indigo', 'purple', 'pink', 'slate'));

-- The banner lives in the DB (a few hundred KB at most), so backups include it and there's no
-- upload directory to manage. A separate table keeps campaign queries from loading the bytes.
CREATE TABLE campaign_banners (
    campaign_id INTEGER PRIMARY KEY,
    content_type TEXT NOT NULL CHECK (content_type IN ('image/png', 'image/jpeg', 'image/webp')),
    data BLOB NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),

    FOREIGN KEY (campaign_id) REFERENCES campaigns(id)
) STRICT;
