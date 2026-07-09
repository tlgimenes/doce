import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { KeyboardIcon } from "lucide-react";
import Dialog from "@/components/Dialog";
import { TopbarHost, TopbarProvider } from "@/components/Topbar";
import { Button } from "@/components/ui/button";
import Onboarding from "@/views/onboarding/Onboarding";
import ConversationList, { type ConversationListHandle } from "@/views/chat/ConversationList";
import EmptyState from "@/views/chat/EmptyState";
import SearchPanel from "@/views/chat/SearchPanel";
import Workspace from "@/views/workspace/Workspace";
import Settings from "@/views/settings/Settings";
import ShortcutsDialog from "@/views/shortcuts/ShortcutsDialog";
import WidgetGallery from "@/views/design-system/WidgetGallery";
import { commands, type Conversation } from "@/lib/ipc";
import { buildShortcuts } from "@/lib/shortcuts";
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
  const [searchRecentConversations, setSearchRecentConversations] = useState<Conversation[]>([]);
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

  const openSearch = useCallback(() => {
    setShowCommandCenter(false);
    setShowSearch(true);
    commands.listConversations().then(setSearchRecentConversations).catch(console.error);
  }, []);

  const openShortcutsDialog = useCallback(() => {
    setShowCommandCenter(false);
    setShowShortcutsDialog(true);
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
        newConversation: () => {
          conversationListRef.current?.createNew();
        },
        openSearch: () => {
          openSearch();
        },
        openCommandCenter: () => {
          setShowCommandCenter(true);
        },
        toggleWidgetGallery: () => {
          setShowWidgetGallery((prev) => !prev);
        },
      }),
    [activeConversation, openSearch],
  );

  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      if (e.key === "Escape" && showCommandCenter) {
        e.preventDefault();
        setShowCommandCenter(false);
        return;
      }
      const match = shortcuts.find((s) => s.metaKey === e.metaKey && e.key.toLowerCase() === s.key);
      if (!match) return;
      // FR-009 / Task 2: once an app-owned surface is open, only Cmd+K may
      // continue through the global handler until that surface yields.
      if (showShortcutsDialog && match.id !== "open-command-center") return;
      if (showCommandCenter && match.id !== "open-command-center") return;
      e.preventDefault();
      match.action();
    }

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [shortcuts, showCommandCenter, showShortcutsDialog]);

  // US5/FR-026: the scheduler's priority is evaluated dynamically at pickup
  // time against whichever conversation is currently focused — every view
  // change needs to tell it, not just the initial selection.
  useEffect(() => {
    commands.setFocusedConversation(activeConversation?.id ?? null);
  }, [activeConversation]);

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

  const openConversationFromSearch = useCallback(
    async (conversationId: string) => {
      const recentConversation = searchRecentConversations.find((c) => c.id === conversationId);
      const allConversations = recentConversation
        ? searchRecentConversations
        : await commands.listConversations();
      const conversation = allConversations.find((c) => c.id === conversationId);

      if (!conversation) return;

      setShowSearch(false);
      setShowSettings(false);
      setPendingInitialTurn(null);
      setActiveConversation(conversation);
      markSeen(conversation.id);
    },
    [markSeen, searchRecentConversations],
  );

  const activateConversation = (conversation: Conversation, initialTurn?: PendingInitialTurn) => {
    runViewTransition(() => {
      setShowSettings(false);
      setPendingInitialTurn(initialTurn ?? null);
      setActiveConversation(conversation);
    });
  };

  if (ready === null) return null;
  if (!ready) return <Onboarding onReady={() => setReady(true)} />;

  return (
    <TopbarProvider>
      <div className="flex h-dvh">
        <div className="flex w-64 shrink-0 flex-col border-r border-sidebar-border bg-sidebar">
          <TopbarHost target="sidebar" className="px-2">
            <div className="flex w-full items-center justify-end" data-topbar-no-drag>
              <Button
                variant="ghost"
                size="icon-sm"
                className="text-sidebar-foreground/70 hover:bg-sidebar-foreground/8 hover:text-sidebar-foreground"
                onClick={openShortcutsDialog}
                data-testid="open-shortcuts-dialog"
                aria-label="Keyboard shortcuts"
              >
                <KeyboardIcon size={14} />
              </Button>
            </div>
          </TopbarHost>
          <ConversationList
            ref={conversationListRef}
            activeId={activeConversation?.id ?? null}
            onSelect={(conversation) => {
              setShowSettings(false);
              setPendingInitialTurn(null);
              setActiveConversation(conversation);
              markSeen(conversation.id);
            }}
            onNewConversation={() => {
              setShowSettings(false);
              setPendingInitialTurn(null);
              setActiveConversation(null);
              setEmptyStateAutoFocusToken((current) => (current ?? 0) + 1);
            }}
            onOpenSearch={() => openSearch()}
            onOpenSettings={() => setShowSettings(true)}
            onActiveConversationChange={syncActiveConversation}
            onArchive={(conversationId) => {
              if (activeConversation?.id !== conversationId) return;
              setShowSettings(false);
              setPendingInitialTurn(null);
              setActiveConversation(null);
            }}
          />
        </div>
        <div className="flex min-w-0 flex-1 flex-col">
          <TopbarHost
            target="main"
            className={
              !showWidgetGallery && !showSettings && activeConversation
                ? "border-b border-sidebar-border bg-sidebar px-4 shadow-sm"
                : "px-4"
            }
          />
          <div
            className="min-h-0 flex-1 [view-transition-name:chat-surface]"
            data-testid="app-content-pane"
          >
            {showWidgetGallery ? (
              <WidgetGallery onClose={() => setShowWidgetGallery(false)} />
            ) : showSettings ? (
              <Settings onClose={() => setShowSettings(false)} />
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
        </div>
        <ShortcutsDialog
          open={showShortcutsDialog}
          onClose={() => setShowShortcutsDialog(false)}
          shortcuts={shortcuts}
        />
        <Dialog open={showSearch} onClose={() => setShowSearch(false)}>
          <SearchPanel
            recentConversations={searchRecentConversations}
            onSelect={(conversationId) => {
              void openConversationFromSearch(conversationId);
            }}
          />
        </Dialog>
      </div>
    </TopbarProvider>
  );
}
