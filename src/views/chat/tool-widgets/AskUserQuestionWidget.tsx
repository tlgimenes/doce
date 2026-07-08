import type { AskUserQuestionDetail } from "@/lib/ipc";

interface AskUserQuestionWidgetProps {
  detail: AskUserQuestionDetail;
}

/**
 * Read-only "already answered" rendering for a resolved AskUserQuestion
 * tool_result (data-model.md) -- the only caller is MessageContent.tsx,
 * rendering a historical, resolved message. The live, still-pending
 * interaction (option buttons, free-text fallback) lives in UserAskWidget
 * instead, rendered in the composer slot by Workspace.tsx.
 *
 * `answer` can come from either a button click (every entry matches a
 * known option label) or typed free text (it won't) -- there's no backend
 * field recording which, so this is a client-side heuristic computed at
 * render time, not a stored fact.
 */
export default function AskUserQuestionWidget({ detail }: AskUserQuestionWidgetProps) {
  const answer = detail.answer ?? [];
  const isFreeText = !answer.every((a) => detail.options.some((o) => o.label === a));

  return (
    <div
      className="rounded-lg border border-border bg-card p-3 text-sm"
      data-testid="question-answered"
    >
      <p className="mb-1 text-muted-foreground">{detail.question}</p>
      {detail.interrupted ? (
        // A healed crash-orphaned question carries answer: [] — rendering
        // "You chose: " would read as answered-with-nothing.
        <p className="font-medium text-amber-600 dark:text-amber-400">
          Interrupted — the app closed before this was answered
        </p>
      ) : (
        <p className="font-medium">
          {isFreeText ? "You replied" : "You chose"}: {answer.join(", ")}
        </p>
      )}
    </div>
  );
}
