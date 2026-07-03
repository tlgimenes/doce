import { useEffect, useState } from "react";
import { homeDir } from "@tauri-apps/api/path";
import { Button } from "@/components/ui/button";
import { commands, type Conversation } from "@/lib/ipc";
import FolderPicker from "@/views/shared/FolderPicker";

export interface FolderTarget {
  kind: "home" | "recent" | "browsed";
  path: string;
  displayLabel: string;
}

interface EmptyStateProps {
  // Reports the full Conversation (not just its id) so App.tsx can route by
  // its workspaceId without a second lookup — every conversation created
  // here always has one set (FR-004), matching what onSelect already
  // reports for conversations picked from the sidebar.
  onConversationCreated: (conversation: Conversation) => void;
}

/**
 * 006-chat-empty-state: replaces the old static placeholder. Every
 * conversation created here is always workspace-scoped and tool-enabled
 * (FR-004) — "Home" is itself a folder selection, not an opt-out
 * (confirmed via interview, see spec.md's Assumptions). Submitting runs
 * the existing-command sequence from contracts/conversation-creation.md:
 * open_workspace -> create_conversation -> send_agent_message, as one
 * action (FR-003).
 */
export default function EmptyState({ onConversationCreated }: EmptyStateProps) {
  const [target, setTarget] = useState<FolderTarget | null>(null);
  const [pickerOpen, setPickerOpen] = useState(false);
  const [input, setInput] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    homeDir().then((path) => setTarget({ kind: "home", path, displayLabel: "Home" }));
  }, []);

  const submit = async () => {
    if (!input.trim() || submitting || !target) return;
    const content = input;
    setSubmitting(true);
    setError(null);
    try {
      const workspace = await commands.openWorkspace(target.path);
      const conversation = await commands.createConversation(workspace.id);
      await commands.sendAgentMessage(conversation.id, content);
      setInput("");
      onConversationCreated(conversation);
    } catch (e) {
      setError(String(e));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div
      className="flex h-dvh flex-col items-center justify-center gap-4 bg-background text-foreground"
      data-testid="empty-state"
    >
      <div className="relative w-full max-w-xl space-y-3">
        <button
          type="button"
          className="rounded-md border border-border bg-card px-3 py-1.5 text-sm text-muted-foreground hover:bg-muted"
          onClick={() => setPickerOpen(true)}
          data-testid="folder-target-selector"
        >
          {target?.displayLabel ?? "Home"}
        </button>
        {pickerOpen && (
          <FolderPicker
            currentPath={target?.path ?? ""}
            onSelect={(picked) => {
              setTarget(picked);
              setPickerOpen(false);
            }}
            onDismiss={() => setPickerOpen(false)}
          />
        )}
        <div className="flex gap-2">
          <input
            className="flex-1 rounded-md border border-border bg-card px-3 py-2"
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && submit()}
            placeholder="What do you want to work on?"
            data-testid="empty-state-input"
          />
          <Button
            variant="primary"
            onClick={submit}
            disabled={!input.trim() || submitting}
            data-testid="empty-state-submit"
          >
            Send
          </Button>
        </div>
        {error && (
          <p className="text-sm text-destructive" data-testid="empty-state-error">
            {error}
          </p>
        )}
      </div>
    </div>
  );
}
