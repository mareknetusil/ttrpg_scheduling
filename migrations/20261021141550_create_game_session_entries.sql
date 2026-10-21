-- Content written about a game session: recaps, notes, loot, … (R5). Any member can write one.
CREATE TABLE game_session_entries (
    id INTEGER PRIMARY KEY,
    game_session_id INTEGER NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('recap', 'notes', 'loot', 'other')),
    title TEXT NOT NULL DEFAULT '',
    body_md TEXT NOT NULL,
    author_id INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),

    FOREIGN KEY (game_session_id) REFERENCES game_sessions(id),
    FOREIGN KEY (author_id) REFERENCES users(id)
) STRICT;

CREATE INDEX game_session_entries_session ON game_session_entries (game_session_id);
