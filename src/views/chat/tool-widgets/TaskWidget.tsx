import { Bot } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Marker, MarkerContent, MarkerIcon } from "@/components/ui/marker";
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
    <Marker data-testid="task-widget">
      <MarkerIcon>
        <Bot />
      </MarkerIcon>
      <MarkerContent className="flex min-w-0 flex-col">
        <span data-testid="task-status" className="truncate">
          {running && <Spinner role="presentation" aria-label={undefined} />}
          {interrupted
            ? "Interrupted — the app closed before this finished"
            : running
              ? "Running…"
              : "Complete"}
        </span>
        <span className="text-xs" title={detail.prompt}>
          {detail.prompt}
        </span>
      </MarkerContent>
      {!running && (
        <Badge variant={interrupted ? "outline" : "secondary"} className="ml-auto shrink-0">
          {interrupted ? "Interrupted" : "Complete"}
        </Badge>
      )}
    </Marker>
  );
}
