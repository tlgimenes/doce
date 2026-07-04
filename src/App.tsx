import { useEffect, useMemo, useRef, useState } from "react";
import Onboarding from "@/views/onboarding/Onboarding";
import Chat from "@/views/chat/Chat";
import ConversationList, { type ConversationListHandle } from "@/views/chat/ConversationList";
import EmptyState from "@/views/chat/EmptyState";
import Workspace from "@/views/workspace/Workspace";
import Settings from "@/views/settings/Settings";
import ShortcutsDialog from "@/views/shortcuts/ShortcutsDialog";
import { commands, type Conversation } from "@/lib/ipc";
import { buildShortcuts } from "@/lib/shortcuts";
import { wireConversationStreamEvents } from "@/state/conversationStreamStore";

export default function App() {
  const [ready, setReady] = useState<boolean | null>(null);
  // Temporary diagnostic (not permanent app logic): investigating why the
  // app's very first invoke() call — listModels(), below — appears to
  // never settle in GitHub Actions CI specifically (confirmed the webview
  // navigates and runs JS, but with ready stuck at null the app renders
  // literally nothing, which was otherwise invisible to any e2e query).
  // Surfaces the raw promise outcome as a real, queryable DOM element
  // instead of relying on console capture, which has proven unreliable in
  // this same CI environment.
  const [ipcDiag, setIpcDiag] = useState("pending");
  // 006-chat-empty-state: the active conversation's own `workspaceId` (not a
  // separate `agentMode` flag) decides which view renders — a flag
  // disconnected from the actually-selected conversation was already a
  // latent bug source (research.md § 4), and every new conversation is now
  // always workspace-scoped, which would have made that disconnect far more
  // visible.
  const [activeConversation, setActiveConversation] = useState<Conversation | null>(null);
  const [showSettings, setShowSettings] = useState(false);
  const [showShortcutsDialog, setShowShortcutsDialog] = useState(false);
  const conversationListRef = useRef<ConversationListHandle>(null);

  useEffect(() => {
    wireConversationStreamEvents();
    setIpcDiag(`calling at ${Date.now()}`);
    commands
      .listModels()
      .then((models) => {
        setIpcDiag(`resolved: ${JSON.stringify(models)}`);
        setReady(models.some((m) => m.installed));
      })
      .catch((err) => {
        setIpcDiag(`rejected: ${String(err)}`);
        setReady(false);
      });
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
          const selector = !activeConversation
            ? '[data-testid="empty-state-input"]'
            : activeConversation.workspaceId != null
              ? '[data-testid="agent-input"]'
              : '[data-testid="chat-input"]';
          document.querySelector<HTMLElement>(selector)?.focus();
        },
        newConversation: () => {
          conversationListRef.current?.createNew();
        },
        openSearch: () => {
          conversationListRef.current?.openSearch();
        },
        toggleShortcutsDialog: () => {
          setShowShortcutsDialog((prev) => !prev);
        },
      }),
    [activeConversation],
  );

  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      const match = shortcuts.find((s) => s.metaKey === e.metaKey && e.key.toLowerCase() === s.key);
      if (!match) return;
      // FR-009: while the shortcuts dialog is open, only the shortcut that
      // toggles it may act — Cmd+L/Cmd+N must not reach the conversation.
      if (showShortcutsDialog && match.id !== "show-shortcuts") return;
      e.preventDefault();
      match.action();
    }

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [shortcuts, showShortcutsDialog]);

  // US5/FR-026: the scheduler's priority is evaluated dynamically at pickup
  // time against whichever conversation is currently focused — every view
  // change needs to tell it, not just the initial selection.
  useEffect(() => {
    commands.setFocusedConversation(activeConversation?.id ?? null);
  }, [activeConversation]);

  if (ready === null) return <div data-testid="ipc-diagnostic">{ipcDiag}</div>;
  if (!ready) return <Onboarding onReady={() => setReady(true)} />;

  return (
    <div className="flex h-dvh">
      <ConversationList
        ref={conversationListRef}
        activeId={activeConversation?.id ?? null}
        onSelect={(conversation) => {
          setShowSettings(false);
          setActiveConversation(conversation);
        }}
        onNewConversation={() => {
          setShowSettings(false);
          setActiveConversation(null);
        }}
        onOpenSettings={() => setShowSettings(true)}
      />
      <div className="flex-1">
        {showSettings ? (
          <Settings onClose={() => setShowSettings(false)} />
        ) : activeConversation ? (
          activeConversation.workspaceId != null ? (
            <Workspace key={activeConversation.id} conversationId={activeConversation.id} />
          ) : (
            <Chat key={activeConversation.id} conversationId={activeConversation.id} />
          )
        ) : (
          <EmptyState
            onConversationCreated={(conversation) => setActiveConversation(conversation)}
          />
        )}
      </div>
      <ShortcutsDialog
        open={showShortcutsDialog}
        onClose={() => setShowShortcutsDialog(false)}
        shortcuts={shortcuts}
      />
    </div>
  );
}
