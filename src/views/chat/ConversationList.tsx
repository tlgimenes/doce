import { forwardRef, useEffect, useImperativeHandle, useState } from "react";
import { MagnifyingGlassIcon, GearIcon } from "@phosphor-icons/react";
import { Button } from "@/components/ui/button";
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

    return (
      <div
        className="relative flex h-dvh w-64 shrink-0 flex-col border-r border-sidebar-border bg-sidebar text-sidebar-foreground"
        data-testid="conversation-list"
      >
        <div className="window-drag-region flex h-10 items-center justify-center border-b border-sidebar-border/60 px-3 py-1.5">
          <span className="inline-block h-1.5 w-10 rounded-full bg-muted" />
        </div>
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
        <div className="window-no-drag mb-3 flex gap-2 px-3 pt-3">
          <Button
            variant="ghost"
            size="sm"
            className="flex-1 justify-start bg-background/80 py-2 font-medium shadow-sm hover:bg-background"
            onClick={createNew}
            data-testid="new-conversation"
          >
            + New conversation
          </Button>
          <Button
            variant="ghost"
            size="sm"
            className="bg-background/80 py-2 shadow-sm hover:bg-background"
            onClick={() => setSearching(true)}
            data-testid="open-search"
            aria-label="Search conversations"
          >
            <MagnifyingGlassIcon size={16} />
          </Button>
          <Button
            variant="ghost"
            size="sm"
            className="bg-background/80 py-2 shadow-sm hover:bg-background"
            onClick={onOpenSettings}
            data-testid="open-settings"
            aria-label="Settings"
          >
            <GearIcon size={16} />
          </Button>
        </div>
        <div className="window-no-drag flex-1 space-y-1 overflow-y-auto px-3 pb-3">
          {conversations.map((c) => (
            <Button
              key={c.id}
              variant="ghost"
              size="sm"
              onClick={() => onSelect(c)}
              data-testid="conversation-item"
              data-conversation-id={c.id}
              className={`window-no-drag w-full justify-start gap-2 py-2 text-left ${
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
