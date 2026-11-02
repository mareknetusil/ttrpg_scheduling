-- The scheduling poll's voting rules, written once. Every query that writes an answer, decides,
-- or counts dates to answer selects from these views instead of repeating the conditions.

-- Scheduling polls members can vote in: open, and the deadline (if any) hasn't passed.
CREATE VIEW voting_polls AS
SELECT id
FROM schedule_polls
WHERE status = 'open'
  AND (deadline_at IS NULL OR deadline_at > strftime('%Y-%m-%dT%H:%M:%SZ', 'now'));

-- Candidate dates still ahead (whatever their poll's state).
CREATE VIEW upcoming_options AS
SELECT id, poll_id, starts_at
FROM schedule_options
WHERE starts_at > strftime('%Y-%m-%dT%H:%M:%SZ', 'now');

-- Candidate dates that can be answered right now: still ahead, in a poll open for voting.
CREATE VIEW votable_options AS
SELECT id, poll_id
FROM upcoming_options
WHERE poll_id IN (SELECT id FROM voting_polls);
