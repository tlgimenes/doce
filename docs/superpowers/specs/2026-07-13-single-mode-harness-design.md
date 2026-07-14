# Single-mode harness: Todo replaces the plan machine

**Date:** 2026-07-13
**Status:** design approved in conversation; implementation pending spec
review. Benchmark-gated — this REPLACES the two-mode plan engine, and the
ladder (old harness at its last commit vs this) is the acceptance test.

## Why

The two-mode harness (PLANNING/EXECUTING, five plan tools, union prompt
with two rule sections, sampler-gated per-mode tool sets) was built to
scaffold small non-thinking models. Its costs landed on exactly the wrong
side: prompt complexity and mode role-play are burdens on the MODEL (the
stated pain), while its benefits are mostly enforceable harness-side.
Thinking models change the calculus — the `<think>` scratchpad provides
per-turn deliberation, overlapping with what PLANNING mode existed to
force.

## The toolset (9, from 15)

| Tool | Absorbs | Notes |
|---|---|---|
| `Read` | — | unchanged |
| `Update` | Write, Edit | `content` → create/overwrite; `old_string`+`new_string` → surgical replace. Argument shape selects behavior; dispatch validates exclusive shapes and keeps Edit's guardrails (gutter detection, no-match hints) |
| `Bash` | — | unchanged, incl. denylist + counting guidance |
| `Grep` | — | kept separate (return shapes differ; one prompt line is cheap) |
| `Glob` | — | kept separate, same reason |
| `Task` | — | unchanged (subagent flavor omits it, as today) |
| `AskUserQuestion` | — | unchanged, top-level only |
| `Todo` | CreatePlan, AddStep, ResumeExecution, StepDone, RefuseStep | see below |
| `FinishTask` | — | terminal tool; Require grammar still demands "exactly one tool call per response" |

## Todo semantics

`Todo(items: [{"text": string, "done": bool}])` — full replace, no diff
protocol, no status enum. `in_progress` is INFERRED: the first undone
item is the current one (PlanTracker already highlights exactly that).
The model's whole mental model: keep a checklist, flip `done` as you go.

- Calling `Todo` never switches modes or hands off execution — it's pure
  state reporting; the loop feeds back a terse ack (`Todo updated: 2/5
  done`) as the tool result.
- The tracker UI consumes it via the existing plan-update event path
  (`PlanSnapshot { goal: first user message or elided, steps, current =
  first undone }`) — PlanTracker/PlanTrackerCard render unchanged.
- Empty list is legal (clears the tracker).

## The prompt (one narrative, no modes)

Keeps: doce identity, dialect-specific call-format teaching
(tool-dialects design), the `<tools>` block, "# Size up the request
first" triage, "# Counting and sampling", "every response is exactly one
tool call". Replaces both mode sections and the plan-granularity essay
with roughly:

> For anything multi-step, keep a todo list with `Todo` — one item per
> file or unit of work, `done: true` as you finish each. Work the list in
> order. Before `FinishTask`, verify your own work with Read/Grep — a
> belief that something is done is not proof. `FinishTask` delivers your
> final answer.

## Harness-side replacements for what the machine did

| Old mechanism | Replacement |
|---|---|
| Per-mode sampler gating | Gone — one name enum, always the full set (subagent set still omits Task/AskUserQuestion). Grammar+think prefix unchanged |
| Recitation state tail | A one-line volatile tail when todos exist: `Todos: [x] a [→] b [ ] c` (same KV-stable-prompt + tail mechanics) |
| Bundled-step / stops-partway failure | **FinishTask bounce**: undone todos ⇒ reject once per streak with "N todos remain — finish them or remove them with Todo", consuming a turn. Bounded by the existing futile-streak/max-turns guards |
| Forced verification pass | Prompt line (above) + the bounce naturally forces a last look at the list |
| Per-step turn budgets | Gone (accepted risk). Mitigations: turn cap, futile streak, empty-response retry, truncation retry |
| Plan tools invisible in transcript | Same treatment: `Todo` calls render as tracker updates, not transcript widgets |

## What gets deleted

`plan.rs`'s state machine (PlanState transitions, allowed-per-state tool
sets, ResumeExecution/StepDone/RefuseStep semantics, recitation
renderer), the union-prompt mode narrative, the plan-tool dispatch arms.
The plan-update event, PlanSnapshot shape, and PlanTracker UI stay (fed
by Todo). `Write`/`Edit` dispatch arms merge into `Update` (tool names in
persisted history: old transcripts still render via the existing widgets
— `EditDiffWidget`/`WriteWidget` keep their `toolName` matches, and
`Update` results map to the same two widgets by argument shape).

## Risks, explicitly

1. Tier-4 regression (the 20-step planned tasks) — the whole reason the
   ladder gates this. Fallback position pre-agreed: approach B ("plan as
   data": stiffer `Plan`+`CheckOff`, still one mode).
2. `Update`'s argument-shape polymorphism on 1B models — dispatch errors
   must name the wrong shape as precisely as the wrong-key hints do.
3. The old harness lives only in git history once this lands (the
   src-tauri tree is one big uncommitted batch — commit the current state
   FIRST so the A/B baseline is reproducible).

## Verification

- Unit: Update shape validation + both behaviors; Todo parse/ack/tracker
  snapshot mapping; FinishTask bounce (bounces once, then permits);
  prompt contains single narrative + dialect teaching; grammar enum =
  full set.
- Ladder: old-harness commit vs this, both models, thinking on.
