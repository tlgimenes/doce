import type * as React from "react";
import MessageContent from "@/components/MessageContent";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { MessageGroup } from "@/components/ui/message";
import BashWidget from "@/views/chat/tool-widgets/BashWidget";
import TaskWidget from "@/views/chat/tool-widgets/TaskWidget";
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
}: TranscriptTurnProps): React.JSX.Element {
  return (
    <MessageGroup
      className="pb-2"
      data-chat-turn="true"
      data-testid="transcript-turn"
      data-last-turn={isLastTurn ? "true" : "false"}
    >
      {turn.user && <MessageContent message={turn.user} />}
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
          <Alert variant="destructive" className="mb-6" data-testid="workspace-error">
            <AlertDescription>{error}</AlertDescription>
          </Alert>
        )}
      </div>
    </MessageGroup>
  );
}
