# Chat Streaming Status

**Status**: Approved

## Motivation

The current chat UI renders the generic `Working...` state as a transcript row. That makes active-turn state look like assistant content, and it competes with tool widgets and final answers for transcript space.

The streaming/status state should instead live at the bottom of the chat surface, immediately above the chat input, in the same visual zone as the composer. It should communicate that the model is active without adding another message-like item to the transcript.

## Scope

- Move the generic active-turn indicator out of the transcript message list.
- Render a live status row above the chat input.
- Replace the static `Working...` text with a subtle animated `Thinking` indicator.
- Add a stable elapsed-time chron that starts when the user submits the message.
- Preserve existing pending widgets for `AskUserQuestion`, `Bash`, and `Task`.
- Preserve composer blocking behavior while a turn is active.

Out of scope:

- True token streaming.
- Backend timing schema changes.
- Per-tool progress details.
- Reworking message persistence or active-generation semantics.
- Changing the sidebar conversation state label.

## Placement

The status row sits between the transcript scroller and the composer.

Rules:

- It is not rendered as a `chat-message`.
- It is not inside the rich input.
- It is above the input divider, not overlaid on top of it and not below it.
- The bottom edge of the status row touches the divider line.
- When the status row is visible, its bottom border is the divider between transcript/status and composer.
- When the status row is hidden, the composer keeps its normal top divider so idle layout remains unchanged.

This produces the approved visual relationship:

```text
transcript
...
Thinking 12.4s
----------------  <- bottom of Thinking row touches this divider
chat input
```

## Status Content

The live row should be compact and quiet:

```text
[animated activity mark] Thinking 12.4s
```

Rules:

- Use `Thinking`, not `Working...`, for the generic model-active state.
- Use a small activity animation similar in feel to Claude Code/Codex terminal status indicators: subtle, inline, and non-card-like.
- Keep the animation decorative with `aria-hidden="true"`.
- Expose the live text as a status region for assistive technology.
- Use tabular numbers for the chron so the width does not jitter while ticking.
- Keep visual weight close to existing muted status text.

The exact animation can be simple: animated dots, a pulse, or a narrow shimmer. It should not introduce large layout movement or draw attention away from the transcript.

## Chron Behavior

The chron starts from the user-submitted message that began the active turn.

Rules:

- On send, use the optimistic user message `createdAt` as the start timestamp.
- During a persisted active turn, derive the start timestamp from the latest user message in the conversation.
- Keep counting through the whole active turn.
- Do not reset when tool calls or tool results appear.
- Stop/hide when the turn becomes idle.
- If no user-message timestamp is available, fall back to the time the active status first appears in the current webview session.

This differs from the existing assistant-message `Timer`, which is tied to persisted assistant rows and optional `durationMs`. The streaming chron is an active-turn timer, not a message-duration timer.

## State Rules

Show the generic status row when:

- a local send is in flight;
- `thinking` is true for the current optimistic turn;
- backend active-generation state says the current conversation has a turn running;
- the latest message is an unpaired tool call with no dedicated pending widget.

Hide the generic status row when:

- the conversation is idle;
- the latest pending state is an `AskUserQuestion` widget;
- the latest pending state is a pending `Bash` widget;
- the latest pending state is a pending `Task` widget.

Dedicated pending widgets remain the primary UI for their states. They should not be combined with the generic `Thinking` row.

## Component Shape

Add a focused component near the workspace/chat surface, for example:

```tsx
<StreamingStatus startedAt={activeTurnStartedAt} />
```

Responsibilities:

- Render the animated mark, `Thinking`, and elapsed chron.
- Tick locally while mounted.
- Use tabular-number styling.
- Avoid knowing about chat messages, tool calls, or composer rules.

`Workspace` remains responsible for deriving:

- whether the generic status should be shown;
- which timestamp should seed `startedAt`;
- whether the composer is disabled;
- whether a dedicated pending widget replaces normal input.

## Data Flow

`Workspace` already owns the relevant signals:

- `thinking`
- `sendInFlight`
- `backendTurnActive`
- `pendingToolCall`
- `messages`

Implementation should derive:

```ts
const turnInFlight = sendInFlight || backendTurnActive;
const showGenericStreamingStatus =
  pendingToolCall?.kind === "other" || (!pendingToolCall && showThinking);
```

Then derive the active-turn start timestamp:

1. Prefer a local optimistic user-submit timestamp for the current send.
2. Otherwise use the latest user message `createdAt` from `messages`.
3. Otherwise fall back to a local status-mounted timestamp.

The start timestamp must remain stable while the active turn continues.

## Accessibility

- The visible row should use `role="status"` or equivalent live-region semantics.
- The animated mark is decorative.
- The text `Thinking` and chron remain readable as plain text.
- Avoid frequent full-text announcements on every timer tick if possible. The visual chron can tick often, but screen-reader output should not become noisy.

## Testing

Add focused tests around `Workspace` and the new status component.

Workspace behavior:

- Sending a task shows `agent-thinking` outside the transcript message list and above the composer shell.
- The old transcript-positioned `Working...` row no longer renders as a `chat-message`.
- The status row text says `Thinking`, not `Working...`.
- The status row is hidden when idle.
- Pending `AskUserQuestion`, `Bash`, and `Task` states still suppress the generic status row.
- Composer remains disabled while a turn is active or while a pending tool call exists.

Chron behavior:

- The timer starts from the optimistic user message timestamp when sending.
- The timer derives from the latest persisted user message during backend-active reload state.
- Tool calls/results do not reset the timer.
- Tabular-number styling is present.

Layout behavior:

- The status row renders before the composer shell in DOM order.
- The status row owns the divider line when visible.
- The composer keeps its divider when the status row is hidden.

