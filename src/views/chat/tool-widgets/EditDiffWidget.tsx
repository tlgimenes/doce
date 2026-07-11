import { diffLines } from "diff";
import { ChevronRight, FilePen } from "lucide-react";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { Item, ItemContent, ItemMedia, ItemTitle } from "@/components/ui/item";
import type { EditDetail } from "@/lib/ipc";

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
  if (!detail.outcome.ok) {
    return (
      <div
        data-slot="widget-frame"
        className="overflow-hidden rounded-lg border border-border bg-card text-sm"
        data-testid="edit-failed"
      >
        <Item data-slot="widget-frame-header" size="xs" className="w-full">
          <ItemMedia variant="icon">
            <FilePen />
          </ItemMedia>
          <ItemContent>
            <ItemTitle>{detail.filePath ?? "(no file path)"}</ItemTitle>
          </ItemContent>
        </Item>
        <div className="p-3 pt-0">
          <Alert variant="destructive">
            <AlertDescription>{detail.outcome.error}</AlertDescription>
          </Alert>
        </div>
      </div>
    );
  }

  const changes = diffLines(detail.oldString, detail.newString);
  const lineCount = (value: string) => value.replace(/\n$/, "").split("\n").length;
  const addedCount = changes.filter((c) => c.added).reduce((n, c) => n + lineCount(c.value), 0);
  const removedCount = changes.filter((c) => c.removed).reduce((n, c) => n + lineCount(c.value), 0);

  return (
    <Collapsible
      data-slot="widget-frame"
      defaultOpen
      className="overflow-hidden rounded-lg border border-border bg-card text-sm"
      data-testid="edit-diff"
    >
      <CollapsibleTrigger
        nativeButton={false}
        render={
          <Item
            data-slot="widget-frame-header"
            size="xs"
            className="group/widget-frame w-full cursor-pointer rounded-none hover:bg-accent"
          />
        }
      >
        <ItemMedia variant="icon">
          <FilePen />
        </ItemMedia>
        <ItemContent>
          <ItemTitle>{detail.filePath}</ItemTitle>
        </ItemContent>
        <span className="flex items-center gap-2">
          <Badge variant="outline">+{addedCount}</Badge>
          <Badge variant="outline">−{removedCount}</Badge>
        </span>
        <ChevronRight
          aria-hidden="true"
          data-slot="widget-frame-chevron"
          className="ml-auto size-4 shrink-0 text-muted-foreground transition-transform group-aria-expanded/widget-frame:rotate-90"
        />
      </CollapsibleTrigger>
      <CollapsibleContent data-slot="widget-frame-content" className="border-t border-border">
        <pre
          data-slot="code-block"
          data-tone="default"
          className="overflow-x-auto p-0 font-mono text-xs whitespace-pre wrap-break-word text-foreground"
        >
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
                    data-slot="code-block-line"
                    data-variant={variant}
                    className={`px-3 py-0.5 whitespace-pre ${lineClass[variant]}`}
                  >
                    {prefix} {line}
                  </div>
                ))}
              </div>
            );
          })}
        </pre>
      </CollapsibleContent>
    </Collapsible>
  );
}
