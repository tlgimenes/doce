import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { estimateTokenCount } from "@/lib/estimateTokenCount";
import { runViewTransition } from "@/lib/viewTransition";
import { Alert, AlertDescription } from "@/components/ui/alert";
import {
  MessageScroller,
  MessageScrollerButton,
  MessageScrollerContent,
  MessageScrollerProvider,
  MessageScrollerViewport,
  useMessageScroller,
} from "@/components/ui/message-scroller";
import RichInput from "@/views/chat/rich-input/RichInput";
import UserAskWidget from "@/views/chat/tool-widgets/UserAskWidget";
import PlanTracker from "@/views/workspace/PlanTracker";
import StreamingStatus from "@/views/workspace/StreamingStatus";
import WorkspaceTopbar from "@/views/workspace/WorkspaceTopbar";
import TranscriptTurn, { type PendingTurnWidget } from "@/views/workspace/TranscriptTurn";
import { accumulateTurnTokens, groupTranscriptTurns } from "@/views/workspace/transcriptTurns";
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
  if (latest?.contentType !== "tool_call" || latest.toolName !== "AskUserQuestion") return false;
  return parseAskUserQuestionCallDetail(latest.content) !== null;
}

function getLatestUserMessageCreatedAt(messages: Message[]): number | null {
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    if (messages[i].role === "user") {
      return messages[i].createdAt;
    }
  }
  return null;
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

/**
 * Test-only: clears the module-global in-flight-send registry. The registry
 * is deliberately module-scoped and survives unmount (so a pending send stays
 * "in flight" across a conversation remount — the very behavior the remount
 * test exercises), which means a test that leaves a turn pending (a
 * never-resolving `sendAgentMessage` mock) would otherwise leak that state
 * into the next test. Not referenced in production code.
 */
export function __resetSendInFlightRegistryForTests() {
  conversationsWithSendInFlight.clear();
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
 * useMessageScroller must be called under MessageScrollerProvider, but the
 * send() callback lives in Workspace (the Provider's renderer). This inert
 * bridge hands the scrollToEnd handle up via a ref.
 */
function ScrollToEndBridge({
  scrollToEndRef,
}: {
  scrollToEndRef: { current: (() => void) | null };
}) {
  const { scrollToEnd } = useMessageScroller();
  useEffect(() => {
    scrollToEndRef.current = () => {
      scrollToEnd({ behavior: "smooth" });
    };
    return () => {
      scrollToEndRef.current = null;
    };
  }, [scrollToEnd, scrollToEndRef]);
  return null;
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
  const [optimisticTurnStartedAt, setOptimisticTurnStartedAt] = useState<number | null>(null);
  // The in-flight prompt's chars/4 estimate, carried ACROSS refetches: the
  // backend emits agent-message-persisted for the user row immediately
  // after persisting it, while its token_count is still NULL (the real
  // count is UPDATEd only after the engine loads) — without this, the
  // wholesale setMessages(loadedMessages) on that first event would wipe
  // the optimistic row's estimate and blank the streaming ↑ counter.
  const optimisticPromptTokensRef = useRef<number | null>(null);
  // The in-flight generation's raw sampled text (mostly <think> reasoning
  // under Require mode) — ephemeral ticker for the working shimmer, reset
  // at every persisted-row boundary and on turn end.
  const [liveGenText, setLiveGenText] = useState("");
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
  // observer-verified completion + goals (composer relocation): the
  // conversation's current goal, fetched below and passed to the main
  // RichInput's `goal` prop. `null` means "no goal set" (the resolved,
  // steady state) — also the value used on a failed/unavailable fetch (see
  // the effect below), matching the old topbar `GoalBar`'s own
  // never-crash-on-a-failed-invoke guard.
  const [goal, setGoal] = useState<string | null>(null);
  const isMountedRef = useRef(true);
  const genericStatusFallbackStartedAtRef = useRef<number | null>(null);
  const currentConversationIdRef = useRef(conversationId);
  currentConversationIdRef.current = conversationId;
  const onConversationSeenRef = useRef(onConversationSeen);
  onConversationSeenRef.current = onConversationSeen;
  const consumedInitialTurnRef = useRef<string | null>(null);
  const dispatchedInitialTurnRef = useRef<string | null>(null);
  const scrollToEndRef = useRef<(() => void) | null>(null);
  const sendInFlight = useSyncExternalStore(
    subscribeToSendInFlight,
    () => conversationsWithSendInFlight.has(conversationId),
    getServerSnapshot,
  );

  const syncBackendTurnActive = useCallback(() => {
    const targetConversationId = conversationId;
    void Promise.resolve(commands.isGenerationActive(targetConversationId))
      .then((active) => {
        if (isMountedRef.current && currentConversationIdRef.current === targetConversationId) {
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
    let loadedMessages = await commands.listMessages(targetConversationId);
    if (!isMountedRef.current || currentConversationIdRef.current !== targetConversationId) return;

    // Keep the estimate on the just-sent prompt until its real tokenizer
    // count lands (see optimisticPromptTokensRef).
    const promptEstimate = optimisticPromptTokensRef.current;
    if (promptEstimate != null) {
      let lastUserIndex = -1;
      for (let i = loadedMessages.length - 1; i >= 0; i--) {
        if (loadedMessages[i].role === "user") {
          lastUserIndex = i;
          break;
        }
      }
      if (lastUserIndex !== -1 && loadedMessages[lastUserIndex].tokenCount == null) {
        loadedMessages = loadedMessages.slice();
        loadedMessages[lastUserIndex] = {
          ...loadedMessages[lastUserIndex],
          tokenCount: promptEstimate,
        };
      }
    }

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
    setOptimisticTurnStartedAt(null);
    setError(null);
    setBackendTurnActive(false);
    syncBackendTurnActive();
    dispatchedInitialTurnRef.current = null;
    commands.listMessages(conversationId).then((loadedMessages) => {
      if (cancelled || currentConversationIdRef.current !== conversationId) return;

      if (loadedMessages.length === 0 && dispatchedInitialTurnRef.current === conversationId) {
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

  // observer-verified completion + goals (composer relocation): load the
  // conversation's goal on mount / whenever the active conversation
  // changes, feeding the main RichInput's `goal.current`. RESILIENCE:
  // wrapped in try/catch, defaulting to `null` on any throw — a test's
  // `commands` mock may not stub `getConversationGoal` at all (a missing
  // mock property calling it throws synchronously, "not a function",
  // before any Promise even exists to `.catch()`), and this fetch must
  // never crash the workspace either way. Mirrors the old topbar
  // `GoalBar.tsx`'s own guard exactly.
  useEffect(() => {
    let cancelled = false;
    setGoal(null);
    try {
      commands
        .getConversationGoal(conversationId)
        .then((loadedGoal) => {
          if (cancelled || currentConversationIdRef.current !== conversationId) return;
          setGoal(loadedGoal);
        })
        .catch(() => {
          if (!cancelled) setGoal(null);
        });
    } catch {
      setGoal(null);
    }
    return () => {
      cancelled = true;
    };
  }, [conversationId]);

  // Sets (non-empty string) or clears (`null`) the conversation's goal —
  // updates local state optimistically, then persists via
  // `setConversationGoal` best-effort (same try/catch-around-the-call
  // guard as the read path above, for the same reason: a test/host that
  // hasn't stubbed this command must not crash the composer).
  const handleSetGoal = useCallback(
    (nextGoal: string | null) => {
      setGoal(nextGoal);
      try {
        commands.setConversationGoal(conversationId, nextGoal).catch(() => {
          // Best-effort — the optimistic local state is left in place even
          // if the persist call fails; a reload would reveal the mismatch,
          // exactly as it would have if `GoalBar.save()`'s own failure path
          // had reverted (it deliberately did not).
        });
      } catch {
        // `setConversationGoal` missing/unavailable — ignore, matching the
        // read-path guard.
      }
    },
    [conversationId],
  );

  useEffect(() => {
    let cancelled = false;
    let unlistenPersisted: (() => void) | undefined;
    let unlistenPiece: (() => void) | undefined;

    (async () => {
      unlistenPersisted = await events.onAgentMessagePersisted((p) => {
        if (p.conversationId !== conversationId) return;
        if (cancelled) return;
        // A persisted row is a generation boundary — the live ticker's
        // text now exists (stripped) in the transcript, or was reasoning
        // that intentionally never will.
        setLiveGenText("");
        void refreshMessages();
      });
      if (cancelled) {
        unlistenPersisted();
      }
    })();

    (async () => {
      unlistenPiece = await events.onAgentGenerationPiece((p) => {
        if (p.conversationId !== conversationId) return;
        if (cancelled) return;
        setLiveGenText((prev) => prev + p.piece);
      });
      if (cancelled) {
        unlistenPiece();
      }
    })();

    return () => {
      cancelled = true;
      unlistenPersisted?.();
      unlistenPiece?.();
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
  const pendingQuestion = pendingToolCall?.kind === "question" ? pendingToolCall.detail : null;
  const turnInFlight = sendInFlight || backendTurnActive;
  const showThinking = thinking || turnInFlight;
  const latestUserMessageCreatedAt = useMemo(
    () => getLatestUserMessageCreatedAt(messages),
    [messages],
  );
  const showGenericStreamingStatus =
    pendingToolCall?.kind === "other" || (!pendingToolCall && showThinking);
  const activeTurnStartedAtCandidate = optimisticTurnStartedAt ?? latestUserMessageCreatedAt;

  if (!showGenericStreamingStatus) {
    genericStatusFallbackStartedAtRef.current = null;
  } else if (activeTurnStartedAtCandidate !== null) {
    genericStatusFallbackStartedAtRef.current = activeTurnStartedAtCandidate;
  } else if (genericStatusFallbackStartedAtRef.current === null) {
    genericStatusFallbackStartedAtRef.current = Date.now();
  }

  const activeTurnStartedAt = showGenericStreamingStatus
    ? (activeTurnStartedAtCandidate ?? genericStatusFallbackStartedAtRef.current)
    : null;
  const transcriptTurns = useMemo(() => groupTranscriptTurns(messages), [messages]);
  // Live in/out accumulation for the in-flight turn — feeds the working
  // shimmer's token counter and grows as tool results land.
  const activeTurnTokens = useMemo(() => {
    const lastTurn = transcriptTurns[transcriptTurns.length - 1];
    return lastTurn ? accumulateTurnTokens(lastTurn) : null;
  }, [transcriptTurns]);
  const pendingTurnWidget: PendingTurnWidget | null =
    pendingToolCall?.kind === "bash" || pendingToolCall?.kind === "task" ? pendingToolCall : null;

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
            if (!isMountedRef.current || currentConversationIdRef.current !== conversationId) {
              requestConversationRefresh(conversationId, {
                contextUsage: usage,
              });
              return;
            }

            useContextUsageStore.getState().setUsage(usage);
            await refreshMessages();
          } catch (e) {
            if (isMountedRef.current && currentConversationIdRef.current === conversationId) {
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
      if ((!content.trim() && !richContent) || turnInFlight || pendingToolCall) return false;
      if (!markSendInFlight(conversationId)) return false;

      const submittedAt = Date.now();
      setError(null);
      setLiveGenText("");
      optimisticPromptTokensRef.current = estimateTokenCount(content);
      setMessages((prev) => [
        ...prev,
        {
          id: `u-${submittedAt}`,
          conversationId,
          role: "user",
          contentType: richContent ? "rich_text" : "text",
          content: richContent ? JSON.stringify(richContent) : content,
          toolName: null,
          createdAt: submittedAt,
          durationMs: null,
          // A chars/4 estimate so the streaming counter shows the prompt's
          // ↑ cost immediately on submit. The real tokenizer count lands
          // with the first agent-message-persisted refetch (the backend
          // UPDATEs the persisted row before the loop's first generation).
          tokenCount: estimateTokenCount(content),
        },
      ]);
      setOptimisticTurnStartedAt(submittedAt);
      setThinking(true);
      // Sending your own message always re-engages autoscroll and snaps to
      // the end. This used to be use-stick-to-bottom's job before the swap
      // to the shadcn message-scroller, and it's still load-bearing here:
      // the message-scroller only follows content growth while its internal
      // "following-bottom" mode is engaged, and ANY upward scroll/wheel
      // (even a stray trackpad flick) escapes it. Without this call a send
      // made after scrolling up would add content off-screen instead of
      // following it.
      scrollToEndRef.current?.();
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
          if (isMountedRef.current && currentConversationIdRef.current === conversationId) {
            setError(String(e));
          }
        } finally {
          clearSendInFlight(conversationId);
          optimisticPromptTokensRef.current = null;
          if (isMountedRef.current && currentConversationIdRef.current === conversationId) {
            setThinking(false);
            setLiveGenText("");
            setOptimisticTurnStartedAt(null);
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

  // "Send as goal": persist the goal, THEN start a turn to pursue it (the goal
  // text becomes the turn's message). Setting a goal on an idle conversation
  // should actually begin work, not silently wait for the next manual send
  // (which is what a persist-only `handleSetGoal` did). The persist is AWAITED
  // before `send` because the loop reads the goal from the DB at task start
  // (`send_agent_message` -> `Plan.goal`); sending first would race it. `send`
  // already no-ops while a turn is in flight, so this is idle-only by
  // construction. Defined AFTER `send` so it can list it as a dependency.
  const handleSendAsGoal = useCallback(
    async (text: string) => {
      setGoal(text);
      try {
        await commands.setConversationGoal(conversationId, text);
      } catch {
        // Best-effort persist (same guard as handleSetGoal); still start the
        // turn so the user's intent isn't dropped on a persist hiccup.
      }
      send(text);
    },
    [conversationId, send],
  );

  // Generation-cancellation (Task 4.2b): fire-and-forget stop of the running
  // turn, following the same invoke convention as `send`/`/compact`. A user's
  // own stop must NEVER paint an error banner, so failures are only logged —
  // stopping an already-finished turn is a documented backend no-op. The
  // transcript + status refresh themselves off the backend's own
  // `agent-message-persisted` event once the loop halts (the cancel arm now
  // persists a stopped marker), so nothing to update here.
  const handleStop = useCallback(() => {
    void commands.stopGeneration(conversationId).catch((e) => {
      console.error("stop_generation failed", e);
    });
  }, [conversationId]);

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
    <div className="flex h-full flex-col">
      {conversation && <WorkspaceTopbar conversation={conversation} />}
      {/* `key` remounts the scroller's scroll state per conversation — a
          fresh conversation starts pinned at the end (`defaultScrollPosition
          ="end"`), matching the old reset-pinning-on-switch behavior.
          Autoscroll (following content growth while pinned, escaping on
          upward scroll, the scroll-to-end button's show/hide) is the
          message-scroller primitive's own job now. */}
      <MessageScrollerProvider key={conversationId} autoScroll defaultScrollPosition="end">
        <ScrollToEndBridge scrollToEndRef={scrollToEndRef} />
        <MessageScroller className="h-auto min-h-0 flex-1">
          <MessageScrollerViewport className="p-4" data-testid="workspace-scroll-container">
            <MessageScrollerContent data-testid="workspace-transcript-content">
              <div className="mx-auto w-full max-w-xl">
                {transcriptTurns.map((turn, index) => {
                  const isLastTurn = index === transcriptTurns.length - 1;
                  return (
                    <TranscriptTurn
                      key={turn.id}
                      turn={turn}
                      isLastTurn={isLastTurn}
                      pendingWidget={isLastTurn ? pendingTurnWidget : null}
                      error={isLastTurn ? error : null}
                    />
                  );
                })}
                {transcriptTurns.length === 0 && error && (
                  <Alert variant="destructive" className="mb-6" data-testid="workspace-error">
                    <AlertDescription>{error}</AlertDescription>
                  </Alert>
                )}
              </div>
            </MessageScrollerContent>
          </MessageScrollerViewport>
          <MessageScrollerButton data-testid="scroll-to-bottom" />
        </MessageScroller>
        <PlanTracker conversationId={conversationId} />
        {showGenericStreamingStatus && (
          <StreamingStatus
            startedAt={activeTurnStartedAt}
            tokens={activeTurnTokens}
            stream={liveGenText}
          />
        )}
        <div className="p-4" data-testid="workspace-composer-shell">
          {/* The view-transition name lives on the max-w-xl column, matching
              EmptyState's named element exactly — same width on both sides
              of the transition, so the morph never grows on the x axis. */}
          <div className="mx-auto w-full max-w-xl [view-transition-name:chat-composer]">
            {pendingQuestion ? (
              <UserAskWidget detail={pendingQuestion} />
            ) : (
              <RichInput
                onSubmit={(content, richContent) => {
                  send(content, richContent);
                }}
                skillsEnabled={true}
                disabled={turnInFlight || pendingToolCall !== null}
                isGenerating={turnInFlight}
                onStop={handleStop}
                placeholder="Describe a task…"
                inputTestId="agent-input"
                submitTestId="agent-send"
                goal={{ current: goal, onSet: handleSetGoal, onSendAsGoal: handleSendAsGoal }}
              />
            )}
          </div>
        </div>
      </MessageScrollerProvider>
    </div>
  );
}
