import { MessageCircleQuestion } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Marker, MarkerContent, MarkerIcon } from "@/components/ui/marker";
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
    <Marker data-testid="question-answered">
      <MarkerIcon>
        <MessageCircleQuestion />
      </MarkerIcon>
      <MarkerContent className="flex min-w-0 flex-col">
        <span className="truncate" title={detail.question}>
          {detail.question}
        </span>
        {detail.interrupted ? (
          // A healed crash-orphaned question carries answer: [] — rendering
          // "You chose: " would read as answered-with-nothing.
          <span className="text-xs">Interrupted — the app closed before this was answered</span>
        ) : (
          <span className="text-xs" title={answer.join(", ")}>
            {isFreeText ? "You replied" : "You chose"}: {answer.join(", ")}
          </span>
        )}
      </MarkerContent>
      {detail.interrupted && (
        <Badge variant="outline" className="ml-auto shrink-0">
          Interrupted
        </Badge>
      )}
    </Marker>
  );
}
