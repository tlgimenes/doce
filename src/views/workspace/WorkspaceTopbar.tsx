import { useEffect, useMemo, useState } from "react";
import { homeDir } from "@tauri-apps/api/path";
import { TopbarPortal } from "@/components/Topbar";
import { Item, ItemDescription, ItemTitle } from "@/components/ui/item";
import { Progress } from "@/components/ui/progress";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { commands, type Conversation, type Workspace } from "@/lib/ipc";
import { useContextUsageStore } from "@/state/contextUsageStore";
import { getConversationWorkspaceLabel } from "@/views/chat/sidebarConversationRow";

interface WorkspaceTopbarProps {
  conversation: Conversation;
}

/**
 * 010-context-window-management (UI refactor): a small usage indicator in
 * the topbar. Display-only — hovering shows the exact percentage in a
 * tooltip; compaction is triggered by typing `/compact` in the composer.
 */
function ContextUsageIndicator({ conversationId }: { conversationId: string }) {
  const usage = useContextUsageStore((s) => s.usage[conversationId]);
  const setUsage = useContextUsageStore((s) => s.setUsage);

  useEffect(() => {
    let cancelled = false;
    commands
      .getContextUsage(conversationId)
      .then((u) => {
        if (!cancelled) setUsage(u);
      })
      .catch(() => {
        // No model loaded yet, or nothing to report — leave the indicator
        // unrendered rather than surfacing an error for a background
        // enrichment call.
      });
    return () => {
      cancelled = true;
    };
  }, [conversationId, setUsage]);

  if (!usage) return null;

  const pct = usage.tokenBudget > 0 ? (usage.tokensUsed / usage.tokenBudget) * 100 : 0;
  const clampedPct = Math.min(100, Math.max(0, pct));
  const tooltipText =
    usage.state === "justCompacted"
      ? `${Math.round(pct)}% of context used · just compacted`
      : `${Math.round(pct)}% of context used`;

  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <div
            className="flex h-8 w-16 items-center"
            data-testid="context-usage-gauge"
            role="status"
            aria-label={tooltipText}
          />
        }
      >
        <Progress value={clampedPct} />
      </TooltipTrigger>
      <TooltipContent data-testid="context-usage-tooltip">{tooltipText}</TooltipContent>
    </Tooltip>
  );
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
        {/* One row, not a column: the title anchors left and grows; the
            path sits right — both give way with truncation when the
            window narrows. */}
        <Item size="xs" className="min-w-0 flex-1 justify-between gap-3 p-0">
          <ItemTitle className="min-w-0 flex-1 truncate" data-testid="workspace-topbar-title">
            {conversation.title}
          </ItemTitle>
          <ItemDescription className="min-w-0 truncate" data-testid="workspace-topbar-path">
            {workspaceLabel}
          </ItemDescription>
        </Item>
        <div className="pointer-events-auto" data-topbar-no-drag>
          <ContextUsageIndicator conversationId={conversation.id} />
        </div>
      </div>
    </TopbarPortal>
  );
}
