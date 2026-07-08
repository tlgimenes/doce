ALTER TABLE conversations ADD COLUMN last_seen_at INTEGER NOT NULL DEFAULT 0;

UPDATE conversations
SET last_seen_at = updated_at
WHERE last_seen_at = 0;
