-- Accounts are never deleted (their campaigns, votes and comments keep pointing at them);
-- an admin deactivates them instead. A deactivated user can't log in.
ALTER TABLE users ADD COLUMN deactivated_at TEXT;
