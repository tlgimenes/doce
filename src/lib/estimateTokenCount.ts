/**
 * Cheap chars/4 token estimate for optimistic UI, used only until the
 * backend's real tokenizer count lands (the first `agent-message-persisted`
 * refetch replaces it) — never persisted, never shown as a final number.
 */
export function estimateTokenCount(text: string): number {
  return Math.ceil(text.length / 4);
}
