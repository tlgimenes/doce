/**
 * Last path segment for display in tool-widget summaries ("Read
 * composer.tsx" instead of the full absolute path). Trailing slashes are
 * ignored; the full path stays available to users via the row's hover
 * title.
 */
export function pathBasename(path: string): string {
  const trimmed = path.replace(/\/+$/, "");
  const lastSlash = trimmed.lastIndexOf("/");
  const base = lastSlash === -1 ? trimmed : trimmed.slice(lastSlash + 1);
  return base || path;
}
