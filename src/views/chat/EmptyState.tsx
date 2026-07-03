import { useEffect, useRef, useState } from "react";
import { homeDir } from "@tauri-apps/api/path";
import { CaretDownIcon, PaperPlaneRightIcon } from "@phosphor-icons/react";
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

const toDisplayFolderLabel = (path: string, homePath: string | null) => {
  if (!homePath) return path;
  const normalizedHome = homePath.endsWith("/") && homePath.length > 1 ? homePath.slice(0, -1) : homePath;
  if (path === normalizedHome || path === `${normalizedHome}/`) return "Home";
  if (path.startsWith(`${normalizedHome}/`)) {
    return `~${path.slice(normalizedHome.length)}`;
  }
  return path;
};

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
  const [homePath, setHomePath] = useState<string | null>(null);
  const [pickerOpen, setPickerOpen] = useState(false);
  const [input, setInput] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

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
    homeDir().then((path) => {
      setHomePath(path);
      setTarget({ kind: "home", path, displayLabel: "Home" });
    });
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
        {/* 008-shared-design-system exemption: a compact inline text+caret
            trigger, not a standard button shape — the shared Button
            component's variants don't have a natural fit for this, and the
            hand-tuned padding/sizing here is the intentionally-kept look
            (FR-008 exemption, documented per T018 rather than migrated). */}
        <button
          type="button"
          className="inline-flex cursor-pointer items-center gap-1 bg-transparent p-0 pl-2 text-sm text-muted-foreground"
          onClick={() => setPickerOpen(true)}
          data-testid="folder-target-selector"
        >
          {target ? toDisplayFolderLabel(target.path, homePath) : "Home"}
          <CaretDownIcon size={12} />
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
                submit();
              }
            }}
            placeholder="What do you want to work on?"
            data-testid="empty-state-input"
          />
          <Button
            type="button"
            variant="primary"
            className="h-8 w-8 shrink-0 rounded-full p-0"
            onClick={submit}
            disabled={!input.trim() || submitting}
            aria-label="Send message"
            data-testid="empty-state-submit"
          >
            <PaperPlaneRightIcon size={16} />
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
