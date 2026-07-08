# UserAskWidget: pending questions move into the composer

**Status**: Approved, not yet implemented
**Context**: Today, a pending `AskUserQuestion` tool call renders `AskUserQuestionWidget` inline in the message scroll list (`Workspace.tsx`), and the chat composer (`RichInput`) is fully disabled while it's unanswered — the only way to answer is clicking an option button. There's no way to answer with free text.

## Motivation

Answering a clarifying question by clicking a button works for closed-ended cases, but breaks down whenever the real answer is "sort of, but actually do X instead" — something a fixed set of option labels can't capture. The chat input is right there, disabled, for no structural reason: the backend's `answer_user_question(question_id, answer: Vec<String>)` already accepts arbitrary strings, not just strings matching an option label. This is a UI-only change: let the user answer either by clicking an option, or by closing the option widget and typing a free-text answer in the normal composer.

## Scope

- Move the *live, unanswered* question widget out of the message list and into the composer slot (replacing `RichInput` while a question is pending).
- Add a close (✕) affordance that swaps to the full `RichInput`, whose submission becomes the answer's text.
- The *already-answered* read-only rendering (used in message history once the question resolves) is unchanged in behavior, with one wording tweak (Section 3).
- No backend changes. `answer_user_question`'s signature, `PendingQuestions`, and the persisted tool_result shape are untouched — a free-text answer is just `answer_user_question(questionId, [typedText])`.

## Section 1: Component split

- **`AskUserQuestionWidget`** (`src/views/chat/tool-widgets/AskUserQuestionWidget.tsx`, existing file) shrinks to *only* the read-only "already answered" branch. Its sole remaining caller is `MessageContent.tsx`, rendering a resolved `tool_result` in history — that call site always has `detail.answer` set already, so no behavior changes there beyond Section 3's wording.
- **`UserAskWidget`** (`src/views/chat/tool-widgets/UserAskWidget.tsx`, new file) owns the *live*, unanswered question: option buttons, multi-select confirm, the close affordance, and the free-text fallback via `RichInput`. Props: `{ detail: AskUserQuestionDetail }`, same shape as today (always called with `detail.answer === null`).
- `Workspace.tsx`:
  - The message-list pending block drops its `{pendingQuestion && <AskUserQuestionWidget detail={pendingQuestion} />}` line. Bash/Task pending widgets keep rendering there, unchanged.
  - The composer shell renders `pendingQuestion ? <UserAskWidget detail={pendingQuestion} /> : <RichInput ... />` inside the same outer wrapper (`border-t border-border p-4`, `chat-composer` view-transition name) — only the inner component swaps.
  - `Workspace.tsx` no longer imports `AskUserQuestionWidget`; it imports `UserAskWidget` instead.

## Section 2: `UserAskWidget` behavior

Local component state only (`mode: "options" | "text"`, `selected: string[]`, `submitting: boolean`) — nothing persisted, nothing new in `AskUserQuestionDetail`.

- **`options` mode (default)**: same body as today's pending branch — optional header text, question, a multi-select hint, and option buttons (single-select submits immediately on click; multi-select accumulates a selection and requires an explicit "Submit"). Adds a small ✕ close button.
- **✕ click → `text` mode**: swaps to a small "Answering: *{question}*" label, a "back to options" link, and a full `RichInput` (`placeholder="Type your answer…"`, its own `inputTestId`/`submitTestId`). Submitting calls `commands.answerUserQuestion(detail.questionId, [content])` — only the flat `content` string; any `richContent` (attachments, skill mentions) is ignored, since the answer is a plain string and the ask was specifically for "the full text the user typed."
- **"back to options" click → `options` mode**: any in-progress typed draft is discarded (the `RichInput` instance unmounts) — symmetric with closing discarding an in-progress multi-select.
- **Submitting (either mode)** sets `submitting`, disabling further interaction until `answer_user_question` resolves, matching today's guard on button clicks. No new error handling — a failed call isn't currently surfaced to the user in the existing widget either, so this doesn't introduce a new pattern here.
- Once the backend resolves the question, the next `onAgentMessagePersisted`-triggered `refreshMessages()` makes `pendingQuestion` fall away on its own — the latest message becomes a `tool_result`, so `pendingToolCall` recomputes to `null` and the composer swaps back to `RichInput`. No explicit "close myself" logic needed in `UserAskWidget`.

## Section 3: Answered-wording heuristic

`AskUserQuestionWidget`'s answered branch currently always renders `"You chose: {answer.join(", ")}"`. Since an answer can now come from typed free text instead of a button click, and there's no backend field recording which, this is a pure client-side heuristic computed at render time:

```ts
const isFreeText = !detail.answer.every((a) => detail.options.some((o) => o.label === a));
```

If `isFreeText`, render `"You replied: {answer.join(", ")}"`; otherwise keep `"You chose: ..."`. A typed answer that happens to exactly match an option label renders as "You chose" (cosmetic-only false negative, not worth tracking a real flag through the backend for).

## Section 4: `WidgetGallery` and test updates

- `src/views/design-system/WidgetGallery.tsx`: the "Pending, single-select" / "Pending, multi-select" examples switch from `AskUserQuestionWidget` to `UserAskWidget` (that's what actually renders for a pending question now). The "Answered" example stays on `AskUserQuestionWidget`. Add a new "Pending, free-text fallback" example showing the closed/typing state, so the gallery keeps documenting every visual state of this tool.
- `Workspace.test.tsx`'s existing case ("shows the pending question widget… when the latest message is an unanswered AskUserQuestion tool_call") gets updated to assert the widget renders in the composer slot (replacing `RichInput`) rather than in the message list.
- New tests for `UserAskWidget`: option click submits immediately (single-select), multi-select accumulate + submit, ✕ switches to text mode and submitting text calls `answerUserQuestion` with `[typedText]`, "back to options" returns to button mode.
- `AskUserQuestionWidget`'s existing answered-state tests extend to cover the new "You replied" wording for a non-option-matching answer.

## Out of scope (explicitly deferred)

- Any backend/schema change to track answer provenance (button vs. typed) — the client-side heuristic in Section 3 covers the only user-visible consequence.
- Preserving an in-progress typed draft across a "back to options" → close round-trip.
- New error UI for a failed `answer_user_question` call — matches existing (absent) error handling.
