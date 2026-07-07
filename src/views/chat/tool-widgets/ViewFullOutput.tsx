import { useState } from "react";
import { commands } from "@/lib/ipc";
import { base64ToUtf8 } from "@/lib/base64";

interface ViewFullOutputProps {
  path: string;
}

/**
 * 010-context-window-management/US3: when a tool result was offloaded (the
 * model saw only a preview), lets the user view the full original output —
 * reusing the existing `read_attached_file` command rather than a new IPC
 * surface, since it already reads an arbitrary trusted path's bytes.
 * Shared by BashWidget and ReadWidget, the two tool results large enough to
 * plausibly be offloaded.
 */
export default function ViewFullOutput({ path }: ViewFullOutputProps) {
  const [fullText, setFullText] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = async () => {
    setLoading(true);
    setError(null);
    try {
      const file = await commands.readAttachedFile(path);
      setFullText(base64ToUtf8(file.data));
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  if (fullText != null) {
    return (
      <pre
        className="overflow-x-auto whitespace-pre-wrap break-words border-t border-border px-3 py-2 font-mono text-xs"
        data-testid="view-full-output-content"
      >
        {fullText}
      </pre>
    );
  }

  return (
    <div className="border-t border-border px-3 py-1">
      <button
        type="button"
        className="text-xs text-muted-foreground underline hover:text-foreground"
        onClick={load}
        disabled={loading}
        data-testid="view-full-output-button"
      >
        {loading ? "Loading…" : "View full output"}
      </button>
      {error && <p className="mt-1 text-xs text-destructive">{error}</p>}
    </div>
  );
}
