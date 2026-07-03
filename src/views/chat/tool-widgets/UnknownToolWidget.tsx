import type { ToolResultDetail, UnknownToolDetail } from "@/lib/ipc";

interface UnknownToolWidgetProps {
  detail: ToolResultDetail | UnknownToolDetail;
}

/**
 * FR-011/SC-004: the fallback for any `toolName` without a dedicated
 * widget (including a completely unrecognized one, or a tool with a
 * dedicated widget that simply hasn't landed yet) — the tool's name plus a
 * readable rendering of its detail payload, never blank or broken.
 */
export default function UnknownToolWidget({ detail }: UnknownToolWidgetProps) {
  return (
    <div
      className="rounded-lg border border-border bg-card p-3 text-sm"
      data-testid="unknown-tool-widget"
    >
      <p className="mb-1 font-mono text-xs text-muted-foreground">{detail.toolName}</p>
      <pre className="overflow-x-auto whitespace-pre-wrap break-words font-mono text-xs">
        {JSON.stringify(detail, null, 2)}
      </pre>
    </div>
  );
}
