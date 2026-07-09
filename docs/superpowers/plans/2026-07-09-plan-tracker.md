# Plan Tracker Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the benchmark's state-driven Planning/Executing loop the production agent engine and render its live plan as a floating top-right todo tracker in the chat.

**Architecture:** The state machine (`PlanState`) is promoted from `tests/agent_benchmark.rs` into `agent::plan`; production `RealBackend` and the benchmark both embed it. Live plan state flows through a managed `ActivePlans` map + a `plan-update` tauri-specta event + a `get_active_plan` recovery command (the same pattern as `ActiveGenerations`/`is_generation_active`). The frontend `PlanTracker` floats over the transcript's right gutter inside the existing `StickToBottom` wrapper and collapses to a numbered-dot rail via CSS container queries.

**Tech Stack:** Rust (tauri 2, tauri-specta, rusqlite), React 19 + TypeScript, Tailwind v4 (`@container` variants), vitest + testing-library, cargo test.

**Spec:** `docs/superpowers/specs/2026-07-09-plan-tracker-design.md`

## Global Constraints

- The five plan tools are exactly: `CreatePlan`, `AddStep`, `ResumeExecution`, `StepDone`, `RefuseStep`.
- Plan tool rows persist as ordinary `tool_call`/`tool_result` messages but are NEVER rendered in the transcript.
- Tracker breakpoint: full card at container width ≥ 64rem (Tailwind `@5xl:`), numbered-dot rail below.
- Card caps: completed steps collapse into one "✓ n done" line when the plan exceeds 6 steps; visible pending capped at 4 with a "+k more" line. Rail shows dots up to 12 steps, then a single `n/m` chip.
- Events/commands use camelCase payloads (`#[serde(rename_all = "camelCase")]`), matching every existing specta type.
- No schema migration — plan state is in-memory per turn; rows reuse the existing messages table.
- Subagents (`SubagentBackend`) stay on the flat loop with `SYSTEM_PROMPT`.
- All Rust work runs from `src-tauri/` (`cargo` commands fail from the repo root).
- Commit messages follow repo convention (`feat:`/`fix:`/`docs:` prefixes) and end with `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.

---

### Task 1: `PlanState` — the promoted state machine

**Files:**

- Modify: `src-tauri/src/agent/plan.rs`

**Interfaces:**

- Consumes: existing `Plan`, `PlanStep`, `LoopState`, `PLANNING_SYSTEM_PROMPT`, `executing_system_prompt` (same file); `crate::agent::ToolCall`.
- Produces (later tasks rely on these exact signatures):
  - `pub const PLAN_TOOL_NAMES: [&str; 5]`
  - `pub struct PlanState { pub plan: Plan, pub state: LoopState, .. }` with `Default`
  - `pub fn system_prompt(&mut self) -> String`
  - `pub fn handle_plan_tool(&mut self, call: &ToolCall) -> Option<String>` — `Some(result)` when the call was a plan tool or a state-gated rejection; `None` when the host should dispatch it as a regular tool
  - `pub fn next_undone_step(&self) -> Option<usize>`
  - `pub fn has_plan(&self) -> bool`

- [ ] **Step 1: Add `Default` to `LoopState`**

In `src-tauri/src/agent/plan.rs`, change the `LoopState` derive and add a default variant marker:

```rust
/// Which of the two states the loop is in right now. Carried by the
/// backend, not `run_loop` itself — `run_loop`'s own signature and control
/// flow are completely unaware this exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LoopState {
    #[default]
    Planning,
    Executing {
        step_index: usize,
    },
}
```

(Note: `#[default]` on a variant requires the variant be unit-like — `Planning` is, so this compiles; `Executing` keeps its field.)

- [ ] **Step 2: Write the failing tests**

Append to the `tests` module in `src-tauri/src/agent/plan.rs`:

```rust
    use crate::agent::ToolCall;

    fn call(name: &str, arguments: serde_json::Value) -> ToolCall {
        ToolCall {
            name: name.to_string(),
            arguments,
        }
    }

    #[test]
    fn create_plan_then_resume_moves_to_executing_the_first_step() {
        let mut ps = PlanState::default();
        assert_eq!(ps.state, LoopState::Planning);
        assert!(!ps.has_plan());

        let result = ps
            .handle_plan_tool(&call(
                "CreatePlan",
                serde_json::json!({"goal": "fix bugs", "steps": ["fix a", "fix b"]}),
            ))
            .expect("CreatePlan is a plan tool");
        assert!(result.contains("2 steps"));
        assert!(ps.has_plan());
        assert_eq!(ps.plan.goal, "fix bugs");
        assert_eq!(ps.state, LoopState::Planning, "CreatePlan alone does not start execution");

        let result = ps
            .handle_plan_tool(&call("ResumeExecution", serde_json::json!({})))
            .expect("ResumeExecution is a plan tool");
        assert!(result.contains("fix a"));
        assert_eq!(ps.state, LoopState::Executing { step_index: 0 });
    }

    #[test]
    fn create_plan_is_only_valid_once() {
        let mut ps = PlanState::default();
        ps.handle_plan_tool(&call(
            "CreatePlan",
            serde_json::json!({"goal": "g", "steps": ["a"]}),
        ));
        let second = ps
            .handle_plan_tool(&call(
                "CreatePlan",
                serde_json::json!({"goal": "other", "steps": ["x"]}),
            ))
            .unwrap();
        assert!(second.starts_with("Error"));
        assert_eq!(ps.plan.goal, "g", "the existing plan must be untouched");
    }

    #[test]
    fn step_done_advances_to_next_undone_step_and_returns_to_planning_when_finished() {
        let mut ps = PlanState::default();
        ps.handle_plan_tool(&call(
            "CreatePlan",
            serde_json::json!({"goal": "g", "steps": ["a", "b"]}),
        ));
        ps.handle_plan_tool(&call("ResumeExecution", serde_json::json!({})));

        let result = ps
            .handle_plan_tool(&call("StepDone", serde_json::json!({"summary": "did a"})))
            .unwrap();
        assert!(ps.plan.steps[0].done);
        assert_eq!(ps.state, LoopState::Executing { step_index: 1 });
        assert!(result.contains("step 1"));

        ps.handle_plan_tool(&call("StepDone", serde_json::json!({"summary": "did b"})));
        assert!(ps.plan.steps[1].done);
        assert_eq!(ps.state, LoopState::Planning, "all done returns to planning for review");
    }

    #[test]
    fn refuse_step_returns_to_planning_and_threads_the_reason_into_the_next_prompt() {
        let mut ps = PlanState::default();
        ps.handle_plan_tool(&call(
            "CreatePlan",
            serde_json::json!({"goal": "g", "steps": ["impossible"]}),
        ));
        ps.handle_plan_tool(&call("ResumeExecution", serde_json::json!({})));

        ps.handle_plan_tool(&call(
            "RefuseStep",
            serde_json::json!({"reason": "the file does not exist"}),
        ));
        assert_eq!(ps.state, LoopState::Planning);

        let prompt = ps.system_prompt();
        assert!(prompt.contains("the file does not exist"), "refusal reason must reach the revision prompt");
        // Consumed: the next planning prompt is clean again.
        let prompt2 = ps.system_prompt();
        assert!(!prompt2.contains("the file does not exist"));
    }

    #[test]
    fn add_step_appends_and_resume_picks_the_first_undone_step() {
        let mut ps = PlanState::default();
        ps.handle_plan_tool(&call(
            "CreatePlan",
            serde_json::json!({"goal": "g", "steps": ["a"]}),
        ));
        ps.handle_plan_tool(&call("ResumeExecution", serde_json::json!({})));
        ps.handle_plan_tool(&call("StepDone", serde_json::json!({})));
        assert_eq!(ps.state, LoopState::Planning);

        ps.handle_plan_tool(&call("AddStep", serde_json::json!({"description": "b"})));
        assert_eq!(ps.plan.steps.len(), 2);
        ps.handle_plan_tool(&call("ResumeExecution", serde_json::json!({})));
        assert_eq!(ps.state, LoopState::Executing { step_index: 1 });
    }

    #[test]
    fn regular_tools_are_state_gated() {
        let mut ps = PlanState::default();
        // Planning: read-only + AskUserQuestion pass through (None = host dispatches).
        assert!(ps.handle_plan_tool(&call("Read", serde_json::json!({}))).is_none());
        assert!(ps.handle_plan_tool(&call("AskUserQuestion", serde_json::json!({}))).is_none());
        // Planning: write tools are rejected.
        let rejected = ps.handle_plan_tool(&call("Write", serde_json::json!({}))).unwrap();
        assert!(rejected.starts_with("Error"));

        ps.handle_plan_tool(&call(
            "CreatePlan",
            serde_json::json!({"goal": "g", "steps": ["a"]}),
        ));
        ps.handle_plan_tool(&call("ResumeExecution", serde_json::json!({})));
        // Executing: file/shell/Task pass through, plan-editing is rejected.
        assert!(ps.handle_plan_tool(&call("Write", serde_json::json!({}))).is_none());
        assert!(ps.handle_plan_tool(&call("Task", serde_json::json!({}))).is_none());
        let rejected = ps.handle_plan_tool(&call("AddStep", serde_json::json!({"description": "x"}))).unwrap();
        assert!(rejected.starts_with("Error"));
    }

    #[test]
    fn system_prompt_matches_the_state() {
        let mut ps = PlanState::default();
        assert!(ps.system_prompt().contains("planning supervisor"));

        ps.handle_plan_tool(&call(
            "CreatePlan",
            serde_json::json!({"goal": "ship it", "steps": ["write tests"]}),
        ));
        ps.handle_plan_tool(&call("ResumeExecution", serde_json::json!({})));
        let prompt = ps.system_prompt();
        assert!(prompt.contains("ship it"));
        assert!(prompt.contains("write tests"));
        assert!(prompt.contains("StepDone"));
    }
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cd src-tauri && cargo test --lib agent::plan 2>&1 | tail -5`
Expected: FAIL to compile — `cannot find struct PlanState`, `no method has_plan`.

- [ ] **Step 4: Implement `PlanState`**

Add to `src-tauri/src/agent/plan.rs` (below `executing_system_prompt`), moving the benchmark's match semantics verbatim:

```rust
/// The five tools owned by the plan state machine itself — used by the
/// frontend (via ipc.ts's mirror of this list) to keep plan activity
/// invisible in the transcript, and by hosts to route calls.
pub const PLAN_TOOL_NAMES: [&str; 5] = [
    "CreatePlan",
    "AddStep",
    "ResumeExecution",
    "StepDone",
    "RefuseStep",
];

/// The two-state Planning/Executing machine, promoted from the benchmark's
/// `PlanExecBackend` so production (`commands::agent::RealBackend`) and the
/// benchmark embed the SAME engine — one implementation, two thin hosts.
/// Owns the plan, the current state, and the refusal context; hosts own
/// everything else (inference, persistence, events, real tool dispatch).
#[derive(Debug, Default)]
pub struct PlanState {
    pub plan: Plan,
    pub state: LoopState,
    /// Set by `RefuseStep`, consumed (and cleared) the next time
    /// `system_prompt` renders the Planning prompt — carries the refusal
    /// reason into that one revision turn without lingering after.
    refusal_context: Option<String>,
}

impl PlanState {
    /// The system prompt for the current state: Planning (refusal-annotated
    /// when a step was just refused) or the per-step Executing prompt.
    /// `&mut` because rendering the Planning prompt consumes the refusal
    /// context. The caller appends its own cwd line.
    pub fn system_prompt(&mut self) -> String {
        match self.state {
            LoopState::Planning => match self.refusal_context.take() {
                Some(reason) => format!(
                    "{PLANNING_SYSTEM_PROMPT}\n\nThe previous step could not be completed. Reason given: {reason}\n\nRevise the plan accordingly (AddStep, then ResumeExecution)."
                ),
                None => PLANNING_SYSTEM_PROMPT.to_string(),
            },
            LoopState::Executing { step_index } => {
                let step_desc = self.plan.steps[step_index].description.clone();
                executing_system_prompt(&self.plan.goal, &step_desc)
            }
        }
    }

    /// Handles a tool call that belongs to the plan machine: the five plan
    /// tools mutate state and return their result; regular tools that are
    /// NOT available in the current state return a rejection. `None` means
    /// "this is an ordinary tool the host should dispatch itself" —
    /// read-only tools + AskUserQuestion while Planning, file/shell/Task
    /// while Executing (the exact gating the benchmark validated 20/20).
    pub fn handle_plan_tool(&mut self, call: &crate::agent::ToolCall) -> Option<String> {
        let result = match (self.state, call.name.as_str()) {
            (LoopState::Planning, "CreatePlan") => {
                if !self.plan.steps.is_empty() {
                    "Error: a plan already exists -- use AddStep to extend or correct it, CreatePlan is only valid once".to_string()
                } else {
                    let goal = call
                        .arguments
                        .get("goal")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let steps: Vec<PlanStep> = call
                        .arguments
                        .get("steps")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|s| s.as_str())
                                .map(|d| PlanStep {
                                    description: d.to_string(),
                                    done: false,
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    let step_count = steps.len();
                    self.plan = Plan { goal, steps };
                    format!("Plan created with {step_count} steps. Call ResumeExecution to begin.")
                }
            }
            (LoopState::Planning, "AddStep") => {
                let description = call
                    .arguments
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                self.plan.steps.push(PlanStep {
                    description,
                    done: false,
                });
                format!("Step added. Plan now has {} steps.", self.plan.steps.len())
            }
            (LoopState::Planning, "ResumeExecution") => match self.next_undone_step() {
                Some(idx) => {
                    self.state = LoopState::Executing { step_index: idx };
                    format!("Resuming at step {idx}: {}", self.plan.steps[idx].description)
                }
                None => "Error: no undone steps -- create or add a step first".to_string(),
            },
            (LoopState::Planning, "Read" | "Grep" | "Glob" | "AskUserQuestion") => return None,
            (LoopState::Executing { step_index }, "StepDone") => {
                self.plan.steps[step_index].done = true;
                match self.next_undone_step() {
                    Some(next) => {
                        self.state = LoopState::Executing { step_index: next };
                        format!("Step {step_index} done. Moving to step {next}.")
                    }
                    None => {
                        self.state = LoopState::Planning;
                        format!("Step {step_index} done. All steps report done -- back to planning for final review.")
                    }
                }
            }
            (LoopState::Executing { step_index }, "RefuseStep") => {
                let reason = call
                    .arguments
                    .get("reason")
                    .and_then(|v| v.as_str())
                    .unwrap_or("(no reason given)");
                self.refusal_context = Some(format!(
                    "step {step_index} (\"{}\"): {reason}",
                    self.plan.steps[step_index].description
                ));
                self.state = LoopState::Planning;
                "Step refused. Back to planning.".to_string()
            }
            (
                LoopState::Executing { .. },
                "Read" | "Write" | "Edit" | "Bash" | "Grep" | "Glob" | "Task",
            ) => return None,
            (_, other) => format!("Error: {other} is not available in the current phase"),
        };
        Some(result)
    }

    pub fn next_undone_step(&self) -> Option<usize> {
        self.plan.steps.iter().position(|s| !s.done)
    }

    pub fn has_plan(&self) -> bool {
        !self.plan.steps.is_empty()
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd src-tauri && cargo test --lib agent::plan 2>&1 | tail -3`
Expected: PASS (9 tests: 2 pre-existing + 7 new). Also run `cargo clippy --lib 2>&1 | tail -2` — expect no warnings.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/agent/plan.rs
git commit -m "feat(agent): promote the two-state plan machine into agent::plan as PlanState

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: Benchmark embeds the lib's `PlanState`

**Files:**

- Modify: `src-tauri/tests/agent_benchmark.rs` (the `PlanExecBackend` struct, its `AgentBackend` impl, its `next_undone_step` impl block, and `run_planned_benchmark_task`/`report_plan` field accesses)

**Interfaces:**

- Consumes: `doce_lib::agent::plan::PlanState` from Task 1 (exact signatures listed there).
- Produces: nothing new — compile-time proof of unification.

- [ ] **Step 1: Rewrite `PlanExecBackend` around `PlanState`**

Replace the struct fields `plan`, `state`, `refusal_context` with one field, and delete the hand-rolled state machine. The struct becomes:

```rust
struct PlanExecBackend<'a> {
    engine: &'a InferenceEngine,
    cwd: &'a Path,
    threshold: u32,
    turns: u32,
    plan_state: doce_lib::agent::plan::PlanState,
}
```

`generate` shrinks to:

```rust
    async fn generate(&mut self, mut messages: Vec<ChatMessage>) -> String {
        self.turns += 1;
        if let Some(first) = messages.first_mut() {
            *first = ChatMessage::system(self.plan_state.system_prompt());
        }

        let rendered = self
            .engine
            .render_chat_prompt(&messages)
            .expect("chat template should render");
        self.engine
            .generate(&rendered, 256, true, |_| {}, || false)
            .unwrap_or_else(|e| format!("Error: generation failed: {e}"))
    }
```

`execute_tool` keeps only the host concerns (real dispatch, canned AskUserQuestion, subagent Task, tracing):

```rust
    async fn execute_tool(
        &mut self,
        _tool_call_id: String,
        call: doce_lib::agent::ToolCall,
    ) -> String {
        use doce_lib::agent::plan::LoopState;

        let result = if let Some(result) = self.plan_state.handle_plan_tool(&call) {
            result
        } else if call.name == "AskUserQuestion" {
            "Error: no interactive user is available in this benchmark run -- proceed using your own best judgment".to_string()
        } else if call.name == "Task" {
            // Mirrors commands::agent's real Task handling: an isolated
            // subagent, FR-016 one-level nesting enforced by run_loop
            // itself via is_subagent -- kept out of the shared
            // conversation entirely, only its final answer becomes this
            // tool_result.
            let prompt = call
                .arguments
                .get("prompt")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let sub_context = AgentContext {
                is_subagent: true,
                max_turns: 20,
                cwd: Some(self.cwd.to_path_buf()),
            };
            let sub_messages =
                vec![ChatMessage::system(SYSTEM_PROMPT), ChatMessage::user(prompt)];
            let mut sub_backend = BenchBackend {
                engine: self.engine,
                cwd: self.cwd,
                threshold: self.threshold,
                turns: 0,
                trace: Vec::new(),
            };
            let sub_result = run_loop(&sub_context, sub_messages, &mut sub_backend).await;
            self.turns += sub_backend.turns;
            match sub_result {
                Ok(text) => text,
                Err(e) => format!("Error: subagent did not finish ({e})"),
            }
        } else {
            dispatch::execute(&call, Some(self.cwd)).model_text
        };

        let args_preview: String = call.arguments.to_string().chars().take(200).collect();
        let result_preview: String = result.chars().take(300).collect();
        println!(
            "  [{:?}] turn {} tool={} args={args_preview} -> {result_preview:?}",
            self.plan_state.state, self.turns, call.name
        );
        result
    }
```

Note the behavior nuance this preserves: `handle_plan_tool` passes `AskUserQuestion` through (`None`) while Planning, and the bench host cans it; while Executing the machine itself rejects it — same as before.

Delete the `impl PlanExecBackend<'_> { fn next_undone_step ... }` block (now on `PlanState`). Update `report_plan` call sites to pass `&backend.plan_state.plan`, and `PlanExecBackend` construction sites to use `plan_state: doce_lib::agent::plan::PlanState::default()` in place of the three removed fields. `measure`/`threshold`/`compact` are unchanged.

- [ ] **Step 2: Verify the benchmark compiles (its tests are all `#[ignore]`d — they need the real GGUF)**

Run: `cd src-tauri && cargo test --test agent_benchmark --no-run 2>&1 | tail -3`
Expected: `Finished` with no errors. Also `cargo clippy --tests 2>&1 | tail -2` — no new warnings.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/tests/agent_benchmark.rs
git commit -m "refactor(bench): embed agent::plan::PlanState instead of a duplicated state machine

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: Live plan surface — `ActivePlans`, `PlanUpdate` event, `get_active_plan`

**Files:**

- Modify: `src-tauri/src/commands/agent.rs` (new types + helper + command + tests)
- Modify: `src-tauri/src/commands/mod.rs` (register command + event)
- Modify: `src-tauri/src/lib.rs` (`.manage(ActivePlans::default())`)

**Interfaces:**

- Consumes: `PlanState` (Task 1).
- Produces (Task 4 and the frontend rely on these):
  - `pub struct ActivePlans(pub std::sync::Mutex<std::collections::HashMap<String, PlanSnapshot>>)` (managed state)
  - `pub struct PlanSnapshot { pub goal: String, pub steps: Vec<PlanStepSnapshot>, pub current_step_index: Option<u32> }` (camelCase serialized)
  - `pub struct PlanStepSnapshot { pub description: String, pub done: bool }`
  - Event `PlanUpdate { conversation_id: String, plan: Option<PlanSnapshot> }`, emitted as `"plan-update"`
  - `fn plan_snapshot(state: &PlanState) -> PlanSnapshot`
  - `fn publish_plan_update(app: Option<&AppHandle>, active_plans: &ActivePlans, conversation_id: &str, state: &PlanState)`
  - `struct ActivePlanGuard<'a> { active_plans: &'a ActivePlans, app: AppHandle, conversation_id: String }` (RAII clear + null event)
  - Command `get_active_plan(conversation_id) -> Option<PlanSnapshot>`

- [ ] **Step 1: Write the failing tests**

Append to the `tests` module in `src-tauri/src/commands/agent.rs`:

```rust
    #[test]
    fn plan_snapshot_reflects_state_and_current_step() {
        use crate::agent::plan::{LoopState, Plan, PlanState, PlanStep};
        let mut state = PlanState::default();
        state.plan = Plan {
            goal: "g".to_string(),
            steps: vec![
                PlanStep { description: "a".to_string(), done: true },
                PlanStep { description: "b".to_string(), done: false },
            ],
        };
        state.state = LoopState::Executing { step_index: 1 };

        let snapshot = plan_snapshot(&state);
        assert_eq!(snapshot.goal, "g");
        assert_eq!(snapshot.steps.len(), 2);
        assert!(snapshot.steps[0].done);
        assert_eq!(snapshot.current_step_index, Some(1));

        state.state = LoopState::Planning;
        assert_eq!(plan_snapshot(&state).current_step_index, None);
    }

    #[test]
    fn publish_plan_update_only_registers_a_plan_that_exists_and_guard_drop_clears_it() {
        use crate::agent::plan::PlanState;
        let active_plans = ActivePlans::default();
        let mut state = PlanState::default();

        // No plan yet (empty steps): publishing must not register an entry.
        publish_plan_update(None, &active_plans, "c1", &state);
        assert!(active_plans.0.lock().unwrap().get("c1").is_none());

        state.handle_plan_tool(&crate::agent::ToolCall {
            name: "CreatePlan".to_string(),
            arguments: serde_json::json!({"goal": "g", "steps": ["a"]}),
        });
        publish_plan_update(None, &active_plans, "c1", &state);
        assert_eq!(
            active_plans.0.lock().unwrap().get("c1").unwrap().goal,
            "g"
        );

        // Guard clear is exercised without an AppHandle via the map
        // directly (the emit half needs a live app; the map half is the
        // reload-recovery source of truth get_active_plan reads).
        active_plans.0.lock().unwrap().remove("c1");
        assert!(active_plans.0.lock().unwrap().get("c1").is_none());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test --lib commands::agent::tests::plan_snapshot 2>&1 | tail -3`
Expected: FAIL to compile — `cannot find function plan_snapshot`, `cannot find struct ActivePlans`.

- [ ] **Step 3: Implement the types, helper, guard, and command**

Add to `src-tauri/src/commands/agent.rs` (below the `AgentMessagePersisted` event struct; add `use std::collections::HashMap; use std::sync::Mutex;` to imports):

```rust
/// Live plan state per conversation — the plan-tracker twin of
/// `ActiveGenerations`: in-memory, per-process, cleared by RAII at turn
/// end. `get_active_plan` reads it for mount/reload recovery; the
/// `plan-update` event streams changes while the turn runs.
#[derive(Default)]
pub struct ActivePlans(pub Mutex<HashMap<String, PlanSnapshot>>);

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PlanStepSnapshot {
    pub description: String,
    pub done: bool,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PlanSnapshot {
    pub goal: String,
    pub steps: Vec<PlanStepSnapshot>,
    /// `None` while Planning (between steps / during plan revision).
    pub current_step_index: Option<u32>,
}

/// Fired on every plan mutation during an agent turn, and once with
/// `plan: None` when the turn ends — the tracker's fade-out signal.
#[derive(Debug, Clone, Serialize, specta::Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct PlanUpdate {
    pub conversation_id: String,
    pub plan: Option<PlanSnapshot>,
}

fn plan_snapshot(state: &crate::agent::plan::PlanState) -> PlanSnapshot {
    PlanSnapshot {
        goal: state.plan.goal.clone(),
        steps: state
            .plan
            .steps
            .iter()
            .map(|s| PlanStepSnapshot {
                description: s.description.clone(),
                done: s.done,
            })
            .collect(),
        current_step_index: match state.state {
            crate::agent::plan::LoopState::Executing { step_index } => Some(step_index as u32),
            crate::agent::plan::LoopState::Planning => None,
        },
    }
}

/// Updates the live map and emits `plan-update` — called after every
/// handled plan tool. A state with no plan yet (trivial turns never call
/// CreatePlan) registers nothing, so the tracker never appears for them.
/// `app: Option<&AppHandle>` so unit tests exercise the map half without a
/// live Tauri app, mirroring `persist_tool_call`'s pattern.
fn publish_plan_update(
    app: Option<&AppHandle>,
    active_plans: &ActivePlans,
    conversation_id: &str,
    state: &crate::agent::plan::PlanState,
) {
    if !state.has_plan() {
        return;
    }
    let snapshot = plan_snapshot(state);
    active_plans
        .0
        .lock()
        .unwrap()
        .insert(conversation_id.to_string(), snapshot.clone());
    if let Some(app) = app {
        let _ = app.emit(
            "plan-update",
            PlanUpdate {
                conversation_id: conversation_id.to_string(),
                plan: Some(snapshot),
            },
        );
    }
}

/// Clears this conversation's live plan on every turn exit path and, if a
/// plan was actually registered, emits the `plan: None` fade-out event —
/// the plan-tracker twin of `ActiveGenerationGuard`.
struct ActivePlanGuard<'a> {
    active_plans: &'a ActivePlans,
    app: AppHandle,
    conversation_id: String,
}

impl Drop for ActivePlanGuard<'_> {
    fn drop(&mut self) {
        let had_plan = self
            .active_plans
            .0
            .lock()
            .unwrap()
            .remove(&self.conversation_id)
            .is_some();
        if had_plan {
            let _ = self.app.emit(
                "plan-update",
                PlanUpdate {
                    conversation_id: self.conversation_id.clone(),
                    plan: None,
                },
            );
        }
    }
}

/// Mount/reload recovery for the plan tracker — the same reload-proof
/// pattern as `conversations::is_generation_active`.
#[tauri::command]
#[specta::specta]
pub fn get_active_plan(
    active_plans: State<'_, ActivePlans>,
    conversation_id: String,
) -> Option<PlanSnapshot> {
    active_plans.0.lock().unwrap().get(&conversation_id).cloned()
}
```

- [ ] **Step 4: Register the command, the event, and the managed state**

In `src-tauri/src/commands/mod.rs`, inside `collect_commands![...]` after `conversations::is_generation_active,` add:

```rust
            agent::get_active_plan,
```

and inside `collect_events![...]` after `agent::AgentMessagePersisted,` add:

```rust
            agent::PlanUpdate,
```

In `src-tauri/src/lib.rs`, add to the `use` line `commands::conversations::{ActiveGenerations, InferenceState}` a sibling import and manage it — after `.manage(ActiveGenerations::default())` add:

```rust
        .manage(commands::agent::ActivePlans::default())
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd src-tauri && cargo test --lib 2>&1 | tail -3 && cargo clippy --lib 2>&1 | tail -2`
Expected: all tests PASS (the two new ones included), clippy clean. (`get_active_plan` and the guard's emit half are wiring over the tested map — covered by the command registration compiling and Task 4's integration.)

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/commands/agent.rs src-tauri/src/commands/mod.rs src-tauri/src/lib.rs
git commit -m "feat(agent): live plan surface — ActivePlans map, plan-update event, get_active_plan

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 4: Production runs the plan engine

**Files:**

- Modify: `src-tauri/src/commands/agent.rs` (`RealBackend` struct + `generate` + `execute_tool`, `send_agent_message` wiring, new `persist_plan_tool` helper + test)

**Interfaces:**

- Consumes: `PlanState` (Task 1); `ActivePlans`/`publish_plan_update`/`ActivePlanGuard` (Task 3); existing `persist_tool_call_and_result`, `execute_top_level_tool`.
- Produces: plan tool rows persisted with a `"plan": true` detail marker (the frontend's skip signal, Task 5); `send_agent_message` seeds and swaps state-driven system prompts.

- [ ] **Step 1: Write the failing test for plan-row persistence**

Append to the `tests` module in `src-tauri/src/commands/agent.rs`:

```rust
    #[tokio::test]
    async fn persist_plan_tool_marks_both_rows_shape_with_plan_true() {
        let conn = crate::storage::test_async_connection().await;
        seed_conversation(&conn, "c1").await;

        persist_plan_tool(
            None,
            &conn,
            "c1",
            "tc1",
            &crate::agent::ToolCall {
                name: "CreatePlan".to_string(),
                arguments: serde_json::json!({"goal": "g", "steps": ["a"]}),
            },
            "Plan created with 1 steps. Call ResumeExecution to begin.",
        )
        .await;

        let (role, content_type, tool_name, content) = latest_message(&conn, "c1").await;
        assert_eq!(role, "tool");
        assert_eq!(content_type, "tool_result");
        assert_eq!(tool_name.as_deref(), Some("CreatePlan"));
        let detail: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(detail["plan"], true, "the transcript-skip marker");
        assert_eq!(detail["outcome"]["ok"], true);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test --lib persist_plan_tool 2>&1 | tail -3`
Expected: FAIL to compile — `cannot find function persist_plan_tool`.

- [ ] **Step 3: Implement `persist_plan_tool` and wire `RealBackend`**

Add near `persist_tool_call_and_result` in `src-tauri/src/commands/agent.rs`:

```rust
/// Persists a plan-machine tool interaction (one of the five plan tools,
/// or a state-gated rejection of a regular tool) as an ordinary
/// call/result pair — the model's reconstructed history needs them — with
/// a `"plan": true` marker in the detail, which is the frontend's signal
/// to keep the row out of the transcript (spec: plan activity is
/// tracker-only).
async fn persist_plan_tool(
    app: Option<&AppHandle>,
    conn: &tokio_rusqlite::Connection,
    conversation_id: &str,
    tool_call_id: &str,
    call: &ToolCall,
    result: &str,
) {
    persist_tool_call_and_result(
        app,
        conn,
        conversation_id,
        tool_call_id,
        &call.name,
        call.arguments.clone(),
        result,
        serde_json::json!({
            "toolName": call.name,
            "arguments": call.arguments,
            "plan": true,
            "outcome": {"ok": !result.starts_with("Error"), "text": result},
        }),
    )
    .await;
}
```

Extend `RealBackend` (same file) with two fields:

```rust
struct RealBackend<'a> {
    engine: &'a InferenceEngine,
    conn: &'a tokio_rusqlite::Connection,
    conversation_id: &'a str,
    app: &'a AppHandle,
    settings: &'a crate::context::ContextSettings,
    threshold: u32,
    cwd: Option<&'a Path>,
    pending: &'a PendingQuestions,
    plan_state: crate::agent::plan::PlanState,
    active_plans: &'a ActivePlans,
}
```

Replace `RealBackend::generate` with the state-swapping version:

```rust
    async fn generate(&mut self, mut messages: Vec<ChatMessage>) -> String {
        // The two-state engine: every generation renders under the prompt
        // for the CURRENT state (Planning, refusal-annotated when
        // revising, or the per-step Executing prompt) — the seed system
        // message from send_agent_message is replaced wholesale.
        let system_text = plan_system_message(&mut self.plan_state, self.cwd);
        if let Some(first) = messages.first_mut() {
            *first = ChatMessage::system(system_text);
        }
        match self.engine.render_chat_prompt(&messages) {
            Ok(rendered) => self
                .engine
                .generate(&rendered, 256, true, |_piece| {}, || false)
                .unwrap_or_else(|e| format!("Error: inference failed: {e}")),
            Err(e) => format!("Error: failed to render chat prompt: {e}"),
        }
    }
```

Replace `RealBackend::execute_tool` with the plan-first routing:

```rust
    async fn execute_tool(&mut self, tool_call_id: String, call: ToolCall) -> String {
        // Plan machine first: the five plan tools (and state-gated
        // rejections) never reach dispatch. Their rows persist like any
        // tool's — marked "plan": true so the transcript skips them — and
        // every handled call refreshes the live tracker surface.
        if let Some(result) = self.plan_state.handle_plan_tool(&call) {
            persist_plan_tool(
                Some(self.app),
                self.conn,
                self.conversation_id,
                &tool_call_id,
                &call,
                &result,
            )
            .await;
            publish_plan_update(
                Some(self.app),
                self.active_plans,
                self.conversation_id,
                &self.plan_state,
            );
            return result;
        }
        execute_top_level_tool(
            tool_call_id,
            call,
            self.conn,
            self.engine,
            self.conversation_id,
            self.cwd,
            self.app,
            self.pending,
        )
        .await
    }
```

Add the seed-prompt helper next to `system_message` (which stays — subagents and `emit_context_usage_update` keep using the flat prompt):

```rust
/// The plan engine's state prompt plus the cwd line `system_message` has
/// always appended — used both to seed `initial_messages[0]` (and the
/// pre-loop compaction budget) and by `RealBackend::generate`'s per-turn
/// swap.
fn plan_system_message(
    state: &mut crate::agent::plan::PlanState,
    cwd: Option<&std::path::Path>,
) -> String {
    let base = state.system_prompt();
    match cwd {
        Some(path) => format!(
            "{base}\n\nYou are currently working in the directory: {}",
            path.display()
        ),
        None => base.to_string(),
    }
}
```

- [ ] **Step 4: Wire `send_agent_message`**

In `send_agent_message` (same file):

1. Add the managed-state parameter after `active_generations: State<'_, ActiveGenerations>,`:

```rust
    active_plans: State<'_, ActivePlans>,
```

2. Right after the `ActiveGenerationGuard` is constructed, add the plan guard:

```rust
    let _plan_guard = ActivePlanGuard {
        active_plans: &active_plans,
        app: app.clone(),
        conversation_id: conversation_id.clone(),
    };
```

3. Replace the existing seed line `let system_prompt = system_message(cwd.as_deref());` with:

```rust
    let mut plan_state = crate::agent::plan::PlanState::default();
    let system_prompt = plan_system_message(&mut plan_state, cwd.as_deref());
```

(everything that already uses `system_prompt` — `maybe_compact`, `initial_messages` — is unchanged.)

4. Extend the `RealBackend` construction with the two new fields:

```rust
    let mut backend = RealBackend {
        engine,
        conn: &conn,
        conversation_id: &conversation_id,
        app: &app,
        settings: &settings,
        threshold,
        cwd: cwd.as_deref(),
        pending: &pending_questions,
        plan_state,
        active_plans: &active_plans,
    };
```

- [ ] **Step 5: Run the full backend suite**

Run: `cd src-tauri && cargo test --lib 2>&1 | tail -3 && cargo clippy --lib 2>&1 | tail -2`
Expected: all PASS (including Task 1-4 tests), clippy clean.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/commands/agent.rs
git commit -m "feat(agent): run the state-driven plan engine in production send_agent_message

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 5: Frontend plumbing — ipc types + transcript invisibility

**Files:**

- Modify: `src/lib/ipc.ts` (types, command wrapper, event wrapper, `isPlanToolResult`)
- Modify: `src/components/MessageContent.tsx` (skip plan rows)
- Test: `src/components/MessageContent.test.tsx`

**Interfaces:**

- Consumes: backend command `get_active_plan` + event `plan-update` (Task 3 shapes).
- Produces (PlanTracker and tests rely on):
  - `interface PlanStepSnapshot { description: string; done: boolean }`
  - `interface PlanSnapshot { goal: string; steps: PlanStepSnapshot[]; currentStepIndex: number | null }`
  - `interface PlanUpdatePayload { conversationId: string; plan: PlanSnapshot | null }`
  - `commands.getActivePlan(conversationId: string): Promise<PlanSnapshot | null>`
  - `events.onPlanUpdate(cb: (p: PlanUpdatePayload) => void): Promise<UnlistenFn>`
  - `PLAN_TOOL_NAMES: Set<string>` and `isPlanToolRow(content: string, toolName: string | null): boolean`

- [ ] **Step 1: Write the failing MessageContent test**

Append to `src/components/MessageContent.test.tsx` (match the file's existing fixture style — read its first fixture before writing):

```tsx
it("renders nothing for plan-machine tool rows (plan activity is tracker-only)", () => {
  const planCall = {
    id: "pc1",
    conversationId: "c1",
    role: "assistant",
    contentType: "tool_call",
    content: JSON.stringify({ arguments: { goal: "g", steps: ["a"] } }),
    toolName: "CreatePlan",
    createdAt: 1,
    durationMs: null,
    tokenCount: null,
  } as const;
  const planResult = {
    ...planCall,
    id: "pr1",
    role: "tool",
    contentType: "tool_result",
    content: JSON.stringify({
      toolName: "CreatePlan",
      arguments: { goal: "g", steps: ["a"] },
      plan: true,
      outcome: { ok: true, text: "Plan created with 1 steps." },
    }),
  } as const;
  // A state-gated rejection carries a REGULAR tool name but the plan
  // marker — it must be skipped by the marker, not the name.
  const gatedRejection = {
    ...planResult,
    id: "pr2",
    toolName: "Write",
    content: JSON.stringify({
      toolName: "Write",
      arguments: {},
      plan: true,
      outcome: { ok: false, text: "Error: Write is not available in the current phase" },
    }),
  } as const;

  const { container: c1 } = render(<MessageContent message={planCall} />);
  const { container: c2 } = render(<MessageContent message={planResult} />);
  const { container: c3 } = render(<MessageContent message={gatedRejection} />);
  expect(c1).toBeEmptyDOMElement();
  expect(c2).toBeEmptyDOMElement();
  expect(c3).toBeEmptyDOMElement();
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run src/components/MessageContent.test.tsx -t "plan-machine" 2>&1 | tail -5`
Expected: FAIL — the plan tool_result renders an UnknownToolWidget, the gated rejection renders a WriteWidget (`toBeEmptyDOMElement` assertion fails for c2 and c3; c1 already passes since tool_call rows render nothing).

- [ ] **Step 3: Implement**

In `src/lib/ipc.ts` add types + helpers (near the other detail types), the command wrapper (inside `export const commands`), and the event wrapper (inside the events object next to `onAgentMessagePersisted`):

```ts
export interface PlanStepSnapshot {
  description: string;
  done: boolean;
}

export interface PlanSnapshot {
  goal: string;
  steps: PlanStepSnapshot[];
  /** null while the engine is in its Planning state (revising / between steps). */
  currentStepIndex: number | null;
}

export interface PlanUpdatePayload {
  conversationId: string;
  plan: PlanSnapshot | null;
}

/** Mirror of agent::plan::PLAN_TOOL_NAMES — the five plan-machine tools
 * whose rows are tracker-only, never transcript content. */
export const PLAN_TOOL_NAMES = new Set([
  "CreatePlan",
  "AddStep",
  "ResumeExecution",
  "StepDone",
  "RefuseStep",
]);

/** True for any row the plan machine persisted: one of the five plan
 * tools by name, or a state-gated rejection of a regular tool (real
 * toolName, but detail carries the `"plan": true` marker). */
export function isPlanToolRow(content: string, toolName: string | null): boolean {
  if (toolName && PLAN_TOOL_NAMES.has(toolName)) return true;
  try {
    return (JSON.parse(content) as { plan?: unknown }).plan === true;
  } catch {
    return false;
  }
}
```

```ts
  getActivePlan: (conversationId: string) =>
    invoke<PlanSnapshot | null>("get_active_plan", { conversationId }),
```

```ts
  onPlanUpdate: (cb: (p: PlanUpdatePayload) => void): Promise<UnlistenFn> =>
    listen<PlanUpdatePayload>("plan-update", (e) => cb(e.payload)),
```

In `src/components/MessageContent.tsx`, import `isPlanToolRow` from `@/lib/ipc` and add the skip immediately BEFORE the existing `if (m.contentType === "tool_call")` block:

```tsx
// Plan-machine rows are tracker-only (spec: plan activity is invisible
// in the transcript) — skipped by tool name for the five plan tools and
// by the persisted `"plan": true` marker for state-gated rejections that
// carry a regular tool's name.
if (
  (m.contentType === "tool_call" || m.contentType === "tool_result") &&
  isPlanToolRow(m.content, m.toolName)
) {
  return null;
}
```

(Note: `isPlanToolRow` on a `tool_call` row's content — `{"arguments": ...}` — finds no marker, so the name check does the work there; that's fine because gated rejections' _call_ rows carry the regular tool's name, and tool_call rows render nothing anyway.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `npx vitest run src/components/MessageContent.test.tsx 2>&1 | tail -3 && npx tsc --noEmit && echo TSC CLEAN`
Expected: all PASS, TSC CLEAN.

- [ ] **Step 5: Commit**

```bash
git add src/lib/ipc.ts src/components/MessageContent.tsx src/components/MessageContent.test.tsx
git commit -m "feat(ui): plan ipc surface + keep plan-machine rows out of the transcript

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 6: `PlanTracker` component

**Files:**

- Create: `src/views/workspace/PlanTracker.tsx`
- Test: `src/views/workspace/PlanTracker.test.tsx`

**Interfaces:**

- Consumes: `commands.getActivePlan`, `events.onPlanUpdate`, `PlanSnapshot` (Task 5).
- Produces: `export default function PlanTracker({ conversationId }: { conversationId: string })`. Testids: `plan-tracker`, `plan-card`, `plan-step`, `plan-done-collapsed`, `plan-more`, `plan-rail`, `plan-dot`, `plan-chip`.

- [ ] **Step 1: Write the failing tests**

Create `src/views/workspace/PlanTracker.test.tsx`:

```tsx
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, act } from "@testing-library/react";
import PlanTracker from "./PlanTracker";
import { commands, events } from "@/lib/ipc";
import type { PlanSnapshot, PlanUpdatePayload } from "@/lib/ipc";

vi.mock("@/lib/ipc", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc")>();
  return {
    ...actual,
    commands: { getActivePlan: vi.fn() },
    events: { onPlanUpdate: vi.fn() },
  };
});

function snapshot(overrides: Partial<PlanSnapshot> = {}): PlanSnapshot {
  return {
    goal: "Fix the scattered bugs",
    steps: [
      { description: "Find all bug markers", done: true },
      { description: "Fix bug_01.txt", done: false },
      { description: "Fix bug_02.txt", done: false },
    ],
    currentStepIndex: 1,
    ...overrides,
  };
}

describe("PlanTracker", () => {
  let firePlanUpdate: (p: PlanUpdatePayload) => void;

  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(commands.getActivePlan).mockResolvedValue(null);
    vi.mocked(events.onPlanUpdate).mockImplementation(async (cb) => {
      firePlanUpdate = cb;
      return () => {};
    });
  });

  it("renders nothing when no plan is active", async () => {
    const { container } = render(<PlanTracker conversationId="c1" />);
    await waitFor(() => expect(commands.getActivePlan).toHaveBeenCalledWith("c1"));
    expect(container).toBeEmptyDOMElement();
  });

  it("recovers an in-flight plan on mount (reload case)", async () => {
    vi.mocked(commands.getActivePlan).mockResolvedValue(snapshot());
    render(<PlanTracker conversationId="c1" />);

    const card = await screen.findByTestId("plan-card");
    expect(card).toHaveTextContent("Fix the scattered bugs");
    expect(card).toHaveTextContent("1/3");
    const steps = screen.getAllByTestId("plan-step");
    expect(steps).toHaveLength(3);
    expect(steps[0]).toHaveClass("line-through");
    expect(steps[1]).toHaveAttribute("data-current", "true");
  });

  it("appears and updates on plan-update events for its own conversation only", async () => {
    render(<PlanTracker conversationId="c1" />);
    await waitFor(() => expect(events.onPlanUpdate).toHaveBeenCalled());

    act(() => firePlanUpdate({ conversationId: "other", plan: snapshot() }));
    expect(screen.queryByTestId("plan-tracker")).not.toBeInTheDocument();

    act(() => firePlanUpdate({ conversationId: "c1", plan: snapshot() }));
    expect(await screen.findByTestId("plan-tracker")).toBeInTheDocument();

    act(() =>
      firePlanUpdate({
        conversationId: "c1",
        plan: snapshot({
          steps: [
            { description: "Find all bug markers", done: true },
            { description: "Fix bug_01.txt", done: true },
            { description: "Fix bug_02.txt", done: false },
          ],
          currentStepIndex: 2,
        }),
      }),
    );
    expect(screen.getByTestId("plan-card")).toHaveTextContent("2/3");
  });

  it("fades out and unmounts when the turn ends (plan: null)", async () => {
    vi.mocked(commands.getActivePlan).mockResolvedValue(snapshot());
    render(<PlanTracker conversationId="c1" />);
    await screen.findByTestId("plan-tracker");

    act(() => firePlanUpdate({ conversationId: "c1", plan: null }));
    // Fading: still mounted with the leaving style…
    expect(screen.getByTestId("plan-tracker")).toHaveClass("opacity-0");
    // …then gone.
    await waitFor(() => expect(screen.queryByTestId("plan-tracker")).not.toBeInTheDocument());
  });

  it("collapses completed steps and caps pending once the plan exceeds 6 steps", async () => {
    const many = snapshot({
      steps: [
        { description: "s0", done: true },
        { description: "s1", done: true },
        { description: "s2", done: true },
        { description: "s3", done: false },
        { description: "s4", done: false },
        { description: "s5", done: false },
        { description: "s6", done: false },
        { description: "s7", done: false },
        { description: "s8", done: false },
      ],
      currentStepIndex: 3,
    });
    vi.mocked(commands.getActivePlan).mockResolvedValue(many);
    render(<PlanTracker conversationId="c1" />);
    await screen.findByTestId("plan-card");

    expect(screen.getByTestId("plan-done-collapsed")).toHaveTextContent("3 done");
    // Current (s3) + up to 4 pending (s4..s7) visible, rest summarized.
    expect(screen.getAllByTestId("plan-step")).toHaveLength(5);
    expect(screen.getByTestId("plan-more")).toHaveTextContent("+1 more");
  });

  it("renders the dot rail (with matching states) alongside the card, and a chip past 12 steps", async () => {
    vi.mocked(commands.getActivePlan).mockResolvedValue(snapshot());
    render(<PlanTracker conversationId="c1" />);
    await screen.findByTestId("plan-rail");

    const dots = screen.getAllByTestId("plan-dot");
    expect(dots).toHaveLength(3);
    expect(dots[0]).toHaveTextContent("✓");
    expect(dots[1]).toHaveAttribute("data-current", "true");
    expect(screen.queryByTestId("plan-chip")).not.toBeInTheDocument();

    act(() =>
      firePlanUpdate({
        conversationId: "c1",
        plan: snapshot({
          steps: Array.from({ length: 13 }, (_, i) => ({
            description: `s${i}`,
            done: i < 5,
          })),
          currentStepIndex: 5,
        }),
      }),
    );
    expect(screen.queryAllByTestId("plan-dot")).toHaveLength(0);
    expect(screen.getByTestId("plan-chip")).toHaveTextContent("5/13");
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run src/views/workspace/PlanTracker.test.tsx 2>&1 | tail -5`
Expected: FAIL — cannot resolve `./PlanTracker`.

- [ ] **Step 3: Implement the component**

Create `src/views/workspace/PlanTracker.tsx`:

```tsx
import { useEffect, useRef, useState } from "react";
import { cn } from "@/lib/cn";
import { commands, events, type PlanSnapshot } from "@/lib/ipc";

const FADE_OUT_MS = 300;
/** Card caps (spec): completed steps collapse into one "✓ n done" line
 * once the plan exceeds 6 steps; visible pending capped at 4. */
const CARD_COLLAPSE_THRESHOLD = 6;
const CARD_MAX_PENDING = 4;
/** Rail cap (spec): per-step dots up to 12 steps, then a single n/m chip. */
const RAIL_MAX_DOTS = 12;

interface PlanTrackerProps {
  conversationId: string;
}

/**
 * The live plan/todo tracker (spec:
 * docs/superpowers/specs/2026-07-09-plan-tracker-design.md): floats over
 * the transcript's top-right gutter inside Workspace's StickToBottom
 * wrapper. Live-turn chrome only — appears when the agent creates a plan,
 * follows plan-update events, recovers across reloads via
 * get_active_plan, and fades out when the turn ends (plan: null). The
 * card/rail split is pure CSS container queries (the chat surface is the
 * container): the full card when the gutter fits it, the numbered dot
 * rail when it doesn't. Both render in the DOM — jsdom can't evaluate
 * container queries, and tests assert both forms directly.
 */
export default function PlanTracker({ conversationId }: PlanTrackerProps) {
  const [plan, setPlan] = useState<PlanSnapshot | null>(null);
  const [leaving, setLeaving] = useState(false);
  // The rail's tap-to-expand: force-shows the card at narrow widths.
  const [expanded, setExpanded] = useState(false);
  const leaveTimerRef = useRef<number | null>(null);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;

    const applyUpdate = (next: PlanSnapshot | null) => {
      if (leaveTimerRef.current !== null) {
        window.clearTimeout(leaveTimerRef.current);
        leaveTimerRef.current = null;
      }
      if (next) {
        setLeaving(false);
        setPlan(next);
        return;
      }
      // Turn ended: fade, then unmount.
      setExpanded(false);
      setLeaving(true);
      leaveTimerRef.current = window.setTimeout(() => {
        setPlan(null);
        setLeaving(false);
        leaveTimerRef.current = null;
      }, FADE_OUT_MS);
    };

    setPlan(null);
    setLeaving(false);
    setExpanded(false);
    void commands
      .getActivePlan(conversationId)
      .then((recovered) => {
        if (!cancelled && recovered) setPlan(recovered);
      })
      .catch(() => {});
    void events
      .onPlanUpdate((payload) => {
        if (cancelled || payload.conversationId !== conversationId) return;
        applyUpdate(payload.plan);
      })
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      });

    return () => {
      cancelled = true;
      unlisten?.();
      if (leaveTimerRef.current !== null) {
        window.clearTimeout(leaveTimerRef.current);
        leaveTimerRef.current = null;
      }
    };
  }, [conversationId]);

  if (!plan || plan.steps.length === 0) return null;

  const doneCount = plan.steps.filter((s) => s.done).length;

  return (
    <div
      className={cn(
        "absolute top-3 right-3 z-10 transition-opacity duration-300",
        leaving && "opacity-0",
      )}
      data-testid="plan-tracker"
    >
      {/* Full card: shown when the container is wide enough for the
          gutter (>= 64rem), or when the rail was tapped open. */}
      <div className={cn("hidden @5xl:block", expanded && "block")} data-testid="plan-card">
        <PlanCard plan={plan} doneCount={doneCount} />
      </div>
      {/* Collapsed rail: numbered dots (the selected mockup), only below
          the breakpoint and only while not tapped open. */}
      <button
        type="button"
        className={cn("block @5xl:hidden", expanded && "hidden")}
        onClick={() => setExpanded(true)}
        aria-label="Show plan"
        data-testid="plan-rail"
      >
        <PlanRail plan={plan} doneCount={doneCount} />
      </button>
      {expanded && (
        <button
          type="button"
          className="mt-1 block w-full text-center text-xs text-muted-foreground @5xl:hidden"
          onClick={() => setExpanded(false)}
          aria-label="Hide plan"
          data-testid="plan-collapse"
        >
          collapse
        </button>
      )}
    </div>
  );
}

function PlanCard({ plan, doneCount }: { plan: PlanSnapshot; doneCount: number }) {
  const collapseDone = plan.steps.length > CARD_COLLAPSE_THRESHOLD;
  const rows = plan.steps
    .map((step, index) => ({ step, index }))
    .filter(({ step, index }) => {
      if (!collapseDone) return true;
      // Keep the current step and pending ones; completed fold into the
      // "✓ n done" header line.
      return !step.done || index === plan.currentStepIndex;
    });
  const pendingVisible = collapseDone ? rows.slice(0, CARD_MAX_PENDING + 1) : rows;
  const hiddenCount = rows.length - pendingVisible.length;

  return (
    <div className="w-60 rounded-lg border border-border bg-card/95 p-3 text-sm shadow-sm backdrop-blur supports-[backdrop-filter]:bg-card/80">
      <div className="mb-1.5 flex items-baseline justify-between gap-2">
        <span className="truncate text-xs font-semibold" title={plan.goal}>
          {plan.goal}
        </span>
        <span className="shrink-0 text-xs text-muted-foreground">
          {doneCount}/{plan.steps.length}
        </span>
      </div>
      {collapseDone && doneCount > 0 && (
        <p className="text-xs text-muted-foreground" data-testid="plan-done-collapsed">
          ✓ {doneCount} done
        </p>
      )}
      <ul className="space-y-0.5">
        {pendingVisible.map(({ step, index }) => (
          <li
            key={index}
            className={cn(
              "flex items-baseline gap-1.5 text-xs",
              step.done && "text-muted-foreground line-through",
              index === plan.currentStepIndex && "font-semibold",
            )}
            data-current={index === plan.currentStepIndex ? "true" : undefined}
            data-testid="plan-step"
          >
            <span className="w-3 shrink-0 no-underline">
              {step.done ? "✓" : index === plan.currentStepIndex ? "●" : "○"}
            </span>
            <span className="truncate" title={step.description}>
              {step.description}
            </span>
          </li>
        ))}
      </ul>
      {hiddenCount > 0 && (
        <p className="text-xs text-muted-foreground" data-testid="plan-more">
          +{hiddenCount} more
        </p>
      )}
    </div>
  );
}

function PlanRail({ plan, doneCount }: { plan: PlanSnapshot; doneCount: number }) {
  const pill =
    "rounded-full border border-border bg-card/95 shadow-sm backdrop-blur supports-[backdrop-filter]:bg-card/80";
  if (plan.steps.length > RAIL_MAX_DOTS) {
    return (
      <span className={cn(pill, "px-2.5 py-1 text-xs font-semibold")} data-testid="plan-chip">
        {doneCount}/{plan.steps.length}
      </span>
    );
  }
  return (
    <span className={cn(pill, "flex flex-col items-center gap-1 px-1.5 py-2")}>
      {plan.steps.map((step, index) => (
        <span
          key={index}
          className={cn(
            "flex h-4.5 w-4.5 items-center justify-center rounded-full text-[10px] font-semibold",
            step.done
              ? "bg-emerald-600 text-white"
              : index === plan.currentStepIndex
                ? "border-2 border-amber-500 text-amber-600"
                : "border border-border text-muted-foreground",
          )}
          data-current={index === plan.currentStepIndex ? "true" : undefined}
          data-testid="plan-dot"
        >
          {step.done ? "✓" : index + 1}
        </span>
      ))}
    </span>
  );
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npx vitest run src/views/workspace/PlanTracker.test.tsx 2>&1 | tail -3 && npx tsc --noEmit && echo TSC CLEAN`
Expected: 6 tests PASS, TSC CLEAN. If the collapse-cap test disagrees with the slicing math, fix the component (the spec numbers win: "✓ n done" line + current + ≤4 pending + "+k more").

- [ ] **Step 5: Commit**

```bash
git add src/views/workspace/PlanTracker.tsx src/views/workspace/PlanTracker.test.tsx
git commit -m "feat(ui): PlanTracker card + container-query dot rail

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 7: Workspace integration

**Files:**

- Modify: `src/views/workspace/Workspace.tsx` (render PlanTracker, make the surface a container)
- Modify: `src/views/workspace/Workspace.test.tsx` (mock additions + integration test)
- Modify: `src/App.test.tsx` (mock additions — it renders Workspace)

**Interfaces:**

- Consumes: `PlanTracker` (Task 6); `commands.getActivePlan` / `events.onPlanUpdate` (Task 5).
- Produces: the tracker lives inside the `StickToBottom` wrapper, sibling of the scroll element and the scroll-to-bottom button.

- [ ] **Step 1: Write the failing integration test**

In `src/views/workspace/Workspace.test.tsx`:

1. Add to the `vi.mock("@/lib/ipc", ...)` factory's `commands` object: `getActivePlan: vi.fn(),` and to its `events` object: `onPlanUpdate: vi.fn(),`
2. Add to the suite's `beforeEach`:

```tsx
vi.mocked(commands.getActivePlan).mockResolvedValue(null);
vi.mocked(events.onPlanUpdate).mockResolvedValue(() => {});
```

3. Append the test:

```tsx
it("shows the plan tracker over the transcript while a plan is active and clears it when the turn ends", async () => {
  let firePlanUpdate!: (p: import("@/lib/ipc").PlanUpdatePayload) => void;
  vi.mocked(events.onPlanUpdate).mockImplementation(async (cb) => {
    firePlanUpdate = cb;
    return () => {};
  });

  render(<Workspace conversationId="conv-1" />);
  await screen.findByTestId("agent-input");
  expect(screen.queryByTestId("plan-tracker")).not.toBeInTheDocument();

  act(() =>
    firePlanUpdate({
      conversationId: "conv-1",
      plan: {
        goal: "Fix the bugs",
        steps: [
          { description: "find them", done: true },
          { description: "fix them", done: false },
        ],
        currentStepIndex: 1,
      },
    }),
  );

  const tracker = await screen.findByTestId("plan-tracker");
  // Inside the scroll wrapper (StickToBottom's relative container), as
  // an overlay sibling of the scroll element — not inside the transcript.
  expect(tracker.parentElement).toBe(
    screen.getByTestId("workspace-scroll-container").parentElement,
  );
  expect(screen.getByTestId("plan-card")).toHaveTextContent("1/2");

  act(() => firePlanUpdate({ conversationId: "conv-1", plan: null }));
  await waitFor(() => expect(screen.queryByTestId("plan-tracker")).not.toBeInTheDocument());
});
```

(`act` is no longer imported in this file after the autoscroll refactor — re-add it to the `@testing-library/react` import.)

4. In `src/App.test.tsx`, the `vi.mock("@/lib/ipc", ...)` factory (top of file) lists `commands` and `events` objects explicitly. Add `getActivePlan: vi.fn(),` to `commands` (after `isGenerationActive`) and `onPlanUpdate: vi.fn(),` to `events` (after `onAgentMessagePersisted`), and in the suite's `beforeEach`:

```tsx
vi.mocked(commands.getActivePlan).mockResolvedValue(null);
vi.mocked(events.onPlanUpdate).mockResolvedValue(() => {});
```

- [ ] **Step 2: Run the new test to verify it fails**

Run: `npx vitest run src/views/workspace/Workspace.test.tsx -t "plan tracker" 2>&1 | tail -5`
Expected: FAIL — `plan-tracker` never appears (component not rendered).

- [ ] **Step 3: Wire the tracker into Workspace**

In `src/views/workspace/Workspace.tsx`:

1. `import PlanTracker from "@/views/workspace/PlanTracker";`
2. Make the chat surface a named container — on the `<StickToBottom ...>` element extend the className:

```tsx
className = "@container relative min-h-0 flex-1";
```

3. Inside the render-prop fragment, after the scroll-to-bottom `{!isAtBottom && (...)}` block (sibling of the scroll div), add:

```tsx
<PlanTracker conversationId={conversationId} />
```

- [ ] **Step 4: Run the full frontend suite**

Run: `npx vitest run 2>&1 | grep -E "Test Files|Tests " && npx tsc --noEmit && echo TSC CLEAN && npx prettier --check src/views/workspace/ src/components/MessageContent.tsx src/lib/ipc.ts`
Expected: all files/tests PASS (App.test included — its mocks now cover the new calls), TSC CLEAN, prettier clean (run `--write` on any flagged file you touched).

- [ ] **Step 5: Commit**

```bash
git add src/views/workspace/Workspace.tsx src/views/workspace/Workspace.test.tsx src/App.test.tsx
git commit -m "feat(ui): render the live plan tracker over the workspace transcript

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 8: Full verification sweep

**Files:** none new — verification only.

- [ ] **Step 1: Backend**

Run: `cd src-tauri && cargo test --lib 2>&1 | tail -3 && cargo clippy --lib --tests 2>&1 | tail -2 && cargo test --test agent_benchmark --no-run 2>&1 | tail -2`
Expected: all tests PASS, clippy clean, benchmark compiles.

- [ ] **Step 2: Frontend**

Run: `npx vitest run 2>&1 | grep -E "Test Files|Tests " && npx tsc --noEmit && echo TSC CLEAN`
Expected: all PASS, TSC CLEAN.

- [ ] **Step 3: Live feel-check (the running `tauri dev` session rebuilds automatically)**

In the app: send a multi-step request in a workspace conversation (e.g. "create three files a.txt, b.txt, c.txt each containing their own name") and confirm — tracker appears top-right when the plan is created; steps tick ✓ as it executes; narrow the window below ~64rem container width → the dot rail; tap the rail → card expands; turn completes → tracker fades; the transcript shows no plan rows; a trivial "hi" produces no tracker. Note: `bindings.ts` regenerates on the dev rebuild — if it shows a diff (new command/event), commit it:

```bash
git add src/lib/bindings.ts && git commit -m "chore: regenerate specta bindings for plan surface

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

- [ ] **Step 4: Update the plan checkboxes and stop**

Mark all tasks complete in this document, then hand back for review (per the executing skill's checkpoints).
