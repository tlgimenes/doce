import { diffLines } from "diff";
import { FilePen } from "lucide-react";
import { Marker, MarkerContent, MarkerIcon } from "@/components/ui/marker";
import type { EditDetail } from "@/lib/ipc";
import { pathBasename } from "@/lib/pathBasename";

interface EditDiffWidgetProps {
  detail: EditDetail;
}

type DiffLineVariant = "default" | "added" | "removed";

const lineClass: Record<DiffLineVariant, string> = {
  default: "text-foreground",
  added: "bg-emerald-500/15 text-emerald-700 dark:text-emerald-400",
  removed: "bg-destructive/15 text-destructive",
};

/**
 * US1/FR-002: a real, labeled diff for `Edit` tool calls — computed
 * client-side from the raw `oldString`/`newString` the dispatch layer
 * already captured (research.md § 6/§ 4), not a heavier editor component.
 */
export default function EditDiffWidget({ detail }: EditDiffWidgetProps) {
  const fileLabel = detail.filePath ? pathBasename(detail.filePath) : "file";

  if (!detail.outcome.ok) {
    return (
      <Marker data-testid="edit-failed">
        <MarkerIcon>
          <FilePen />
        </MarkerIcon>
        <MarkerContent className="flex min-w-0 flex-col">
          <span className="truncate" title={detail.filePath ?? undefined}>
            Couldn&apos;t edit {fileLabel}
          </span>
          <span className="text-xs">{detail.outcome.error}</span>
        </MarkerContent>
      </Marker>
    );
  }

  const changes = diffLines(detail.oldString, detail.newString);
  const lineCount = (value: string) => value.replace(/\n$/, "").split("\n").length;
  const addedCount = changes.filter((c) => c.added).reduce((n, c) => n + lineCount(c.value), 0);
  const removedCount = changes.filter((c) => c.removed).reduce((n, c) => n + lineCount(c.value), 0);

  // The diff is the one widget panel that keeps its always-visible body —
  // it carries real review value; every other widget is a single line.
  return (
    <div data-testid="edit-diff">
      <Marker>
        <MarkerIcon>
          <FilePen />
        </MarkerIcon>
        <MarkerContent className="min-w-0 truncate" title={detail.filePath ?? undefined}>
          Edited {fileLabel}{" "}
          <span className="text-xs text-muted-foreground tabular-nums">
            +{addedCount} −{removedCount}
          </span>
        </MarkerContent>
      </Marker>
      <div className="pl-6">
        <div className="overflow-x-auto p-0 font-mono text-xs whitespace-pre text-foreground">
          {changes.map((change, i) => {
            const lines = change.value.replace(/\n$/, "").split("\n");
            const testId = change.added
              ? "diff-added"
              : change.removed
                ? "diff-removed"
                : undefined;
            const prefix = change.added ? "+" : change.removed ? "-" : " ";
            const variant: DiffLineVariant = change.added
              ? "added"
              : change.removed
                ? "removed"
                : "default";
            return (
              <div key={i} data-testid={testId}>
                {lines.map((line, j) => (
                  <div
                    key={j}
                    data-variant={variant}
                    className={`px-3 py-0.5 whitespace-pre ${lineClass[variant]}`}
                  >
                    {prefix} {line}
                  </div>
                ))}
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
