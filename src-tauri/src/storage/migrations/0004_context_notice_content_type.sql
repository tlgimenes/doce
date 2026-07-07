-- 010-context-window-management: widens messages.content_type's CHECK
-- constraint to accept 'context_notice' (an inline record of a compaction
-- pass -- see data-model.md's `context_notice` row shapes). SQLite can't
-- ALTER a CHECK constraint in place, so this is the same table-rebuild
-- migration 0003 already established: create the new shape, copy every row
-- across with its rowid preserved explicitly (messages_fts is an
-- external-content FTS5 table keyed on content_rowid='rowid' against this
-- table -- letting rowids renumber on the copy would silently desync every
-- existing search hit from its source row), drop the old table, rename the
-- new one into place, then recreate the index and the three FTS5 sync
-- triggers that dropping the old table also drops (they're defined ON
-- messages and do not survive a rename).

CREATE TABLE messages_new (
  id               TEXT PRIMARY KEY,
  conversation_id  TEXT NOT NULL REFERENCES conversations(id),
  role             TEXT NOT NULL CHECK (role IN ('user', 'assistant', 'tool')),
  content_type     TEXT NOT NULL CHECK (content_type IN ('text', 'tool_call', 'tool_result', 'error', 'rich_text', 'context_notice')),
  content          TEXT NOT NULL,
  tool_name        TEXT,
  created_at       INTEGER NOT NULL,
  sequence         INTEGER NOT NULL,
  duration_ms      INTEGER
);

INSERT INTO messages_new (rowid, id, conversation_id, role, content_type, content, tool_name, created_at, sequence, duration_ms)
  SELECT rowid, id, conversation_id, role, content_type, content, tool_name, created_at, sequence, duration_ms
  FROM messages;

DROP TABLE messages;

ALTER TABLE messages_new RENAME TO messages;

CREATE INDEX idx_messages_conversation_sequence ON messages(conversation_id, sequence);

CREATE TRIGGER messages_ai AFTER INSERT ON messages
WHEN (SELECT spawned_by_conversation_id FROM conversations WHERE id = new.conversation_id) IS NULL
BEGIN
  INSERT INTO messages_fts(rowid, content) VALUES (new.rowid, new.content);
END;

CREATE TRIGGER messages_ad AFTER DELETE ON messages BEGIN
  INSERT INTO messages_fts(messages_fts, rowid, content) VALUES ('delete', old.rowid, old.content);
END;

CREATE TRIGGER messages_au AFTER UPDATE ON messages BEGIN
  INSERT INTO messages_fts(messages_fts, rowid, content) VALUES ('delete', old.rowid, old.content);
  INSERT INTO messages_fts(rowid, content)
    SELECT new.rowid, new.content
    WHERE (SELECT spawned_by_conversation_id FROM conversations WHERE id = new.conversation_id) IS NULL;
END;
