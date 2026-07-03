import { useEffect, useRef, useState } from "react";
import ReactMarkdown from "react-markdown";
import Timer from "@/components/Timer";
import { commands, events, type Message } from "@/lib/ipc";
import { useConversationStreamStore } from "@/state/conversationStreamStore";

type GenerationStatus = "queued" | "generating" | null;

interface PendingAssistant {
  id: string;
  requestId: string;
  createdAt: number;
  status: GenerationStatus;
  queuePosition: number | null;
}

interface ChatProps {
  conversationId: string;
}

/**
 * User Story 2: streaming, markdown-rendering chat with locally persisted
 * history (FR-006, FR-007). Messages render as a single, sequence-ordered
 * list (not "all user bubbles, then the reply") so a multi-turn
 * conversation reads as real back-and-forth turns.
 *
 * Takes `conversationId` as a prop (User Story 7): the conversation list
 * sidebar owns which conversation is selected, including creating new
 * ones — this view just renders whichever one it's given.
 *
 * User Story 5: reflects the real scheduler's queued/generating state
 * (not a fake spinner) and offers cancellation, which preserves whatever
 * partial output had streamed so far as the persisted message.
 */
export default function Chat({ conversationId }: ChatProps) {
  const [messages, setMessages] = useState<Message[]>([]);
  const [pending, setPending] = useState<PendingAssistant | null>(null);
  const [input, setInput] = useState("");
  const [error, setError] = useState<string | null>(null);
  // The completion listener below is registered once per conversationId,
  // not once per `pending` update — it needs the *current* pending value
  // without re-subscribing on every status change, so a ref instead of the
  // stale value the effect closed over.
  const pendingRef = useRef<PendingAssistant | null>(null);
  pendingRef.current = pending;

  const streamText = useConversationStreamStore((s) => s.streams[conversationId] ?? "");

  useEffect(() => {
    setMessages([]);
    setPending(null);
    setInput("");
    setError(null);
    commands.listMessages(conversationId).then(setMessages);
  }, [conversationId]);

  useEffect(() => {
    let unlistenToken: (() => void) | undefined;
    let unlistenComplete: (() => void) | undefined;
    let unlistenError: (() => void) | undefined;
    let unlistenQueue: (() => void) | undefined;

    (async () => {
      unlistenToken = await events.onAssistantToken((p) => {
        if (p.conversationId !== conversationId) return;
        setPending((prev) =>
          prev && prev.status === "queued" ? { ...prev, status: "generating", queuePosition: null } : prev,
        );
      });

      unlistenQueue = await events.onGenerationQueueUpdate((p) => {
        if (p.conversationId !== conversationId) return;
        setPending((prev) =>
          prev && prev.requestId === p.requestId
            ? { ...prev, status: p.state, queuePosition: p.position }
            : prev,
        );
      });

      unlistenComplete = await events.onAssistantMessageComplete((p) => {
        if (p.conversationId !== conversationId) return;
        const finalText = useConversationStreamStore.getState().streams[conversationId] ?? "";
        setMessages((prev) => [
          ...prev,
          {
            id: p.messageId,
            conversationId,
            role: "assistant",
            contentType: "text",
            content: finalText,
            toolName: null,
            createdAt: pendingRef.current?.createdAt ?? Date.now() - p.durationMs,
            durationMs: p.durationMs,
          },
        ]);
        useConversationStreamStore.getState().clearStream(conversationId);
        setPending(null);
      });

      unlistenError = await events.onAssistantMessageError((p) => {
        if (p.conversationId !== conversationId) return;
        setError(p.error);
        useConversationStreamStore.getState().clearStream(conversationId);
        setPending(null);
      });
    })();

    return () => {
      unlistenToken?.();
      unlistenComplete?.();
      unlistenError?.();
      unlistenQueue?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [conversationId]);

  const send = async () => {
    if (!input.trim()) return;
    const content = input;
    setInput("");
    setError(null);

    const optimisticUserMessage: Message = {
      id: `pending-${Date.now()}`,
      conversationId,
      role: "user",
      contentType: "text",
      content,
      toolName: null,
      createdAt: Date.now(),
      durationMs: null,
    };
    setMessages((prev) => [...prev, optimisticUserMessage]);

    try {
      const result = await commands.sendMessage(conversationId, content);
      setPending({
        id: result.assistantMessageId,
        requestId: result.requestId,
        createdAt: result.assistantCreatedAt,
        status: "queued",
        queuePosition: null,
      });
    } catch (e) {
      setError(String(e));
    }
  };

  const cancel = async () => {
    if (!pending) return;
    await commands.cancelGeneration(pending.requestId);
  };

  return (
    <div className="flex h-dvh flex-col bg-background text-foreground">
      <div className="flex-1 overflow-y-auto p-4">
        <div className="mx-auto max-w-3xl">
          {messages.map((m) =>
            m.role === "user" ? (
              <div key={m.id} className="mb-6 rounded-lg bg-muted p-3" data-testid="chat-message">
                <ReactMarkdown>{m.content}</ReactMarkdown>
              </div>
            ) : (
              <div key={m.id} className="mb-6" data-testid="chat-message">
                <ReactMarkdown>{m.content}</ReactMarkdown>
                <p className="mt-1 text-xs text-muted-foreground">
                  <Timer createdAt={m.createdAt} durationMs={m.durationMs} />
                </p>
              </div>
            ),
          )}
          {pending && (
            <div className="mb-6">
              {streamText ? (
                <div data-testid="assistant-stream">
                  <ReactMarkdown>{streamText}</ReactMarkdown>
                </div>
              ) : (
                <p className="text-sm text-muted-foreground" data-testid="generation-status">
                  {pending.status === "queued"
                    ? pending.queuePosition != null && pending.queuePosition > 0
                      ? `Queued (${pending.queuePosition} ahead)…`
                      : "Queued…"
                    : "Generating…"}
                </p>
              )}
              <div className="mt-1 flex items-center justify-between">
                <p className="text-xs text-muted-foreground">
                  <Timer createdAt={pending.createdAt} durationMs={null} />
                </p>
                <button
                  className="text-xs text-muted-foreground underline hover:text-foreground"
                  onClick={cancel}
                  data-testid="cancel-generation"
                >
                  Cancel
                </button>
              </div>
            </div>
          )}
          {error && (
            <div className="mb-6 rounded-lg bg-destructive/10 p-3 text-sm text-destructive" data-testid="chat-error">
              {error}
            </div>
          )}
        </div>
      </div>
      <div className="flex gap-2 border-t border-border p-4">
        <input
          className="flex-1 rounded-md border border-border bg-card px-3 py-2"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && send()}
          placeholder="Message Doce…"
          data-testid="chat-input"
        />
        <button
          className="rounded-md bg-primary px-4 py-2 text-primary-foreground disabled:cursor-not-allowed disabled:opacity-50"
          onClick={send}
          disabled={!input.trim()}
          data-testid="chat-send"
        >
          Send
        </button>
      </div>
    </div>
  );
}
