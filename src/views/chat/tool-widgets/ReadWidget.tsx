import type { ReadDetail } from "@/lib/ipc";
import ViewFullOutput from "./ViewFullOutput";

interface ReadWidgetProps {
  detail: ReadDetail;
}

/** US4/FR-005: a compact file-reference card, not a plain-text dump of the file's contents. */
export default function ReadWidget({ detail }: ReadWidgetProps) {
  if (!detail.outcome.ok) {
    return (
      <div
        className="rounded-lg border border-destructive/40 bg-destructive/10 p-3 text-sm"
        data-testid="read-widget"
      >
        <p className="mb-1 font-mono text-xs text-muted-foreground">
          Read <span>{detail.filePath}</span>
        </p>
        <p className="text-destructive">{detail.outcome.error}</p>
      </div>
    );
  }

  return (
    <div className="rounded-lg border border-border bg-card p-3 text-sm" data-testid="read-widget">
      <p className="font-mono text-xs text-muted-foreground">
        Read <span>{detail.filePath}</span>
      </p>
      {detail.outcome.truncated && (
        <p className="mt-1 text-xs text-muted-foreground" data-testid="read-truncated">
          Output truncated
        </p>
      )}
      {detail.offloadedTo && <ViewFullOutput path={detail.offloadedTo} />}
    </div>
  );
}
