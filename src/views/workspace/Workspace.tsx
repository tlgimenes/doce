import { PaperPlaneRightIcon } from "@phosphor-icons/react";
import { useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import MessageContent from "@/components/MessageContent";
import { commands, type Message } from "@/lib/ipc";

interface WorkspaceProps {
  conversationId: string;
}

/**
 * 006-chat-empty-state: restructured from a self-contained "pick a folder,
 * then chat" component into a `conversationId`-driven message view, the
 * same shape as `Chat.tsx` — folder selection now happens once, up front,
 * in `EmptyState.tsx`/`FolderPicker.tsx`. Still uses `sendAgentMessage`
 * (runs the full tool-use loop to completion before returning) rather than
 * `Chat.tsx`'s streamed `sendMessage`, so there's no Timer/stream state
 * here — just a single "thinking…" placeholder.
 */
export default function Workspace({ conversationId }: WorkspaceProps) {
  const [messages, setMessages] = useState<Message[]>([]);
  const [input, setInput] = useState("");
  const [thinking, setThinking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    setMessages([]);
    setInput("");
    setError(null);
    commands.listMessages(conversationId).then(setMessages);
  }, [conversationId]);

  const send = async () => {
    if (!input.trim() || thinking) return;
    const content = input;
    setInput("");
    setError(null);
    setMessages((prev) => [
      ...prev,
      {
        id: `u-${Date.now()}`,
        conversationId,
        role: "user",
        contentType: "text",
        content,
        toolName: null,
        createdAt: Date.now(),
        durationMs: null,
      },
    ]);
    setThinking(true);
    try {
      const reply = await commands.sendAgentMessage(conversationId, content);
      setMessages((prev) => [
        ...prev,
        {
          id: `a-${Date.now()}`,
          conversationId,
          role: "assistant",
          contentType: "text",
          content: reply,
          toolName: null,
          createdAt: Date.now(),
          durationMs: null,
        },
      ]);
    } catch (e) {
      setError(String(e));
    } finally {
      setThinking(false);
    }
  };

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

  return (
    <div className="flex h-dvh flex-col bg-background text-foreground">
      <div className="flex-1 overflow-y-auto p-4">
        <div className="mx-auto max-w-3xl">
          {messages.map((m) => (
            <MessageContent key={m.id} message={m} />
          ))}
          {thinking && (
            <p className="text-sm text-muted-foreground" data-testid="agent-thinking">
              Working…
            </p>
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
            placeholder="Describe a task…"
            data-testid="agent-input"
          />
          <Button
            type="button"
            variant="primary"
            className="h-8 w-8 shrink-0 rounded-full p-0"
            onClick={send}
            disabled={!input.trim() || thinking}
            aria-label="Send message"
            data-testid="agent-send"
          >
            <PaperPlaneRightIcon size={16} />
          </Button>
        </div>
      </div>
    </div>
  );
}
