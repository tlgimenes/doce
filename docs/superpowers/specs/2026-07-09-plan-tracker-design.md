# Plan Tracker: state-driven planning in production + top-right todo UI

**Date:** 2026-07-09
**Status:** Approved (brainstormed with visual companion; mockups in `.superpowers/brainstorm/35349-1783561097/content/`)

## Problem

The state-driven Planning/Executing agent loop (`agent::plan`) exists only as
benchmark wiring (`tests/agent_benchmark.rs`), where it scored 20/20 on the
20-scattered-bugs task versus 2-4/20 for the old design. Production
`send_agent_message` still runs the flat single-prompt loop: no plan is ever
created, persisted, or emitted — so there is nothing for the UI to render, and
benchmark and production exercise different engines.

This feature (1) makes the state-driven engine THE production loop, with the
benchmark importing the same type, and (2) renders the live plan as a todo
tracker in the chat UI.

## Decisions (from brainstorming)

- **Always-on, single engine.** Every `send_agent_message` turn runs the
  two-state loop. Trivial requests answer in plain text straight from
  Planning without creating a plan — no plan, no tracker.
- **UI form: floating top-right tracker** over the transcript's right gutter
  (the message column is centered `max-w-3xl`, so on wide windows the gutter
  is empty space). Not a pinned panel, not an inline transcript widget.
- **Container-query collapse.** Wide container: full card (goal, `n/m`
  counter, per-step checklist). Narrow: a vertical rail of numbered dots
  (✓ done / current ring / pending muted). CSS container queries only — no
  JS measurement.
- **Plan activity is invisible in the transcript.** Plan tool rows persist as
  messages (the model's history needs them) but `MessageContent` never
  renders them. The tracker is the only plan surface.
- **The tracker is live-turn chrome.** Visible while a plan is active in the
  current turn (including after a webview reload mid-turn); fades out when
  the turn completes. Reopening a finished conversation shows no plan trace.

## Architecture

### 1. `PlanState` — the promoted state machine (`agent::plan`)

The unit shared by production and benchmark is the state machine, not the
backend host. New lib type:

```rust
pub struct PlanState {
    pub plan: Plan,                       // existing type: goal + Vec<PlanStep>
    pub state: LoopState,                 // existing type: Planning | Executing { step_index }
    refusal_context: Option<String>,      // set by RefuseStep, consumed by next Planning prompt
}

impl PlanState {
    /// Planning prompt (refusal-annotated when revising) or the per-step
    /// Executing prompt. The caller appends its own cwd line.
    pub fn system_prompt(&mut self) -> String;

    /// Handles CreatePlan / AddStep / ResumeExecution / StepDone /
    /// RefuseStep, and rejects tools not available in the current state.
    /// Returns None when the tool is a regular tool the host should
    /// dispatch itself (Read/Write/Edit/Bash/Grep/Glob/Task/AskUserQuestion,
    /// as gated by the current state).
    pub fn handle_plan_tool(&mut self, call: &ToolCall) -> Option<String>;

    pub fn next_undone_step(&self) -> Option<usize>;
}
```

Semantics are exactly the benchmark's `PlanExecBackend` match (CreatePlan
valid only once per turn; StepDone advances to next undone step or returns to
Planning; RefuseStep records the reason and returns to Planning; regular
tools state-gated: read-only + AskUserQuestion in Planning, file/shell/Task
in Executing).

### 2. Production host (`commands::agent::RealBackend`)

- `RealBackend` gains a `PlanState`, fresh per `send_agent_message` call.
- `generate` replaces `messages[0]` with `plan_state.system_prompt()` + the
  existing cwd suffix before rendering — the flat `SYSTEM_PROMPT` is no
  longer used by the top-level loop.
- `execute_tool` offers each call to `handle_plan_tool` first. Plan tools
  persist their `tool_call`/`tool_result` rows through the existing
  `persist_tool_call_and_result` path (model history reconstruction after
  reload needs them). Non-plan tools fall through to the existing
  dispatch/persistence path unchanged — including `AskUserQuestion`'s real
  pause-and-answer flow in Planning (the benchmark cans it; production
  keeps it) and `Task`'s isolated subagent in Executing.
- Existing 200-turn cap, per-turn compaction, offloading, and context-usage
  events apply unchanged.
- The subagent loop (`SubagentBackend`) stays flat — subagents execute one
  delegated task; they don't plan.

### 3. Live plan surface (the `is_generation_active` pattern)

- **`ActivePlans`**: managed `Mutex<HashMap<String, PlanSnapshot>>`, sibling
  of `ActiveGenerations`.

  ```rust
  #[derive(Serialize, specta::Type, Clone)]
  #[serde(rename_all = "camelCase")]
  pub struct PlanSnapshot {
      pub goal: String,
      pub steps: Vec<PlanStepSnapshot>,   // { description, done }
      pub current_step_index: Option<u32>, // None while Planning
  }
  ```

- On every plan mutation (create/add/resume/step-done/refuse), the backend
  updates the map and emits a **`plan-update`** tauri-specta event:
  `{ conversationId, plan: PlanSnapshot | null }`. Full snapshot every time —
  plans are small; no delta protocol.
- **`get_active_plan(conversation_id) -> Option<PlanSnapshot>`** command for
  mount/reload recovery.
- An RAII guard (sibling of `ActiveGenerationGuard`) clears the map entry on
  every turn exit path and emits a final `plan-update` with `plan: null` —
  the tracker's fade-out signal.
- Crash mid-plan: the in-memory map dies with the process; startup healing
  already marks the turn interrupted; the tracker stays hidden. The
  single-instance guard keeps the map authoritative for the one DB.

### 4. `PlanTracker` component (`src/views/workspace/PlanTracker.tsx`)

- Rendered inside the `StickToBottom` wrapper (already `position: relative`)
  as a sibling of the scroll element — floats over the top-right gutter,
  consumes no layout space, independent of scroll behavior.
- Data: `get_active_plan(conversationId)` on mount; then `plan-update`
  events filtered by conversation. No active plan → renders nothing.
  `plan: null` → fade-out transition, then unmount.
- The chat surface gets `container-type: inline-size`; Tailwind v4
  `@container` variants switch the two forms. Initial breakpoint: card at
  container width ≥ 64rem (the `max-w-3xl` = 48rem column + room for the
  15rem card and margins), rail below it — tune visually during
  implementation.
  - **Card** (wide): goal + `n/m` counter; done steps struck through with a
    green check; current step highlighted; pending muted. Completed steps
    collapse into one "✓ n done" line once the plan exceeds 6 steps;
    visible pending capped at 4 with a "+k more" line.
  - **Rail** (narrow): vertical pill of per-step dots — ✓ filled green,
    current amber ring, pending muted numbered circles. Click toggles the
    full card as a temporary overlay; click-away collapses.
- Both forms render in the DOM with container-variant visibility classes
  (jsdom can't evaluate container queries; tests assert both sub-renders).
- **Transcript invisibility:** `MessageContent` skips rows whose `toolName`
  is one of `CreatePlan`/`AddStep`/`ResumeExecution`/`StepDone`/`RefuseStep`.

## Edge cases

- **Trivial turn:** Planning answers plain text; no `CreatePlan`; tracker
  never appears.
- **Refusal loop:** RefuseStep → Planning with the refusal reason in the
  prompt → AddStep/ResumeExecution; tracker shows the plan growing and the
  current-step marker moving back and forth. `plan-update` fires on each
  mutation.
- **Reload mid-plan:** `get_active_plan` recovers the snapshot (same
  reload-proofing as `is_generation_active`); the composer is already gated
  by `backendTurnActive`.
- **Crash mid-plan:** map gone, healing marks the turn interrupted, tracker
  hidden, next turn starts a fresh plan.
- **Old plan rows in history:** prior turns' plan tool rows remain in model
  history (compaction bounds growth); each turn's `PlanState` is fresh, so
  `CreatePlan` is valid again per turn.
- **Long plans:** card caps visible rows (see §4); the rail shows per-step
  dots up to 12 steps, beyond which it falls back to a single `n/m` chip
  (the chip is overflow behavior only — the numbered-dot rail is the
  default collapsed form, per the mockup selection).

## Testing

- **Rust — `agent::plan`:** `PlanState` unit tests: transitions
  (CreatePlan→ResumeExecution→Executing; StepDone advance/return-to-Planning;
  RefuseStep reason threading), CreatePlan-only-once, state-gated tool
  rejection, `next_undone_step`.
- **Rust — production host:** plan tool rows persisted with correct shapes;
  `ActivePlans` updated per mutation; entry cleared + null event on every
  exit path (RAII); `get_active_plan` command (per `is_generation_active`'s
  test pattern).
- **Benchmark:** `PlanExecBackend` rewritten around the lib's `PlanState` —
  compile-time unification proof; the 20/20 task remains the engine's
  regression net (ignored/manual, as today).
- **Frontend:** `PlanTracker` unit tests (card states, done-collapse line,
  rail dot states, expand/collapse, fade on null); Workspace integration
  (`plan-update` → tracker appears; `null` → gone; mount recovery via
  `get_active_plan`); `MessageContent` skips plan rows.

## Out of scope

- Archival plan view for finished conversations (explicitly declined —
  tracker is live-turn chrome only).
- Plan editing by the user (the plan is the model's; the user steers via
  chat/AskUserQuestion).
- Subagent planning (subagents stay on the flat loop).
- Scheduler integration changes (agent turns keep bypassing the queue, as
  today — pre-existing, unchanged).
