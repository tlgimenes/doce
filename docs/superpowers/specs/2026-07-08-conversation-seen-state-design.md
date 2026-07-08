# Conversation Seen State Design

## Goal

Render a sidebar conversation title in bold when that conversation has newer activity than the last time the user saw it. Opening the conversation clears the bold state. If the conversation is already active, newly arriving messages are considered seen because the user is watching that thread.

## Current State

Conversations currently expose `updatedAt`, which changes when messages are added. They do not expose a persisted "seen" or "read" timestamp. The frontend tracks only the currently active conversation in React state, and the scheduler has a focused-conversation command for priority, but neither is durable read state.

## Data Model

Add `last_seen_at INTEGER NOT NULL` to the `conversations` table.

Migration behavior:

- Existing rows get `last_seen_at = updated_at`, so old conversations do not all become bold after upgrade.
- New conversations insert `last_seen_at = now`, matching the current "created and already visible" behavior.
- Subagent conversations can carry the same column for schema consistency, even though they are hidden from the normal sidebar list.

The frontend `Conversation` type gains `lastSeenAt: number`.

Unread predicate:

```ts
conversation.id !== activeId && conversation.updatedAt > conversation.lastSeenAt
```

## Backend API

Add a Tauri command:

```ts
markConversationSeen(conversationId: string): Promise<void>
```

The command updates one row:

```sql
UPDATE conversations
SET last_seen_at = MAX(?now, updated_at)
WHERE id = ?conversation_id
```

Using `MAX(now, updated_at)` makes the marker robust if a message timestamp lands slightly ahead of the frontend-open timestamp or if the conversation was updated just before the mark command runs.

`list_conversations` selects and returns `last_seen_at` with each conversation.

## Frontend Behavior

The sidebar title font weight becomes stateful:

- Active conversation: normal title weight, regardless of `updatedAt`.
- Inactive conversation with `updatedAt > lastSeenAt`: bold title.
- Inactive conversation with no unseen updates: normal title.

When the user selects a conversation, the app calls `markConversationSeen(conversation.id)` and updates local state optimistically so the title normalizes immediately.

When messages refresh for the active conversation during streaming, the active conversation should remain normal. The workspace view will notify the app after message refreshes or persisted-message events for the active conversation, and the app will call `markConversationSeen(activeConversation.id)`.

The sidebar's existing polling of `listConversations` remains the source of truth, so any optimistic local update is corrected by the next refresh.

## Error Handling

`markConversationSeen` is best-effort from the UI perspective. If it fails, log the error and leave the sidebar state as returned by `listConversations`; a later successful mark or refresh can recover.

If `markConversationSeen` is called for a missing conversation, it can no-op successfully. This keeps selection and event timing tolerant of conversations being deleted or hidden in future features.

## Testing

Backend:

- Migration adds `last_seen_at` and backfills from `updated_at`.
- `create_conversation` sets `last_seen_at`.
- `list_conversations` returns `last_seen_at`.
- `mark_conversation_seen` updates `last_seen_at` to at least `updated_at`.

Frontend:

- Sidebar renders unseen inactive conversation title as bold.
- Active conversation title renders normal even if `updatedAt > lastSeenAt`.
- Selecting a conversation calls `markConversationSeen` and normalizes the title.
- Active workspace refresh path marks the active conversation seen after new messages arrive.

## Non-Goals

- No unread count badges.
- No per-message read receipts.
- No user-configurable read state.
- No special handling for subagent conversations in the sidebar, because they are already hidden there.
