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
import { withTimeout } from "@/lib/withTimeout";

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
    let cancelled = false;
    checkReadyWithRetries().then((isReady) => {
      if (!cancelled) setReady(isReady);
    });
    return () => {
      cancelled = true;
    };
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

  if (ready === null) return null;
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
