# Contract: Empty-State Conversation Creation

This feature adds no new Tauri IPC command. This document is the contract
for the **sequence** in which existing commands (from
`001-doce-v1-core/contracts/tauri-ipc.md`) must be composed when the
empty-state composer is submitted, since that ordering is new even though
each individual command isn't.

## Sequence

| Step | Command | Input | Output | Notes |
|------|---------|-------|--------|-------|
| 1 | `open_workspace` | `path: string` (resolved Folder Target — Home or picked) | `Workspace` | Idempotent: returns the existing row if this path was opened before (per `list_workspaces` already ordering by `last_opened_at DESC`, this call should also refresh that timestamp) |
| 2 | `create_conversation` | `workspaceId: string` (from step 1) | `Conversation` | `workspaceId` MUST be passed — this path never creates an unscoped conversation (FR-004) |
| 3 | `send_agent_message` | `conversationId` (from step 2), `content: string` (the composer's typed text) | `string` (assistant's reply) | Same command `Workspace.tsx` already uses for every subsequent turn |

## Failure handling

- **Step 1 fails** (path doesn't exist / isn't a directory — the same
  validation `open_workspace` already performs): surface the error in the
  composer itself; no conversation is created (steps 2-3 never run).
- **Step 2 fails**: surface the error; the workspace from step 1 still
  exists (harmless — it may already exist from a prior use, or is now
  simply available for a future attempt) but no conversation was created.
- **Step 3 fails**: the conversation from step 2 exists but has no
  assistant reply yet — this is the same failure shape `Workspace.tsx`
  already handles for any subsequent message send, not a new failure mode
  this feature introduces.

## Post-condition

On success, the app has one new `Conversation` (with non-null
`workspaceId`), that conversation is `set_focused_conversation`'d and
selected as active, and it renders via the workspace-scoped view because
its own `workspaceId` is non-null (see `research.md` § 4) — not because of
any separate "just created via the composer" flag.
