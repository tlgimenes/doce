# Single Workspace Chat And Autoscroll Design

## Goal

Make the app use one chat surface: the workspace/agent chat reached from the empty state. Remove the legacy plain chat UI path and simplify dependent frontend code that only existed because conversations could be rendered by either `Chat` or `Workspace`.

Add bottom-following autoscroll behavior to that single chat surface so new messages and agent progress stay visible while the user is already at the bottom, but do not yank the viewport when the user scrolls upward to inspect prior output.

## Scope

In scope:

- Remove `src/views/chat/Chat.tsx` from the active UI.
- Remove `src/views/chat/Chat.test.tsx`.
- Simplify `App.tsx` so every selected conversation renders `Workspace`.
- Simplify input focus shortcuts so they target only `empty-state-input` or `agent-input`.
- Remove frontend code that only supports legacy plain-chat streaming if it becomes unused after `Chat` removal.
- Add autoscroll behavior to `Workspace`.
- Add focused tests for the simplified routing and autoscroll state.

Out of scope:

- Backwards compatibility for old `workspaceId: null` conversations.
- Database migrations to rewrite old conversations.
- Backend removal of existing plain-chat commands unless they become unreachable and the removal is mechanically safe in the same pass.
- Redesigning the transcript or composer visuals.

## Product Behavior

Any active conversation renders the workspace chat. The UI does not branch on `conversation.workspaceId`; the empty state remains the only supported way to create a new conversation.

Cmd+L focuses:

- `empty-state-input` when no conversation is selected.
- `agent-input` when any conversation is selected.
- Nothing when settings is open and no chat input is mounted.

When messages are appended, replaced by refreshed history, or the `Working...` indicator appears/disappears, `Workspace` follows the transcript only if the user is already near the bottom.

The user can scroll up to pause following. While paused, incoming agent progress does not move the viewport. Scrolling back near the bottom resumes following automatically.

No separate "plain chat" behavior, `chat-input`, `chat-send`, assistant token streaming placeholder, or cancel-generation UI remains in the frontend.

## Autoscroll Design

`Workspace` owns the scroll container with a ref on the existing `flex-1 overflow-y-auto` element.

The behavior uses a small near-bottom state:

- `isAutoScrollPinned`: true when the scroll container is within a small threshold of the bottom.
- On user scroll, recompute near-bottom and update the pinned state.
- On transcript-affecting changes, if pinned, scroll to the bottom on the next animation frame.

Transcript-affecting changes include:

- `messages` changes.
- `showThinking` changes.
- `pendingQuestion` changes.
- `conversationId` changes.

The threshold should be forgiving, roughly `48px`, so tiny layout changes or subpixel rounding do not accidentally pause autoscroll.

For a new conversation route, the view should start pinned. If a conversation is switched, reset pinned state to true and let the newly loaded transcript settle at the bottom.

No "new messages" button is required in this pass. The primary requirement is the pause/resume behavior.

## Frontend Simplification

`App.tsx` should no longer import or render `Chat`.

The active conversation render path becomes:

```tsx
activeConversation ? (
  <Workspace ... />
) : (
  <EmptyState ... />
)
```

`pendingInitialTurn` remains scoped by conversation id. This still protects against stale pending turns even though all active conversations now use `Workspace`.

`buildShortcuts` usage in `App.tsx` should no longer compute a `chat-input` selector. The focus target only depends on whether `activeConversation` exists.

If `wireConversationStreamEvents` and `conversationStreamStore` become unused after deleting `Chat`, remove their frontend references/tests/files in the same change. If backend plain-chat IPC commands remain used by other layers or tests, leave them alone.

## Testing

Update `App.test.tsx`:

- Remove tests asserting `Chat`/`chat-input` behavior for `workspaceId: null`.
- Add or update a test proving a selected conversation renders `Workspace` even if its fixture has `workspaceId: null`.
- Update Cmd+L tests to assert selected conversations focus `agent-input`.

Remove `Chat.test.tsx`.

Add `Workspace` autoscroll tests:

- Starts pinned and scrolls to the bottom after messages render.
- While pinned, appending/refetching messages scrolls to the bottom.
- When the user scrolls above the threshold, appending messages does not change `scrollTop`.
- When the user scrolls back near the bottom, appending messages scrolls to the bottom again.
- Switching `conversationId` resets pinned state.

Keep existing `Workspace` tests around pending initial turns, stale async guards, rich content, `/compact`, and pending question behavior.

## Risks

`jsdom` does not perform real layout, so autoscroll tests must define `scrollHeight`, `clientHeight`, and `scrollTop` on the scroll container explicitly.

The module-level in-flight send guard in `Workspace` must keep clearing on all send completion/error paths; autoscroll must not introduce dependencies that cause duplicate pending initial sends.

Deleting `Chat` may reveal stale references in docs/specs. Historical specs can remain unchanged, but source imports, tests, and active app code should be clean.
