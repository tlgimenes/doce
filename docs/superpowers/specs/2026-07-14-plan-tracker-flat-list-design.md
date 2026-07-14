# Plan Tracker Flat List Design

## Goal

Replace the collapsible plan tracker with a compact, scrollable task list that keeps every plan step in its original order and makes the current and completed states immediately legible.

## Presentation

- Render a compact status header above and outside the task scroller so it does not consume one of the three visible task rows.
- Derive `doneCount` from completed steps and `queuedCount` as `plan.steps.length - doneCount`.
- Render a single left-aligned summary with no separate overall-status label.
- While `queuedCount` is greater than zero, render `{doneCount} done · {queuedCount} queued`; otherwise render `{doneCount} completed`.
- Match the task rows' horizontal padding and add no vertical gap between the header and scroller.
- Render `plan.steps` directly without sorting, filtering, grouping, or truncating the data set.
- Render each step with the existing `Marker`, `MarkerIcon`, and `MarkerContent` primitives.
- Place a disabled `Checkbox` inside every `MarkerIcon`; the tracker displays agent state and is not user-editable.
- Render the current step using the normal foreground color.
- Render every non-current step using the muted foreground color.
- For a completed step, check its checkbox and apply a line-through to its description. Completion styling applies whether or not the completed step is current.
- Keep descriptions to one truncated line so each row has a stable height.

## Scrolling

- Wrap the marker rows in Shadcn's `MessageScroller` primitives.
- Use the standard `MessageScrollerViewport`, including `overflow-y-auto` and its bottom scroll fade.
- Set the viewport maximum height to exactly three marker rows.
- Keep all rows mounted; plans longer than three steps are reached by scrolling.

## Removed Behavior

- Remove the accordion/collapsible interaction and trigger.
- Remove the separate current-step and progress summary.
- Remove the progress bar, count badge, chevron, completed-step summary, pending-step cap, and `+n more` summary.
- Remove special sorting or placement for the current step.

## Data Flow and Lifecycle

The live `PlanTracker` container keeps its existing plan recovery, update subscription, conversation filtering, and turn-end unmount behavior. Only `PlanTrackerCard` presentation changes. A null plan or a plan without steps still renders nothing.

## Error Handling

No new error path is introduced. Existing recovery failures remain ignored, and malformed current-step indexes simply leave every row muted because no index matches.

## Tests

Update presentation tests to assert done and queued count derivation, omission of the zero queued metric, left alignment, original DOM order, one checkbox per step, checked and line-through completed rows, normal current-row color, muted non-current rows, absence of collapsible/progress UI, and a three-row scroll viewport. Preserve the existing lifecycle and stale-recovery race tests.
