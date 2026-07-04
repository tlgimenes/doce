import { useEffect, useRef, useState } from "react";
import ReactMarkdown from "react-markdown";
import { Button } from "@/components/ui/button";
import Timer from "@/components/Timer";
import MessageContent from "@/components/MessageContent";
import RichInput from "./rich-input/RichInput";
import { commands, events, type Message, type RichMessageContent } from "@/lib/ipc";
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

  const send = async (content: string, richContent?: RichMessageContent) => {
    // Guards against a second send landing while the first reply is still
    // in flight — without this, the assistant reply's sequence number
    // (assigned server-side only once generation finishes) can land after
    // a second user message's, permanently reordering the conversation,
    // and the second request's `pending` state clobbers the first's.
    // richContent's own presence counts as "something to send" even when
    // content (the flat-text extraction) is empty — a message that's
    // entirely a chip (e.g. just a pasted-text node, no additional typed
    // text) must not be silently dropped here.
    if ((!content.trim() && !richContent) || pending) return;
    setError(null);

    const optimisticUserMessage: Message = {
      id: `pending-${Date.now()}`,
      conversationId,
      role: "user",
      contentType: richContent ? "rich_text" : "text",
      content: richContent ? JSON.stringify(richContent) : content,
      toolName: null,
      createdAt: Date.now(),
      durationMs: null,
    };
    setMessages((prev) => [...prev, optimisticUserMessage]);

    try {
      const result = await commands.sendMessage(
        conversationId,
        content,
        richContent ? JSON.stringify(richContent) : undefined,
      );
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
            <div className="mb-6" role="group" aria-label="doce replied">
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
        <RichInput
          onSubmit={send}
          skillsEnabled={false}
          disabled={!!pending}
          placeholder="Message doce…"
          inputTestId="chat-input"
          submitTestId="chat-send"
        />
      </div>
    </div>
  );
}
