import { useState } from "react";
import ReactMarkdown from "react-markdown";
import { commands } from "@/lib/ipc";

interface AgentMessage {
  id: string;
  role: "user" | "assistant";
  content: string;
}

/**
 * User Story 3: opening a folder turns the app into a coding/system agent
 * (FR-008). Minimal by design for this pass — `send_agent_message` runs
 * the full tool-use loop to completion before returning (see
 * commands/agent.rs for why: no live per-turn streaming yet), so this
 * view shows a single "thinking…" state rather than a trace of each tool
 * call the way `quickstart.md` §3's full vision describes. The workspace
 * file tree / diff viewer / terminal panel (tasks.md T060) aren't built
 * yet either — this is a working vertical slice (open folder -> agent
 * uses real tools -> real answer), not the full workspace UI.
 */
export default function Workspace() {
  const [pathInput, setPathInput] = useState("");
  const [conversationId, setConversationId] = useState<string | null>(null);
  const [messages, setMessages] = useState<AgentMessage[]>([]);
  const [input, setInput] = useState("");
  const [thinking, setThinking] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const openFolder = async () => {
    if (!pathInput.trim()) return;
    setError(null);
    try {
      const workspace = await commands.openWorkspace(pathInput.trim());
      const conv = await commands.createConversation(workspace.id);
      setConversationId(conv.id);
      setMessages([]);
    } catch (e) {
      setError(String(e));
    }
  };

  const send = async () => {
    if (!conversationId || !input.trim() || thinking) return;
    const content = input;
    setInput("");
    setError(null);
    setMessages((prev) => [...prev, { id: `u-${Date.now()}`, role: "user", content }]);
    setThinking(true);
    try {
      const reply = await commands.sendAgentMessage(conversationId, content);
      setMessages((prev) => [...prev, { id: `a-${Date.now()}`, role: "assistant", content: reply }]);
    } catch (e) {
      setError(String(e));
    } finally {
      setThinking(false);
    }
  };

  if (!conversationId) {
    return (
      <div className="flex h-dvh flex-col items-center justify-center gap-4 bg-background text-foreground">
        <h2 className="text-balance text-lg font-medium">Open a folder to start an agent session</h2>
        <div className="flex gap-2">
          <input
            className="w-96 rounded-md border border-border bg-card px-3 py-2"
            placeholder="/path/to/project"
            value={pathInput}
            onChange={(e) => setPathInput(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && openFolder()}
            data-testid="workspace-path-input"
          />
          <button
            className="rounded-md bg-primary px-4 py-2 text-primary-foreground disabled:cursor-not-allowed disabled:opacity-50"
            onClick={openFolder}
            disabled={!pathInput.trim()}
            data-testid="open-workspace"
          >
            Open
          </button>
        </div>
        {error && <p className="text-sm text-destructive">{error}</p>}
      </div>
    );
  }

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
              // No Timer here (unlike Chat.tsx's assistant branch): send_agent_message
              // runs synchronously to completion, and AgentMessage carries no
              // createdAt/durationMs to feed one — not a copy-paste omission.
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
            <div className="mb-6 rounded-lg bg-destructive/10 p-3 text-sm text-destructive" data-testid="workspace-error">
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
        <button
          className="rounded-md bg-primary px-4 py-2 text-primary-foreground disabled:cursor-not-allowed disabled:opacity-50"
          onClick={send}
          disabled={!input.trim() || thinking}
          data-testid="agent-send"
        >
          Send
        </button>
      </div>
    </div>
  );
}
