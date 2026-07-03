import type { WriteDetail } from "@/lib/ipc";

interface WriteWidgetProps {
  detail: WriteDetail;
}

/** US4/FR-006: distinct from ReadWidget and from a plain reply — a compact file-reference card for a created/overwritten file. */
export default function WriteWidget({ detail }: WriteWidgetProps) {
  if (!detail.outcome.ok) {
    return (
      <div
        className="rounded-lg border border-destructive/40 bg-destructive/10 p-3 text-sm"
        data-testid="write-widget"
      >
        <p className="mb-1 font-mono text-xs text-muted-foreground">
          Write <span>{detail.filePath}</span>
        </p>
        <p className="text-destructive">{detail.outcome.error}</p>
      </div>
    );
  }

  return (
    <div
      className="rounded-lg border border-emerald-500/30 bg-emerald-500/5 p-3 text-sm"
      data-testid="write-widget"
    >
      <p className="font-mono text-xs text-muted-foreground">
        Write <span>{detail.filePath}</span> ({detail.byteCount} bytes)
      </p>
    </div>
  );
}
