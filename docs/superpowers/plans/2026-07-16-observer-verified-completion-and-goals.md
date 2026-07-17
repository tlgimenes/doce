# Observer-Verified Completion + User Goals — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove the model's self-report of completion. Every `TodoDone`/`FinishTask` is adjudicated by an authoritative observer LLM against an append-only mutation log; add an optional user goal that rides in the tail slot and is checked at FinishTask.

**Architecture:** `PlanState` gains an append-only mutation log, a goal, and per-item reject counters. Completion tools become _proposals_ (`PlanToolReply::ProposeComplete`); the backend runs the observer and commits/rejects. The observer's pure pieces (message building, verdict parsing, prompt, schema) live in a new `agent::observer` module shared by both backends. `run_loop` is untouched.

**Tech Stack:** Rust (Tauri backend), rusqlite (storage/migrations), llama-server sidecar (OpenAI `/v1/chat/completions`, `tool_choice:"required"` + grammar), the `bench` cargo feature for the deterministic benchmark.

## Global Constraints

- `run_loop` (`src/agent/mod.rs`) BYTE-UNTOUCHED. Every task verifies `git diff <base> -- src/agent/mod.rs` is empty.
- Do NOT change the agent system prompt, `SUMMARIZATION_PROMPT`, or `MEMORY_EXTRACTION_PROMPT` bytes, EXCEPT the intended new gated surfaces this feature adds: the new `OBSERVER_PROMPT`, the generalized `state_tail` (todo + goal), and any `TodoDone`/`FinishTask` reply-string change. Every model-facing byte change is benchmark-gated by the CONTROLLER (not the implementer).
- NEVER touch, stage, or revert the four parallel-session files: `src/views/chat/tool-widgets/BashWidget.tsx`, `src/views/chat/tool-widgets/BashWidget.test.tsx`, `src/views/design-system/WidgetGallery.tsx`, `src/views/workspace/Workspace.test.tsx`. Scope every `git add` to explicit paths.
- `bench` must stay production-faithful: ONE shared observer implementation, called by BOTH `commands/agent.rs` and `bench/mod.rs`. No second copy (the repo just eradicated hand-rolled shape drift).
- Determinism: the bench path is seeded (`DOCE_GEN_SEED`, `StableToolCallIds`). Observer calls MUST be deterministic under a fixed seed — same seed → identical score AND turns.
- `cargo test --lib` must NOT need `--features bench`; `cargo build --release` (no features) must NOT pull the bench module or `tempfile`.
- `tracing` is NOT a dependency — use `eprintln!`. Run `cargo fmt`. Frontend formats with **oxfmt, not prettier** (check `package.json` scripts).
- Work in place on `main` (no worktrees). SDD ledger: `.superpowers/sdd/progress.md`.

## File Structure

- `src/agent/plan.rs` — `PlanState` gains `mutation_log`, `goal`, reject counters; `PlanToolReply::ProposeComplete` + `CompletionKind`; completion arms return proposals; `record_mutation`/`commit_todo_done`/reject-cap methods; `todo_tail` → `state_tail` (renders goal + todos).
- `src/agent/observer.rs` (NEW) — `Verdict`, `OBSERVER_PROMPT`, `build_observer_messages` (pure), `parse_verdict` (pure), `request_verdict` (async, shared server call).
- `src/agent/mod.rs` — declare `pub mod observer;` ONLY (one line; `run_loop` body untouched). Verify the diff is exactly that one line.
- `src/inference/http.rs` — the observer `Verdict` tool schema (in `tool_def`), reachable for the observer's forced-tool request.
- `src/commands/agent.rs` — record mutations in the execute path; consume `ProposeComplete` (call `request_verdict`, commit/reject with cap); load goal at task start; `set_conversation_goal` command; auto-finish + `goal-complete` UI event.
- `src/bench/mod.rs` — the SAME integration (record mutations, consume proposal, call `request_verdict`) so the benchmark exercises the real observer.
- `src/storage/migrations.rs` + `src/storage/conversations.rs` (or `messages.rs`) — `goal TEXT` column + get/set.
- Frontend — a new goal control component + wiring (NOT any of the four forbidden files).

## Interfaces (the contract every task shares)

```rust
// src/agent/plan.rs
#[derive(Clone, Debug, PartialEq)]
pub struct MutationRecord {
    pub tool: String,           // "Update", "Write", "Bash"
    pub target: Option<String>, // file path for Update/Write; None for Bash
    pub ok: bool,               // did the call succeed
}

#[derive(Clone, Debug, PartialEq)]
pub enum CompletionKind {
    TodoItem(usize),            // 0-based index proposed done
    FinishTask,
}

pub enum PlanToolReply {
    Reply(String),
    Finish(String),
    ProposeComplete { kind: CompletionKind, answer: Option<String> }, // NEW
}

// PlanState new fields: mutation_log: Vec<MutationRecord>, goal: Option<String>,
//                       reject_counts: std::collections::HashMap<CompletionKind, u32>
// (derive Hash/Eq on CompletionKind for the map key)
pub const OBSERVER_REJECT_CAP: u32 = 2; // rejects allowed before the model's claim wins

impl PlanState {
    pub fn record_mutation(&mut self, tool: &str, target: Option<String>, ok: bool);
    pub fn commit_todo_done(&mut self, index: usize) -> String; // flips done, returns reply string
    pub fn note_reject(&mut self, kind: &CompletionKind) -> u32; // ++ and return new count
    pub fn reject_cap_reached(&self, kind: &CompletionKind) -> bool; // count >= OBSERVER_REJECT_CAP
    pub fn state_tail(&self) -> String; // goal line (if any) + todo recitation; "" only when BOTH empty
}

// src/agent/observer.rs
#[derive(Clone, Debug, PartialEq)]
pub struct Verdict { pub complete: bool, pub missing: String }

pub const OBSERVER_PROMPT: &str = /* new, gated */;

pub fn build_observer_messages(
    kind: &crate::agent::plan::CompletionKind,
    plan: &crate::agent::plan::Plan,
    mutation_log: &[crate::agent::plan::MutationRecord],
    answer: Option<&str>,
    goal: Option<&str>,
) -> Vec<crate::inference::ChatMessage>;                 // PURE

pub fn parse_verdict(tool_args: &serde_json::Value) -> Verdict; // PURE (from the Verdict tool call args)

pub async fn request_verdict(
    endpoint: &str,                                       // base URL of the llama-server
    kind: &crate::agent::plan::CompletionKind,
    plan: &crate::agent::plan::Plan,
    mutation_log: &[crate::agent::plan::MutationRecord],
    answer: Option<&str>,
    goal: Option<&str>,
    seed: Option<u64>,
) -> Result<Verdict, String>;                            // builds msgs, POSTs Verdict-forced req, parses
```

---

### Task 1: Mutation log — accumulate tool-mutation evidence

**Files:**

- Modify: `src/agent/plan.rs` (add `MutationRecord`, `mutation_log` field + `record_mutation`)
- Modify: `src/commands/agent.rs` (call `record_mutation` in the real-tool execute path)
- Modify: `src/bench/mod.rs` (same call in its execute path)
- Test: `src/agent/plan.rs` unit tests

**Interfaces:**

- Produces: `MutationRecord`, `PlanState.mutation_log`, `PlanState::record_mutation` (see contract).
- Consumes: nothing new.

- [ ] **Step 1: Failing test** in `plan.rs` `single_mode_tests`:

```rust
#[test]
fn record_mutation_appends_evidence_and_never_clears() {
    let mut st = PlanState::default();
    st.record_mutation("Update", Some("/x/bug_04.txt".into()), false);
    st.record_mutation("Update", Some("/x/bug_04.txt".into()), true);
    st.record_mutation("Bash", None, true);
    assert_eq!(st.mutation_log.len(), 3);
    assert_eq!(st.mutation_log[0], MutationRecord { tool: "Update".into(), target: Some("/x/bug_04.txt".into()), ok: false });
    assert!(st.mutation_log[2].target.is_none());
}
```

- [ ] **Step 2: Run it — FAIL** (`cargo test --lib record_mutation_appends`): field/method missing.
- [ ] **Step 3: Implement** the `MutationRecord` struct, the `mutation_log: Vec<MutationRecord>` field on `PlanState` (default empty), and `record_mutation` (push). Derive `Clone, Debug, PartialEq` on `MutationRecord`.
- [ ] **Step 4: Run it — PASS.**
- [ ] **Step 5: Wire both backends.** In `commands/agent.rs` and `bench/mod.rs`, at the site where a REAL (non-harness) tool has executed and its result is known, call `self.plan_state.record_mutation(&tool_name, target, ok)`. `target` = the `file_path`/`path` argument for `Update`/`Write` (else `None`); `ok` = the result was not an error (mirror how the backend already classifies success/error strings). Log the file-mutating set: `Update`(Edit), `Write`, `Bash`. Do NOT log read-only tools (`Read`/`Grep`/`Glob`). Keep the two call sites behaviorally identical (shared shape).
- [ ] **Step 6: Compile check** `cargo check --tests --features bench` — clean; `git diff -- src/agent/mod.rs` empty.
- [ ] **Step 7: Commit** `git add src/agent/plan.rs src/commands/agent.rs src/bench/mod.rs` — `feat(plan): accumulate an append-only tool-mutation log for the observer`

---

### Task 2: Propose → commit refactor (behavior-preserving, always-approve stub)

**Files:**

- Modify: `src/agent/plan.rs` (`PlanToolReply::ProposeComplete`, `CompletionKind`, completion arms return proposals, `commit_todo_done`, reject-cap methods)
- Modify: `src/commands/agent.rs`, `src/bench/mod.rs` (consume `ProposeComplete` via an always-approve stub; commit path byte-identical to today)
- Test: `src/agent/plan.rs` unit tests

**Interfaces:**

- Produces: `ProposeComplete`, `CompletionKind`, `commit_todo_done`, `note_reject`, `reject_cap_reached`, `OBSERVER_REJECT_CAP`.
- Consumes: Task 1's `PlanState`.

**CRITICAL — behavior-preserving:** With the stub always approving, the reply bytes the model receives MUST be byte-identical to today's, so the benchmark trajectory does not move. `commit_todo_done(index)` returns the EXACT current `TodoDone` reply string ("Marked done{note}: {desc}. {done}/{total} done."). The approved-FinishTask path reuses the current Finish handling verbatim.

- [ ] **Step 1: Failing tests** (update existing + add):

```rust
#[test]
fn todo_done_now_proposes_instead_of_committing() {
    let mut st = plan_with_two_undone("a", "b"); // helper: two undone items
    let reply = st.handle_todo_tool(&tool_call("TodoDone", json!({"index": 0}))).unwrap();
    assert!(matches!(reply, PlanToolReply::ProposeComplete { kind: CompletionKind::TodoItem(0), answer: None }));
    assert!(!st.plan.steps[0].done, "proposal must NOT commit the flip");
}
#[test]
fn commit_todo_done_flips_and_returns_the_reply() {
    let mut st = plan_with_two_undone("a", "b");
    let reply = st.commit_todo_done(0);
    assert!(st.plan.steps[0].done);
    assert!(reply.contains("Marked done") && reply.contains("1/2 done"));
}
#[test]
fn reject_cap_lets_the_model_win_after_two_rejects() {
    let mut st = plan_with_two_undone("a", "b");
    let k = CompletionKind::TodoItem(0);
    assert_eq!(st.note_reject(&k), 1); assert!(!st.reject_cap_reached(&k));
    assert_eq!(st.note_reject(&k), 2); assert!(st.reject_cap_reached(&k));
}
#[test]
fn finish_task_still_bounces_undone_before_proposing() {
    let mut st = plan_with_two_undone("a", "b");
    let reply = st.handle_todo_tool(&tool_call("FinishTask", json!({"answer": "done"}))).unwrap();
    assert!(matches!(reply, PlanToolReply::Reply(_)), "cheap undone bounce still fires first");
}
```

Also KEEP passing (update to the new commit path where needed): `todo_done_flips_exactly_one_item` (now via `commit_todo_done`), `no_tool_sequence_rewrites_an_active_list_to_all_done_without_doing_the_work` (immutability holds — `Todo` still append-only), `append_merge_never_removes_reorders_relabels_or_undones_existing_items`.

- [ ] **Step 2: Run — FAIL.**
- [ ] **Step 3: Implement.** Add `ProposeComplete`/`CompletionKind` (derive `Hash,Eq,PartialEq,Clone,Debug` on `CompletionKind`). In `handle_todo_tool`: the `TodoDone` arm validates the index (range + resolvable) exactly as today, but on success returns `ProposeComplete { kind: TodoItem(index), answer: None }` (bad index → `Reply(error)` as today). The `FinishTask` arm keeps the cheap undone-count bounce (first gate) unchanged, then returns `ProposeComplete { kind: FinishTask, answer }` instead of `Finish`. Add `commit_todo_done` (the old flip+reply), `note_reject`, `reject_cap_reached`, `OBSERVER_REJECT_CAP`.
- [ ] **Step 4: Run — PASS.**
- [ ] **Step 5: Backends consume the proposal (stub).** In both backends, add a local `async fn verdict_stub(...) -> Verdict { Verdict { complete: true, missing: String::new() } }` placeholder (Task 4 replaces it with `request_verdict`). On `ProposeComplete`: call the stub; if `complete` → `TodoItem(i)` ⇒ feed back `commit_todo_done(i)`; `FinishTask` ⇒ run the EXISTING finish path with `answer` (factor it into a helper both the old direct-Finish and this approved-proposal call). The reject branch (cap logic) is wired but unreachable with the always-true stub — implement it now so Task 4 only swaps the verdict source.
- [ ] **Step 6:** `cargo test --lib` green; `cargo check --tests --features bench` clean; `git diff -- src/agent/mod.rs` empty.
- [ ] **Step 7: Commit** explicit paths — `refactor(plan): completion tools propose; backend commits (always-approve stub)`

> CONTROLLER GATE (after Task 2, optional but cheap): one determinism run (tier4 seed 11) should still score 19/20 with IDENTICAL turns — proving the refactor did not move the trajectory. If it moved, a reply string diverged; fix before Task 3.

---

### Task 3: Observer module — pure pieces (messages, verdict, prompt, schema)

**Files:**

- Create: `src/agent/observer.rs`
- Modify: `src/agent/mod.rs` (add `pub mod observer;` — ONE line)
- Modify: `src/inference/http.rs` (`Verdict` tool schema in `tool_def`)
- Test: `src/agent/observer.rs` unit tests

**Interfaces:**

- Produces: `Verdict`, `OBSERVER_PROMPT`, `build_observer_messages` (pure), `parse_verdict` (pure), the `Verdict` tool schema.
- Consumes: `plan::{CompletionKind, Plan, MutationRecord}`.

- [ ] **Step 1: Failing tests** (pure):

```rust
#[test]
fn verdict_parses_from_tool_args() {
    let v = parse_verdict(&json!({"complete": false, "missing": "no edit to bug_04.txt"}));
    assert!(!v.complete);
    assert_eq!(v.missing, "no edit to bug_04.txt");
}
#[test]
fn observer_messages_for_a_todo_include_the_item_and_its_evidence() {
    let plan = plan_with(&[("Fix bug_04.txt", false)]);
    let log = vec![MutationRecord{tool:"Update".into(), target:Some("/x/bug_04.txt".into()), ok:false}];
    let msgs = build_observer_messages(&CompletionKind::TodoItem(0), &plan, &log, None, None);
    let joined = msgs.iter().map(|m| m.text()).collect::<Vec<_>>().join("\n");
    assert!(joined.contains("Fix bug_04.txt"));
    assert!(joined.contains("bug_04.txt") && joined.to_lowercase().contains("ok=false") || joined.contains("failed"));
}
#[test]
fn observer_messages_for_finish_include_goal_and_answer() {
    let plan = plan_with(&[("Fix bug_04.txt", true)]);
    let msgs = build_observer_messages(&CompletionKind::FinishTask, &plan, &[], Some("all fixed"), Some("ship the fix"));
    let joined = msgs.iter().map(|m| m.text()).collect::<Vec<_>>().join("\n");
    assert!(joined.contains("ship the fix") && joined.contains("all fixed"));
}
```

- [ ] **Step 2: Run — FAIL.**
- [ ] **Step 3: Implement** `observer.rs`: `Verdict`; `OBSERVER_PROMPT` (a tight system prompt: "You verify whether a coding agent's completion claim is actually supported by the evidence. You are shown the item/goal and a log of the agent's file mutations. Approve ONLY if the evidence shows the work was done; otherwise reject and name what is missing. Judge evidence, not the agent's assertions."); `build_observer_messages` (system = `OBSERVER_PROMPT`; one user message rendering: the `CompletionKind` (the specific todo text for `TodoItem`, or "final answer + goal" for `FinishTask`), the relevant `MutationRecord`s (`tool target ok`), the `answer`, the `goal`); `parse_verdict` (read `complete`/`missing` from the Verdict tool args). Add the `Verdict` tool schema to `http.rs::tool_def` (`{complete: boolean, missing: string}`, both required). Add `pub mod observer;` to `agent/mod.rs`.
- [ ] **Step 4: Run — PASS.**
- [ ] **Step 5:** `git diff -- src/agent/mod.rs` shows EXACTLY the one `pub mod observer;` line. `cargo test --lib` green (no `--features bench`).
- [ ] **Step 6: Commit** — `feat(observer): pure verdict/message/prompt pieces for completion adjudication`

---

### Task 4: Wire the real observer call into both backends

**Files:**

- Modify: `src/agent/observer.rs` (`request_verdict` async — build req, POST to endpoint with `Verdict` forced + seed, parse)
- Modify: `src/commands/agent.rs`, `src/bench/mod.rs` (replace the Task-2 stub with `request_verdict`; pass the backend's endpoint + seed)
- Test: a real-model integration test (mirror the `summarize_and_persist` real-model test style) — `src/agent/observer.rs` `#[ignore]` or `tests/`

**Interfaces:**

- Produces: `request_verdict`.
- Consumes: Task 3 pure pieces; the backends' server endpoint + seed.

- [ ] **Step 1: Failing real-model test** (`#[ignore]`, needs the model): construct a plan with one todo "Fix bug_04.txt" marked proposed-done and a `mutation_log` that has NO successful edit to `bug_04.txt`; call `request_verdict(endpoint, TodoItem(0), …)`; assert `!verdict.complete` and `verdict.missing` mentions the missing edit. A second case with a successful edit asserts `verdict.complete`.
- [ ] **Step 2: Run — FAIL** (`request_verdict` unimplemented).
- [ ] **Step 3: Implement** `request_verdict`: `build_observer_messages` → `ChatRequest::build` with `to_openai_messages`, the `Verdict` tool, `tool_choice:"required"`, the passed `seed`; POST to `endpoint`'s `/v1/chat/completions` reusing the existing chat-post path; extract the `Verdict` tool call args; `parse_verdict`. On any transport/parse failure return `Err` — the backend treats an errored verdict as APPROVE (fail-open: never trap the loop on observer failure) and logs via `eprintln!`.
- [ ] **Step 4: Run the real-model test — PASS.**
- [ ] **Step 5: Swap the stub.** Both backends now call `request_verdict(self.endpoint(), &kind, &self.plan_state.plan, &self.plan_state.mutation_log, answer.as_deref(), self.plan_state.goal.as_deref(), self.seed())`. Reject branch: `note_reject`; if `reject_cap_reached` → commit anyway with a reply noting the unresolved disagreement (`"Closed despite unresolved concern: {missing}"`); else do NOT commit and reply (`TodoItem` ⇒ `"Not done: {missing}. Do the work, then mark it done again."`; `FinishTask` ⇒ `Reply("Not finished: {missing}. Keep working.")`). Ensure `seed` threads from `DOCE_GEN_SEED` in the bench path (determinism) and the endpoint is each backend's live server.
- [ ] **Step 6:** `cargo test --lib` green; `cargo check --tests --features bench` clean; `git diff -- src/agent/mod.rs` empty.
- [ ] **Step 7: Commit** explicit paths — `feat(observer): authoritative verdict on every TodoDone/FinishTask`

> **CONTROLLER GATE (mandatory).** This changes the trajectory (rejections feed back). Controller runs: tier4 seeds 11/22/33 and tier6 seed 42. PASS = seed 11 recovers toward 20/20 (observer catches bug_04's false completion) AND seeds 22/33 hold 20/20 AND tier6 holds 14/14 AND determinism (same seed → identical score+turns). Report observer-call count + added wall-clock. If seed 11 does not recover, capture the trajectory and diagnose before Task 5.

---

### Task 5: Goals — persistence, state_tail injection, FinishTask goal-check

**Files:**

- Modify: `src/storage/migrations.rs` (add `goal TEXT` to `conversations`)
- Modify: `src/storage/conversations.rs` or `messages.rs` (get/set goal)
- Create/Modify: a `set_conversation_goal` Tauri command (`src/commands/…`)
- Modify: `src/agent/plan.rs` (`goal` field already in contract; `todo_tail` → `state_tail`)
- Modify: `src/commands/agent.rs` (load goal into `PlanState` at task start; on approved FinishTask with a set goal, mark satisfied + emit `goal-complete`), `src/bench/mod.rs` (set `goal` for the tier fixtures that use it — optional)
- Test: `plan.rs` (`state_tail`), storage (migration/get/set)

**Interfaces:**

- Produces: `state_tail`, `set_conversation_goal`, goal persistence.
- Consumes: Task 4 observer (goal is judged in the `FinishTask` verdict via `build_observer_messages`'s `goal` arg — already wired).

- [ ] **Step 1: Failing `state_tail` tests:**

```rust
#[test] fn state_tail_renders_goal_even_with_no_todos() {
    let mut st = PlanState::default();
    st.goal = Some("ship the fix".into());
    assert!(st.state_tail().contains("ship the fix"));
}
#[test] fn state_tail_shows_goal_and_todos_together() {
    let mut st = plan_with_two_undone("a", "b");
    st.goal = Some("ship it".into());
    let t = st.state_tail();
    assert!(t.contains("ship it") && t.contains("0. [ ] a"));
}
#[test] fn state_tail_empty_when_no_goal_and_no_todos() {
    assert!(PlanState::default().state_tail().is_empty());
}
```

- [ ] **Step 2: Run — FAIL.**
- [ ] **Step 3: Implement** `state_tail` (rename `todo_tail`; prepend a `Goal: <goal>` line when `goal.is_some()`; keep the todo recitation exactly as-is when steps exist; return `""` only when goal is None AND steps empty). Update the two call sites (`commands/agent.rs:742`, `bench/mod.rs:708`) and the `todo_tail` unit tests to `state_tail`.
- [ ] **Step 4: Run — PASS.**
- [ ] **Step 5: Persistence.** Add a `goal TEXT` column migration on `conversations` (follow the existing migration pattern; test it applies and round-trips). Add `get_conversation_goal`/`set_conversation_goal` storage fns + a `set_conversation_goal` Tauri command. Load the goal into `PlanState.goal` when a task starts in `commands/agent.rs`.
- [ ] **Step 6: Auto-finish.** On an observer-APPROVED `FinishTask` where a goal is set, emit a `goal-complete` app event (UI notify) and finish (the approval already means the observer judged the goal met, since `build_observer_messages` passed the goal). No human gate.
- [ ] **Step 7:** `cargo test --lib` green; `cargo check --tests --features bench` clean; `git diff -- src/agent/mod.rs` empty.
- [ ] **Step 8: Commit** explicit paths — `feat(goal): persist a user goal, render it in the state tail, check it at finish`

> CONTROLLER GATE (if `state_tail` bytes reach the model in a tiered fixture): tier4/tier6 no-regression. The goal line only renders when a goal is set, so tiers WITHOUT a goal see byte-identical tails — confirm the tier fixtures set no goal (then no gate needed) or gate if they do.

---

### Task 6: Goal UI

**Files:**

- Create: a new goal control component (e.g. `src/views/…/GoalBar.tsx` — a NEW file, NOT any of the four forbidden files) + wire it to the `set_conversation_goal` command
- Test: a component test alongside it

**Interfaces:**

- Consumes: `set_conversation_goal` (Task 5).

- [ ] **Step 1: Failing component test** — rendering the goal control shows the current goal and calls the command on save/clear. (Follow the repo's existing view-test patterns; format with **oxfmt**.)
- [ ] **Step 2: Run — FAIL.**
- [ ] **Step 3: Implement** the control (set/edit/clear goal on the active conversation). Do NOT modify any of the four forbidden files; if the natural mount point is one of them, mount from a different component and note it in the report for the parallel session to integrate.
- [ ] **Step 4: Run — PASS.** Run the frontend formatter (oxfmt) and the frontend test runner.
- [ ] **Step 5: Commit** explicit paths (only the new/implemented files) — `feat(ui): set a conversation goal`

---

## Final Whole-Branch Review + Gate

After Task 6: dispatch the final whole-branch code review (most capable model), then the CONTROLLER runs the full gate — tier4 seeds 11/22/33 + tier6 seed 42 — and records results + honest cost in the ledger. Merge only if seed 11 recovers, seeds 22/33 + tier6 hold, and determinism is intact. Then use superpowers:finishing-a-development-branch.
