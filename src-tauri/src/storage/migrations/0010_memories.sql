-- SP4: durable per-workspace agent memory. `workspace_id` mirrors
-- conversations.workspace_id exactly (nullable, same FK target) so a
-- conversation with no workspace recalls the NULL bucket and nothing else.
CREATE TABLE memories (
  id            TEXT PRIMARY KEY,
  workspace_id  TEXT REFERENCES workspaces(id),
  content       TEXT NOT NULL,
  created_at    INTEGER NOT NULL,
  updated_at    INTEGER NOT NULL
);
CREATE INDEX idx_memories_workspace ON memories(workspace_id);
