# Data Model: Chat Empty State Composer

No new persisted entities or schema changes — `Workspace` and
`Conversation` (both already defined in `001-doce-v1-core`'s data model)
cover everything this feature needs. This document covers the transient
UI state and how it maps onto that existing data.

## Folder target (transient UI state, not persisted)

Owned by `EmptyState.tsx` until the composer is submitted.

| Field | Type | Notes |
|-------|------|-------|
| `kind` | `"home"` \| `"recent"` \| `"browsed"` | How the current target was chosen — informational, doesn't change what happens on submit (all three resolve to a path) |
| `path` | string | The resolved absolute path — the user's home directory for `"home"`, the picked `Workspace.path` for `"recent"`, or whatever the native dialog returned for `"browsed"` |
| `displayLabel` | string | What the selector shows — `"Home"`, or the folder's display name |

**Validation rules**:
- `path` MUST be a real, existing directory at submit time — the same
  validation `open_workspace` already performs (existing behavior, not
  new).
- Selecting a folder target alone (without submitting a message) MUST NOT
  create or modify anything server-side (FR-009).

## Recent folders list (derived, not stored separately)

`FolderPicker.tsx` renders this from `commands.listWorkspaces()` (already
ordered `last_opened_at DESC` server-side — verified directly in
`src-tauri/src/commands/workspaces.rs`), with a synthetic "Home" entry
pinned first, filtered client-side against whatever the user types into
the picker's search field.

| Field | Source |
|-------|--------|
| Pinned "Home" entry | Synthesized client-side; not a `Workspace` row |
| Recent entries | `Workspace[]` from `listWorkspaces()`, unchanged shape (`id`, `path`, `displayName`, `createdAt`, `lastOpenedAt`) |
| Current selection indicator | Compares the list entry's path against the composer's current Folder Target `path` |

## Conversation creation sequence (orchestration of existing entities)

On submit, in order (see `contracts/conversation-creation.md` for the
exact command sequence):

1. Resolve the Folder Target's `path` (already known at this point).
2. `open_workspace(path)` — returns an existing `Workspace` row if that
   path was opened before, or creates one. Existing command, unchanged.
3. `create_conversation(workspaceId)` — creates a new `Conversation` with
   `workspaceId` set (never null from this path). Existing command,
   unchanged signature.
4. `send_agent_message(conversationId, content)` — sends the typed text
   as the conversation's first turn. Existing command, unchanged.
5. The app selects the new conversation as active; `App.tsx` routes it to
   the (restructured) `Workspace.tsx` view because its `workspaceId` is
   non-null.

No step here is a new command — this is a new *sequence*, composed
entirely from existing IPC surface (see `research.md` for why no backend
schema change was needed).
