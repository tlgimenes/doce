# Assistant Message Duration Footer Design

## Motivation

Completed assistant text replies already carry two useful metadata fields:
`durationMs` and `tokenCount`. The UI currently shows output tokens in the
assistant message footer, but Workspace does not enable the existing elapsed
time footer path for normal transcript rendering.

Show the completed reply duration beside the assistant output-token count so
the transcript preserves the same timing context that was shown while the turn
was active.

## Scope

In scope:

- Assistant text messages in the Workspace transcript.
- The existing message metadata footer in `MessageContent`.
- Persisted `durationMs` values from the message model.
- Focused component and Workspace regression tests.

Out of scope:

- Tool-result widgets.
- Error rows.
- Context notices.
- User messages.
- Backend duration calculation.
- Streaming-state timing.
- Token-count calculation.
- Database schema changes.

## Display Rules

Assistant text replies render a muted metadata footer whenever either
`durationMs` or `tokenCount` is available.

Footer combinations:

- Duration and tokens: `1.2s · ↓ 156 tokens`
- Duration only: `1.2s`
- Tokens only: `↓ 156 tokens`

The footer remains hidden when both values are absent.

The duration uses the persisted `durationMs` value. Completed messages do not
tick. If a caller intentionally enables a live timer for a not-yet-complete
assistant text message, the existing `Timer` fallback behavior remains
available, but Workspace should render persisted transcript rows using frozen
durations.

## Component Behavior

`MessageContent` remains responsible for formatting and rendering assistant
message metadata. It already supports `showTimer` plus `tokenCount`; this work
should preserve that boundary rather than duplicating metadata formatting in
Workspace.

Workspace should pass `showTimer` for assistant text messages so persisted
assistant replies can display their duration. The change should not enable
timers for tool widgets, errors, context notices, or user messages.

If the implementation chooses to simplify `MessageContent` so assistant text
messages render duration automatically when `durationMs` exists, it must keep
the public behavior equivalent to the display rules above and avoid adding
metadata to non-text assistant rows.

## Accessibility

The metadata footer is plain text and should remain readable by assistive
technology. The separator is decorative punctuation only; no extra live region
is needed because completed message metadata is static.

## Testing

Add or update focused tests for:

- Assistant text message with both `durationMs` and `tokenCount` renders
  `duration · tokens`.
- Assistant text message with `durationMs` only renders duration.
- Assistant text message with `tokenCount` only renders tokens.
- Assistant text message with neither value renders no metadata footer.
- Workspace renders a persisted assistant text reply with both duration and
  token count in the transcript.
- Non-text rows do not gain duration metadata from this change.

## Acceptance Criteria

- Completed assistant text replies in Workspace show elapsed duration beside
  output-token count when both fields are present.
- Existing token-only display continues to work.
- Duration-only display works for assistant text replies.
- User message token meters remain unchanged.
- Tool widgets remain unchanged.
- No backend, database, or IPC schema changes are made.
