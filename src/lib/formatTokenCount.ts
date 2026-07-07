/** Formats a token count the way Claude Code's own status line does ("15.6k
 * tokens" past four digits, otherwise the exact number) — used for the
 * per-message input/output token meter (010-context-window-management, UI
 * refactor). */
export function formatTokenCount(count: number): string {
  if (count >= 1000) {
    return `${(count / 1000).toFixed(1)}k`;
  }
  return String(count);
}
