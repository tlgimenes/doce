import { MessageCircleQuestion } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Item, ItemContent, ItemDescription, ItemMedia, ItemTitle } from "@/components/ui/item";
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
      data-slot="widget-frame"
      className="overflow-hidden rounded-lg border border-border bg-card text-sm"
      data-testid="question-answered"
    >
      <Item data-slot="widget-frame-header" size="xs" className="w-full">
        <ItemMedia variant="icon">
          <MessageCircleQuestion />
        </ItemMedia>
        <ItemContent>
          <ItemDescription title={detail.question}>{detail.question}</ItemDescription>
          {detail.interrupted ? (
            // A healed crash-orphaned question carries answer: [] — rendering
            // "You chose: " would read as answered-with-nothing.
            <ItemTitle>Interrupted — the app closed before this was answered</ItemTitle>
          ) : (
            <ItemTitle title={answer.join(", ")}>
              {isFreeText ? "You replied" : "You chose"}: {answer.join(", ")}
            </ItemTitle>
          )}
        </ItemContent>
        {detail.interrupted && <Badge variant="outline">Interrupted</Badge>}
      </Item>
    </div>
  );
}
