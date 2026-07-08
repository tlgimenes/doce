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
    <div className="rounded-lg border border-border bg-card p-3 text-sm" data-testid="task-widget">
      <p
        className={`mb-1 text-xs font-medium ${
          interrupted
            ? "text-amber-600 dark:text-amber-400"
            : running
              ? "text-sky-600 dark:text-sky-400"
              : "text-emerald-700 dark:text-emerald-400"
        }`}
        data-testid="task-status"
      >
        {interrupted
          ? "Interrupted — the app closed before this finished"
          : running
            ? "Running…"
            : "Complete"}
      </p>
      <p className="text-muted-foreground">{detail.prompt}</p>
    </div>
  );
}
