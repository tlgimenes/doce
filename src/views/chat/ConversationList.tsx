import { forwardRef, useEffect, useImperativeHandle, useState } from "react";
import { MagnifyingGlassIcon, GearIcon } from "@phosphor-icons/react";
import { Button } from "@/components/ui/button";
import { commands, type Conversation, type ConversationStatus } from "@/lib/ipc";
import SearchPanel from "./SearchPanel";

interface ConversationListProps {
  activeId: string | null;
  onSelect: (id: string) => void;
  onCreated: (id: string) => void;
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
  function ConversationList({ activeId, onSelect, onCreated, onOpenSettings }, ref) {
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

    const createNew = async () => {
      const conv = await commands.createConversation();
      setConversations((prev) => [conv, ...prev]);
      onCreated(conv.id);
    };

    useImperativeHandle(ref, () => ({ createNew }));

    return (
      <div
        className="relative flex h-dvh w-64 shrink-0 flex-col border-r border-sidebar-border bg-sidebar p-3 text-sidebar-foreground"
        data-testid="conversation-list"
      >
        {searching && (
          <SearchPanel
            onClose={() => setSearching(false)}
            onSelect={(id) => {
              onSelect(id);
              setSearching(false);
            }}
          />
        )}
        <div className="mb-3 flex gap-2">
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
        <div className="flex-1 space-y-1 overflow-y-auto">
          {conversations.map((c) => (
            <Button
              key={c.id}
              variant="ghost"
              size="sm"
              onClick={() => onSelect(c.id)}
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
