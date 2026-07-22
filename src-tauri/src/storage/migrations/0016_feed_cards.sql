-- Activity feed: a persisted "card" for each MUTATING/creative MCP tool call
-- the agent makes (emitted as a side-effect of MCP dispatch — see
-- commands/feed.rs::infer_card). Reads/queries never produce a card. Cards
-- are reviewed and dismissed from the additive Activity surface; the chat
-- transcript is untouched.
CREATE TABLE feed_cards (
  id              TEXT PRIMARY KEY,
  conversation_id TEXT,
  kind            TEXT NOT NULL,
  title           TEXT NOT NULL,
  preview         TEXT NOT NULL,
  source_tool     TEXT NOT NULL,
  status          TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'dismissed')),
  created_at      INTEGER NOT NULL
);

CREATE INDEX idx_feed_cards_conversation ON feed_cards(conversation_id);
CREATE INDEX idx_feed_cards_status ON feed_cards(status);
