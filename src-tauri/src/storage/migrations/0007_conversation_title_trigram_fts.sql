-- Title-only fuzzy search support. The regular conversations_fts table remains
-- the exact/token index; this trigram index is used as a typo-tolerant
-- candidate source for conversation titles.
CREATE VIRTUAL TABLE conversations_title_trigram USING fts5(
  title,
  content='conversations',
  content_rowid='rowid',
  tokenize='trigram'
);

INSERT INTO conversations_title_trigram(rowid, title)
  SELECT rowid, title
  FROM conversations
  WHERE spawned_by_conversation_id IS NULL;

CREATE TRIGGER conversations_trigram_ai AFTER INSERT ON conversations
WHEN new.spawned_by_conversation_id IS NULL
BEGIN
  INSERT INTO conversations_title_trigram(rowid, title) VALUES (new.rowid, new.title);
END;

CREATE TRIGGER conversations_trigram_ad AFTER DELETE ON conversations BEGIN
  INSERT INTO conversations_title_trigram(conversations_title_trigram, rowid, title)
    VALUES ('delete', old.rowid, old.title);
END;

CREATE TRIGGER conversations_trigram_au AFTER UPDATE ON conversations BEGIN
  INSERT INTO conversations_title_trigram(conversations_title_trigram, rowid, title)
    VALUES ('delete', old.rowid, old.title);
  INSERT INTO conversations_title_trigram(rowid, title)
    SELECT new.rowid, new.title WHERE new.spawned_by_conversation_id IS NULL;
END;
