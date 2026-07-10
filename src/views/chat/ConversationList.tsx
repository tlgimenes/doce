import {
  forwardRef,
  type MouseEvent,
  useCallback,
  useEffect,
  useImperativeHandle,
  useMemo,
  useState,
} from "react";
import { Archive, Cog, Plus, Search } from "lucide-react";
import { homeDir } from "@tauri-apps/api/path";
import { Button } from "@/components/ui/button";
import { KeyboardShortcut } from "@/components/ui/KeyboardShortcut";
import {
  SidebarContent,
  SidebarGroup,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
} from "@/components/ui/sidebar";
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
  getConversations: () => Conversation[];
  selectById: (conversationId: string) => boolean;
  archiveById: (conversationId: string) => void;
}

const STATUS_COLOR: Record<ConversationStatus, string> = {
  done: "bg-muted-foreground/45",
  in_progress: "bg-[var(--color-doce-caramel)] animate-pulse",
  requires_action: "bg-[var(--color-doce-coral)]",
  failed: "bg-destructive",
};

const STATUS_LABEL: Record<ConversationStatus, string> = {
  done: "Done",
  in_progress: "In progress",
  requires_action: "Needs your input",
  failed: "Failed",
};

const SIDEBAR_ACTION_BUTTON =
  "h-8 w-full justify-start gap-2 rounded-md px-2 text-sm text-sidebar-foreground hover:bg-sidebar-accent hover:text-sidebar-accent-foreground";

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

    const selectById = (conversationId: string) => {
      const conversation = conversations.find((item) => item.id === conversationId);
      if (!conversation) return false;
      selectConversation(conversation);
      return true;
    };

    const archiveConversation = useCallback(
      (conversation: Conversation) => {
        setConversations((current) => current.filter((item) => item.id !== conversation.id));
        onArchive?.(conversation.id);
        commands.archiveConversation(conversation.id).catch((error) => {
          console.error(error);
          refresh();
        });
      },
      [onArchive, refresh],
    );

    const handleArchiveConversation = (
      event: MouseEvent<HTMLButtonElement>,
      conversation: Conversation,
    ) => {
      event.preventDefault();
      event.stopPropagation();
      archiveConversation(conversation);
    };

    const archiveById = (conversationId: string) => {
      const conversation = conversations.find((item) => item.id === conversationId);
      if (!conversation) {
        onArchive?.(conversationId);
        commands.archiveConversation(conversationId).catch((error) => {
          console.error(error);
          refresh();
        });
        return;
      }
      archiveConversation(conversation);
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

    useImperativeHandle(ref, () => ({
      createNew,
      getConversations: () => conversations,
      selectById,
      archiveById,
    }));

    return (
      <div className="relative flex min-h-0 flex-1 flex-col">
        <SidebarContent
          className="px-2 pb-3 text-sidebar-foreground"
          data-testid="conversation-list"
        >
          <SidebarGroup className="mb-3 gap-0.5 p-0" data-testid="sidebar-actions">
            <SidebarMenu className="gap-0.5">
              <SidebarMenuItem>
                <SidebarMenuButton
                  className={`${SIDEBAR_ACTION_BUTTON} group justify-between`}
                  onClick={createNew}
                  data-testid="new-conversation"
                  aria-label="New agent"
                >
                  <span className="flex items-center gap-2">
                    <Plus className="size-4" />
                    New Agent
                  </span>
                  <KeyboardShortcut
                    keys={["⌘", "N"]}
                    className="text-xs text-sidebar-foreground/60 opacity-0 transition-opacity group-hover:opacity-100"
                  />
                </SidebarMenuButton>
              </SidebarMenuItem>
              <SidebarMenuItem>
                <SidebarMenuButton
                  className={`${SIDEBAR_ACTION_BUTTON} group justify-between`}
                  onClick={openSearch}
                  data-testid="open-search"
                  aria-label="Search conversations"
                >
                  <span className="flex items-center gap-2">
                    <Search className="size-4" />
                    Search
                  </span>
                  <KeyboardShortcut
                    keys={["⌘", "F"]}
                    className="text-xs text-sidebar-foreground/60 opacity-0 transition-opacity group-hover:opacity-100 group-focus-visible:opacity-100"
                  />
                </SidebarMenuButton>
              </SidebarMenuItem>
              <SidebarMenuItem>
                <SidebarMenuButton
                  className={SIDEBAR_ACTION_BUTTON}
                  onClick={onOpenSettings}
                  data-testid="open-settings"
                  aria-label="Settings"
                >
                  <Cog className="size-4" />
                  Settings
                </SidebarMenuButton>
              </SidebarMenuItem>
            </SidebarMenu>
          </SidebarGroup>
          <SidebarGroup className="min-h-0 flex-1 p-0">
            <SidebarMenu className="flex-1 gap-1 overflow-y-auto">
              {conversations.map((c) => {
                const isActive = c.id === activeId;
                const hasUnseenUpdates = !isActive && c.updatedAt > c.lastSeenAt;
                const isReadInactive = !isActive && !hasUnseenUpdates;

                return (
                  <SidebarMenuItem
                    key={c.id}
                    className={cn(
                      "rounded-md transition-colors hover:bg-sidebar-accent hover:text-sidebar-accent-foreground focus-within:bg-sidebar-accent focus-within:text-sidebar-accent-foreground",
                      isActive
                        ? "bg-sidebar-accent text-sidebar-accent-foreground"
                        : "bg-transparent",
                    )}
                    data-testid="conversation-item"
                    data-conversation-id={c.id}
                  >
                    <div
                      className="relative grid min-h-12 w-full min-w-0 grid-cols-[minmax(0,1fr)_min-content] items-center gap-2 px-2 py-2"
                      data-testid="conversation-thread-button"
                      onClick={() => selectConversation(c)}
                    >
                      <button
                        type="button"
                        className="absolute inset-0 z-0 rounded-md ring-sidebar-ring outline-hidden focus-visible:ring-2"
                        aria-label={`Open ${c.title}`}
                        onClick={(event) => {
                          event.stopPropagation();
                          selectConversation(c);
                        }}
                      />
                      <span className="relative z-10 grid min-w-0 grid-cols-[auto_minmax(0,1fr)] items-start gap-2 overflow-hidden">
                        <span
                          className={cn(
                            "mt-1.5 size-2 shrink-0 rounded-full",
                            STATUS_COLOR[c.status],
                          )}
                          title={STATUS_LABEL[c.status]}
                          data-testid="conversation-status-dot"
                          data-status={c.status}
                        />
                        <span className="flex min-w-0 flex-col gap-0.5 overflow-hidden">
                          <span
                            className={cn(
                              "block min-w-0 truncate text-[13px] font-medium leading-4",
                              isActive
                                ? "text-sidebar-accent-foreground"
                                : isReadInactive
                                  ? "text-sidebar-foreground/55"
                                  : "text-sidebar-foreground",
                            )}
                          >
                            {c.title}
                          </span>
                          <span
                            className={cn(
                              "block min-w-0 truncate text-[11px] leading-4",
                              isActive
                                ? "text-sidebar-accent-foreground/70"
                                : "text-sidebar-foreground/60",
                            )}
                          >
                            {getConversationWorkspaceLabel(c.workspaceId, workspacesById, homePath)}
                          </span>
                        </span>
                      </span>
                      <span
                        className="relative z-20 grid min-w-0 items-center justify-items-end overflow-hidden"
                        data-testid="conversation-end-slot"
                      >
                        {/* Intrinsically sized on purpose: this sits in a
                            min-content grid column, and percentage widths
                            (w-full) contribute zero intrinsic width there —
                            the whole slot collapses to 0px and hides time,
                            work-state, and the archive action. Labels are
                            short and bounded, so nowrap needs no truncation. */}
                        <span className="flex flex-col items-end justify-center text-right transition-opacity group-hover/menu-item:opacity-0 group-focus-within/menu-item:opacity-0">
                          <span
                            className={cn(
                              "block whitespace-nowrap text-[11px] leading-4 tabular-nums",
                              isActive
                                ? "text-sidebar-accent-foreground/80"
                                : "text-sidebar-foreground/55",
                            )}
                          >
                            {formatConversationRelativeTime(c.updatedAt)}
                          </span>
                          <span
                            className={cn(
                              "block whitespace-nowrap text-[11px] leading-4",
                              isActive
                                ? "text-sidebar-accent-foreground/70"
                                : "text-sidebar-foreground/60",
                            )}
                          >
                            {getConversationWorkStateLabel(c.status)}
                          </span>
                        </span>
                        <span
                          className="pointer-events-none absolute inset-0 flex items-center justify-end opacity-0 transition-opacity group-hover/menu-item:pointer-events-auto group-hover/menu-item:opacity-100 group-focus-within/menu-item:pointer-events-auto group-focus-within/menu-item:opacity-100"
                          data-testid="conversation-archive-action"
                        >
                          <Button
                            type="button"
                            variant="icon"
                            size="icon-sm"
                            aria-label={`Archive ${c.title}`}
                            onClick={(event) => handleArchiveConversation(event, c)}
                          >
                            <Archive className="size-3.5" />
                          </Button>
                        </span>
                      </span>
                    </div>
                  </SidebarMenuItem>
                );
              })}
            </SidebarMenu>
          </SidebarGroup>
        </SidebarContent>
      </div>
    );
  },
);

export default ConversationList;
