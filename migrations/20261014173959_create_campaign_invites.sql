CREATE TABLE campaign_invites (
    id INTEGER PRIMARY KEY,
    campaign_id INTEGER NOT NULL,
    token TEXT NOT NULL UNIQUE,
    created_by INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    expires_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '+14 days')),
    max_uses INTEGER NOT NULL,
    use_count INTEGER NOT NULL DEFAULT 0,

    FOREIGN KEY (campaign_id) REFERENCES campaigns(id),
    FOREIGN KEY (created_by) REFERENCES users(id),
    CHECK(max_uses > 0),
    CHECK(use_count <= max_uses)
) STRICT;
