import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { ArrowDownIcon } from "@phosphor-icons/react";
import MessageContent from "@/components/MessageContent";
import ContextUsageGauge from "@/components/ContextUsageGauge";
import { Button } from "@/components/ui/button";
import RichInput from "@/views/chat/rich-input/RichInput";
import AskUserQuestionWidget from "@/views/chat/tool-widgets/AskUserQuestionWidget";
import {
  commands,
  events,
  parseAskUserQuestionCallDetail,
  type Message,
  type RichMessageContent,
} from "@/lib/ipc";
import { useContextUsageStore } from "@/state/contextUsageStore";
import { isCompactCommand } from "@/lib/compactCommand";
import type { PendingInitialTurn } from "@/views/workspace/pendingInitialTurn";

const AUTOSCROLL_BOTTOM_THRESHOLD_PX = 48;

function isNearScrollBottom(element: HTMLElement): boolean {
  return (
    element.scrollHeight - element.scrollTop - element.clientHeight <=
    AUTOSCROLL_BOTTOM_THRESHOLD_PX
  );
}

function scrollElementToBottom(element: HTMLElement) {
  element.scrollTop = Math.max(0, element.scrollHeight - element.clientHeight);
}

interface WorkspaceProps {
  conversationId: string;
  pendingInitialTurn?: PendingInitialTurn | null;
  onPendingInitialTurnConsumed?: (conversationId: string) => void;
  onConversationSeen?: (conversationId: string) => void;
}

const conversationsWithSendInFlight = new Set<string>();
const sendInFlightListeners = new Set<() => void>();

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
  conversationId,
  pendingInitialTurn = null,
  onPendingInitialTurnConsumed,
  onConversationSeen,
}: WorkspaceProps) {
  const [messages, setMessages] = useState<Message[]>([]);
  const [thinking, setThinking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [isAutoscrollPinned, setIsAutoscrollPinned] = useState(true);
  const scrollContainerRef = useRef<HTMLDivElement | null>(null);
  const autoscrollPinnedRef = useRef(true);
  const currentConversationIdRef = useRef(conversationId);
  currentConversationIdRef.current = conversationId;
  const consumedInitialTurnRef = useRef<string | null>(null);
  const dispatchedInitialTurnRef = useRef<string | null>(null);
  const sendInFlight = useSyncExternalStore(
    subscribeToSendInFlight,
    () => conversationsWithSendInFlight.has(conversationId),
    getServerSnapshot,
  );

  useEffect(() => {
    let cancelled = false;

    setMessages([]);
    setThinking(false);
    setError(null);
    dispatchedInitialTurnRef.current = null;
    commands.listMessages(conversationId).then((loadedMessages) => {
      if (cancelled || currentConversationIdRef.current !== conversationId) return;

      if (loadedMessages.length === 0 && dispatchedInitialTurnRef.current === conversationId) {
        setMessages((prev) => (prev.length > 0 ? prev : loadedMessages));
        onConversationSeen?.(conversationId);
        return;
      }

      setMessages(loadedMessages);
      onConversationSeen?.(conversationId);
    });

    return () => {
      cancelled = true;
    };
  }, [conversationId, onConversationSeen]);

  useEffect(() => {
    let cancelled = false;
    let unlistenPersisted: (() => void) | undefined;

    (async () => {
      unlistenPersisted = await events.onAgentMessagePersisted((p) => {
        if (p.conversationId !== conversationId) return;
        commands.listMessages(conversationId).then((loadedMessages) => {
          if (cancelled || currentConversationIdRef.current !== conversationId) return;
          setMessages(loadedMessages);
          onConversationSeen?.(conversationId);
        });
      });
      if (cancelled) {
        unlistenPersisted();
      }
    })();

    return () => {
      cancelled = true;
      unlistenPersisted?.();
    };
  }, [conversationId, onConversationSeen]);

  // A pending `AskUserQuestion` is derived, not separately tracked state:
  // the backend persists the tool_call (with its questionId folded in —
  // see handle_ask_user_question) *before* it blocks on an answer, and
  // sequence ordering guarantees the paired tool_result can only ever land
  // immediately after it — so "the latest message is that tool_call" is
  // exactly "still awaiting an answer," and it self-clears the instant the
  // tool_result becomes the latest message instead. This also means a
  // pending question survives a reload or switching away and back: it's
  // reconstructed from whatever `list_messages` actually returns, not from
  // catching the live `ask-user-question` event at the moment it fires.
  const pendingQuestion = useMemo(() => {
    const latest = messages[messages.length - 1];
    if (latest?.contentType === "tool_call" && latest.toolName === "AskUserQuestion") {
      return parseAskUserQuestionCallDetail(latest.content);
    }
    return null;
  }, [messages]);
  const showThinking = thinking || sendInFlight;

  const setAutoscrollPinned = useCallback((pinned: boolean) => {
    autoscrollPinnedRef.current = pinned;
    setIsAutoscrollPinned(pinned);
  }, []);

  const scrollToTranscriptBottom = useCallback((force = false) => {
    const element = scrollContainerRef.current;
    if (!element) return;
    if (!force && !autoscrollPinnedRef.current) return;
    scrollElementToBottom(element);
  }, []);

  const scheduleScrollToTranscriptBottom = useCallback(() => {
    const frame = window.requestAnimationFrame(() => {
      scrollToTranscriptBottom();
    });
    return () => window.cancelAnimationFrame(frame);
  }, [scrollToTranscriptBottom]);

  const updateAutoscrollPinned = useCallback(() => {
    const element = scrollContainerRef.current;
    if (!element) return;
    setAutoscrollPinned(isNearScrollBottom(element));
  }, [setAutoscrollPinned]);

  const scrollToBottomAndPin = useCallback(() => {
    setAutoscrollPinned(true);
    scrollToTranscriptBottom(true);
  }, [scrollToTranscriptBottom, setAutoscrollPinned]);

  useEffect(() => {
    setAutoscrollPinned(true);
    return scheduleScrollToTranscriptBottom();
  }, [conversationId, scheduleScrollToTranscriptBottom, setAutoscrollPinned]);

  useEffect(() => {
    if (!autoscrollPinnedRef.current) return;
    return scheduleScrollToTranscriptBottom();
  }, [messages, pendingQuestion, scheduleScrollToTranscriptBottom, showThinking]);

  const send = useCallback(
    (content: string, richContent?: RichMessageContent): boolean => {
      // 010-context-window-management (UI refactor): `/compact`, typed and
      // submitted like any other message, is intercepted here before it ever
      // becomes a persisted agent turn — it triggers compaction directly and
      // refreshes the transcript instead of going through send_agent_message.
      if (!richContent && isCompactCommand(content) && !sendInFlight) {
        void (async () => {
          setError(null);
          try {
            const usage = await commands.compactConversation(conversationId);
            if (currentConversationIdRef.current !== conversationId) return;

            useContextUsageStore.getState().setUsage(usage);
            const refreshed = await commands.listMessages(conversationId);
            if (currentConversationIdRef.current !== conversationId) return;

            setMessages(refreshed);
          } catch (e) {
            if (currentConversationIdRef.current === conversationId) {
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
      // `pendingQuestion` (not just `thinking`, which is local state that
      // resets to false on reload) also blocks a new turn: sending another
      // message here wouldn't reach the model anyway (the previous turn's
      // `send_agent_message` is still genuinely blocked on `rx.await`,
      // holding the one global inference-engine lock) -- it would just
      // persist and then hang right alongside it. Answer via the widget
      // instead.
      if ((!content.trim() && !richContent) || sendInFlight || pendingQuestion) return false;
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
          if (currentConversationIdRef.current === conversationId) {
            setError(String(e));
          }
        } finally {
          clearSendInFlight(conversationId);
          if (currentConversationIdRef.current === conversationId) {
            setThinking(false);
            dispatchedInitialTurnRef.current = null;
            // Safety net: a real refetch regardless of event timing/ordering,
            // so the transcript is always correct once the turn is fully done --
            // covers both the happy path and an error partway through the loop.
            commands.listMessages(conversationId).then((loadedMessages) => {
              if (currentConversationIdRef.current !== conversationId) return;
              setMessages(loadedMessages);
            });
          }
        }
      })();
      return true;
    },
    [conversationId, pendingQuestion, sendInFlight],
  );

  useEffect(() => {
    if (!pendingInitialTurn) return;
    if (pendingInitialTurn.conversationId !== conversationId) return;
    if (consumedInitialTurnRef.current === conversationId) return;

    const dispatched = send(pendingInitialTurn.content, pendingInitialTurn.richContent);
    if (!dispatched) return;

    consumedInitialTurnRef.current = conversationId;
    dispatchedInitialTurnRef.current = conversationId;
    onPendingInitialTurnConsumed?.(conversationId);
  }, [conversationId, onPendingInitialTurnConsumed, pendingInitialTurn, send]);

  return (
    <div className="flex h-dvh flex-col bg-background text-foreground">
      <div className="relative min-h-0 flex-1">
        <div
          ref={scrollContainerRef}
          className="h-full overflow-y-auto p-4"
          data-testid="workspace-scroll-container"
          onScroll={updateAutoscrollPinned}
        >
          <div className="mx-auto max-w-3xl">
            {messages.map((m) => (
              <MessageContent key={m.id} message={m} />
            ))}
            {pendingQuestion ? (
              <div
                className="mb-6"
                data-testid="chat-message"
                role="group"
                aria-label="doce replied"
              >
                <AskUserQuestionWidget detail={pendingQuestion} />
              </div>
            ) : (
              showThinking && (
                <p className="text-sm text-muted-foreground" data-testid="agent-thinking">
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
        {!isAutoscrollPinned && (
          <Button
            type="button"
            variant="secondary"
            size="icon"
            className="absolute bottom-4 left-1/2 z-10 -translate-x-1/2 rounded-full bg-card/95 shadow-sm backdrop-blur supports-[backdrop-filter]:bg-card/80"
            onClick={scrollToBottomAndPin}
            aria-label="Scroll to bottom"
            data-testid="scroll-to-bottom"
          >
            <ArrowDownIcon size={16} />
          </Button>
        )}
      </div>
      <div
        className="border-t border-border p-4 [view-transition-name:chat-composer]"
        data-testid="workspace-composer-shell"
      >
        <RichInput
          onSubmit={(content, richContent) => {
            send(content, richContent);
          }}
          skillsEnabled={true}
          disabled={sendInFlight || pendingQuestion !== null}
          placeholder="Describe a task…"
          inputTestId="agent-input"
          submitTestId="agent-send"
          contextGauge={<ContextUsageGauge conversationId={conversationId} />}
        />
      </div>
    </div>
  );
}
