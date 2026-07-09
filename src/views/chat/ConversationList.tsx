import {
  forwardRef,
  type KeyboardEvent,
  type MouseEvent,
  useCallback,
  useEffect,
  useImperativeHandle,
  useMemo,
  useState,
} from "react";
import { MagnifyingGlassIcon, GearIcon, PlusIcon, TrashIcon } from "@phosphor-icons/react";
import { homeDir } from "@tauri-apps/api/path";
import { Button } from "@/components/ui/button";
import { KeyboardShortcut } from "@/components/ui/KeyboardShortcut";
import { cn } from "@/lib/cn";
import { commands, type Conversation, type ConversationStatus, type Workspace } from "@/lib/ipc";
import {
  formatConversationRelativeTime,
  getConversationWorkspaceLabel,
  getConversationWorkStateLabel,
} from "./sidebarConversationRow";

interface ConversationListProps {
  activeId: string | null;
  onSelect: (conversation: Conversation) => void;
  onNewConversation: () => void;
  onOpenSearch: () => void;
  onOpenSettings: () => void;
  onActiveConversationChange?: (conversation: Conversation) => void;
  onArchive?: (conversationId: string) => void;
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
  "w-full justify-start gap-1 h-8 rounded-lg border-0 bg-transparent px-1 py-0 text-sm text-sidebar-foreground/95 hover:bg-sidebar-foreground/8 hover:text-sidebar-foreground focus-visible:ring-0 focus-visible:outline-none";

/**
 * User Story 7: at-a-glance conversation status (FR-011) and auto-generated
 * titles (FR-012), refreshed on a short poll — there's no dedicated
 * "conversation changed" event yet, and status can change from generation
 * activity happening entirely on the backend with no user action to hang
 * a refresh off of.
 */
const ConversationList = forwardRef<ConversationListHandle, ConversationListProps>(
  function ConversationList(
    {
      activeId,
      onSelect,
      onNewConversation,
      onOpenSearch,
      onOpenSettings,
      onActiveConversationChange,
      onArchive,
    },
    ref,
  ) {
    const [conversations, setConversations] = useState<Conversation[]>([]);
    const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
    const [homePath, setHomePath] = useState<string | null>(null);

    const refresh = useCallback(() => {
      commands.listConversations().then(setConversations);
      commands.listWorkspaces().then(setWorkspaces).catch(console.error);
    }, []);

    const markConversationSeenLocally = (conversationId: string) => {
      setConversations((current) =>
        current.map((conversation) =>
          conversation.id === conversationId
            ? {
                ...conversation,
                lastSeenAt: Math.max(Date.now(), conversation.updatedAt, conversation.lastSeenAt),
              }
            : conversation,
        ),
      );
    };

    const selectConversation = (conversation: Conversation) => {
      markConversationSeenLocally(conversation.id);
      onSelect(conversation);
    };

    const archiveConversation = (
      event: MouseEvent<HTMLButtonElement>,
      conversation: Conversation,
    ) => {
      event.preventDefault();
      event.stopPropagation();
      setConversations((current) => current.filter((item) => item.id !== conversation.id));
      onArchive?.(conversation.id);
      commands.archiveConversation(conversation.id).catch((error) => {
        console.error(error);
        refresh();
      });
    };

    const handleConversationKeyDown = (
      event: KeyboardEvent<HTMLDivElement>,
      conversation: Conversation,
    ) => {
      if (event.target !== event.currentTarget) return;
      if (event.key !== "Enter" && event.key !== " ") return;
      event.preventDefault();
      selectConversation(conversation);
    };

    useEffect(() => {
      refresh();
      const id = setInterval(refresh, 2000);
      return () => clearInterval(id);
    }, [refresh]);

    useEffect(() => {
      if (!activeId) return;
      const activeConversation = conversations.find((conversation) => conversation.id === activeId);
      if (activeConversation) {
        onActiveConversationChange?.(activeConversation);
      }
    }, [activeId, conversations, onActiveConversationChange]);

    useEffect(() => {
      homeDir()
        .then(setHomePath)
        .catch(() => setHomePath(""));
    }, []);

    const workspacesById = useMemo(
      () => new Map(workspaces.map((workspace) => [workspace.id, workspace])),
      [workspaces],
    );

    // 006-chat-empty-state: no longer creates a conversation itself (FR-002)
    // — it tells the parent to show the empty-state composer instead, which
    // only actually creates one once a first message is submitted (FR-003).
    // Cmd+N (005-keyboard-shortcuts) calls this exact same ref method, so it
    // automatically gets the same behavior rather than a second, divergent
    // path.
    const createNew = () => {
      onNewConversation();
    };

    const openSearch = () => {
      onOpenSearch();
    };

    useImperativeHandle(ref, () => ({ createNew }));

    return (
      <div
        className="relative flex min-h-0 flex-1 flex-col px-2 pb-3 text-sidebar-foreground"
        data-testid="conversation-list"
      >
        <div className="mb-3 flex flex-col gap-0.5" data-testid="sidebar-actions">
          <Button
            variant="ghost"
            size="sm"
            className={`${SIDEBAR_ACTION_BUTTON} group justify-between`}
            onClick={createNew}
            data-testid="new-conversation"
            aria-label="New agent"
          >
            <span className="flex items-center gap-1">
              <PlusIcon size={16} weight="bold" />
              New Agent
            </span>
            <KeyboardShortcut
              keys={["⌘", "N"]}
              className="text-xs text-sidebar-foreground/60 opacity-0 transition-opacity group-hover:opacity-100"
            />
          </Button>
          <Button
            variant="ghost"
            size="sm"
            className={`${SIDEBAR_ACTION_BUTTON} group justify-between`}
            onClick={openSearch}
            data-testid="open-search"
            aria-label="Search conversations"
          >
            <span className="flex items-center gap-1">
              <MagnifyingGlassIcon size={16} />
              Search
            </span>
            <KeyboardShortcut
              keys={["⌘", "F"]}
              className="text-xs text-sidebar-foreground/60 opacity-0 transition-opacity group-hover:opacity-100 group-focus-visible:opacity-100"
            />
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
          {conversations.map((c) => {
            const isActive = c.id === activeId;
            const hasUnseenUpdates = !isActive && c.updatedAt > c.lastSeenAt;
            const isReadInactive = !isActive && !hasUnseenUpdates;

            return (
              <div
                key={c.id}
                role="button"
                tabIndex={0}
                onClick={() => selectConversation(c)}
                onKeyDown={(event) => handleConversationKeyDown(event, c)}
                data-testid="conversation-item"
                data-conversation-id={c.id}
                className={cn(
                  "group flex h-auto min-h-12 w-full cursor-pointer items-start justify-start gap-2 rounded-lg border-0 px-2 py-2 text-left text-foreground shadow-none transition-colors hover:bg-sidebar-foreground/8 focus-visible:outline-none",
                  isActive ? "bg-sidebar-foreground/8" : "bg-transparent",
                )}
              >
                <span
                  className={cn("mt-1.5 size-2 shrink-0 rounded-full", STATUS_COLOR[c.status])}
                  title={STATUS_LABEL[c.status]}
                  data-testid="conversation-status-dot"
                  data-status={c.status}
                />
                <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                  <span className="flex min-w-0 items-baseline gap-2">
                    <span
                      className={cn(
                        "min-w-0 flex-1 truncate text-[13px] font-medium leading-4",
                        isReadInactive ? "text-sidebar-foreground/55" : "text-sidebar-foreground",
                      )}
                    >
                      {c.title}
                    </span>
                  </span>
                  <span className="min-w-0 truncate text-[11px] leading-4 text-sidebar-foreground/60">
                    {getConversationWorkspaceLabel(c.workspaceId, workspacesById, homePath)}
                  </span>
                </span>
                <span className="relative h-8 w-10 shrink-0 self-center">
                  <span className="absolute right-0 top-0 text-[11px] leading-4 text-sidebar-foreground/55 tabular-nums transition-opacity group-hover:opacity-0">
                    {formatConversationRelativeTime(c.updatedAt)}
                  </span>
                  <span className="absolute bottom-0 right-0 text-[11px] leading-4 text-sidebar-foreground/60 transition-opacity group-hover:opacity-0">
                    {getConversationWorkStateLabel(c.status)}
                  </span>
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon-sm"
                    className="pointer-events-none absolute right-0 top-1/2 -translate-y-1/2 rounded-full text-sidebar-foreground/70 opacity-0 hover:bg-sidebar-foreground/10 hover:text-sidebar-foreground group-hover:pointer-events-auto group-hover:opacity-100"
                    aria-label={`Archive ${c.title}`}
                    onClick={(event) => archiveConversation(event, c)}
                  >
                    <TrashIcon size={14} />
                  </Button>
                </span>
              </div>
            );
          })}
        </div>
      </div>
    );
  },
);

export default ConversationList;
