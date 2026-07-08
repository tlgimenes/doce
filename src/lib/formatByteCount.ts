/** Formats a byte count as a compact, human-readable size ("1.5KB" past
 * 1000 bytes, otherwise the exact count) — mirrors `formatTokenCount`'s
 * own convention, since the two are shown together on a tool-call widget's
 * cost badge. */
export function formatByteCount(bytes: number): string {
  if (bytes >= 1000) {
    return `${(bytes / 1000).toFixed(1)}KB`;
  }
  return `${bytes}B`;
}
