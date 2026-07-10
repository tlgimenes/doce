import { diffLines } from "diff";
import { FilePen } from "lucide-react";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { CodeBlock, CodeBlockLine } from "@/components/ui/code-block";
import { ItemContent, ItemMedia, ItemTitle } from "@/components/ui/item";
import { WidgetFrame, WidgetFrameContent, WidgetFrameHeader } from "@/components/ui/widget-frame";
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
      <WidgetFrame data-testid="edit-failed">
        <WidgetFrameHeader>
          <ItemMedia variant="icon">
            <FilePen />
          </ItemMedia>
          <ItemContent>
            <ItemTitle>{detail.filePath ?? "(no file path)"}</ItemTitle>
          </ItemContent>
        </WidgetFrameHeader>
        <div className="p-3 pt-0">
          <Alert variant="destructive">
            <AlertDescription>{detail.outcome.error}</AlertDescription>
          </Alert>
        </div>
      </WidgetFrame>
    );
  }

  const changes = diffLines(detail.oldString, detail.newString);
  const lineCount = (value: string) => value.replace(/\n$/, "").split("\n").length;
  const addedCount = changes.filter((c) => c.added).reduce((n, c) => n + lineCount(c.value), 0);
  const removedCount = changes.filter((c) => c.removed).reduce((n, c) => n + lineCount(c.value), 0);

  return (
    <WidgetFrame collapsible defaultOpen data-testid="edit-diff">
      <WidgetFrameHeader>
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
      </WidgetFrameHeader>
      <WidgetFrameContent>
        <CodeBlock className="p-0 whitespace-pre">
          {changes.map((change, i) => {
            const lines = change.value.replace(/\n$/, "").split("\n");
            const testId = change.added
              ? "diff-added"
              : change.removed
                ? "diff-removed"
                : undefined;
            const prefix = change.added ? "+" : change.removed ? "-" : " ";
            const variant = change.added ? "added" : change.removed ? "removed" : "default";
            return (
              <div key={i} data-testid={testId}>
                {lines.map((line, j) => (
                  <CodeBlockLine key={j} variant={variant}>
                    {prefix} {line}
                  </CodeBlockLine>
                ))}
              </div>
            );
          })}
        </CodeBlock>
      </WidgetFrameContent>
    </WidgetFrame>
  );
}
