import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import Onboarding from "@/views/onboarding/Onboarding";
import Chat from "@/views/chat/Chat";
import ConversationList from "@/views/chat/ConversationList";
import Workspace from "@/views/workspace/Workspace";
import Settings from "@/views/settings/Settings";
import { commands } from "@/lib/ipc";
import { wireConversationStreamEvents } from "@/state/conversationStreamStore";

export default function App() {
  const [ready, setReady] = useState<boolean | null>(null);
  const [activeConversationId, setActiveConversationId] = useState<string | null>(null);
  const [agentMode, setAgentMode] = useState(false);
  const [showSettings, setShowSettings] = useState(false);

  useEffect(() => {
    wireConversationStreamEvents();
    commands
      .listModels()
      .then((models) => setReady(models.some((m) => m.installed)))
      .catch(() => setReady(false));
  }, []);

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
    </div>
  );
}
