-- Comments on a scheduling poll ("Sat works only if we start at 19:00"). A comment belongs to the
-- campaign itself, to one game session, or to one scheduling poll, never to both of the latter.
ALTER TABLE comments ADD COLUMN schedule_poll_id INTEGER
    REFERENCES schedule_polls(id)
    CHECK (schedule_poll_id IS NULL OR game_session_id IS NULL);

CREATE INDEX comments_schedule_poll ON comments (schedule_poll_id);
