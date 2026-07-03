import { useEffect, useMemo, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import Onboarding from "@/views/onboarding/Onboarding";
import Chat from "@/views/chat/Chat";
import ConversationList, { type ConversationListHandle } from "@/views/chat/ConversationList";
import Workspace from "@/views/workspace/Workspace";
import Settings from "@/views/settings/Settings";
import ShortcutsDialog from "@/views/shortcuts/ShortcutsDialog";
import { commands } from "@/lib/ipc";
import { buildShortcuts } from "@/lib/shortcuts";
import { wireConversationStreamEvents } from "@/state/conversationStreamStore";

export default function App() {
  const [ready, setReady] = useState<boolean | null>(null);
  const [activeConversationId, setActiveConversationId] = useState<string | null>(null);
  const [agentMode, setAgentMode] = useState(false);
  const [showSettings, setShowSettings] = useState(false);
  const [showShortcutsDialog, setShowShortcutsDialog] = useState(false);
  const conversationListRef = useRef<ConversationListHandle>(null);

  useEffect(() => {
    wireConversationStreamEvents();
    commands
      .listModels()
      .then((models) => setReady(models.some((m) => m.installed)))
      .catch(() => setReady(false));
  }, []);

  // 005-keyboard-shortcuts: the app's first global (not input-scoped)
  // keyboard shortcuts. One shared registry (lib/shortcuts.ts) drives both
  // the listener below and the shortcuts dialog rendered further down
  // (FR-010) — the exact same array, not two descriptions of it.
  const shortcuts = useMemo(
    () =>
      buildShortcuts({
        focusInput: () => {
          const selector = agentMode ? '[data-testid="agent-input"]' : '[data-testid="chat-input"]';
          document.querySelector<HTMLElement>(selector)?.focus();
        },
        newConversation: () => {
          conversationListRef.current?.createNew();
        },
        toggleShortcutsDialog: () => {
          setShowShortcutsDialog((prev) => !prev);
        },
      }),
    [agentMode],
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
    commands.setFocusedConversation(activeConversationId);
  }, [activeConversationId]);

  if (ready === null) return null;
  if (!ready) return <Onboarding onReady={() => setReady(true)} />;

  return (
    <div className="flex h-dvh">
      <ConversationList
        ref={conversationListRef}
        activeId={activeConversationId}
        onSelect={(id) => {
          setAgentMode(false);
          setShowSettings(false);
          setActiveConversationId(id);
        }}
        onCreated={(id) => {
          setAgentMode(false);
          setShowSettings(false);
          setActiveConversationId(id);
        }}
        onOpenSettings={() => setShowSettings(true)}
      />
      <div className="flex-1">
        {showSettings ? (
          <Settings onClose={() => setShowSettings(false)} />
        ) : agentMode ? (
          <Workspace />
        ) : activeConversationId ? (
          <Chat key={activeConversationId} conversationId={activeConversationId} />
        ) : (
          <div className="flex h-dvh flex-col items-center justify-center gap-3 text-muted-foreground">
            <p>Start a new conversation, or</p>
            <Button
              variant="secondary"
              size="sm"
              onClick={() => setAgentMode(true)}
              data-testid="enter-agent-mode"
            >
              Open a folder (agent mode)
            </Button>
          </div>
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
