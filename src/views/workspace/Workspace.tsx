import { useEffect, useState } from "react";
import MessageContent from "@/components/MessageContent";
import RichInput from "@/views/chat/rich-input/RichInput";
import { commands, type Message, type RichMessageContent } from "@/lib/ipc";

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
  const [thinking, setThinking] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setMessages([]);
    setError(null);
    commands.listMessages(conversationId).then(setMessages);
  }, [conversationId]);

  const send = async (content: string, richContent?: RichMessageContent) => {
    // richContent's own presence counts as "something to send" even when
    // content (the flat-text extraction) is empty — a message that's
    // entirely a chip (e.g. just a pasted-text or attachment node, no
    // additional typed text) must not be silently dropped here.
    if ((!content.trim() && !richContent) || thinking) return;
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
      },
    ]);
    setThinking(true);
    try {
      const reply = await commands.sendAgentMessage(
        conversationId,
        content,
        richContent ? JSON.stringify(richContent) : undefined,
      );
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
        <RichInput
          onSubmit={(content, richContent) => {
            send(content, richContent);
          }}
          skillsEnabled={true}
          disabled={thinking}
          placeholder="Describe a task…"
          inputTestId="agent-input"
          submitTestId="agent-send"
        />
      </div>
    </div>
  );
}
