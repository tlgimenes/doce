// 010-context-window-management (UI refactor): the `/compact` slash
// command, mirroring Claude Code's own `/compact` convention — typing it in
// the composer triggers compaction directly (via `commands.compactConversation`)
// instead of being sent as a normal chat/agent message. Shared by Chat.tsx
// and Workspace.tsx so the two composing surfaces can't drift on what
// counts as the command.
export const COMPACT_COMMAND = "/compact";

export function isCompactCommand(content: string): boolean {
  return content.trim().toLowerCase().startsWith(COMPACT_COMMAND);
}
