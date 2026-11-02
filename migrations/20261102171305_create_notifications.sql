-- Notifications (see GLOSSARY.md): one row per recipient, written in the same transaction as the
-- change that caused it. What it points to is a set of nullable foreign keys rather than a
-- polymorphic target_type/target_id, like comments and polls. While unseen, a newer action of the
-- same kind by the same actor on the same thing updates the row instead of adding one.
CREATE TABLE notifications (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL,           -- the recipient
    campaign_id INTEGER NOT NULL,
    actor_id INTEGER NOT NULL,          -- who did it (never the recipient)
    kind TEXT NOT NULL CHECK (kind IN (
        'schedule_poll_opened', 'schedule_poll_decided', 'schedule_poll_cancelled',
        'schedule_poll_reopened', 'poll_created', 'poll_closed', 'session_added',
        'session_moved', 'session_cancelled', 'entry_added', 'entry_edited', 'comment_added',
        'page_added', 'page_edited', 'campaign_edited', 'gm_changed')),
    game_session_id INTEGER,
    schedule_poll_id INTEGER,
    poll_id INTEGER,
    entry_id INTEGER,
    comment_id INTEGER,
    page_id INTEGER,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),  -- latest action
    seen_at TEXT,

    FOREIGN KEY (user_id) REFERENCES users(id),
    FOREIGN KEY (campaign_id) REFERENCES campaigns(id),
    FOREIGN KEY (actor_id) REFERENCES users(id),
    FOREIGN KEY (game_session_id) REFERENCES game_sessions(id),
    FOREIGN KEY (schedule_poll_id) REFERENCES schedule_polls(id),
    FOREIGN KEY (poll_id) REFERENCES polls(id),
    FOREIGN KEY (entry_id) REFERENCES game_session_entries(id),
    FOREIGN KEY (comment_id) REFERENCES comments(id),
    FOREIGN KEY (page_id) REFERENCES campaign_pages(id)
) STRICT;

CREATE INDEX notifications_user ON notifications (user_id, seen_at, updated_at);
CREATE INDEX notifications_campaign ON notifications (campaign_id);
