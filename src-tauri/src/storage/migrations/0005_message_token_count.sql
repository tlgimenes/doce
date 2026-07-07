-- 010-context-window-management (UI refactor): frozen token count for a
-- message, stamped once at persistence time via the real tokenizer --
-- input tokens for a user message, output tokens for an assistant reply.
-- Nullable (older rows, and any row persisted before a model was ever
-- loaded, simply have no count) -- mirrors duration_ms's exact pattern
-- (migration 0002): computed once, frozen, survives a reload without a
-- live recomputation.
ALTER TABLE messages ADD COLUMN token_count INTEGER;
