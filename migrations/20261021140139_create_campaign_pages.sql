CREATE TABLE campaign_pages (
    id INTEGER PRIMARY KEY,
    campaign_id INTEGER NOT NULL,
    title TEXT NOT NULL,
    slug TEXT NOT NULL,
    body_md TEXT NOT NULL DEFAULT '',
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_by INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),

    FOREIGN KEY (campaign_id) REFERENCES campaigns(id),
    FOREIGN KEY (created_by) REFERENCES users(id),
    UNIQUE (campaign_id, slug)
) STRICT;
