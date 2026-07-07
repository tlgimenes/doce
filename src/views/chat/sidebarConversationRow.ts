import type { ConversationStatus, Workspace } from "@/lib/ipc";

type WorkspaceLookup = Map<string, Pick<Workspace, "path">>;

const MINUTE_MS = 60_000;
const HOUR_MS = 60 * MINUTE_MS;
const DAY_MS = 24 * HOUR_MS;
const MONTH_MS = 30 * DAY_MS;
const YEAR_MS = 365 * DAY_MS;

const WORK_STATE_LABEL: Record<ConversationStatus, string> = {
  in_progress: "Working",
  requires_action: "Review",
  failed: "Blocked",
  done: "Ready",
};

const normalizePath = (path: string) =>
  path.length > 1 && path.endsWith("/") ? path.slice(0, -1) : path;

export function formatConversationRelativeTime(updatedAt: number, now = Date.now()) {
  const elapsed = Math.max(0, now - updatedAt);

  if (elapsed < MINUTE_MS) return "now";
  if (elapsed < HOUR_MS) return `${Math.floor(elapsed / MINUTE_MS)}m`;
  if (elapsed < DAY_MS) return `${Math.floor(elapsed / HOUR_MS)}h`;
  if (elapsed < MONTH_MS) return `${Math.floor(elapsed / DAY_MS)}d`;
  if (elapsed < YEAR_MS) return `${Math.floor(elapsed / MONTH_MS)}mo`;
  return `${Math.floor(elapsed / YEAR_MS)}y`;
}

export function formatWorkspacePathLabel(
  path: string | null | undefined,
  homePath: string | null,
) {
  if (!path) return "Home";

  const normalizedPath = normalizePath(path);
  if (!homePath) return normalizedPath;

  const normalizedHome = normalizePath(homePath);
  if (normalizedPath === normalizedHome) return "Home";
  if (normalizedPath.startsWith(`${normalizedHome}/`)) {
    return `~${normalizedPath.slice(normalizedHome.length)}`;
  }

  return normalizedPath;
}

export function getConversationWorkspaceLabel(
  workspaceId: string | null,
  workspacesById: WorkspaceLookup,
  homePath: string | null,
) {
  if (!workspaceId || !homePath) return "Home";

  const workspace = workspacesById.get(workspaceId);
  if (!workspace) return "Home";

  return formatWorkspacePathLabel(workspace.path, homePath);
}

export function getConversationWorkStateLabel(status: ConversationStatus) {
  return WORK_STATE_LABEL[status];
}
