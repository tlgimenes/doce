import MessageContent from "@/components/MessageContent";
import BashWidget from "@/views/chat/tool-widgets/BashWidget";
import TaskWidget from "@/views/chat/tool-widgets/TaskWidget";
import StickyUserMessage from "@/views/workspace/StickyUserMessage";
import type { BashDetail, TaskDetail } from "@/lib/ipc";
import type { TranscriptTurn as TranscriptTurnModel } from "./transcriptTurns";

export type PendingTurnWidget =
  | { kind: "bash"; detail: BashDetail }
  | { kind: "task"; detail: TaskDetail };

export interface TranscriptTurnProps {
  turn: TranscriptTurnModel;
  isLastTurn?: boolean;
  pendingWidget?: PendingTurnWidget | null;
  error?: string | null;
}

export default function TranscriptTurn({
  turn,
  isLastTurn = false,
  pendingWidget = null,
  error = null,
}: TranscriptTurnProps): JSX.Element {
  return (
    <div
      className="flex flex-col pb-2"
      data-testid="transcript-turn"
      data-last-turn={isLastTurn ? "true" : "false"}
    >
      {turn.user && (
        <>
          <div
            aria-hidden="true"
            className="sticky top-0 z-40 h-4 w-full bg-background"
            data-testid="sticky-user-background"
          />
          <StickyUserMessage message={turn.user} />
        </>
      )}
      <div data-testid="transcript-turn-body" className="min-w-0">
        {turn.rows.map((message) => (
          <MessageContent key={message.id} message={message} />
        ))}
        {pendingWidget && (
          <div className="mb-6" data-testid="chat-message" role="group" aria-label="doce replied">
            {pendingWidget.kind === "bash" ? (
              <BashWidget detail={pendingWidget.detail} />
            ) : (
              <TaskWidget detail={pendingWidget.detail} />
            )}
          </div>
        )}
        {error && (
          <div
            className="mb-6 rounded-lg bg-destructive/10 p-3 text-sm text-destructive"
            data-testid="workspace-error"
          >
            {error}
          </div>
        )}
      </div>
    </div>
  );
}
