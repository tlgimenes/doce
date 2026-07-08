import ReactMarkdown from "react-markdown";
import Timer from "@/components/Timer";
import { formatTokenCount } from "@/lib/formatTokenCount";
import {
  parseContextNoticeDetail,
  parseToolResultDetail,
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
import UserMessageContent from "@/views/chat/rich-input/UserMessageContent";
import UnknownToolWidget from "@/views/chat/tool-widgets/UnknownToolWidget";
import EditDiffWidget from "@/views/chat/tool-widgets/EditDiffWidget";
import BashWidget from "@/views/chat/tool-widgets/BashWidget";
import AskUserQuestionWidget from "@/views/chat/tool-widgets/AskUserQuestionWidget";
import ReadWidget from "@/views/chat/tool-widgets/ReadWidget";
import WriteWidget from "@/views/chat/tool-widgets/WriteWidget";
import SearchResultsWidget from "@/views/chat/tool-widgets/SearchResultsWidget";
import TaskWidget from "@/views/chat/tool-widgets/TaskWidget";

interface MessageContentProps {
  message: Message;
  // Historical message rows may include duration metadata. Workspace keeps
  // this off by default because `send_agent_message` has no useful
  // per-message duration for the optimistic in-progress turn.
  showTimer?: boolean;
}

/**
 * 004-tool-call-widgets (FR-013): the single transcript renderer. A
 * `tool_result` row dispatches to its matching widget by `toolName`, while
 * ordinary text/rich-text/context rows render through the shared message
 * components below.
 */
export default function MessageContent({ message: m, showTimer = false }: MessageContentProps) {
  if (m.role === "user") {
    return (
      <div className="mb-6" data-testid="chat-message" role="group" aria-label="You said">
        <div className="prose prose-sm dark:prose-invert max-w-none rounded-lg bg-muted p-3 text-foreground">
          {/* 009-rich-chat-input, US2 (T026): a rich_text user message (a
              paste-collapse chip, and eventually attachment/skill chips)
              dispatches to UserMessageContent, mirroring this file's existing
              tool_result -> ToolWidget dispatch — every other user message
              (contentType 'text', today's only other case) renders exactly as
              it always has. */}
          {m.contentType === "rich_text" ? (
            <UserMessageContent content={m.content} />
          ) : (
            <ReactMarkdown>{m.content}</ReactMarkdown>
          )}
        </div>
        {/* 010-context-window-management (UI refactor): input tokens for
            what the user sent — mirrors Claude Code's own per-turn token
            meter, arrow-directioned the same way (↑ sent/uploaded, ↓
            received/downloaded — see the assistant-side meter below). */}
        {m.tokenCount != null && (
          <p className="mt-1 text-xs text-muted-foreground" data-testid="token-meter">
            ↑ {formatTokenCount(m.tokenCount)} tokens
          </p>
        )}
      </div>
    );
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
      <div className="mb-6" data-testid="chat-message" role="group" aria-label="doce replied">
        <ToolWidget detail={detail} />
      </div>
    );
  }

  if (m.contentType === "error") {
    return (
      <div
        className="mb-6 rounded-lg bg-destructive/10 p-3 text-sm text-destructive"
        data-testid="chat-message"
        role="group"
        aria-label="doce replied"
      >
        {m.content}
      </div>
    );
  }

  // 010-context-window-management/US2 (FR-008): an inline transcript
  // notice, not a tool widget — "cleared" (tier 1) renders as a small,
  // muted line; "summarized" (tier 2) renders as a more visible notice,
  // matching Claude Desktop's unobtrusive treatment rather than a dense
  // breakdown.
  if (m.contentType === "context_notice") {
    const detail = parseContextNoticeDetail(m.content);
    const isSummarized = detail.kind === "summarized";
    return (
      <div
        className={
          isSummarized
            ? "mb-6 rounded-lg bg-muted p-3 text-sm text-muted-foreground"
            : "mb-6 text-xs text-muted-foreground/70"
        }
        data-testid="context-notice"
        data-notice-kind={detail.kind}
        role="status"
      >
        {detail.notice}
      </div>
    );
  }

  return (
    <div className="mb-6" data-testid="chat-message" role="group" aria-label="doce replied">
      <div className="prose prose-sm dark:prose-invert max-w-none">
        <ReactMarkdown>{m.content}</ReactMarkdown>
      </div>
      {(showTimer || m.tokenCount != null) && (
        <p className="mt-1 text-xs text-muted-foreground" data-testid="token-meter">
          {showTimer && <Timer createdAt={m.createdAt} durationMs={m.durationMs} />}
          {/* 010-context-window-management (UI refactor): output tokens
              for this reply, combined with the elapsed-time chron on the
              same line — mirrors Claude Code's own status line ("3m 51s ·
              ↓ 15.6k tokens"). */}
          {showTimer && m.tokenCount != null && " · "}
          {m.tokenCount != null && `↓ ${formatTokenCount(m.tokenCount)} tokens`}
        </p>
      )}
    </div>
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
