import { useEffect, useState } from "react";
import { MagnifyingGlassIcon, GearIcon } from "@phosphor-icons/react";
import { commands, type Conversation, type ConversationStatus } from "@/lib/ipc";
import SearchPanel from "./SearchPanel";

interface ConversationListProps {
  activeId: string | null;
  onSelect: (id: string) => void;
  onCreated: (id: string) => void;
  onOpenSettings: () => void;
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
export default function ConversationList({ activeId, onSelect, onCreated, onOpenSettings }: ConversationListProps) {
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
        <button
          className="flex-1 rounded-md bg-background/80 px-3 py-2 text-left text-sm font-medium shadow-sm hover:bg-background"
          onClick={createNew}
          data-testid="new-conversation"
        >
          + New conversation
        </button>
        <button
          className="flex items-center rounded-md bg-background/80 px-3 py-2 text-sm shadow-sm hover:bg-background"
          onClick={() => setSearching(true)}
          data-testid="open-search"
          aria-label="Search conversations"
        >
          <MagnifyingGlassIcon size={16} />
        </button>
        <button
          className="flex items-center rounded-md bg-background/80 px-3 py-2 text-sm shadow-sm hover:bg-background"
          onClick={onOpenSettings}
          data-testid="open-settings"
          aria-label="Settings"
        >
          <GearIcon size={16} />
        </button>
      </div>
      <div className="flex-1 space-y-1 overflow-y-auto">
        {conversations.map((c) => (
          <button
            key={c.id}
            onClick={() => onSelect(c.id)}
            data-testid="conversation-item"
            data-conversation-id={c.id}
            className={`flex w-full items-center gap-2 rounded-md px-3 py-2 text-left text-sm ${
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
          </button>
        ))}
      </div>
    </div>
  );
}
