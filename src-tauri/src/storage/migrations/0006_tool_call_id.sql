-- Structured tool calls: links a `tool_call` row to its paired
-- `tool_result` row by a shared id, replacing sequence-adjacency as the
-- only signal. Nullable (older rows, and every non-tool row, have none).
-- `model_text` is the plain, model-facing text for a `tool_result` row
-- (what the model actually read live, post-offload-truncation if
-- applicable) -- distinct from `content`, which stays the rich detail
-- JSON widgets render from. Reconstructing a `tool_result` row's
-- in-memory ChatMessage on reload reads `model_text`, not `content`.
ALTER TABLE messages ADD COLUMN tool_call_id TEXT;
ALTER TABLE messages ADD COLUMN model_text TEXT;
