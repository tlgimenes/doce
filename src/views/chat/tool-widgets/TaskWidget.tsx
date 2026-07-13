import { useEffect, useState } from "react";
import { Bot } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Marker, MarkerContent, MarkerIcon } from "@/components/ui/marker";
import { commands, parseToolResultDetail, type ReadDetail, type TaskDetail } from "@/lib/ipc";

interface TaskWidgetProps {
  detail: TaskDetail;
}

/**
 * Summarizes a completed delegation as an outcome sentence ("Explored 4
 * files") by counting the distinct files the subagent Read on its own
 * conversation row — display-only enrichment; a fetch failure just leaves
 * the verb-only fallback. Returns null while loading (or on error).
 */
function useExploredFileCount(subagentConversationId: string, enabled: boolean) {
  const [count, setCount] = useState<number | null>(null);

  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;
    commands
      .listMessages(subagentConversationId)
      .then((messages) => {
        if (cancelled) return;
        const files = new Set<string>();
        for (const message of messages) {
          if (message.contentType !== "tool_result") continue;
          const parsed = parseToolResultDetail(message.content, message.toolName);
          // `UnknownToolDetail.toolName` is a plain string, so the union
          // doesn't discriminate on its own — but "Read" is in
          // KNOWN_TOOL_NAMES, so a "Read" result always parses as ReadDetail.
          if (parsed.toolName !== "Read") continue;
          const { filePath } = parsed as ReadDetail;
          if (filePath) files.add(filePath);
        }
        setCount(files.size);
      })
      .catch(() => {
        // Keep the verb-only fallback copy.
      });
    return () => {
      cancelled = true;
    };
  }, [subagentConversationId, enabled]);

  return count;
}

/**
 * US4/FR-010: a running/complete status indicator only — the subagent's
 * own intermediate tool calls live on its own conversation row and are
 * never surfaced here (FR-015/SC-008, unchanged by this feature).
 */
export default function TaskWidget({ detail }: TaskWidgetProps) {
  // `interrupted` wins over `state`: a healed crash-orphaned delegation
  // carries state:"complete" (the shape constraint) but never finished —
  // presenting it as done would be a lie.
  const interrupted = detail.interrupted === true;
  const running = !interrupted && detail.state === "running";
  const complete = !interrupted && !running;
  const exploredCount = useExploredFileCount(detail.subagentConversationId, complete);

  const statusLabel = interrupted
    ? "Interrupted — the app closed before this finished"
    : running
      ? "Exploring…"
      : exploredCount != null && exploredCount > 0
        ? `Explored ${exploredCount} ${exploredCount === 1 ? "file" : "files"}`
        : "Finished exploring";

  return (
    <Marker data-testid="task-widget">
      <MarkerIcon>
        <Bot />
      </MarkerIcon>
      <MarkerContent className="flex min-w-0 flex-col">
        <span data-testid="task-status" className={running ? "shimmer truncate" : "truncate"}>
          {statusLabel}
        </span>
        <span className="text-xs" title={detail.prompt}>
          {detail.prompt}
        </span>
      </MarkerContent>
      {interrupted && (
        <Badge variant="outline" className="ml-auto shrink-0">
          Interrupted
        </Badge>
      )}
    </Marker>
  );
}
