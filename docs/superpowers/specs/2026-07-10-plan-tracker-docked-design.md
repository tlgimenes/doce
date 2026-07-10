# Plan Tracker Docked Above Composer Design

## Summary

Move the live plan/todo tracker from its floating overlay in the transcript
gutter to a docked strip directly above the chat input, Claude Code style:
a collapsed one-liner (current step + n/m) that expands upward into the full
step list. The floating card/rail overlay, its `@5xl` container-query split,
and the tap-to-expand rail are deleted — one presentation at every width.
Chrome stays 100% stock shadcn (PlanTracker already composes
Card/Item/Badge/Progress/Spinner from the transcript refactor); this is a
relocation and recomposition, not a re-theming.

Supersedes the placement portion of
2026-07-09-plan-tracker-design.md (gutter overlay, card/rail split, numbered
dot rail). Step-state semantics, caps, and lifecycle survive unchanged.

## Decisions (user-confirmed)

1. Resting presentation: collapsed one-liner (icon + current step title +
   n/m Badge), click to expand.
2. The docked strip fully replaces the overlay — card/rail split and rail
   deleted, no wide-screen overlay retained.

## Placement

`Workspace.tsx` renders PlanTracker as a flex sibling in the main column,
between the transcript scroller and StreamingStatus:

    transcript scroller → plan strip → StreamingStatus → composer shell

- The strip aligns to the composer column: `mx-auto max-w-3xl` wrapper
  (layout utilities only).
- PlanTracker no longer mounts inside the MessageScroller; the scroller's
  `@container` class (added solely for the rail split) is removed from
  `Workspace.tsx`.
- StreamingStatus placement and suppression rules are untouched; while a
  planned turn streams, strip and status stack vertically.

## Component structure (all ui-layer primitives)

- Root: `Collapsible` (uncontrolled, closed by default) with
  `data-testid="plan-tracker"`.
- `CollapsibleContent` renders BEFORE the trigger so the list expands
  upward, Claude Code style:
  - `Progress` (done/total) at the top,
  - the existing `ItemGroup`/`Item` step rows exactly as PlanTracker
    renders them today: lucide `Check` / `Spinner` (decorative,
    `role="presentation"`) / `Circle` in `ItemMedia`,
    `data-state="done|current|todo"`, `data-current`,
    `data-testid="plan-step"`, done-steps folding into the
    "✓ n done" line (`plan-done-collapsed`) past 6 steps, pending cap 4
    with "+N more" (`plan-more`).
- `CollapsibleTrigger`: an `Item`-based one-liner row,
  `data-testid="plan-current-step"`:
  - `ItemMedia variant="icon"`: `Spinner` while the current step runs,
    `Check` when all steps are done,
  - `ItemTitle`: current step description; falls back to the plan goal
    while `currentStepIndex` is null (planning phase),
  - `ItemActions`: `Badge variant="secondary"` with `doneCount/total` and
    a chevron that reflects open state.
- Deleted: `PlanCard`'s standalone Card shell, `PlanRail`, the `@5xl:`
  visibility classes, the `expanded` tap-state, `plan-card`, `plan-rail`,
  `plan-dot`, `plan-collapse`, `plan-chip` testids.

## Lifecycle (unchanged)

- Subscription to plan-update events, mount-time recovery via
  `get_active_plan`, the stale-recovery and late-null race guards, and the
  immediate unmount on `plan: null` all stay byte-for-byte.
- The Collapsible's open state lives in the component and disappears with
  it on unmount/conversation switch.

## Strictness

Same rules as the transcript spec: PlanTracker and Workspace may add only
layout utilities; every visual comes from ui-layer primitives. No new
theme.css rules, no arbitrary values, no palette colors.

## Testing

- `PlanTracker.test.tsx` updates: one-liner content (current step title,
  goal fallback while planning, n/m badge, all-done Check), expand/collapse
  via the trigger (`data-state` on the Collapsible), step-list assertions
  (`plan-step` rows, `plan-done-collapsed`, `plan-more`) survive as-is,
  overlay/rail tests deleted.
- `Workspace.test.tsx`: placement assertion — plan strip renders between
  the scroller and StreamingStatus (DOM order), and NOT inside the
  scroller.
- jsdom cannot verify the upward expansion visually; the running app is
  the verification surface (a planned turn, or WidgetGallery if it gains a
  plan-strip showcase — optional, not required).

## Verification gates

- `npm run build`, `npm test`, `npm run lint`; compliance sweep over
  `PlanTracker.tsx` (layout-only utilities).
- Real-app check during a planned turn: strip visible above the input,
  one-liner shows the running step, expansion opens the list upward,
  turn end removes the strip.

## Risks

- Vertical space: strip + StreamingStatus + composer stack while
  streaming; the one-liner keeps resting cost to a single row.
- The upward-opening CollapsibleContent (content before trigger) is an
  uncommon composition; if the stock Collapsible animation assumes
  downward expansion, accept the default animation direction rather than
  adding custom CSS.
