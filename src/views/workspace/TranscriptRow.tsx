import Timer from "@/components/Timer";
import MarkdownPreview from "@/components/MarkdownPreview";
import UserMessageContent from "@/views/chat/rich-input/UserMessageContent";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Bubble, BubbleContent } from "@/components/ui/bubble";
import { Marker, MarkerContent } from "@/components/ui/marker";
import {
  Message as ChatMessage,
  MessageContent as ChatMessageContent,
  MessageFooter,
} from "@/components/ui/message";
import { formatTokenCount } from "@/lib/formatTokenCount";
import {
  parseContextNoticeDetail,
  parseToolResultDetail,
  isPlanToolRow,
  type AskUserQuestionDetail,
  type BashDetail,
  type EditDetail,
  type GlobDetail,
  type GrepDetail,
  type Message,
  type ReadDetail,
  type TaskDetail,
  type ToolResultDetail,
  type UnknownToolDetail,
  type WriteDetail,
} from "@/lib/ipc";
import type { TurnTokenTotals } from "@/views/workspace/transcriptTurns";
import UnknownToolWidget from "@/views/chat/tool-widgets/UnknownToolWidget";
import EditDiffWidget from "@/views/chat/tool-widgets/EditDiffWidget";
import BashWidget from "@/views/chat/tool-widgets/BashWidget";
import AskUserQuestionWidget from "@/views/chat/tool-widgets/AskUserQuestionWidget";
import ReadWidget from "@/views/chat/tool-widgets/ReadWidget";
import WriteWidget from "@/views/chat/tool-widgets/WriteWidget";
import SearchResultsWidget from "@/views/chat/tool-widgets/SearchResultsWidget";
import TaskWidget from "@/views/chat/tool-widgets/TaskWidget";

interface TranscriptRowProps {
  message: Message;
  // Historical message rows may include duration metadata. Workspace keeps
  // this off by default because `send_agent_message` has no useful
  // per-message duration for the optimistic in-progress turn.
  showTimer?: boolean;
  /** The whole turn's accumulated in/out token totals — set only on the
   * turn's final assistant text row (TranscriptTurn decides which). */
  turnTokens?: TurnTokenTotals | null;
}

/**
 * 004-tool-call-widgets (FR-013): the single transcript renderer. A
 * `tool_result` row dispatches to its matching widget by `toolName`, while
 * ordinary text/rich-text/context rows render through the shared message
 * components below.
 */
export default function TranscriptRow({
  message: m,
  showTimer = false,
  turnTokens = null,
}: TranscriptRowProps) {
  if (m.role === "user") {
    return (
      <ChatMessage
        align="end"
        className="mb-5"
        data-testid="chat-message"
        role="group"
        aria-label="You said"
      >
        <ChatMessageContent>
          <Bubble align="end" variant="secondary">
            <BubbleContent data-testid="user-message-bubble">
              {m.contentType === "rich_text" ? (
                <UserMessageContent content={m.content} />
              ) : (
                <MarkdownPreview>{m.content}</MarkdownPreview>
              )}
            </BubbleContent>
          </Bubble>
        </ChatMessageContent>
      </ChatMessage>
    );
  }

  // Plan-machine rows are tracker-only (spec: plan activity is invisible
  // in the transcript) — skipped by tool name for the five plan tools and
  // by the persisted `"plan": true` marker for state-gated rejections that
  // carry a regular tool's name.
  if (
    (m.contentType === "tool_call" || m.contentType === "tool_result") &&
    isPlanToolRow(m.content, m.toolName)
  ) {
    return null;
  }

  // research.md § 5: a tool_call row's data is folded into its paired
  // tool_result row (data-model.md) — nothing to render standalone here in
  // this synchronous-execution pass. The degenerate case (a tool_call with
  // no following tool_result — e.g. the app quit mid-call) is intentionally
  // not distinguished from the ordinary "wait for the pair" case; it's rare
  // enough that silently rendering nothing for it is an acceptable trade
  // against the complexity of detecting an orphaned call.
  if (m.contentType === "tool_call") {
    return null;
  }

  if (m.contentType === "tool_result") {
    const detail = parseToolResultDetail(m.content, m.toolName);
    return (
      <ChatMessage
        className="mb-6"
        data-testid="chat-message"
        role="group"
        aria-label="doce replied"
      >
        <ChatMessageContent>
          <ToolWidget detail={detail} />
        </ChatMessageContent>
      </ChatMessage>
    );
  }

  if (m.contentType === "error") {
    return (
      <ChatMessage
        className="mb-5"
        data-testid="chat-message"
        role="group"
        aria-label="doce replied"
      >
        <ChatMessageContent>
          <Alert variant="destructive" role="status" data-testid="error-message">
            <AlertDescription>{m.content}</AlertDescription>
          </Alert>
        </ChatMessageContent>
      </ChatMessage>
    );
  }

  // 010-context-window-management/US2 (FR-008): an inline transcript
  // notice, not a tool widget — "cleared" (tier 1) renders as a small,
  // muted line; "summarized" (tier 2) renders as a more visible notice,
  // matching Claude Desktop's unobtrusive treatment rather than a dense
  // breakdown.
  if (m.contentType === "context_notice") {
    const detail = parseContextNoticeDetail(m.content);
    return (
      <ChatMessage className="mb-5" role="group" aria-label="doce replied">
        <ChatMessageContent>
          <Marker data-testid="context-notice" data-notice-kind={detail.kind} role="status">
            <MarkerContent>{detail.notice}</MarkerContent>
          </Marker>
        </ChatMessageContent>
      </ChatMessage>
    );
  }

  const showTurnTokens = turnTokens != null && (turnTokens.input > 0 || turnTokens.output > 0);
  const showAssistantDuration = showTimer || m.durationMs != null;
  const showAssistantMetadata =
    m.contentType === "text" && (showAssistantDuration || showTurnTokens);

  return (
    <ChatMessage className="mb-5" data-testid="chat-message" role="group" aria-label="doce replied">
      <ChatMessageContent>
        <Bubble variant="ghost">
          <BubbleContent>
            <MarkdownPreview>{m.content}</MarkdownPreview>
          </BubbleContent>
        </Bubble>
        {showAssistantMetadata && (
          <MessageFooter data-testid="token-meter" className="gap-3">
            {showAssistantDuration && <Timer createdAt={m.createdAt} durationMs={m.durationMs} />}
            {/* The TURN's accumulated token flow, not this one message's:
                ↑ everything the turn added to context (prompt + tool
                results), ↓ everything the model generated as text
                (transcriptTurns.accumulateTurnTokens). */}
            {showTurnTokens && (
              <span>
                {turnTokens.input > 0 && <>↑ {formatTokenCount(turnTokens.input)} </>}
                {turnTokens.output > 0 && <>↓ {formatTokenCount(turnTokens.output)} </>}
                tokens
              </span>
            )}
          </MessageFooter>
        )}
      </ChatMessageContent>
    </ChatMessage>
  );
}

// FR-011: every branch below is added by its own story; until then, every
// toolName (including ones that will eventually have a dedicated widget)
// falls through to the fallback — never blank, broken, or dropped.
//
// Explicit `as` casts per case, not a discriminated-union switch: the
// fallback's `toolName: string` is deliberately non-literal (data-model.md
// § Validation rules — "unrecognized toolName" has to be representable),
// which makes `ToolResultDetail | UnknownToolDetail` un-narrowable by TS
// (a plain `string` can't be excluded from a specific literal case). The
// cast is sound because `parseToolResultDetail` only ever tags an object
// with a known `toolName` after already validating it matches that shape.
function ToolWidget({ detail }: { detail: ToolResultDetail | UnknownToolDetail }) {
  if (detail.toolName === "Edit") {
    return <EditDiffWidget detail={detail as EditDetail} />;
  }
  if (detail.toolName === "Bash") {
    return <BashWidget detail={detail as BashDetail} />;
  }
  if (detail.toolName === "AskUserQuestion") {
    return <AskUserQuestionWidget detail={detail as AskUserQuestionDetail} />;
  }
  if (detail.toolName === "Read") {
    return <ReadWidget detail={detail as ReadDetail} />;
  }
  if (detail.toolName === "Write") {
    return <WriteWidget detail={detail as WriteDetail} />;
  }
  if (detail.toolName === "Glob" || detail.toolName === "Grep") {
    return <SearchResultsWidget detail={detail as GlobDetail | GrepDetail} />;
  }
  if (detail.toolName === "Task") {
    return <TaskWidget detail={detail as TaskDetail} />;
  }
  return <UnknownToolWidget detail={detail} />;
}
