/** Decodes a base64 string (no `data:` prefix) as UTF-8 text — used for
 * viewing offloaded tool-output files (010-context-window-management/US3),
 * which are always plain text, unlike the image-attachment base64 payloads
 * elsewhere in this codebase that stay base64-encoded for `<img src>`. */
export function base64ToUtf8(base64: string): string {
  const binary = atob(base64);
  const bytes = Uint8Array.from(binary, (c) => c.charCodeAt(0));
  return new TextDecoder("utf-8").decode(bytes);
}
