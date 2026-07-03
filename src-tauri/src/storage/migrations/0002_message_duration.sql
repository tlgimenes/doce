-- Frozen generation duration for assistant messages, stamped once at
-- persistence time (duration_ms = persisted_at - created_at). Lets the chat
-- UI show a fixed "took Xs" badge that survives a reload without needing a
-- live stopwatch tied to component lifetime.
ALTER TABLE messages ADD COLUMN duration_ms INTEGER;
