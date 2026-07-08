import { useEffect, useMemo, useState } from "react";
import { homeDir } from "@tauri-apps/api/path";
import ContextUsageGauge from "@/components/ContextUsageGauge";
import { TopbarPortal } from "@/components/Topbar";
import { commands, type Conversation, type Workspace } from "@/lib/ipc";
import { getConversationWorkspaceLabel } from "@/views/chat/sidebarConversationRow";

interface WorkspaceTopbarProps {
  conversation: Conversation;
}

export default function WorkspaceTopbar({ conversation }: WorkspaceTopbarProps) {
  const [homePath, setHomePath] = useState<string | null>(null);
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);

  useEffect(() => {
    let cancelled = false;

    homeDir()
      .then((path) => {
        if (!cancelled) setHomePath(path);
      })
      .catch(() => {
        if (!cancelled) setHomePath("");
      });

    commands
      .listWorkspaces()
      .then((loadedWorkspaces) => {
        if (!cancelled) setWorkspaces(loadedWorkspaces);
      })
      .catch(console.error);

    return () => {
      cancelled = true;
    };
  }, []);

  const workspacesById = useMemo(
    () => new Map(workspaces.map((workspace) => [workspace.id, workspace])),
    [workspaces],
  );
  const workspaceLabel = getConversationWorkspaceLabel(
    conversation.workspaceId,
    workspacesById,
    homePath,
  );

  return (
    <TopbarPortal target="main">
      <div
        className="pointer-events-none flex min-w-0 flex-1 items-center justify-between gap-3"
        data-testid="workspace-topbar"
      >
        <div className="min-w-0">
          <div
            className="truncate text-sm font-medium text-foreground"
            data-testid="workspace-topbar-title"
          >
            {conversation.title}
          </div>
          <div
            className="truncate text-xs text-muted-foreground"
            data-testid="workspace-topbar-path"
          >
            {workspaceLabel}
          </div>
        </div>
        <div className="pointer-events-auto" data-topbar-no-drag>
          <ContextUsageGauge conversationId={conversation.id} />
        </div>
      </div>
    </TopbarPortal>
  );
}
