import { useState } from "react";
import { commands } from "@/lib/ipc";
import { base64ToUtf8 } from "@/lib/base64";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";

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
        data-testid="view-full-output-content"
        className="overflow-x-auto px-3 py-2 font-mono text-xs whitespace-pre-wrap wrap-break-word text-foreground"
      >
        {fullText}
      </pre>
    );
  }

  return (
    <div className="flex flex-col items-start gap-1 px-3 py-1">
      <Button
        type="button"
        variant="ghost"
        size="sm"
        onClick={load}
        disabled={loading}
        data-testid="view-full-output-button"
      >
        {loading && <Spinner role="presentation" aria-label={undefined} />}
        {loading ? "Loading…" : "View full output"}
      </Button>
      {error && (
        <Alert variant="destructive">
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      )}
    </div>
  );
}
