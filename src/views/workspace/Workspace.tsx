import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import { ArrowDownIcon } from "@phosphor-icons/react";
import { StickToBottom, type StickToBottomContext } from "use-stick-to-bottom";
import MessageContent from "@/components/MessageContent";
import { runViewTransition } from "@/lib/viewTransition";
import { Button } from "@/components/ui/button";
import RichInput from "@/views/chat/rich-input/RichInput";
import UserAskWidget from "@/views/chat/tool-widgets/UserAskWidget";
import BashWidget from "@/views/chat/tool-widgets/BashWidget";
import WorkspaceTopbar from "@/views/workspace/WorkspaceTopbar";
import TaskWidget from "@/views/chat/tool-widgets/TaskWidget";
import {
  commands,
  events,
  parseAskUserQuestionCallDetail,
  type ContextUsage,
  type Conversation,
  parsePendingBashCallDetail,
  parsePendingTaskCallDetail,
  type Message,
  type RichMessageContent,
} from "@/lib/ipc";
import { useContextUsageStore } from "@/state/contextUsageStore";
import { isCompactCommand } from "@/lib/compactCommand";
import type { PendingInitialTurn } from "@/views/workspace/pendingInitialTurn";

/**
 * Whether the latest message is a still-pending, successfully-parsed
 * AskUserQuestion tool_call -- the one condition that actually changes
 * what the composer shows (RichInput vs UserAskWidget). Used to decide
 * whether a refreshMessages() update is worth a view transition; every
 * other kind of refresh (a plain new message, a pending Bash/Task call)
 * doesn't change the composer, so wrapping those in a transition too
 * would just make the whole chat-surface region flicker for no reason
 * (chat-surface, App.tsx's own view-transition-named region, has
 * unconditional fade keyframes in theme.css that play on every
 * transition regardless of whether chat-surface's own content changed).
 */
function isQuestionPending(messages: Message[]): boolean {
  const latest = messages[messages.length - 1];
  if (
    latest?.contentType !== "tool_call" ||
    latest.toolName !== "AskUserQuestion"
  )
    return false;
  return parseAskUserQuestionCallDetail(latest.content) !== null;
}

interface WorkspaceProps {
  conversation?: Conversation;
  conversationId?: string;
  pendingInitialTurn?: PendingInitialTurn | null;
  onPendingInitialTurnConsumed?: (conversationId: string) => void;
  onConversationSeen?: (conversationId: string) => void;
}

const conversationsWithSendInFlight = new Set<string>();
const sendInFlightListeners = new Set<() => void>();
interface ConversationRefreshPayload {
  contextUsage?: ContextUsage;
}

const conversationRefreshListeners = new Map<
  string,
  Set<(payload: ConversationRefreshPayload) => void>
>();

function notifySendInFlightListeners() {
  sendInFlightListeners.forEach((listener) => listener());
}

function subscribeToSendInFlight(listener: () => void) {
  sendInFlightListeners.add(listener);
  return () => {
    sendInFlightListeners.delete(listener);
  };
}

function markSendInFlight(conversationId: string): boolean {
  if (conversationsWithSendInFlight.has(conversationId)) return false;
  conversationsWithSendInFlight.add(conversationId);
  notifySendInFlightListeners();
  return true;
}

function clearSendInFlight(conversationId: string) {
  if (!conversationsWithSendInFlight.delete(conversationId)) return;
  notifySendInFlightListeners();
}

function requestConversationRefresh(
  conversationId: string,
  payload: ConversationRefreshPayload = {},
) {
  const listeners = conversationRefreshListeners.get(conversationId);
  if (!listeners) return;
  Array.from(listeners).forEach((listener) => listener(payload));
}

function subscribeToConversationRefresh(
  conversationId: string,
  listener: (payload: ConversationRefreshPayload) => void,
) {
  let listeners = conversationRefreshListeners.get(conversationId);
  if (!listeners) {
    listeners = new Set();
    conversationRefreshListeners.set(conversationId, listeners);
  }
  listeners.add(listener);

  return () => {
    listeners.delete(listener);
    if (listeners.size === 0) {
      conversationRefreshListeners.delete(conversationId);
    }
  };
}

function getServerSnapshot() {
  return false;
}

/**
 * 006-chat-empty-state: message view for a selected conversation. Folder
 * selection happens once, up front, in `EmptyState.tsx`/`FolderPicker.tsx`.
 *
 * Streaming (UI refactor): `send_agent_message`'s single promise does not
 * resolve until the whole tool-use loop finishes. During that loop, every
 * persisted tool_call/tool_result/final-answer row fires an
 * `agent-message-persisted` event, and this view re-fetches `list_messages`
 * each time so the transcript grows message-by-message.
 */
export default function Workspace({
  conversation,
  conversationId: conversationIdProp,
  pendingInitialTurn = null,
  onPendingInitialTurnConsumed,
  onConversationSeen,
}: WorkspaceProps) {
  const conversationId = conversation?.id ?? conversationIdProp;
  if (!conversationId) {
    throw new Error("Workspace requires either conversation or conversationId");
  }

  const [messages, setMessages] = useState<Message[]>([]);
  const messagesRef = useRef<Message[]>(messages);
  messagesRef.current = messages;
  const [thinking, setThinking] = useState(false);
  // The backend's reload-proof "a turn is genuinely running" signal
  // (ActiveGenerations, via is_generation_active). `sendInFlight` and
  // `thinking` are in-memory webview state a reload wipes; during the
  // model-generation phases (latest persisted row = user text or a paired
  // tool_result) the transcript alone looks idle, which used to re-open
  // the duplicate-send window after a reload. Re-checked on every
  // refreshMessages (i.e. on each agent-message-persisted event and on
  // send completion) — the final answer's own event flips it back off.
  const [backendTurnActive, setBackendTurnActive] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const isMountedRef = useRef(true);
  const currentConversationIdRef = useRef(conversationId);
  currentConversationIdRef.current = conversationId;
  const onConversationSeenRef = useRef(onConversationSeen);
  onConversationSeenRef.current = onConversationSeen;
  const consumedInitialTurnRef = useRef<string | null>(null);
  const dispatchedInitialTurnRef = useRef<string | null>(null);
  const stickToBottomContextRef = useRef<StickToBottomContext | null>(null);
  const sendInFlight = useSyncExternalStore(
    subscribeToSendInFlight,
    () => conversationsWithSendInFlight.has(conversationId),
    getServerSnapshot,
  );

  const syncBackendTurnActive = useCallback(() => {
    const targetConversationId = conversationId;
    void Promise.resolve(commands.isGenerationActive(targetConversationId))
      .then((active) => {
        if (
          isMountedRef.current &&
          currentConversationIdRef.current === targetConversationId
        ) {
          setBackendTurnActive(Boolean(active));
        }
      })
      // Best-effort: an IPC failure leaves the last known value in place
      // rather than breaking the composer either way.
      .catch(() => {});
  }, [conversationId]);

  const refreshMessages = useCallback(async () => {
    const targetConversationId = conversationId;
    syncBackendTurnActive();
    const loadedMessages = await commands.listMessages(targetConversationId);
    if (
      !isMountedRef.current ||
      currentConversationIdRef.current !== targetConversationId
    )
      return;

    const questionPendingBefore = isQuestionPending(messagesRef.current);
    const questionPendingAfter = isQuestionPending(loadedMessages);
    const applyUpdate = () => {
      setMessages(loadedMessages);
      onConversationSeenRef.current?.(targetConversationId);
    };
    if (questionPendingBefore !== questionPendingAfter) {
      runViewTransition(applyUpdate);
    } else {
      applyUpdate();
    }
  }, [conversationId, syncBackendTurnActive]);

  useEffect(() => {
    isMountedRef.current = true;
    return () => {
      isMountedRef.current = false;
    };
  }, []);

  useEffect(
    () =>
      subscribeToConversationRefresh(conversationId, (payload) => {
        if (payload.contextUsage) {
          useContextUsageStore.getState().setUsage(payload.contextUsage);
        }
        void refreshMessages();
      }),
    [conversationId, refreshMessages],
  );

  useEffect(() => {
    let cancelled = false;

    setMessages([]);
    setThinking(false);
    setError(null);
    setBackendTurnActive(false);
    syncBackendTurnActive();
    dispatchedInitialTurnRef.current = null;
    commands.listMessages(conversationId).then((loadedMessages) => {
      if (cancelled || currentConversationIdRef.current !== conversationId)
        return;

      if (
        loadedMessages.length === 0 &&
        dispatchedInitialTurnRef.current === conversationId
      ) {
        setMessages((prev) => (prev.length > 0 ? prev : loadedMessages));
        onConversationSeenRef.current?.(conversationId);
        return;
      }

      setMessages(loadedMessages);
      onConversationSeenRef.current?.(conversationId);
    });

    return () => {
      cancelled = true;
    };
  }, [conversationId, syncBackendTurnActive]);

  useEffect(() => {
    let cancelled = false;
    let unlistenPersisted: (() => void) | undefined;

    (async () => {
      unlistenPersisted = await events.onAgentMessagePersisted((p) => {
        if (p.conversationId !== conversationId) return;
        if (cancelled) return;
        void refreshMessages();
      });
      if (cancelled) {
        unlistenPersisted();
      }
    })();

    return () => {
      cancelled = true;
      unlistenPersisted?.();
    };
  }, [conversationId]);

  // Generalizes the same "latest message is an unpaired tool_call" signal
  // AskUserQuestion has always used (sequence ordering guarantees a
  // tool_result can only ever land immediately after its tool_call, so
  // this is a reliable "still in flight" check for any tool). Question/
  // Bash/Task get their dedicated pending widgets; *every other* tool —
  // including a parse-failure of those three — falls back to
  // `{kind: "other"}` rather than null, because null re-enables the
  // composer: that let a turn stuck inside a slow Grep accept a duplicate
  // user message after a reload wiped the in-memory send-in-flight flag
  // (a real production bug — the duplicate then just queued behind the
  // wedged turn). A conversation orphaned mid-tool by a crash can't lock
  // up here: the backend pairs any trailing unpaired tool_call with an
  // interrupted-error tool_result at startup
  // (storage::heal_interrupted_tool_calls), so a trailing tool_call always
  // means a genuinely live turn.
  const pendingToolCall = useMemo(() => {
    const latest = messages[messages.length - 1];
    if (latest?.contentType !== "tool_call") return null;
    if (latest.toolName === "AskUserQuestion") {
      const detail = parseAskUserQuestionCallDetail(latest.content);
      if (detail) return { kind: "question" as const, detail };
    } else if (latest.toolName === "Bash") {
      const detail = parsePendingBashCallDetail(latest.content);
      if (detail) return { kind: "bash" as const, detail };
    } else if (latest.toolName === "Task") {
      const detail = parsePendingTaskCallDetail(latest.content);
      if (detail) return { kind: "task" as const, detail };
    }
    return { kind: "other" as const };
  }, [messages]);
  const pendingQuestion =
    pendingToolCall?.kind === "question" ? pendingToolCall.detail : null;
  const turnInFlight = sendInFlight || backendTurnActive;
  const showThinking = thinking || turnInFlight;

  const send = useCallback(
    (content: string, richContent?: RichMessageContent): boolean => {
      // 010-context-window-management (UI refactor): `/compact`, typed and
      // submitted like any other message, is intercepted here before it ever
      // becomes a persisted agent turn — it triggers compaction directly and
      // refreshes the transcript instead of going through send_agent_message.
      if (!richContent && isCompactCommand(content) && !turnInFlight) {
        void (async () => {
          setError(null);
          try {
            const usage = await commands.compactConversation(conversationId);
            if (
              !isMountedRef.current ||
              currentConversationIdRef.current !== conversationId
            ) {
              requestConversationRefresh(conversationId, {
                contextUsage: usage,
              });
              return;
            }

            useContextUsageStore.getState().setUsage(usage);
            await refreshMessages();
          } catch (e) {
            if (
              isMountedRef.current &&
              currentConversationIdRef.current === conversationId
            ) {
              setError(String(e));
            }
          }
        })();
        return false;
      }

      // richContent's own presence counts as "something to send" even when
      // content (the flat-text extraction) is empty — a message that's
      // entirely a chip (e.g. just a pasted-text or attachment node, no
      // additional typed text) must not be silently dropped here.
      //
      // `pendingToolCall` (not just `thinking`, which is local state that
      // resets to false on reload) also blocks a new turn: sending another
      // message here wouldn't reach the model anyway (the previous turn's
      // `send_agent_message` is still genuinely blocked on `rx.await`,
      // holding the one global inference-engine lock) -- it would just
      // persist and then hang right alongside it. A pending AskUserQuestion
      // is answerable via its widget; a pending Bash/Task just has to run
      // its course.
      if ((!content.trim() && !richContent) || turnInFlight || pendingToolCall)
        return false;
      if (!markSendInFlight(conversationId)) return false;

      setError(null);
      setMessages((prev) => [
        ...prev,
        {
          id: `u-${Date.now()}`,
          conversationId,
          role: "user",
          contentType: richContent ? "rich_text" : "text",
          content: richContent ? JSON.stringify(richContent) : content,
          toolName: null,
          createdAt: Date.now(),
          durationMs: null,
          // Not known until reload -- these are optimistic/synthetic
          // messages, not the real persisted row (which does get a real
          // token_count via a backend follow-up update).
          tokenCount: null,
        },
      ]);
      setThinking(true);
      // Sending your own message always re-engages autoscroll and snaps to
      // the bottom — the library README's own ChatBox pattern, and load-
      // bearing here: use-stick-to-bottom only follows content growth
      // while its sticky lock is engaged, and ANY upward scroll/wheel
      // (even a stray trackpad flick) silently escapes it. Within the
      // library's 70px near-bottom threshold the scroll-to-bottom button
      // stays hidden, so without this call a send can look pinned yet
      // never follow.
      void stickToBottomContextRef.current?.scrollToBottom();
      void (async () => {
        try {
          // The `agent-message-persisted` event (subscribed above) is what
          // actually keeps `messages` up to date turn-by-turn while this
          // promise is pending -- this call is awaited for its errors and for
          // knowing when to clear `thinking`, not for its return value, which
          // by the time it resolves the live events have already rendered.
          await commands.sendAgentMessage(
            conversationId,
            content,
            richContent ? JSON.stringify(richContent) : undefined,
          );
        } catch (e) {
          if (
            isMountedRef.current &&
            currentConversationIdRef.current === conversationId
          ) {
            setError(String(e));
          }
        } finally {
          clearSendInFlight(conversationId);
          if (
            isMountedRef.current &&
            currentConversationIdRef.current === conversationId
          ) {
            setThinking(false);
            dispatchedInitialTurnRef.current = null;
            // Safety net: a real refetch regardless of event timing/ordering,
            // so the transcript is always correct once the turn is fully done --
            // covers both the happy path and an error partway through the loop.
            void refreshMessages();
          } else {
            requestConversationRefresh(conversationId);
          }
        }
      })();
      return true;
    },
    [conversationId, pendingToolCall, refreshMessages, turnInFlight],
  );

  useEffect(() => {
    if (!pendingInitialTurn) return;
    if (pendingInitialTurn.conversationId !== conversationId) return;
    if (consumedInitialTurnRef.current === conversationId) return;

    const dispatched = send(
      pendingInitialTurn.content,
      pendingInitialTurn.richContent,
    );
    if (!dispatched) return;

    consumedInitialTurnRef.current = conversationId;
    dispatchedInitialTurnRef.current = conversationId;
    onPendingInitialTurnConsumed?.(conversationId);
  }, [conversationId, onPendingInitialTurnConsumed, pendingInitialTurn, send]);

  return (
    <div className="flex h-full flex-col bg-background text-foreground">
      {conversation && <WorkspaceTopbar conversation={conversation} />}
      {/* Autoscroll is use-stick-to-bottom's job (ResizeObserver-driven:
          follows content growth while pinned, escapes on upward scroll,
          re-pins near the bottom), replacing the hand-rolled
          pin/scroll-effect machinery this component used to carry. The
          render-prop form (not <StickToBottom.Content>) keeps ownership of
          the scroll/content divs' classes and testids. `key` remounts the
          scroll state per conversation — a fresh conversation starts
          pinned at the bottom (`initial="instant"`), matching the old
          reset-pinning-on-switch effect. */}
      <StickToBottom
        key={conversationId}
        className="relative min-h-0 flex-1"
        initial="instant"
        contextRef={stickToBottomContextRef}
      >
        {({ scrollRef, contentRef, isAtBottom, scrollToBottom }) => (
          <>
            <div
              ref={scrollRef}
              className="h-full overflow-y-auto p-4"
              data-testid="workspace-scroll-container"
            >
              <div ref={contentRef} className="mx-auto max-w-3xl">
                {messages.map((m) => (
                  <MessageContent key={m.id} message={m} />
                ))}
                {pendingToolCall?.kind === "bash" ||
                pendingToolCall?.kind === "task" ? (
                  <div
                    className="mb-6"
                    data-testid="chat-message"
                    role="group"
                    aria-label="doce replied"
                  >
                    {pendingToolCall.kind === "bash" && (
                      <BashWidget detail={pendingToolCall.detail} />
                    )}
                    {pendingToolCall.kind === "task" && (
                      <TaskWidget detail={pendingToolCall.detail} />
                    )}
                  </div>
                ) : (
                  // "other" shows the indicator even when `thinking`/
                  // send-in-flight were wiped by a reload — the trailing
                  // unpaired tool_call itself is the proof a turn is running.
                  (pendingToolCall?.kind === "other" ||
                    (!pendingToolCall && showThinking)) && (
                    <p
                      className="text-sm text-muted-foreground"
                      data-testid="agent-thinking"
                    >
                      Working…
                    </p>
                  )
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
            {!isAtBottom && (
              <Button
                type="button"
                variant="secondary"
                size="icon"
                className="absolute bottom-4 left-1/2 z-10 -translate-x-1/2 rounded-full bg-card/95 shadow-sm backdrop-blur supports-[backdrop-filter]:bg-card/80"
                onClick={() => void scrollToBottom()}
                aria-label="Scroll to bottom"
                data-testid="scroll-to-bottom"
              >
                <ArrowDownIcon size={16} />
              </Button>
            )}
          </>
        )}
      </StickToBottom>
      <div
        className="border-t border-border p-4 [view-transition-name:chat-composer]"
        data-testid="workspace-composer-shell"
      >
        {pendingQuestion ? (
          <UserAskWidget detail={pendingQuestion} />
        ) : (
          <RichInput
            onSubmit={(content, richContent) => {
              send(content, richContent);
            }}
            skillsEnabled={true}
            disabled={turnInFlight || pendingToolCall !== null}
            placeholder="Describe a task…"
            inputTestId="agent-input"
            submitTestId="agent-send"
          />
        )}
      </div>
    </div>
  );
}
