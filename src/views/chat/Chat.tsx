import { PaperPlaneRightIcon } from "@phosphor-icons/react";
import { useEffect, useRef, useState } from "react";
import ReactMarkdown from "react-markdown";
import { Button } from "@/components/ui/button";
import Timer from "@/components/Timer";
import MessageContent from "@/components/MessageContent";
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
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  // The completion listener below is registered once per conversationId,
  // not once per `pending` update — it needs the *current* pending value
  // without re-subscribing on every status change, so a ref instead of the
  // stale value the effect closed over.
  const pendingRef = useRef<PendingAssistant | null>(null);
  pendingRef.current = pending;

  const streamText = useConversationStreamStore((s) => s.streams[conversationId] ?? "");

  const adjustInputHeight = () => {
    const minHeight = 96;
    const textarea = textareaRef.current;
    if (!textarea) return;
    textarea.style.height = "auto";
    textarea.style.height = `${Math.min(Math.max(textarea.scrollHeight, minHeight), 180)}px`;
  };

  useEffect(() => {
    adjustInputHeight();
  }, [input]);

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
          prev && prev.status === "queued"
            ? { ...prev, status: "generating", queuePosition: null }
            : prev,
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
    // Guards against a second send landing while the first reply is still
    // in flight — without this, the assistant reply's sequence number
    // (assigned server-side only once generation finishes) can land after
    // a second user message's, permanently reordering the conversation,
    // and the second request's `pending` state clobbers the first's.
    if (!input.trim() || pending) return;
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
          {messages.map((m) => (
            <MessageContent key={m.id} message={m} showTimer />
          ))}
          {pending && (
            <div className="mb-6" role="group" aria-label="Doce replied">
              <p className="text-sm text-muted-foreground" data-testid="generation-status">
                {pending.status === "queued"
                  ? pending.queuePosition != null && pending.queuePosition > 0
                    ? `Queued (${pending.queuePosition} ahead)…`
                    : "Queued…"
                  : "Generating…"}
              </p>
              {streamText && (
                <div
                  className="prose prose-sm dark:prose-invert max-w-none"
                  data-testid="assistant-stream"
                >
                  <ReactMarkdown>{streamText}</ReactMarkdown>
                </div>
              )}
              <div className="mt-1 flex items-center justify-between">
                <p className="text-xs text-muted-foreground">
                  <Timer createdAt={pending.createdAt} durationMs={null} />
                </p>
                <Button
                  variant="ghost"
                  size="sm"
                  className="p-0 text-xs text-muted-foreground underline hover:bg-transparent hover:text-foreground"
                  onClick={cancel}
                  data-testid="cancel-generation"
                >
                  Cancel
                </Button>
              </div>
            </div>
          )}
          {error && (
            <div
              className="mb-6 rounded-lg bg-destructive/10 p-3 text-sm text-destructive"
              data-testid="chat-error"
            >
              {error}
            </div>
          )}
        </div>
      </div>
      <div className="border-t border-border p-4">
        <div className="flex items-end gap-2 rounded-2xl border border-border bg-card px-3 py-2 shadow-sm">
          <textarea
            ref={textareaRef}
            rows={4}
            className="min-h-[96px] flex-1 resize-none bg-transparent border-none px-0 py-1.5 text-sm leading-6 outline-none"
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                send();
              }
            }}
            placeholder="Message Doce…"
            data-testid="chat-input"
          />
          <Button
            type="button"
            variant="primary"
            className="h-8 w-8 shrink-0 rounded-full p-0"
            onClick={send}
            disabled={!input.trim() || !!pending}
            aria-label="Send message"
            data-testid="chat-send"
          >
            <PaperPlaneRightIcon size={16} />
          </Button>
        </div>
      </div>
    </div>
  );
}
