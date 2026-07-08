# UserAskWidget: Grouped Form List redesign

**Status**: Approved, not yet implemented
**Context**: Follow-up to `2026-07-08-user-ask-widget-design.md` (which moved the pending `AskUserQuestion` prompt into the composer slot). A `/brainstorming` design pass explored a general visual/motion polish pass: three independent redesign directions were generated in parallel (each from a distinct creative lens), scored and synthesized by a judge, then iterated live with the user against an interactive HTML mockup reproducing doce's actual theme tokens. Converged on "Grouped Form List" plus two revisions from user feedback.

## Motivation

The shipped `UserAskWidget` uses `flex-wrap` pill buttons for options (no shape precedent elsewhere in doce), hides each option's description behind a hover-only `title=` attribute (unreachable by keyboard/screen-reader users), and has an inconsistent answering interaction — single-select answers immediately on click, multi-select requires a separate confirm button. This pass gives the widget a visual identity that reuses doce's own existing conventions (the row style already exists almost verbatim in the sidebar's `ConversationList`; the module shape already exists in `RichInput`'s own card), fixes a real bug (text-mode's card nested inside the widget's own outer card), and unifies the answering interaction across all three states — single-select, multi-select, and free-text — into one deliberate "press send" action.

## Scope

- Visual/structural redesign of `UserAskWidget`'s "options" mode: real radio/checkbox rows instead of pill buttons, always-visible descriptions, one shared bordered module card containing both the row list and the submit footer.
- Unified answer interaction: picking an option (single- or multi-select alike) only selects it; a submit button — styled, sized, and positioned identically to `RichInput`'s own send button — is the one way to answer, always present, disabled until at least one option is selected.
- Motion: the composer-level arrival (`RichInput` ↔ `UserAskWidget`) and the internal mode switch (options ↔ text) both ride the app's existing view-transition language (`src/lib/viewTransition.ts`'s `runViewTransition`, already used by `App.tsx` for conversation switches); option rows get a mount-time entrance stagger.
- No change to `AskUserQuestionWidget`'s answered rendering, the backend, or `answer_user_question`'s wire contract — `commands.answerUserQuestion(questionId, answer)` is still called with exactly the same shapes as today (`[label]` for single-select, `selected` for multi-select, `[content]` for free text), just gated behind an explicit send press instead of auto-firing on a bare option click for single-select.
- Text-mode (the `RichInput` fallback) is visually unchanged — it already has the matching card/shadow/send-button treatment this redesign gives the options module. Only the header row's icon (✕ in options mode, back-arrow in text mode) shares one slot across both modes, which is already true of the current implementation.

## Section 1: Shell & header (unchanged)

The header row — optional eyebrow (`detail.header`) above the question text, with an `ml-auto` icon button (✕ to enter free-text mode, back-arrow to return) — is unchanged from what's already shipped. This redesign is scoped entirely to the module beneath the header.

## Section 2: The options module

Replaces the current `flex flex-wrap gap-2` Button-pill layout entirely with one bordered card that mirrors `RichInput`'s own outer wrapper (`rounded-lg border border-border bg-card px-3 py-2 shadow-xs transition-shadow focus-within:shadow-sm`), containing two stacked regions:

- **Option rows** — a vertical stack (`flex flex-col gap-...`), wrapped in a container with `role="radiogroup"` (single-select) or `role="group"` (multi-select) and `aria-labelledby` pointing at the question text's element id. Each row is a real `<button type="button" role="radio"|"checkbox" aria-checked={...}>`, full-width, left-aligned: a 16px glyph on the left (an empty ring for radio / an empty square for checkbox at rest; a selected radio gets a filled center dot, a selected checkbox gets a filled square plus a checkmark), then the option's label and its description stacked to the right of the glyph. The description — today only reachable via a hover `title=` attribute — becomes always-visible `text-xs text-muted-foreground` text. Hover and selected rows both get a `bg-muted` background; the selected label additionally gets `font-semibold`.
- **Footer row** — `flex items-center justify-between`, living inside the *same* card directly beneath the rows (not below/outside the card). Left side: a `text-xs text-muted-foreground` "`N selected`" fragment, rendered only for multi-select once `selected.length > 0` (nothing renders here otherwise, including for single-select — no standing "Select at least one" hint text anywhere). Right side: the submit button (Section 3).

## Section 3: The one submit button

A single shared visual, reused across single-select, multi-select, and (already, via `RichInput` itself, unchanged) free-text: `variant="primary"`, `className="h-8 w-8 shrink-0 rounded-full p-0 enabled:bg-gradient-to-r enabled:from-[var(--color-primary)] enabled:via-[var(--color-gray-2)] enabled:to-[var(--color-gray-1)] enabled:hover:from-[var(--color-gray-2)] enabled:hover:via-[var(--color-gray-1)] enabled:hover:to-[var(--color-foreground)]"` — the exact classes `RichInput`'s own send button already uses — with a `PaperPlaneRightIcon size={16}` and `aria-label="Send answer"`.

Always rendered, for both single- and multi-select. `disabled` whenever `selected.length === 0`. Clicking it calls the existing `submit(selected)` function, unchanged from today's implementation — only the trigger changes.

**Interaction change**: clicking an option row, in either select mode, now only toggles/sets `selected` — it no longer calls `submit` directly. `toggleOption`'s single-select early-return (`if (!detail.multiSelect) { submit([label]); return; }`) is removed; single-select becomes structurally identical to multi-select's selection-toggling, except picking a *different* option replaces `selected` outright (`[label]`) rather than appending to it, so exactly zero or one option is ever selected at a time. Native radio-button semantics apply: clicking the *already-selected* single-select option again is a no-op (it stays selected) — single-select never toggles back down to zero once something is chosen, only multi-select can reach zero again by un-checking its last selection.

## Section 4: Motion

- **Composer-level arrival** (the `RichInput` ↔ `UserAskWidget` swap in `Workspace.tsx`, driven by `pendingQuestion` flipping): wrap the state update that causes this swap in the existing `runViewTransition` helper (`src/lib/viewTransition.ts`), so it rides the `chat-composer` view-transition group instead of hard-cutting. This is a real, verified gap: `Workspace.tsx`'s composer wrapper div already carries `[view-transition-name:chat-composer]` and `theme.css` already defines that group's `animation-duration`, but nothing today calls `runViewTransition` around the state change responsible for this particular swap (unlike `App.tsx`'s conversation-switch, which already does).
- **Options ↔ text mode switch**: give the module's swappable inner content a stable `view-transition-name` (e.g. `user-ask-module`), reused identically across both the options-mode and text-mode render branches, so the browser treats it as one continuous element morphing between two contents rather than two independent old/new elements. Small dedicated keyframes (~120ms out / ~180ms in, `translateY` 3px, `cubic-bezier(0.2, 0, 0, 1)` — the same easing curve already used by `chat-surface`/`chat-composer`). `setMode` calls wrap in `runViewTransition`.
- **Option row entrance**: on the module's initial mount only (not replayed on every selection-change re-render), rows get a subtle staggered entrance (opacity + small `translateY`, roughly an 18ms delay per row).
- All of the above is duration/animation-based CSS, already covered for free by the existing blanket `@media (prefers-reduced-motion: reduce)` rule in `theme.css` (zeroes all animation/transition durations app-wide) — no new guards needed.

## Section 5: Accessibility

- Options container: `role="radiogroup"` (single-select) or `role="group"` (multi-select), `aria-labelledby` referencing the question text's element id.
- Each row is a real `<button>` with `role="radio"|"checkbox"` and `aria-checked` reflecting selection state — today's implementation has no ARIA toggle state at all on its option buttons; this is a net accessibility improvement, not a regression risk.
- Option descriptions move from an unreachable hover `title=` attribute to always-visible, screen-reader-reachable text — another net improvement.
- Focus order is unchanged in spirit: header icon button → option rows in document order → submit button. The app's global `:focus-visible` 2px ring (`theme.css`, never suppressed) applies to rows and the submit button exactly as it does everywhere else in the app.
- No roving-tabindex arrow-key navigation for the radiogroup pattern in this iteration (Tab-per-row instead of arrow-key-per-row) — a known, deliberate simplification, not full WAI-ARIA authoring-practice compliance for `role="radiogroup"`. Flagged as a possible follow-up, not a blocker for this pass.

## Testing

- `UserAskWidget.test.tsx` needs updating for the interaction change: a single-select option click must no longer call `commands.answerUserQuestion` directly — it must only select, requiring a subsequent submit-button click to answer. Existing single-select/multi-select/close/free-text tests get adjusted accordingly (their option-click assertions become "click option, then click submit"); new coverage for "the submit button is disabled until an option is selected" and "clicking the submit button answers with the selected option(s)."
- No change needed to `AskUserQuestionWidget` or its tests — the answered/read-only rendering is untouched by this pass.
- `Workspace.test.tsx`'s existing pending-question integration tests need their option-click assertions updated to include the additional submit-button click, since single-select no longer answers on a bare option click.
- Motion/view-transition wiring: covered by reusing the existing, already-unit-tested `runViewTransition` helper (`src/lib/viewTransition.test.ts`) — no new unit tests are needed for the transition mechanics themselves, matching how `App.tsx`'s existing conversation-switch transition is exercised today (asserting `startViewTransition` was invoked via the shared helper, not the animation's visual behavior, which jsdom can't render anyway).

## Out of scope (explicitly deferred)

- Roving-tabindex arrow-key navigation for the `radiogroup`/`group` pattern.
- Any change to `RichInput` itself, `AskUserQuestionWidget`'s answered rendering, or the backend/`answer_user_question` wire contract.
- The other two explored directions and their specific elements — "Elevated Question Rail"'s drop shadow + top accent bar + numbered index badges (no precedent anywhere in doce, reads as notification/alert chrome), and "Featherweight Ask"'s fully unboxed layout + `rounded-full` pill shape (under-signals a state that's genuinely blocking the conversation, no shape precedent either) — considered and explicitly not adopted, per the judge's scoring and the user's own preference for "Grouped Form List."
