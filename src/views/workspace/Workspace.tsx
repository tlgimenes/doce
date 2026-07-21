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
import AgentActivity from "@/views/workspace/AgentActivity";
import QueuedMessages from "@/views/workspace/QueuedMessages";
import {
  enqueueMessage,
  getQueueSnapshot,
  removeQueuedMessage,
  subscribeToQueue,
  type QueuedMessage,
} from "@/views/workspace/messageQueueRegistry";
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
  // Whether the observer has confirmed the current goal as met (from the
  // backend `goal-complete` event). Ephemeral/live only — resets whenever the
  // goal changes, clears, or the conversation switches. The banner shows
  // "Goal achieved" (muted, no edit/delete) when this is true.
  const [goalAchieved, setGoalAchieved] = useState(false);
  // Bumped when the status line's "edit goal" control is clicked — forwarded
  // to RichInput's `editGoalToken`, which loads the goal back into the
  // composer (goal mode + prefill). A changing token (not a boolean) so a
  // second edit click after cancelling still re-triggers.
  const [editGoalToken, setEditGoalToken] = useState(0);
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
  // Queue & steer: messages the user composed while this conversation's turn is
  // in flight, held client-side until they drain (FIFO, as new turns) or are
  // steered ("Send now"). Per-conversation, module-global, remount-proof — see
  // messageQueueRegistry.ts.
  const queue = useSyncExternalStore(
    subscribeToQueue,
    () => getQueueSnapshot(conversationId),
    () => getQueueSnapshot(conversationId),
  );
  // A subtle inline error when a steer is refused (a `rejected` outcome); the
  // row stays queued. Cleared on the next enqueue or a successful steer.
  const [steerError, setSteerError] = useState<string | null>(null);
  // Recall token for RichInput's "edit": bumping `token` re-fires the prefill
  // effect even when re-editing the same text.
  const [recall, setRecall] = useState<{
    token: number;
    content: string;
    richContent?: RichMessageContent;
  } | null>(null);
  // Holds the conversation id whose NEXT idle-drain a manual Stop must skip — a
  // Stop leaves queued messages intact (they drain only after a
  // naturally-completing turn). Conversation-scoped (not a bare boolean) so a
  // Stop in one conversation never suppresses another's drain; consumed on the
  // first idle pass for that conversation REGARDLESS of queue contents, so a
  // Stop with an empty queue can't strand the flag and wrongly suppress a later
  // turn's drain.
  const drainSuppressedRef = useRef<string | null>(null);

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
    setGoalAchieved(false);
    try {
      commands
        .getConversationGoal(conversationId)
        .then((loaded) => {
          if (cancelled || currentConversationIdRef.current !== conversationId) return;
          setGoal(loaded.goal);
          setGoalAchieved(loaded.achieved);
        })
        .catch(() => {
          if (!cancelled) {
            setGoal(null);
            setGoalAchieved(false);
          }
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
      setGoalAchieved(false);
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
    let unlistenGoal: (() => void) | undefined;
    let unlistenGoalComplete: (() => void) | undefined;

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

    // Unidirectional goal flow: the goal banner's single source of truth is
    // this event, fired by the backend on BOTH write paths (`sendAgentMessage`'s
    // `setGoal` flag and `setConversationGoal`'s edit/clear). RESILIENT by
    // construction (optional-chaining + try/catch): the forbidden
    // Workspace.test.tsx's `events` mock does not stub this listener at
    // all, so `events.onConversationGoalChanged` is `undefined` there —
    // this must not crash the workspace either way.
    (async () => {
      try {
        const un = await events.onConversationGoalChanged?.((p) => {
          if (p.conversationId !== conversationId) return;
          if (cancelled) return;
          setGoal(p.goal);
          // A goal change/clear means we're (re)pursuing, not achieved.
          setGoalAchieved(false);
        });
        if (cancelled) {
          un?.();
        } else {
          unlistenGoal = un;
        }
      } catch {
        // Event unavailable (e.g. a test without the stub) — the banner
        // just won't live-update; still fine, since `getConversationGoal`
        // above already covers mount/reload.
      }
    })();

    // The observer confirmed the goal at FinishTask -> flip the banner to
    // "Goal achieved". Same resilient pattern as above (the forbidden
    // Workspace.test.tsx mock doesn't stub this listener).
    (async () => {
      try {
        const un = await events.onGoalComplete?.((p) => {
          if (p.conversationId !== conversationId || cancelled) return;
          setGoalAchieved(true);
        });
        if (cancelled) {
          un?.();
        } else {
          unlistenGoalComplete = un;
        }
      } catch {
        // Event unavailable — banner just won't flip to achieved live.
      }
    })();

    return () => {
      cancelled = true;
      unlistenPersisted?.();
      unlistenPiece?.();
      unlistenGoal?.();
      unlistenGoalComplete?.();
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
    (content: string, richContent?: RichMessageContent, setGoal = false): boolean => {
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
          //
          // `setGoal` is passed only when true (not unconditionally as a
          // 4th positional arg every send) -- Workspace.test.tsx asserts
          // exact call shapes like `toHaveBeenCalledWith("conv-1", text,
          // undefined)` for ordinary sends across many cases, and a
          // trailing `false` would break every one of them.
          if (setGoal) {
            await commands.sendAgentMessage(
              conversationId,
              content,
              richContent ? JSON.stringify(richContent) : undefined,
              true,
            );
          } else {
            await commands.sendAgentMessage(
              conversationId,
              content,
              richContent ? JSON.stringify(richContent) : undefined,
            );
          }
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

  // "Send as goal" (unidirectional goal flow): ONE request, not
  // persist-then-send. `send`'s new `setGoal` flag rides along on the same
  // `send_agent_message` call the turn itself uses -- the backend persists
  // the goal (the message content IS the goal text) before it loads
  // `Plan.goal` for this same turn, then emits `ConversationGoalChanged`,
  // which the subscription below reacts to. No local optimistic `setGoal`
  // here and no `await` on a separate persist call: there's nothing left to
  // race, since a single backend call now does both in the right order.
  const handleSendAsGoal = useCallback(
    (text: string) => {
      send(text, undefined, true);
    },
    [send],
  );

  // Queue & steer: enqueue a message composed while the conversation is busy
  // (or while other messages are already queued, so the queue is never jumped).
  // Empty guard mirrors `send`'s — a chips-only message has empty flat text but
  // must not be dropped. A fresh id per enqueue keys the row.
  const enqueue = useCallback(
    (content: string, richContent?: RichMessageContent, setGoalIntent = false) => {
      if (!content.trim() && !richContent) return;
      setSteerError(null);
      enqueueMessage(conversationId, {
        id: crypto.randomUUID(),
        content,
        richContent,
        setGoal: setGoalIntent,
      });
    },
    [conversationId],
  );

  // "Send now": steer a queued message into the running turn. On `injected` the
  // row is removed and the message renders via the normal
  // `agent-message-persisted` refresh (no manual insert). On `noActiveTurn` (the
  // turn ended between queueing and clicking) fall back to a fresh turn. On
  // `rejected` (a standalone /compact holds the conversation) keep the row and
  // surface a subtle error.
  const handleSteer = useCallback(
    (item: QueuedMessage) => {
      void (async () => {
        const rich = item.richContent ? JSON.stringify(item.richContent) : undefined;
        try {
          const outcome = await commands.steerGeneration(conversationId, item.content, rich);
          if (!isMountedRef.current || currentConversationIdRef.current !== conversationId) return;
          if (outcome === "injected") {
            removeQueuedMessage(conversationId, item.id);
            setSteerError(null);
          } else if (outcome === "noActiveTurn") {
            if (send(item.content, item.richContent, item.setGoal ?? false)) {
              removeQueuedMessage(conversationId, item.id);
            }
          } else {
            setSteerError("Couldn't send now — the turn isn't accepting messages.");
          }
        } catch (e) {
          if (isMountedRef.current && currentConversationIdRef.current === conversationId) {
            setSteerError(String(e));
          }
        }
      })();
    },
    [conversationId, send],
  );

  // "Edit": recall a queued message back into the composer and drop its row.
  const handleEditQueued = useCallback(
    (item: QueuedMessage) => {
      setRecall({ token: Date.now(), content: item.content, richContent: item.richContent });
      removeQueuedMessage(conversationId, item.id);
    },
    [conversationId],
  );

  // Generation-cancellation (Task 4.2b): fire-and-forget stop of the running
  // turn, following the same invoke convention as `send`/`/compact`. A user's
  // own stop must NEVER paint an error banner, so failures are only logged —
  // stopping an already-finished turn is a documented backend no-op. The
  // transcript + status refresh themselves off the backend's own
  // `agent-message-persisted` event once the loop halts (the cancel arm now
  // persists a stopped marker), so nothing to update here.
  //
  // Queue & steer: suppress the ONE drain that the falling edge of
  // `turnInFlight` would otherwise trigger — a manual Stop leaves the queue
  // intact (rows stay actionable and drain only after a naturally-completing
  // turn), unlike a natural completion which does drain.
  const handleStop = useCallback(() => {
    drainSuppressedRef.current = conversationId;
    void commands.stopGeneration(conversationId).catch((e) => {
      console.error("stop_generation failed", e);
    });
  }, [conversationId]);

  // Queue & steer: drain the queue FIFO once the conversation goes idle. Each
  // dequeued message is dispatched as its own new turn; `send` re-marks
  // in-flight synchronously, so this effect then waits for that turn to complete
  // before the next iteration dispatches the following message. The head is
  // removed before dispatch so a `send` that consumes-without-a-turn (an
  // intercepted `/compact`) can't re-drain forever.
  useEffect(() => {
    if (turnInFlight || pendingToolCall) return;
    // Consume a pending Stop-suppression for THIS conversation on the first idle
    // pass — before the empty-queue check — so an empty-queue Stop can't leave
    // the flag set to poison a future turn's drain.
    if (drainSuppressedRef.current === conversationId) {
      drainSuppressedRef.current = null;
      return;
    }
    if (queue.length === 0) return;
    const head = queue[0];
    removeQueuedMessage(conversationId, head.id);
    send(head.content, head.richContent, head.setGoal ?? false);
  }, [turnInFlight, pendingToolCall, queue, send, conversationId]);

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
        <div className="p-4" data-testid="workspace-composer-shell">
          {/* The view-transition name lives on the max-w-xl column, matching
              EmptyState's named element exactly — same width on both sides of
              the transition, so the morph never grows on the x axis. Queue &
              steer + agent-activity stack as one surface (gap-1) in the order
              queued messages → status line → input. */}
          <div className="mx-auto flex w-full max-w-xl flex-col gap-1 [view-transition-name:chat-composer]">
            {/* Queued messages sit at the top of the stack — above the status
                line and the ask widget, so they stay manageable even when a
                question is pending. */}
            <QueuedMessages
              items={queue}
              onSteer={handleSteer}
              onEdit={handleEditQueued}
              onDelete={(id) => removeQueuedMessage(conversationId, id)}
              steerError={steerError}
            />
            {/* The single agent-activity status line, docked directly on the
                composer: goal › current todo › thinking, plus progress and the
                working indicator. */}
            <AgentActivity
              conversationId={conversationId}
              goal={{
                current: goal,
                achieved: goalAchieved,
                onEdit: () => setEditGoalToken((token) => token + 1),
                onDelete: () => handleSetGoal(null),
              }}
              streaming={{
                active: showGenericStreamingStatus,
                startedAt: activeTurnStartedAt,
                tokens: activeTurnTokens,
                stream: liveGenText,
              }}
            />
            {pendingQuestion ? (
              <UserAskWidget detail={pendingQuestion} />
            ) : (
              <RichInput
                // Queue & steer: while the conversation is busy (or the queue is
                // non-empty, so the queue is never jumped) a submit ENQUEUES;
                // otherwise it sends immediately as before.
                onSubmit={(content, richContent) => {
                  if (turnInFlight || pendingToolCall !== null || queue.length > 0) {
                    enqueue(content, richContent);
                  } else {
                    send(content, richContent);
                  }
                }}
                skillsEnabled={true}
                // Editable even while a turn runs — that's how a message gets
                // composed to queue. Duplicate-send is still prevented because a
                // busy submit enqueues rather than sends.
                disabled={false}
                isGenerating={turnInFlight}
                onStop={handleStop}
                recall={recall ?? undefined}
                placeholder="Describe a task…"
                inputTestId="agent-input"
                submitTestId="agent-send"
                editGoalToken={editGoalToken}
                goal={{
                  current: goal,
                  onSet: handleSetGoal,
                  // Goal submits queue too when busy (drain as a goal turn); the
                  // row hides "Send now" since steering carries no goal intent.
                  onSendAsGoal: (text) => {
                    if (turnInFlight || pendingToolCall !== null || queue.length > 0) {
                      enqueue(text, undefined, true);
                    } else {
                      handleSendAsGoal(text);
                    }
                  },
                }}
              />
            )}
          </div>
        </div>
      </MessageScrollerProvider>
    </div>
  );
}
