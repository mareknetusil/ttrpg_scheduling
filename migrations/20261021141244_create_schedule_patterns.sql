-- A campaign's regular slot, e.g. "Fridays and Saturdays at 18:00, every week" (R2).
-- It doesn't create anything by itself: it fills the candidate dates of a new scheduling poll.
CREATE TABLE schedule_patterns (
    id INTEGER PRIMARY KEY,
    campaign_id INTEGER NOT NULL,
    weekdays INTEGER NOT NULL CHECK (weekdays BETWEEN 1 AND 127),   -- bit 0 = Monday … bit 6 = Sunday
    start_time TEXT NOT NULL CHECK (start_time GLOB '[0-2][0-9]:[0-5][0-9]'),  -- local, DISPLAY_TZ
    every_n_weeks INTEGER NOT NULL DEFAULT 1 CHECK (every_n_weeks BETWEEN 1 AND 4),
    anchor_date TEXT NOT NULL,          -- the Monday of a week that has sessions ('YYYY-MM-DD')
    created_by INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),

    FOREIGN KEY (campaign_id) REFERENCES campaigns(id),
    FOREIGN KEY (created_by) REFERENCES users(id)
) STRICT;

CREATE INDEX schedule_patterns_campaign ON schedule_patterns (campaign_id);
