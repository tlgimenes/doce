# Empty State View Transition Design

Date: 2026-07-07
Status: Approved for implementation planning

## Goal

Make the first-message experience feel immediate. When the user submits from the empty state, the UI should leave the empty state as soon as the local backend has created the workspace-scoped conversation. It should not wait for the LLM or agent loop to finish.

The transition should set up a path for animating from the centered empty-state composer into the workspace chat using the same-document View Transition API, while keeping a non-animated fallback for unsupported runtimes.

## Current Problem

`EmptyState.submit` currently runs:

```text
openWorkspace -> createConversation -> sendAgentMessage -> onConversationCreated
```

`sendAgentMessage` resolves only after the full agent loop completes. That means the empty-state screen stays visible until the model has produced its first answer, even though the conversation and the user's first message can be represented in the chat UI much earlier.

## Approved Interaction

The new flow is:

```text
openWorkspace -> createConversation -> transition into Workspace -> Workspace sends first message
```

After `createConversation` resolves:

- `EmptyState` calls the parent with the created conversation and the pending initial turn.
- `App` switches `activeConversation` inside `document.startViewTransition()` when the API is available.
- `Workspace` mounts immediately.
- `Workspace` renders the user's first message optimistically and shows `Working...`.
- `Workspace` calls the existing `sendAgentMessage` path after mount.
- Existing `agent-message-persisted` events and final `listMessages` refresh keep the transcript converged with persisted backend state.

## Transition Behavior

Only the main content pane participates in the transition. The sidebar remains stable.

The empty-state composer and workspace composer should share a named transition target, for example:

```css
view-transition-name: chat-composer;
```

The content pane can use the root transition for a subtle fade and small vertical motion. The initial user message should appear in the new view snapshot, not after the animation finishes.

The implementation must feature-detect the API:

```text
if document.startViewTransition exists:
  run the route-state update inside it
else:
  update route state immediately
```

Use React `flushSync` for the state update inside the transition callback so the new route is committed during the captured DOM update.

## Data Flow

Introduce a small pending first-turn object owned by `App`:

```text
{
  conversationId,
  content,
  richContent?
}
```

`EmptyState` no longer calls `sendAgentMessage` directly. It still owns folder selection and fast setup:

```text
openWorkspace(target.path)
createConversation(workspace.id)
onConversationCreated(conversation, pendingInitialTurn)
```

`App` stores `pendingInitialTurn`, sets `activeConversation`, and passes the pending turn to `Workspace` only for the matching conversation id.

`Workspace` consumes the pending turn once. It should reuse the same send path used by normal workspace composer submissions, so the first message gets the same optimistic bubble, error behavior, rich content handling, and `Working...` state.

## Error Handling

If `openWorkspace` or `createConversation` fails, stay on the empty state and show the existing inline error.

If the first `sendAgentMessage` fails after the transition, show the error inside `Workspace`. Do not navigate back to the empty state, because the conversation already exists.

If View Transition API support is unavailable or fails, the UI should still switch views immediately without animation.

## Accessibility And Motion

The transition must not trap focus or leave duplicate interactive elements active.

Respect reduced motion:

```css
@media (prefers-reduced-motion: reduce) {
  ::view-transition-group(*) {
    animation-duration: 0.001s;
  }
}
```

After the transition, focus should land in the workspace composer unless a pending tool question or disabled state prevents it.

## Testing

Add or update tests for:

- Empty-state submit calls `onConversationCreated` after `createConversation`, without waiting for `sendAgentMessage`.
- `EmptyState` no longer calls `sendAgentMessage`.
- `App` stores and forwards the pending initial turn when the empty state creates a conversation.
- `Workspace` consumes the pending initial turn once and calls `sendAgentMessage`.
- First-turn errors after navigation render in `Workspace`.
- View transition wrapper uses `document.startViewTransition` when available and falls back when unavailable.

## Out Of Scope

- Backend command changes.
- Streaming protocol changes.
- Cross-document view transitions.
- Sidebar animation.
- Redesigning message bubbles or composer layout.
