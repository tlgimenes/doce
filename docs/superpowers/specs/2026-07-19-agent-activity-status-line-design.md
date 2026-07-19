# Agent activity status line — design

**Date:** 2026-07-19
**Status:** implemented (`src/views/workspace/AgentActivity.tsx`)

## Problem

Three widgets stacked above the composer — the plan tracker, the working/
reasoning status, and the conversation-goal banner — each with its own shape,
spacing, and visual language. Together they read as three disconnected strips,
not one coherent "here's what the agent is doing" surface.

## Design

One status line docked above the composer (replacing all three), built from
doce's own monochrome tokens.

### Collapsed pill

A single rounded row: `primary · progress · working`.

- **Primary slot** — grows to fill the line, truncates with an ellipsis (full
  text on hover). Content, in priority order:
  1. the conversation **goal** (◎; muted with a ✓ when observer-confirmed
     achieved),
  2. with no goal, the **current todo** (the plan's active step),
  3. with neither, nothing (the slot collapses and the working indicator
     justifies right).
- **Progress** — a slim bar + `done/total`, shown when a plan exists. Fenced
  from its neighbours by hairline vertical dividers.
- **Working indicator** — a pulsing filled circle + elapsed chron + the turn's
  `↑in ↓out` token totals, justified to the right edge, shown only while a turn
  is in flight.

### Thinking row

Above the pill, shown only while the model is actively reasoning: a ✳ +
shimmering "Thinking" + the model's current reasoning line (truncated). It
vanishes the moment reasoning closes (a `</think>`/tool-call boundary), so a
running tool call shows the pill alone.

### Expanded panel

The pill's ⌄ expands a panel below it (present only when there's a plan or an
editable goal to reveal): the goal's edit/delete controls (for a live,
un-achieved goal) followed by the full plan checklist in source order, current
step emphasised, done steps struck through.

## Data & seams

No backend change — the line consumes exactly what the old three widgets did:

- **plan**: `get_active_plan` recovery + `onPlanUpdate` events (subscription
  moved verbatim from the old `PlanTracker`);
- **working**: the workspace's `showGenericStreamingStatus` gate, active-turn
  `startedAt`, live `TurnTokenTotals`, and the raw generation `stream` (the
  reasoning line is derived by `currentThinkingLine`, lifted verbatim from the
  old `StreamingStatus`);
- **goal**: the workspace's existing goal state. The goal is now *displayed*
  here; the composer's ◎ toggle still *sets* it, and the panel's edit control
  drives RichInput's prefill via a new `editGoalToken` prop (the display and
  the editor live in different components, so the edit action is a token, not a
  direct call).

`AgentActivity` (container) owns the subscription + chron; `AgentActivityView`
(presentational) is what the WidgetGallery renders from static snapshots.

## Deviation from the brainstorm mock

The mock floated the token count on the thinking row. It lives in the pill's
working segment instead, so the optimistic input-token estimate stays visible
whether or not the model is mid-reasoning (and to preserve the existing
token-meter behaviour and its tests).
