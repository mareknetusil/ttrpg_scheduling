-- Game sessions exist from M6 on, because deciding a scheduling poll creates one (R3).
CREATE TABLE game_sessions (
    id INTEGER PRIMARY KEY,
    campaign_id INTEGER NOT NULL,
    number INTEGER NOT NULL,                  -- 1, 2, 3, … per campaign
    title TEXT NOT NULL DEFAULT '',
    starts_at TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'planned' CHECK (status IN ('planned', 'played', 'cancelled')),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),

    FOREIGN KEY (campaign_id) REFERENCES campaigns(id),
    UNIQUE (campaign_id, number)
) STRICT;

CREATE INDEX game_sessions_campaign_starts ON game_sessions (campaign_id, starts_at);

CREATE TABLE game_session_attendance (
    game_session_id INTEGER NOT NULL,
    user_id INTEGER NOT NULL,

    FOREIGN KEY (game_session_id) REFERENCES game_sessions(id),
    FOREIGN KEY (user_id) REFERENCES users(id),
    PRIMARY KEY (game_session_id, user_id)
) STRICT;

CREATE TABLE schedule_polls (
    id INTEGER PRIMARY KEY,
    campaign_id INTEGER NOT NULL,
    title TEXT NOT NULL,
    description_md TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'decided', 'cancelled')),
    deadline_at TEXT,                         -- optional "please vote by"
    created_by INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    decided_option_id INTEGER,
    game_session_id INTEGER,

    FOREIGN KEY (campaign_id) REFERENCES campaigns(id),
    FOREIGN KEY (created_by) REFERENCES users(id),
    FOREIGN KEY (decided_option_id) REFERENCES schedule_options(id),
    FOREIGN KEY (game_session_id) REFERENCES game_sessions(id),
    -- decided ⇔ both decision columns are set
    CHECK ((status = 'decided') = (decided_option_id IS NOT NULL AND game_session_id IS NOT NULL))
) STRICT;

CREATE INDEX schedule_polls_campaign ON schedule_polls (campaign_id);

CREATE TABLE schedule_options (
    id INTEGER PRIMARY KEY,
    poll_id INTEGER NOT NULL,
    starts_at TEXT NOT NULL,

    FOREIGN KEY (poll_id) REFERENCES schedule_polls(id),
    UNIQUE (poll_id, starts_at)
) STRICT;

CREATE TABLE schedule_votes (
    option_id INTEGER NOT NULL,
    user_id INTEGER NOT NULL,
    answer TEXT NOT NULL CHECK (answer IN ('yes', 'maybe', 'no')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),

    FOREIGN KEY (option_id) REFERENCES schedule_options(id),
    FOREIGN KEY (user_id) REFERENCES users(id),
    PRIMARY KEY (option_id, user_id)
) STRICT;
