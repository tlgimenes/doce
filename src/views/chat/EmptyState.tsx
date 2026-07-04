import { useEffect, useState } from "react";
import { homeDir } from "@tauri-apps/api/path";
import { CaretDownIcon } from "@phosphor-icons/react";
import { commands, type Conversation, type RichMessageContent } from "@/lib/ipc";
import FolderPicker from "@/views/shared/FolderPicker";
import RichInput from "@/views/chat/rich-input/RichInput";

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
  const normalizedHome =
    homePath.endsWith("/") && homePath.length > 1 ? homePath.slice(0, -1) : homePath;
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
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    homeDir().then((path) => {
      setHomePath(path);
      setTarget({ kind: "home", path, displayLabel: "Home" });
    });
  }, []);

  const submit = async (content: string, richContent?: RichMessageContent) => {
    // richContent's own presence counts as "something to send" even when
    // content (the flat-text extraction) is empty — a message that's
    // entirely a chip (e.g. just a pasted-text node, no additional typed
    // text) must not be silently dropped here.
    if ((!content.trim() && !richContent) || submitting || !target) return;
    setSubmitting(true);
    setError(null);
    try {
      const workspace = await commands.openWorkspace(target.path);
      const conversation = await commands.createConversation(workspace.id);
      await commands.sendAgentMessage(
        conversation.id,
        content,
        richContent ? JSON.stringify(richContent) : undefined,
      );
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
        <RichInput
          onSubmit={submit}
          skillsEnabled={true}
          disabled={submitting}
          placeholder="What do you want to work on?"
          inputTestId="empty-state-input"
          submitTestId="empty-state-submit"
        />
        {error && (
          <p className="text-sm text-destructive" data-testid="empty-state-error">
            {error}
          </p>
        )}
      </div>
    </div>
  );
}
