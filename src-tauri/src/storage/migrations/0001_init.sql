-- data-model.md: Workspace
CREATE TABLE workspaces (
  id            TEXT PRIMARY KEY,
  path          TEXT NOT NULL UNIQUE,
  display_name  TEXT NOT NULL,
  created_at    INTEGER NOT NULL,
  last_opened_at INTEGER NOT NULL
);

-- data-model.md: Conversation
CREATE TABLE conversations (
  id                          TEXT PRIMARY KEY,
  workspace_id                TEXT REFERENCES workspaces(id),
  spawned_by_conversation_id  TEXT REFERENCES conversations(id),
  title                       TEXT NOT NULL,
  created_at                  INTEGER NOT NULL,
  updated_at                  INTEGER NOT NULL
);

CREATE INDEX idx_conversations_workspace ON conversations(workspace_id);
CREATE INDEX idx_conversations_spawned_by ON conversations(spawned_by_conversation_id);

-- data-model.md: Message
CREATE TABLE messages (
  id               TEXT PRIMARY KEY,
  conversation_id  TEXT NOT NULL REFERENCES conversations(id),
  role             TEXT NOT NULL CHECK (role IN ('user', 'assistant', 'tool')),
  content_type     TEXT NOT NULL CHECK (content_type IN ('text', 'tool_call', 'tool_result', 'error')),
  content          TEXT NOT NULL,
  tool_name        TEXT,
  created_at       INTEGER NOT NULL,
  sequence         INTEGER NOT NULL
);

CREATE INDEX idx_messages_conversation_sequence ON messages(conversation_id, sequence);

-- data-model.md: Model
CREATE TABLE models (
  id               TEXT PRIMARY KEY,
  hardware_tier    TEXT NOT NULL,
  source_url       TEXT NOT NULL,
  quantization     TEXT NOT NULL,
  sha256           TEXT NOT NULL,
  local_path       TEXT,
  capability_tags  TEXT NOT NULL DEFAULT '[]',
  installed_at     INTEGER,
  is_active        INTEGER NOT NULL DEFAULT 0 CHECK (is_active IN (0, 1))
);

-- Enforces "exactly one active model" at the database level (data-model.md).
CREATE UNIQUE INDEX idx_models_single_active ON models(is_active) WHERE is_active = 1;

-- data-model.md: MCPServerConnection
CREATE TABLE mcp_server_connections (
  id          TEXT PRIMARY KEY,
  name        TEXT NOT NULL,
  transport   TEXT NOT NULL CHECK (transport IN ('stdio', 'http')),
  config      TEXT NOT NULL,
  enabled     INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
  created_at  INTEGER NOT NULL
);

-- data-model.md: Settings
CREATE TABLE settings (
  key         TEXT PRIMARY KEY,
  value       TEXT NOT NULL,
  updated_at  INTEGER NOT NULL
);

-- data-model.md: Search (FTS5 external-content tables + sync triggers)
CREATE VIRTUAL TABLE messages_fts USING fts5(
  content, content='messages', content_rowid='rowid'
);

CREATE VIRTUAL TABLE conversations_fts USING fts5(
  title, content='conversations', content_rowid='rowid'
);

-- Sync triggers exclude subagent-run conversations (spawned_by_conversation_id
-- IS NOT NULL) from the FTS index, per data-model.md's isolation requirement.
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

CREATE TRIGGER conversations_ai AFTER INSERT ON conversations
WHEN new.spawned_by_conversation_id IS NULL
BEGIN
  INSERT INTO conversations_fts(rowid, title) VALUES (new.rowid, new.title);
END;

CREATE TRIGGER conversations_ad AFTER DELETE ON conversations BEGIN
  INSERT INTO conversations_fts(conversations_fts, rowid, title) VALUES ('delete', old.rowid, old.title);
END;

CREATE TRIGGER conversations_au AFTER UPDATE ON conversations BEGIN
  INSERT INTO conversations_fts(conversations_fts, rowid, title) VALUES ('delete', old.rowid, old.title);
  INSERT INTO conversations_fts(rowid, title)
    SELECT new.rowid, new.title WHERE new.spawned_by_conversation_id IS NULL;
END;
