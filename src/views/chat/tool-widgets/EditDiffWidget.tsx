import { diffLines } from "diff";
import type { EditDetail } from "@/lib/ipc";

interface EditDiffWidgetProps {
  detail: EditDetail;
}

/**
 * US1/FR-002: a real, labeled diff for `Edit` tool calls — computed
 * client-side from the raw `oldString`/`newString` the dispatch layer
 * already captured (research.md § 6/§ 4), not a heavier editor component.
 */
export default function EditDiffWidget({ detail }: EditDiffWidgetProps) {
  if (!detail.outcome.ok) {
    return (
      <div
        className="rounded-lg border border-destructive/40 bg-destructive/10 p-3 text-sm"
        data-testid="edit-failed"
      >
        <p className="mb-1 font-mono text-xs text-muted-foreground">
          {detail.filePath ?? "(no file path)"}
        </p>
        <p className="text-destructive">{detail.outcome.error}</p>
      </div>
    );
  }

  const changes = diffLines(detail.oldString, detail.newString);

  return (
    <div className="overflow-hidden rounded-lg border border-border" data-testid="edit-diff">
      <p className="border-b border-border bg-card px-3 py-1.5 font-mono text-xs text-muted-foreground">
        {detail.filePath}
      </p>
      <pre className="overflow-x-auto p-0 font-mono text-xs">
        {changes.map((change, i) => {
          const lines = change.value.replace(/\n$/, "").split("\n");
          const testId = change.added ? "diff-added" : change.removed ? "diff-removed" : undefined;
          const prefix = change.added ? "+" : change.removed ? "-" : " ";
          const rowClass = change.added
            ? "bg-emerald-500/15 text-emerald-700 dark:text-emerald-400"
            : change.removed
              ? "bg-red-500/15 text-red-700 dark:text-red-400"
              : "text-foreground";
          return (
            <div key={i} data-testid={testId}>
              {lines.map((line, j) => (
                <div key={j} className={`whitespace-pre px-3 py-0.5 ${rowClass}`}>
                  {prefix} {line}
                </div>
              ))}
            </div>
          );
        })}
      </pre>
    </div>
  );
}
