import { Bot } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Item, ItemContent, ItemDescription, ItemMedia, ItemTitle } from "@/components/ui/item";
import { Spinner } from "@/components/ui/spinner";
import type { TaskDetail } from "@/lib/ipc";

interface TaskWidgetProps {
  detail: TaskDetail;
}

/**
 * US4/FR-010: a running/complete status indicator only — the subagent's
 * own intermediate tool calls live on its own conversation row and are
 * never surfaced here (FR-015/SC-008, unchanged by this feature).
 */
export default function TaskWidget({ detail }: TaskWidgetProps) {
  // `interrupted` wins over `state`: a healed crash-orphaned delegation
  // carries state:"complete" (the shape constraint) but never finished —
  // a green Complete badge would be a lie.
  const interrupted = detail.interrupted === true;
  const running = !interrupted && detail.state === "running";
  return (
    <div
      data-slot="widget-frame"
      className="overflow-hidden rounded-lg border border-border bg-card text-sm"
      data-testid="task-widget"
    >
      <Item data-slot="widget-frame-header" size="xs" className="w-full">
        <ItemMedia variant="icon">
          <Bot />
        </ItemMedia>
        <ItemContent>
          <ItemTitle data-testid="task-status">
            {running && <Spinner role="presentation" aria-label={undefined} />}
            {interrupted
              ? "Interrupted — the app closed before this finished"
              : running
                ? "Running…"
                : "Complete"}
          </ItemTitle>
          <ItemDescription title={detail.prompt}>{detail.prompt}</ItemDescription>
        </ItemContent>
        {!running && (
          <Badge variant={interrupted ? "outline" : "secondary"}>
            {interrupted ? "Interrupted" : "Complete"}
          </Badge>
        )}
      </Item>
    </div>
  );
}
