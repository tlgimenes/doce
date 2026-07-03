import { forwardRef, type MouseEvent, useEffect, useImperativeHandle, useState } from "react";
import { MagnifyingGlassIcon, GearIcon, PlusIcon } from "@phosphor-icons/react";
import { Button } from "@/components/ui/button";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { commands, type Conversation, type ConversationStatus } from "@/lib/ipc";
import SearchPanel from "./SearchPanel";

interface ConversationListProps {
  activeId: string | null;
  onSelect: (conversation: Conversation) => void;
  onNewConversation: () => void;
  onOpenSettings: () => void;
}

// 005-keyboard-shortcuts: exposed so Cmd+N (App.tsx) can trigger the exact
// same creation path as clicking "+ New conversation" — one implementation,
// not a duplicate (research.md § 3).
export interface ConversationListHandle {
  createNew: () => void;
}

const STATUS_COLOR: Record<ConversationStatus, string> = {
  done: "bg-emerald-500",
  in_progress: "bg-sky-500 animate-pulse",
  requires_action: "bg-amber-500",
  failed: "bg-red-500",
};

const STATUS_LABEL: Record<ConversationStatus, string> = {
  done: "Done",
  in_progress: "In progress",
  requires_action: "Needs your input",
  failed: "Failed",
};

const SIDEBAR_ACTION_BUTTON =
  "w-full justify-start gap-1 h-10 rounded-lg border-0 bg-transparent px-1 py-0 text-sm text-sidebar-foreground/95 hover:bg-sidebar-foreground/8 hover:text-sidebar-foreground focus-visible:ring-0 focus-visible:outline-none";

/**
 * User Story 7: at-a-glance conversation status (FR-011) and auto-generated
 * titles (FR-012), refreshed on a short poll — there's no dedicated
 * "conversation changed" event yet, and status can change from generation
 * activity happening entirely on the backend with no user action to hang
 * a refresh off of.
 */
const ConversationList = forwardRef<ConversationListHandle, ConversationListProps>(
  function ConversationList({ activeId, onSelect, onNewConversation, onOpenSettings }, ref) {
    const [conversations, setConversations] = useState<Conversation[]>([]);
    const [searching, setSearching] = useState(false);

    const refresh = () => {
      commands.listConversations().then(setConversations);
    };

    useEffect(() => {
      refresh();
      const id = setInterval(refresh, 2000);
      return () => clearInterval(id);
    }, []);

    // 006-chat-empty-state: no longer creates a conversation itself (FR-002)
    // — it tells the parent to show the empty-state composer instead, which
    // only actually creates one once a first message is submitted (FR-003).
    // Cmd+N (005-keyboard-shortcuts) calls this exact same ref method, so it
    // automatically gets the same behavior rather than a second, divergent
    // path.
    const createNew = () => {
      onNewConversation();
    };

    useImperativeHandle(ref, () => ({ createNew }));

    const startDrag = async (event: MouseEvent<HTMLDivElement>) => {
      if (event.button !== 0) return;
      event.preventDefault();
      await getCurrentWindow().startDragging().catch((error) => {
        console.error("Failed to start window dragging", error);
      });
    };

    return (
      <div
        className="relative flex h-dvh w-64 shrink-0 flex-col border-r border-sidebar-border bg-sidebar px-2 pb-3 pt-0 text-sidebar-foreground"
        data-testid="conversation-list"
      >
        <div
          className="absolute left-0 right-0 top-0 h-10 select-none"
          data-tauri-drag-region
          data-testid="sidebar-drag-region"
          onMouseDown={startDrag}
        />
        {searching && (
          <SearchPanel
            onClose={() => setSearching(false)}
            onSelect={(id) => {
              // Search results only carry the id (commands.searchConversations
              // returns a slimmer SearchResult, not a full Conversation) —
              // look it up in the already-loaded list rather than changing
              // onSelect's shape just for this one caller.
              const conversation = conversations.find((c) => c.id === id);
              if (conversation) onSelect(conversation);
              setSearching(false);
            }}
          />
        )}
        <div className="mb-3 mt-8 flex flex-col gap-0.5">
          <Button
            variant="ghost"
            size="sm"
            className={SIDEBAR_ACTION_BUTTON}
            onClick={createNew}
            data-testid="new-conversation"
            aria-label="New agent"
          >
            <PlusIcon size={16} weight="bold" />
            New Agent
          </Button>
          <Button
            variant="ghost"
            size="sm"
            className={SIDEBAR_ACTION_BUTTON}
            onClick={() => setSearching(true)}
            data-testid="open-search"
            aria-label="Search conversations"
          >
            <MagnifyingGlassIcon size={16} />
            Search
          </Button>
          <Button
            variant="ghost"
            size="sm"
            className={SIDEBAR_ACTION_BUTTON}
            onClick={onOpenSettings}
            data-testid="open-settings"
            aria-label="Settings"
          >
            <GearIcon size={16} />
            Settings
          </Button>
        </div>
        <div className="flex-1 space-y-1 overflow-y-auto">
          {conversations.map((c) => (
            <Button
              key={c.id}
              variant="ghost"
              size="sm"
              onClick={() => onSelect(c)}
              data-testid="conversation-item"
              data-conversation-id={c.id}
              className={`w-full justify-start gap-2 py-2 text-left ${
                c.id === activeId ? "bg-background" : "bg-background/40 hover:bg-background/70"
              }`}
            >
              <span
                className={`size-2 shrink-0 rounded-full ${STATUS_COLOR[c.status]}`}
                title={STATUS_LABEL[c.status]}
                data-testid="conversation-status-dot"
                data-status={c.status}
              />
              <span className="truncate">{c.title}</span>
            </Button>
          ))}
        </div>
      </div>
    );
  },
);

export default ConversationList;
