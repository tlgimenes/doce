import { useEffect, useState } from "react";
import ReactMarkdown from "react-markdown";
import { Button } from "@/components/ui/button";
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

  return (
    <div className="flex h-dvh flex-col bg-background text-foreground">
      <div className="flex-1 overflow-y-auto p-4">
        <div className="mx-auto max-w-3xl">
          {messages.map((m) =>
            m.role === "user" ? (
              <div
                key={m.id}
                className="mb-6 rounded-lg bg-muted p-3"
                data-testid="chat-message"
                role="group"
                aria-label="You said"
              >
                <ReactMarkdown>{m.content}</ReactMarkdown>
              </div>
            ) : (
              <div
                key={m.id}
                className="mb-6"
                data-testid="chat-message"
                role="group"
                aria-label="Doce replied"
              >
                <div className="prose prose-sm dark:prose-invert max-w-none">
                  <ReactMarkdown>{m.content}</ReactMarkdown>
                </div>
              </div>
            ),
          )}
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
      <div className="flex gap-2 border-t border-border p-4">
        <input
          className="flex-1 rounded-md border border-border bg-card px-3 py-2"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && send()}
          placeholder="Describe a task…"
          data-testid="agent-input"
        />
        <Button
          variant="primary"
          onClick={send}
          disabled={!input.trim() || thinking}
          data-testid="agent-send"
        >
          Send
        </Button>
      </div>
    </div>
  );
}
