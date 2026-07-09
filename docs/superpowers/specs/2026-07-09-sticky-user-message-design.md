# Sticky User Message

**Status**: Approved
**Reference**: Mesh chat implementation in `~/code/mesh/apps/mesh/src/web/components/chat/index.tsx`, `message/pair.tsx`, and `message/user.tsx`.

## Motivation

Long agent turns can make it hard to remember which user request the visible assistant/tool output belongs to. Mesh solves this by pinning the current turn's user message near the top of the chat scroller while the assistant response scrolls underneath it. When the next user turn reaches the top, that next user message takes over naturally.

Doce should use the same interaction model: the active user request becomes a compact sticky anchor for the surrounding assistant/tool output.

## Scope

- Add a render-only transcript grouping layer for the workspace chat.
- Render user messages as sticky, clipped bubbles inside their owning turn.
- Let each next user message naturally replace the previous sticky bubble while scrolling.
- Preserve the existing message persistence model and `Message` IPC shape.
- Preserve existing assistant text, tool widget, context notice, error, pending widget, autoscroll, and `Working` status behavior.
- Add focused tests for grouping, sticky/clipped classes, expansion behavior, and existing workspace rendering.

Out of scope:

- Backend schema changes.
- Changing how messages are persisted or loaded.
- A minimap, table of contents, or jump list for turns.
- Persisting expanded/collapsed user bubble state across navigation.
- Rewriting the rich user-message renderer.
- Changing the sidebar conversation row behavior.

## Turn Grouping

Create a small render-only grouping helper near the workspace transcript code named `groupTranscriptTurns(messages)`, exported for focused tests.

Rules:

- A turn starts when a `role === "user"` message appears.
- The user message belongs to that turn.
- Subsequent assistant/tool/context/error rows belong to that turn until the next user message.
- Assistant-only rows before the first user message render as standalone assistant turns.
- Plan-machine rows may remain in the flat stream; existing `MessageContent` filtering can still hide them, but grouping must not break when they appear.
- Synthetic pending `Bash` and `Task` widgets belong visually to the latest turn, not to a separate free-floating block.
- Workspace state, IPC calls, optimistic sending, and refresh logic keep using the flat `messages` array.

This keeps data behavior unchanged while giving the DOM the structure CSS sticky needs.

## Transcript Layout

Replace the direct `messages.map(...)` transcript body with a turn renderer:

```tsx
{turns.slice(0, -1).map((turn) => (
  <TranscriptTurn turn={turn} isLastTurn={false} />
))}
{lastTurn && (
  <div className="min-h-[100cqh]">
    <TranscriptTurn turn={lastTurn} isLastTurn />
  </div>
)}
```

The last-turn `min-h-[100cqh]` mirrors Mesh's important invariant: even a short latest response can dock its user message at the top of the scroller while the chat remains bottom-oriented.

The current scroller remains owned by `StickToBottom`. The scroll container keeps vertical overflow. Horizontal clipping should move to the content wrapper as `overflow-x-clip`, not `overflow-x-hidden`, because `hidden` creates a scroll container and can cause sticky elements to stick to the wrong ancestor. This follows Mesh's verified constraint.

## Sticky User Bubble

For turns with a user message, render:

- a sticky background strip at `top-0`, matching the chat background;
- the user bubble at `sticky top-4 z-*`;
- the assistant/tool content below the bubble in normal flow.

The sticky behavior should be pure CSS. No scroll listener or active-turn observer is required for the replacement effect. Each user bubble sticks only inside its own turn container, so the next turn naturally pushes it away.

## Collapsed User Bubble

Copy Mesh's collapsed bubble behavior:

- Default state clips the user message to a compact max height, approximately `84px`.
- If content overflows, apply a subtle bottom fade/mask.
- The bubble remains clickable/focusable.
- On click or keyboard focus, expand inline to a bounded height, approximately `50vh`.
- Expanded content uses internal scrolling if it exceeds that height.
- Blur collapses it again.
- Clicking the bubble also scrolls the owning turn to the top, matching Mesh's re-anchor behavior.

The visual style should stay close to Doce's existing user message style:

- rounded `bg-muted`/muted surface;
- no nested cards;
- readable markdown/rich-content rendering;
- existing user token meter still appears with the user message and follows the bubble visually.

## Component Shape

Introduce small focused components rather than growing `Workspace.tsx` further:

- `TranscriptTurn`: renders one grouped turn, sticky user header, body rows, and any latest pending widget.
- `StickyUserMessage`: renders the user bubble, clipping/expansion behavior, and token meter.

`MessageContent` continues to render assistant/tool/context/error rows. Extract the current user-message branch into a reusable `UserMessageBubble` component, then use it from both `MessageContent` and `StickyUserMessage`. This avoids duplicating the text/rich-text user rendering logic while keeping `MessageContent` compatible with existing callers.

## Pending Widgets

Pending `AskUserQuestion` still replaces the composer and does not render as a normal transcript body widget.

Pending `Bash` and `Task` widgets currently render after the flat message list. With turn grouping, they should render in the latest turn's body so the associated user request remains sticky above them.

Generic `Working` status remains outside the transcript, above the composer, exactly as in the current design.

## Autoscroll

Preserve current `StickToBottom` behavior:

- Sending a message still re-arms autoscroll and scrolls to bottom.
- Scrolling upward still escapes autoscroll.
- The scroll-to-bottom button still appears when detached.
- New persisted rows still grow the content observed by `StickToBottom`.

The grouping wrapper must be measured by the existing `contentRef`; otherwise streaming/tool updates can stop following the bottom.

## Accessibility

- The sticky user bubble remains in normal document flow.
- Use a focusable element or `tabIndex={0}` on the bubble so keyboard users can expand it.
- Keep the app-wide focus ring visible.
- The clipped state must not remove content from the accessibility tree.
- The bubble should retain the existing `aria-label="You said"` semantics through the turn or bubble container.
- Internal scrolling in expanded state should be keyboard reachable.

## Testing

Add focused tests at the smallest useful level:

- `groupTranscriptTurns` groups user + following assistant/tool rows until the next user.
- Assistant-only rows render without a sticky user bubble.
- Pending `Bash`/`Task` widgets render inside the latest turn body.
- User bubbles include sticky positioning classes and compact clipping classes.
- Clicking/focusing a long user message switches it to the expanded bounded-height state.
- The workspace transcript still renders assistant text, tool widgets, context notices, and errors below the owning user turn.
- Existing streaming/status tests still pass: `Working` remains above the composer and outside the transcript.

Browser-level verification should check the actual sticky behavior in a long multi-turn conversation, because jsdom cannot prove CSS sticky positioning.
