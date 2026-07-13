import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Archive, Cog, KeyboardIcon, Plus, Search } from "lucide-react";
import { TopbarHost, TopbarProvider } from "@/components/Topbar";
import { cn } from "@/lib/cn";
import { Button } from "@/components/ui/button";
import { Sidebar, SidebarInset, SidebarProvider } from "@/components/ui/sidebar";
import Onboarding from "@/views/onboarding/Onboarding";
import ConversationList, { type ConversationListHandle } from "@/views/chat/ConversationList";
import ConversationSearchDialog from "@/views/chat/ConversationSearchDialog";
import EmptyState from "@/views/chat/EmptyState";
import Workspace from "@/views/workspace/Workspace";
import Settings from "@/views/settings/Settings";
import ShortcutsDialog from "@/views/shortcuts/ShortcutsDialog";
import CommandCenter, { type CommandCenterAction } from "@/views/command/CommandCenter";
import WidgetGallery from "@/views/design-system/WidgetGallery";
import { commands, type Conversation } from "@/lib/ipc";
import { buildShortcuts, formatCombo, isMacPlatform } from "@/lib/shortcuts";
import { wireContextUsageEvents } from "@/state/contextUsageStore";
import { withTimeout } from "@/lib/withTimeout";
import { runViewTransition } from "@/lib/viewTransition";
import type { PendingInitialTurn } from "@/views/workspace/pendingInitialTurn";

// A real Tauri invoke() call has no built-in timeout: if the IPC bridge
// isn't ready yet, drops the message, or the backend is stuck, the
// promise just never settles. Without a bound, `ready` stays `null`
// forever and the whole app renders nothing, with no way to recover or
// even diagnose it from the outside — found via a still-unresolved CI
// investigation (see specs/001-doce-v1-core/tasks.md's T095 note) where
// this exact call appeared to hang indefinitely in one environment while
// always resolving quickly in every other one tested. `attempts` retries
// give a transient bridge-not-ready race a real chance to clear before
// falling back.
const READY_CHECK_TIMEOUT_MS = 8000;
const READY_CHECK_ATTEMPTS = 3;

function hasSameConversationData(a: Conversation, b: Conversation) {
  return (
    a.id === b.id &&
    a.workspaceId === b.workspaceId &&
    a.title === b.title &&
    a.createdAt === b.createdAt &&
    a.updatedAt === b.updatedAt &&
    a.lastSeenAt === b.lastSeenAt &&
    a.status === b.status
  );
}

export async function checkReadyWithRetries(): Promise<boolean> {
  let lastError: unknown;
  for (let attempt = 0; attempt < READY_CHECK_ATTEMPTS; attempt++) {
    try {
      const models = await withTimeout(
        commands.listModels(),
        READY_CHECK_TIMEOUT_MS,
        "listModels() did not respond in time",
      );
      return models.some((m) => m.installed);
    } catch (err) {
      lastError = err;
    }
  }
  console.error("checkReadyWithRetries: giving up after repeated failures", lastError);
  // Falls back to the onboarding view rather than hanging on a blank
  // screen forever — Onboarding.tsx's own hardware/install-status checks
  // are independent calls that get their own fresh chance to succeed.
  return false;
}

export default function App() {
  const [ready, setReady] = useState<boolean | null>(null);
  const [activeConversation, setActiveConversation] = useState<Conversation | null>(null);
  const [pendingInitialTurn, setPendingInitialTurn] = useState<PendingInitialTurn | null>(null);
  const [showSearch, setShowSearch] = useState(false);
  const [showCommandCenter, setShowCommandCenter] = useState(false);
  const [showSettings, setShowSettings] = useState(false);
  const [showShortcutsDialog, setShowShortcutsDialog] = useState(false);
  const [showWidgetGallery, setShowWidgetGallery] = useState(false);
  const [emptyStateAutoFocusToken, setEmptyStateAutoFocusToken] = useState<number | null>(null);
  const conversationListRef = useRef<ConversationListHandle>(null);

  useEffect(() => {
    wireContextUsageEvents();
    let cancelled = false;
    checkReadyWithRetries().then((isReady) => {
      if (!cancelled) setReady(isReady);
    });
    return () => {
      cancelled = true;
    };
  }, []);

  const clearCommandCenterLatch = useCallback(() => {
    setShowCommandCenter(false);
  }, []);

  const openSearch = useCallback(() => {
    clearCommandCenterLatch();
    setShowSearch(true);
  }, [clearCommandCenterLatch]);

  const openShortcutsDialog = useCallback(() => {
    clearCommandCenterLatch();
    setShowShortcutsDialog(true);
  }, [clearCommandCenterLatch]);

  const openWidgetGallery = useCallback(() => {
    clearCommandCenterLatch();
    setShowSettings(false);
    setShowWidgetGallery(true);
  }, [clearCommandCenterLatch]);

  const openSettings = useCallback(() => {
    clearCommandCenterLatch();
    setShowWidgetGallery(false);
    setShowSettings(true);
  }, [clearCommandCenterLatch]);

  const closeSettings = useCallback(() => {
    clearCommandCenterLatch();
    setShowSettings(false);
  }, [clearCommandCenterLatch]);

  const closeSearch = useCallback(() => {
    clearCommandCenterLatch();
    setShowSearch(false);
  }, [clearCommandCenterLatch]);

  const handleSearchOpenChange = useCallback(
    (open: boolean) => {
      if (open) {
        setShowSearch(true);
        return;
      }
      closeSearch();
    },
    [closeSearch],
  );

  const closeShortcutsDialog = useCallback(() => {
    setShowShortcutsDialog(false);
  }, []);

  const closeWidgetGallery = useCallback(() => {
    clearCommandCenterLatch();
    setShowWidgetGallery(false);
  }, [clearCommandCenterLatch]);

  const startNewConversation = useCallback(() => {
    clearCommandCenterLatch();
    setShowSettings(false);
    setShowWidgetGallery(false);
    setPendingInitialTurn(null);
    setActiveConversation(null);
    setEmptyStateAutoFocusToken((current) => (current ?? 0) + 1);
  }, [clearCommandCenterLatch]);

  const openCommandCenter = useCallback(() => {
    setShowSearch(false);
    setShowShortcutsDialog(false);
    setShowCommandCenter(true);
  }, []);

  // 005-keyboard-shortcuts: the app's first global (not input-scoped)
  // keyboard shortcuts. One shared registry (lib/shortcuts.ts) drives both
  // the listener below and the shortcuts dialog rendered further down
  // (FR-010) — the exact same array, not two descriptions of it.
  const shortcuts = useMemo(
    () =>
      buildShortcuts({
        focusInput: () => {
          // 006-chat-empty-state: no conversation selected now always means
          // the composer is showing (there's no bare, input-less placeholder
          // anymore) — Cmd+L focuses that too, consistent with its whole
          // point ("jump straight into typing" from anywhere).
          const selector = activeConversation
            ? '[data-testid="agent-input"]'
            : '[data-testid="empty-state-input"]';
          document.querySelector<HTMLElement>(selector)?.focus();
        },
        newConversation: startNewConversation,
        openSearch: () => {
          openSearch();
        },
        openCommandCenter: () => {
          openCommandCenter();
        },
        toggleWidgetGallery: () => {
          if (showWidgetGallery) {
            closeWidgetGallery();
            return;
          }
          openWidgetGallery();
        },
        archiveCurrent: () => {
          if (activeConversation) {
            conversationListRef.current?.archiveById(activeConversation.id);
          }
        },
      }),
    [
      activeConversation,
      closeWidgetGallery,
      openCommandCenter,
      openSearch,
      openWidgetGallery,
      showWidgetGallery,
      startNewConversation,
    ],
  );

  const commandActions = useMemo<CommandCenterAction[]>(
    () => [
      {
        id: "new-agent",
        label: "New Agent",
        icon: <Plus />,
        shortcut: formatCombo("Cmd+N"),
        run: startNewConversation,
      },
      {
        id: "search",
        label: "Search Conversations",
        icon: <Search />,
        shortcut: formatCombo("Cmd+F"),
        run: openSearch,
      },
      { id: "settings", label: "Open Settings", icon: <Cog />, run: openSettings },
      {
        id: "shortcuts",
        label: "Open Shortcuts",
        icon: <KeyboardIcon />,
        run: () => setShowShortcutsDialog(true),
      },
      // Cmd+D (widget gallery) stays bound but is deliberately NOT listed
      // here — it's a hidden dev feature, not a user-facing command.
      {
        id: "focus-composer",
        label: "Focus Composer",
        shortcut: formatCombo("Cmd+L"),
        disabled: ready !== true || showSettings || showWidgetGallery,
        run: () => {
          const selector = activeConversation
            ? '[data-testid="agent-input"]'
            : '[data-testid="empty-state-input"]';
          document.querySelector<HTMLElement>(selector)?.focus();
        },
      },
      {
        id: "archive-current",
        label: "Archive Current Conversation",
        icon: <Archive />,
        shortcut: formatCombo("Cmd+E"),
        disabled: !activeConversation,
        run: () => {
          if (activeConversation) {
            conversationListRef.current?.archiveById(activeConversation.id);
          }
        },
      },
      {
        id: "close-surface",
        label: "Close Current Surface",
        run: () => {
          setShowSettings(false);
          setShowSearch(false);
          setShowShortcutsDialog(false);
          setShowWidgetGallery(false);
        },
      },
    ],
    [
      activeConversation,
      openSearch,
      openSettings,
      openWidgetGallery,
      ready,
      showSettings,
      showWidgetGallery,
      startNewConversation,
    ],
  );

  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      if (e.key === "Escape" && showCommandCenter) {
        e.preventDefault();
        setShowCommandCenter(false);
        return;
      }
      // ⌘ on macOS, Ctrl elsewhere — matches what the labels advertise.
      const modifierPressed = isMacPlatform() ? e.metaKey : e.ctrlKey;
      const match = shortcuts.find(
        (s) => s.metaKey === modifierPressed && e.key.toLowerCase() === s.key,
      );
      if (!match) return;
      // FR-009 / Task 2: once an app-owned surface is open, only Cmd+K may
      // continue through the global handler until that surface yields.
      const modalShortcutBlocked =
        match.id !== "open-command-center" &&
        (showSearch || showShortcutsDialog || showCommandCenter);
      if (modalShortcutBlocked) {
        e.preventDefault();
        return;
      }
      e.preventDefault();
      match.action();
    }

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [shortcuts, showCommandCenter, showSearch, showShortcutsDialog]);

  const markSeen = useCallback((conversationId: string) => {
    commands.markConversationSeen(conversationId).catch(console.error);
    setActiveConversation((current) =>
      current?.id === conversationId
        ? { ...current, lastSeenAt: Math.max(Date.now(), current.updatedAt) }
        : current,
    );
  }, []);

  const syncActiveConversation = useCallback((conversation: Conversation) => {
    setActiveConversation((current) => {
      if (current?.id !== conversation.id) return current;

      const next = {
        ...conversation,
        lastSeenAt: Math.max(current.lastSeenAt, conversation.lastSeenAt),
      };
      return hasSameConversationData(current, next) ? current : next;
    });
  }, []);

  const activateConversation = (conversation: Conversation, initialTurn?: PendingInitialTurn) => {
    clearCommandCenterLatch();
    runViewTransition(() => {
      setShowSettings(false);
      setShowWidgetGallery(false);
      setPendingInitialTurn(initialTurn ?? null);
      setActiveConversation(conversation);
    });
  };

  const selectConversationFromSearch = useCallback(
    async (conversationId: string) => {
      if (conversationListRef.current?.selectById(conversationId)) return;

      try {
        const conversations = await commands.listConversations();
        const conversation = conversations.find((item) => item.id === conversationId);
        if (!conversation) return;
        activateConversation(conversation);
        markSeen(conversation.id);
      } catch (error) {
        console.error(error);
      }
    },
    [activateConversation, markSeen],
  );

  if (ready === null) return null;
  if (!ready) return <Onboarding onReady={() => setReady(true)} />;

  return (
    <TopbarProvider>
      <SidebarProvider className="h-dvh">
        <Sidebar collapsible="none" className="w-64 shrink-0 border-r border-sidebar-border">
          <TopbarHost target="sidebar" className="px-2">
            <div className="pointer-events-none flex w-full items-center justify-end">
              <div className="pointer-events-auto" data-topbar-no-drag>
                <Button
                  variant="ghost"
                  size="icon-xs"
                  className="rounded-md text-sidebar-foreground/70 hover:bg-sidebar-accent hover:text-sidebar-accent-foreground"
                  onClick={openShortcutsDialog}
                  data-testid="open-shortcuts-dialog"
                  aria-label="Keyboard shortcuts"
                >
                  <KeyboardIcon size={14} />
                </Button>
              </div>
            </div>
          </TopbarHost>
          <ConversationList
            ref={conversationListRef}
            activeId={activeConversation?.id ?? null}
            onSelect={(conversation) => {
              activateConversation(conversation);
              markSeen(conversation.id);
            }}
            onNewConversation={startNewConversation}
            onOpenSearch={() => openSearch()}
            onOpenSettings={openSettings}
            onActiveConversationChange={syncActiveConversation}
            onArchive={(conversationId) => {
              if (activeConversation?.id !== conversationId) return;
              clearCommandCenterLatch();
              setShowSettings(false);
              setPendingInitialTurn(null);
              setActiveConversation(null);
            }}
          />
        </Sidebar>
        <SidebarInset className="min-w-0">
          {/* The bottom shadow marks the boundary to a scrollable transcript,
              so it only belongs above an open conversation — not over the
              empty state, settings, or the widget gallery. `relative` lifts
              the host's paint order so the shadow isn't covered by the
              content pane that follows it in the DOM. */}
          <TopbarHost
            target="main"
            className={cn(
              "px-4",
              !showWidgetGallery && !showSettings && activeConversation && "relative shadow-sm",
            )}
          />
          <div
            className="min-h-0 flex-1 [view-transition-name:chat-surface]"
            data-testid="app-content-pane"
          >
            {showWidgetGallery ? (
              <WidgetGallery onClose={closeWidgetGallery} />
            ) : showSettings ? (
              <Settings onClose={closeSettings} />
            ) : activeConversation ? (
              <Workspace
                key={activeConversation.id}
                conversation={activeConversation}
                pendingInitialTurn={
                  pendingInitialTurn?.conversationId === activeConversation.id
                    ? pendingInitialTurn
                    : null
                }
                onPendingInitialTurnConsumed={(conversationId) =>
                  setPendingInitialTurn((prev) =>
                    prev?.conversationId === conversationId ? null : prev,
                  )
                }
                onConversationSeen={markSeen}
              />
            ) : (
              <EmptyState
                autoFocusToken={emptyStateAutoFocusToken ?? undefined}
                onConversationCreated={activateConversation}
              />
            )}
          </div>
        </SidebarInset>
        <ShortcutsDialog
          open={showShortcutsDialog}
          onClose={closeShortcutsDialog}
          shortcuts={shortcuts}
        />
        <CommandCenter
          open={showCommandCenter}
          onOpenChange={setShowCommandCenter}
          actions={commandActions}
        />
        <ConversationSearchDialog
          open={showSearch}
          onOpenChange={handleSearchOpenChange}
          recentConversations={conversationListRef.current?.getConversations?.() ?? []}
          onSelectConversationId={selectConversationFromSearch}
        />
      </SidebarProvider>
    </TopbarProvider>
  );
}
