-- R7: comments on a campaign, or on one of its game sessions (game_session_id set).
CREATE TABLE comments (
    id INTEGER PRIMARY KEY,
    campaign_id INTEGER NOT NULL,
    game_session_id INTEGER,
    author_id INTEGER NOT NULL,
    body_md TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    edited_at TEXT,

    FOREIGN KEY (campaign_id) REFERENCES campaigns(id),
    FOREIGN KEY (game_session_id) REFERENCES game_sessions(id),
    FOREIGN KEY (author_id) REFERENCES users(id)
) STRICT;

CREATE INDEX comments_campaign_session ON comments (campaign_id, game_session_id);

-- R8: polls (single or multiple choice) on a campaign, or on one of its game sessions.
CREATE TABLE polls (
    id INTEGER PRIMARY KEY,
    campaign_id INTEGER NOT NULL,
    game_session_id INTEGER,
    question TEXT NOT NULL,
    description_md TEXT NOT NULL DEFAULT '',
    multiple_choice INTEGER NOT NULL DEFAULT 0 CHECK (multiple_choice IN (0, 1)),
    closes_at TEXT,                     -- optional automatic close
    closed_at TEXT,                     -- set when the GM closes it
    created_by INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),

    FOREIGN KEY (campaign_id) REFERENCES campaigns(id),
    FOREIGN KEY (game_session_id) REFERENCES game_sessions(id),
    FOREIGN KEY (created_by) REFERENCES users(id)
) STRICT;

CREATE INDEX polls_campaign_session ON polls (campaign_id, game_session_id);

CREATE TABLE poll_options (
    id INTEGER PRIMARY KEY,
    poll_id INTEGER NOT NULL,
    label TEXT NOT NULL,
    sort_order INTEGER NOT NULL,

    FOREIGN KEY (poll_id) REFERENCES polls(id)
) STRICT;

CREATE INDEX poll_options_poll ON poll_options (poll_id);

CREATE TABLE poll_votes (
    poll_option_id INTEGER NOT NULL,
    user_id INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),

    FOREIGN KEY (poll_option_id) REFERENCES poll_options(id),
    FOREIGN KEY (user_id) REFERENCES users(id),
    PRIMARY KEY (poll_option_id, user_id)
) STRICT;
