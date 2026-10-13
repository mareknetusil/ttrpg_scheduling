CREATE TABLE campaigns (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    system TEXT NOT NULL,
    tagline TEXT NOT NULL DEFAULT '',
    description_md TEXT NOT NULL DEFAULT '',
    gm_user_id INTEGER NOT NULL,
    created_by INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    archived_at TEXT,

    FOREIGN KEY (gm_user_id) REFERENCES users(id),
    FOREIGN KEY (created_by) REFERENCES users(id),
    FOREIGN KEY (id, gm_user_id) REFERENCES campaign_members(campaign_id, user_id) DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE campaign_members (
    campaign_id INTEGER NOT NULL,
    user_id INTEGER NOT NULL,
    joined_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),

    FOREIGN KEY (user_id) REFERENCES users(id),
    FOREIGN KEY (campaign_id) REFERENCES campaigns(id),
    PRIMARY KEY (campaign_id, user_id)
) STRICT;

CREATE INDEX campaign_members_user_id ON campaign_members (user_id);
