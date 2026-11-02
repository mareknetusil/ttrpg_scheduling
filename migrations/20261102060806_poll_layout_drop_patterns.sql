-- How a scheduling poll shows its candidate dates: 'calendar' (week rows, at most one date per
-- day) or 'list' (one row per date). Polls created before this were lists.
ALTER TABLE schedule_polls
    ADD COLUMN layout TEXT NOT NULL DEFAULT 'list' CHECK (layout IN ('calendar', 'list'));

-- Regular slots are gone: the calendar form picks whole weekdays in one tap.
DROP TABLE schedule_patterns;
