# SP1 — Harness Cleanup & Budgeting Soundness — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Delete the vestige the llama-server cutover left behind, restore budgeting soundness (output-token cap, server-derived window, authoritative API usage), and drop the in-process llama-cpp-2 tokenizer for a chars/4 estimate — with compaction fail-safes.

**Architecture:** doce (Tauri v2 + Rust). Generation/tool-calling run in the llama-server sidecar via `/v1/chat/completions`; the API returns authoritative token `usage`. This sub-project makes that usage the source of truth and removes the now-redundant in-process tokenizer.

**Tech Stack:** Rust (tokio, serde, reqwest/SSE), SQLite. Spec: `docs/superpowers/specs/2026-07-14-harness-simplification-design.md` (SP1).

## Global Constraints

- **Single model** (`qwen3.5-4b-q4_k_m`, Hermes-style). Dialect selection may be pinned to `HermesJson`.
- **No prompt-byte changes in SP1.** The system prompt and summarization prompt bytes stay identical (those changes are SP3, benchmark-gated). SP1 changes only sampling params, accounting, deletions, and internals.
- **API `usage` is authoritative** for turn-to-turn accounting; local estimation is a conservative gap-filler only (first send + pre-send delta), never used to *skip* a budget check.
- Each task ends `cargo build` + `cargo clippy --all-targets` (no NEW warnings beyond the pre-existing `dispatch.rs` pair, which A3 fixes) + `cargo test` + `cargo fmt` green. Never commit `bindings.ts`. If a cargo/npm run reformats `docs/*.md`, revert it.
- The run-loop Require-invariant tests (`agent/mod.rs:594-716`) are non-negotiable coverage — they must stay green and unmodified in behavior.

## Ordering & dependencies

A1 → A2 → A3 → A4 (deletions/refactor first, no behavior change) → B1 → B2 → B3 → B4 (accounting; B4 removes the dependency, last) → C1 → C2 (needs A4) → C3. B4 swaps the token estimator introduced in B1, so B1 routes all estimation through one function B4 later re-implements.

## File Structure

- `src-tauri/src/agent/plan.rs` — A1 (delete dead machine), A2 (docs).
- `src-tauri/src/agent/mod.rs` — A2 (docs), B1 (adaptive escalation in run_loop), B3 (usage plumbing).
- `src-tauri/src/agent/dispatch.rs` — A3 (doubled `#[test]`).
- `src-tauri/src/commands/agent.rs` — A4 (staging helper), B1 (max_tokens at build sites + threshold), B3 (usage), C2 (Task result).
- `src-tauri/src/inference/http.rs` — B1 (`max_tokens` field), C3 (`>1 tool_calls` log).
- `src-tauri/src/inference/mod.rs` — B2 (`context_window`), B4 (drop tokenizer / chars-4), plus the `token_estimate` seam.
- `src-tauri/src/inference/server.rs` — B2 (expose the ctx-size).
- `src-tauri/src/context/mod.rs` — B1/B3 (summary max_tokens + usage), C1 (fail-safe).
- `src-tauri/src/context/limits.rs` — constants (B1, B2).
- `src-tauri/Cargo.toml` — B4 (remove `llama-cpp-2`).

---

## Phase A — Safe deletions (no behavior change)

### Task A1: Delete the dead two-state plan machine

**Files:**
- Modify: `src-tauri/src/agent/plan.rs` (delete symbols + their tests)

**Context:** `plan.rs` self-declares this machine dead at `plan.rs:150-157`. Production uses only the single-mode Todo engine. Recon confirmed zero non-test callers.

- [ ] **Step 1: Confirm zero production callers.** For each symbol below, run `rg '\bSYMBOL\b' src-tauri/src` and verify every hit is inside `plan.rs` (impl or `#[cfg(test)]`) or a doc comment — NOT `commands/agent.rs` or `agent/mod.rs` or a non-plan test. Symbols: `LoopState`, `handle_plan_tool`, `state_tail`, `allowed_tool_names`, `recitation_text`, `PLAN_TOOL_NAMES`, `PLANNING_ALLOWED_TOOLS`, `PLANNING_ALLOWED_TOOLS_NO_ASK`, `EXECUTING_ALLOWED_TOOLS`, `EXECUTING_ALLOWED_TOOLS_NO_TASK`, `REFUSAL_WRAP_UP_THRESHOLD`, `REFUSAL_HARD_LIMIT`. Also the `PlanState` fields `state`, `refusal_context`, `refusal_count`. Expected: all hits are plan.rs-internal.

- [ ] **Step 2: Delete the impl symbols.** Remove the `LoopState` enum, `handle_plan_tool`, `state_tail`, `allowed_tool_names`, `recitation_text`, the listed consts, and the three `PlanState` fields (and their initializers in `PlanState::new`/`Default`). KEEP: `Plan`, `PlanStep`, `next_undone_step`, `has_plan`, and the entire single-mode surface (`build_single_mode_system_prompt`, `single_mode_system_prompt`, `single_mode_tool_names`, `todo_tail`, `handle_todo_tool`, `SINGLE_MODE_*`). If `PlanStep.refused` is now written nowhere, leave it for now (removing it ripples into `next_undone_step`; defer to SP3 cleanup) — note it in the report.

- [ ] **Step 3: Delete the dead machine's tests.** Remove the `#[cfg(test)]` tests that exercise the deleted symbols (~`plan.rs:749-1425`): any test naming `handle_plan_tool`, `state_tail`, `allowed_tool_names`, `recitation`, refusal, or Planning/Executing transitions. KEEP tests for the single-mode Todo engine (`handle_todo_tool`, `todo_tail`, `single_mode_tool_names`).

- [ ] **Step 4: Build + test.** Run `cargo build 2>&1` (expect clean), then `cargo test --lib plan 2>&1` (single-mode tests pass; dead-machine tests gone). Then `cargo clippy --all-targets` and `cargo test` (full).
- Expected: green; net a large line reduction (~800+ lines).

- [ ] **Step 5: Commit** — `refactor(agent): delete the dead two-state plan machine`

### Task A2: Purge stale docs & dangling references

**Files:**
- Modify: `src-tauri/src/agent/plan.rs`, `src-tauri/src/agent/mod.rs`

- [ ] **Step 1: Fix dangling `plan_system_prompt` refs.** `plan_system_prompt` does not exist. Fix or delete its mentions in doc comments at `plan.rs:11`, `plan.rs:435` (may have moved after A1), and `mod.rs:27` — point to `single_mode_system_prompt` / `build_single_mode_system_prompt` instead.

- [ ] **Step 2: Update stale type docs** describing the deleted grammar/text-parse world: the `ToolExecution` doc (`mod.rs:194-208`, "grammar-constrained"/"twenty AddStep calls"), the `TurnOutcome` field docs, and the `requires_tool_call` doc (`mod.rs:218-230`, "the flat benchmark harness"). Reword to the current reality: the server owns tool-calling; a Require turn with no tool call is a retriable correction. Do NOT change any code — comments only.

- [ ] **Step 3: Build + verify no code changed.** `cargo build` clean; `git diff` shows only comment lines.

- [ ] **Step 4: Commit** — `docs(agent): retire comments describing the deleted grammar/parse stack`

### Task A3: Fix the doubled `#[test]` attribute

**Files:**
- Modify: `src-tauri/src/agent/dispatch.rs`

- [ ] **Step 1: Delete the duplicate.** `dispatch.rs:1361` and `1363` both annotate `update_with_content_creates_the_file_like_write` with `#[test]`. Remove one so the function carries exactly one `#[test]`.
- [ ] **Step 2: Verify.** `cargo clippy --all-targets 2>&1 | rg 'duplicated attribute'` returns nothing; `cargo test --lib update_with_content_creates_the_file_like_write` passes.
- [ ] **Step 3: Commit** — `fix(dispatch): remove a duplicated #[test] attribute`

### Task A4: Unify the duplicated tool-result staging block

**Files:**
- Modify: `src-tauri/src/commands/agent.rs` (extract helper; two call sites ~`1309-1354` top-level and ~`940-985` subagent)

**Context:** Both sites call `context::payload::stage_tool_result` with near-identical surrounding logic (the Read carve-out, `payloadRef` stamping). This is a behavior-preserving refactor that gives C2 a single seam.

**Interfaces:**
- Produces: `fn stage_and_prepare_tool_result(app_data_dir, conversation_id, tool_call_id, tool_name, outcome, engine) -> (model_text, detail_json_with_payload_ref)` (exact params to match what both sites currently thread — the implementer reads both blocks and lifts the common shape; the Read carve-out stays inside the helper, gated on `tool_name == "Read"`).

- [ ] **Step 1: Characterize current behavior.** Read both staging blocks (`commands/agent.rs:1309-1354`, `940-985`). Identify every difference (there should be none of substance — recon says "near-verbatim"). Note any real divergence in the report before collapsing.

- [ ] **Step 2: Extract the helper.** Create one function capturing the shared staging + Read carve-out + `payloadRef` stamping. Both call sites call it.

- [ ] **Step 3: Verify byte-for-byte behavior.** No test should change. Run the existing agent/dispatch tests + any payload tests: `cargo test --lib payload && cargo test --lib agent`. Add a focused unit test only if a pure helper is testable in isolation (e.g. the carve-out decision) — otherwise rely on existing coverage and note it.

- [ ] **Step 4: Build + full test.** `cargo build`/`clippy`/`test`/`fmt` green.
- [ ] **Step 5: Commit** — `refactor(agent): unify the top-level and subagent tool-result staging`

---

## Phase B — Budgeting soundness

### Task B1: Restore an output-token cap (clamp) + adaptive escalation

**Files:**
- Modify: `src-tauri/src/inference/http.rs` (`ChatRequest` + `build`)
- Modify: `src-tauri/src/inference/mod.rs` (add `token_estimate` seam)
- Modify: `src-tauri/src/commands/agent.rs` (fill `max_tokens` at the two build sites + escalation in the Require correction)
- Modify: `src-tauri/src/agent/mod.rs` (adaptive escalation in `run_loop`)
- Modify: `src-tauri/src/context/limits.rs` (revive `SUMMARY_MAX_TOKENS` usage; add `MIN_OUTPUT_TOKENS`, `ESCALATED_MAX_TOKENS`)
- Test: `src-tauri/src/inference/http.rs` (serialization), `src-tauri/src/context/limits.rs` (clamp math)

**Interfaces:**
- `ChatRequest` gains `#[serde(skip_serializing_if = "Option::is_none")] pub max_tokens: Option<u32>`.
- Produces: `fn clamp_output_tokens(ceiling: u32, window: u32, prompt_estimate: u32) -> u32` in `context::limits` (or `inference`): `min(ceiling, max(MIN_OUTPUT_TOKENS, window.saturating_sub(prompt_estimate + margin)))`, `margin = max(1024, window/20)`, `MIN_OUTPUT_TOKENS = 512`.
- `ChatRequest::build` signature UNCHANGED (max_tokens defaults `None` — the seed pattern from `da6321c`); callers set `req.max_tokens = Some(...)` after building, OR add a `build_with_output_cap`. Choose the field-set approach (matches the existing `req.seed = Some(..)` test pattern) to avoid touching every call site.

- [ ] **Step 1: Failing test — the field serializes when set, omitted when None.** In `http.rs` tests, mirror the existing `chat_request_serializes_seed_when_set`: build a request, set `req.max_tokens = Some(2048)`, assert `v["max_tokens"] == 2048`; and in the defaults test assert `v.get("max_tokens").is_none()` when unset. Run — fails (no field).
- [ ] **Step 2: Add the field.** Add `max_tokens: Option<u32>` to `ChatRequest` (skip-serialize-if-none), default `None` in `build`. Run Step-1 tests — pass.

- [ ] **Step 3: Failing test — clamp math.** In `limits.rs` tests: `clamp_output_tokens(2048, 16384, 4000)` returns 2048 (ceiling wins, headroom ample); `clamp_output_tokens(2048, 16384, 15000)` returns `MIN_OUTPUT_TOKENS` (512) (headroom exhausted → floor); `clamp_output_tokens(2048, 16384, 13000)` returns `16384 - 13000 - max(1024,819) = 2360`→ but capped by ceiling 2048, so 2048. Pick assertions that pin the three regimes (ceiling / middle / floor). Run — fails.
- [ ] **Step 4: Implement `clamp_output_tokens`** + consts `MIN_OUTPUT_TOKENS = 512`, `ESCALATED_MAX_TOKENS = 4096`. Run Step-3 — pass.

- [ ] **Step 5: Wire the cap at the agent build sites.** At `commands/agent.rs:729` (RealBackend) and the subagent build (~883), after building `req`, set `req.max_tokens = Some(clamp_output_tokens(AGENT_TURN_MAX_OUTPUT_TOKENS, engine.context_window(), prompt_estimate))`, where `prompt_estimate` = `inference::token_estimate(&rendered_or_joined_messages)` (the new seam — for now delegates to `engine.count_tokens`). At the summarization build (`context/mod.rs:403`), set `req.max_tokens = Some(SUMMARY_MAX_TOKENS as u32)`.

- [ ] **Step 6: Add the `token_estimate` seam.** In `inference/mod.rs`, add `pub fn token_estimate(engine: &InferenceEngine, text: &str) -> u32` that currently calls `engine.count_tokens(text)` (falling back to `text.len()/4` on error). B4 will re-point this to pure chars/4. All new estimation goes through this one function.

- [ ] **Step 7: Adaptive escalation in run_loop.** In `agent/mod.rs` run_loop's no-tool-call-under-Require branch (`mod.rs:293-325`), when `finish_reason == "length"`, on the FIRST such occurrence for the turn, retry the generate once with the escalated ceiling before falling back to the "keep thinking brief" correction. Implementation: the backend needs to accept an output-cap override. Simplest: add an `escalate: bool` to a re-generate path, or have `generate` read a per-turn ceiling. The implementer picks the least-invasive wiring (likely: run_loop tracks a per-turn `escalated` flag and, on a length finish, re-calls `backend.generate` once with escalation signalled via a new `AgentBackend::generate_with_cap` or a field on the context). Keep the existing correction as the fallback after one escalation.
- [ ] **Step 8: Test the escalation control flow** with a scripted backend: a backend that returns `finish_reason:"length"` + empty tool_call once, then a real tool_call, must (a) not end the task, (b) retry with escalation, (c) succeed on the second attempt within one logical turn. Add to `agent/mod.rs` tests alongside the Require-invariant suite.

- [ ] **Step 9: Full build/clippy/test/fmt.** The `limits.rs` proportionality test may need updating for the new consts — keep it meaningful.
- [ ] **Step 10: Commit** — `feat(inference): clamp output tokens and escalate on length-truncated turns`

### Task B2: Derive the context window from the launch arg

**Files:**
- Modify: `src-tauri/src/inference/server.rs` (expose the ctx-size)
- Modify: `src-tauri/src/inference/mod.rs` (`context_window`)
- Modify: `src-tauri/src/context/limits.rs` (doc; `CONTEXT_WINDOW_TOKENS` becomes derived or documented as the reserve-adjusted input budget)

**Context:** Today `context_window()` returns the hardcoded `CONTEXT_WINDOW_TOKENS = 16384`; the server is launched with a separate `--ctx-size 20480` literal in `server.rs`. Make the server ctx-size the single source; the input budget = server_ctx − OUTPUT_RESERVE.

**Interfaces:**
- Produces: `pub const SERVER_CTX_SIZE: u32 = 20480;` in `inference::server` (replacing the inline `20480` in `launch_args`), and `pub const OUTPUT_RESERVE_TOKENS: u32` (e.g. 4096) such that `input_budget = SERVER_CTX_SIZE - OUTPUT_RESERVE_TOKENS = 16384` — preserving today's 16384 exactly so no threshold shifts.
- `context_window()` returns `server::SERVER_CTX_SIZE - server::OUTPUT_RESERVE_TOKENS`.

- [ ] **Step 1: Failing test — the two are coupled.** In `server.rs` (or `inference` tests): assert `launch_args` contains `--ctx-size` `SERVER_CTX_SIZE.to_string()`, and assert `CONTEXT_WINDOW_TOKENS == SERVER_CTX_SIZE - OUTPUT_RESERVE_TOKENS`. Run — fails (constants not linked).
- [ ] **Step 2: Introduce the constants.** Add `SERVER_CTX_SIZE`/`OUTPUT_RESERVE_TOKENS` to `server.rs`; make `launch_args` use `SERVER_CTX_SIZE`; redefine `CONTEXT_WINDOW_TOKENS` (in `inference/mod.rs`) as `server::SERVER_CTX_SIZE - server::OUTPUT_RESERVE_TOKENS` (value stays 16384). Update the `context_window()` doc (drop "currently always the constant"). Run Step-1 — pass.
- [ ] **Step 3: Full build/test.** Every budget read still sees 16384 → no threshold behavior change. Confirm `cargo test` green (limits proportionality test still holds since 16384 is unchanged).
- [ ] **Step 4: Commit** — `refactor(inference): derive the context window from the sidecar ctx-size`

### Task B3: Consume the API `usage` for turn-to-turn accounting

**Files:**
- Modify: `src-tauri/src/agent/mod.rs` (thread `TurnOutcome.usage` out of the loop)
- Modify: `src-tauri/src/commands/agent.rs` (record last API prompt tokens; feed the pre-send estimate)
- Modify: `src-tauri/src/context/mod.rs` (accept an authoritative `tokens_used` override where available)

**Context:** The server reports `usage=(prompt,completion)` (`http::ChatOutcome.usage`), carried in `TurnOutcome.usage` (`agent/mod.rs:68`) but ignored. Today accounting re-tokenizes the whole history locally every measure. Make the last API `prompt_tokens` the authoritative "tokens used" after each turn; use the local estimate only for the first send and the pre-send delta.

**Interfaces:**
- Produces: a per-conversation `last_prompt_tokens: Option<u32>` cache (in the backend/session state) updated from `outcome.usage` after each `generate`.
- `usage_from_history`/the pre-turn `measure` path gains an authoritative-count fast path: when `last_prompt_tokens` is known, `tokens_used = last_prompt_tokens + token_estimate(messages added since that send)`, instead of a full re-tokenization.

- [ ] **Step 1: Failing test — usage is threaded.** In `agent/mod.rs` tests, a scripted backend returns `usage=Some((1234,50))`; assert the loop exposes/records 1234 as the last prompt-token count (via whatever accessor B3 introduces). Run — fails.
- [ ] **Step 2: Thread the usage.** Wire `outcome.usage` into a recorded `last_prompt_tokens`. Update it after every non-error, non-cancelled turn.
- [ ] **Step 3: Failing test — the measure fast path.** Given a known `last_prompt_tokens = 1000` and one added tool result of estimated size E, `measure` returns `1000 + E` (not a full re-tokenization). Assert via a small unit around the measure helper. Run — fails.
- [ ] **Step 4: Implement the fast path** in the measure/threshold computation; first send (no `last_prompt_tokens`) falls back to the existing whole-history local estimate.
- [ ] **Step 5: Build/clippy/test/fmt** green; note in the report that the local re-tokenization is now a first-send/gap-filler path only (sets up B4).
- [ ] **Step 6: Commit** — `feat(context): use the server's reported prompt-token usage as authoritative`

### Task B4: Drop llama-cpp-2; local counting becomes chars/4

**Files:**
- Modify: `src-tauri/src/inference/mod.rs` (delete tokenizer surface; `token_estimate` → chars/4; `InferenceEngine` reduced/removed)
- Modify: `src-tauri/src/inference/dialect.rs` (the render pair becomes dead once counting no longer renders — assess deletion vs SP3)
- Modify: `src-tauri/Cargo.toml` (remove `llama-cpp-2`)
- Modify: call sites of `InferenceEngine::load`/`count_tokens`/`render_chat_prompt`/`fit_to_context`/`context_window`/`dialect`

**Context:** After B3, local counting is only ever a rough estimate. Replace it with chars/4 and delete the vocab-only load + tokenizer. `context_window()`'s value (B2) must survive as a plain const/function independent of a loaded model. `engine.dialect()` disappears → pin the prompt `dialect` param to `HermesJson` (SP3 removes the param entirely).

**Interfaces:**
- `token_estimate(text) -> u32` becomes pure: ASCII-fast-path `ceil(len/4)`; for strings with multibyte chars, `ceil(chars * 1.1)` for the non-ASCII portion (qwen-code's `textTokenizer` heuristic). No `engine` param.
- `context_window()` becomes a free function/const (no `&self`): `SERVER_CTX_SIZE - OUTPUT_RESERVE_TOKENS`.
- `fit_to_context`'s render-and-recount loop is replaced by the estimate-only `context::fit_to_budget` (whole-message drop by estimated cost) — no chat-template render.

- [ ] **Step 1: Failing test — chars/4 estimate.** Unit: `token_estimate("")==0`; `token_estimate("abcd")==1`; `token_estimate("a".repeat(400))==100`; a multibyte string estimates higher than `len/4`. Run — fails (function still delegates to count_tokens).
- [ ] **Step 2: Re-point `token_estimate`** to the pure heuristic. Run Step-1 — pass. Calibration note: after landing, measure a few real Qwen3.5 prompts' API `prompt_tokens` vs char count and record the observed ratio in the report (adjust the divisor if systematically off — target a conservative UNDER-estimate margin ≤ ~15%).
- [ ] **Step 3: Remove the tokenizer surface.** Delete `InferenceEngine::render_chat_prompt`, `count_tokens`, `fit_to_context`, `dialect`, the `load` vocab-only body, and the `model`/`backend`/`dialect` fields. If nothing else needs `InferenceEngine`, delete the struct and update `AppState`/callers to drop it; otherwise reduce it. Re-point `fit_turn_to_budget`/`fit_to_budget` (context/mod.rs) to estimate-only via `token_estimate` (they already have `fit_to_budget`'s estimate pass — drop the render-recount tail).
- [ ] **Step 4: Pin the dialect.** Wherever `engine.dialect()` fed the prompt builder or `text_for`, substitute `ToolDialect::HermesJson` literally. `ChatMessage::text()` already defaults to Hermes; ensure no remaining caller needs a runtime dialect.
- [ ] **Step 5: Remove the dependency.** Delete `llama-cpp-2` from `Cargo.toml`; `cargo build` to prune `Cargo.lock`. Fix every compile error from the removed surface (the compiler lists them). The dialect *render* pair (`render_tool_use`/`render_tool_result`) is now unused for counting — if it has no other caller, delete it (this is the counting-path half of ToolDialect; `call_format_instructions` stays until SP3). Confirm via `rg`.
- [ ] **Step 6: Re-point measurement tests** from the vocab tokenizer to the estimate. Any test asserting exact token counts becomes an estimate assertion (ranges, not exact). The kept real-model smokes in `tests/real_model_smoke.rs` that exercised `count_tokens`/`render_chat_prompt` are deleted or converted (they tested the now-removed surface).
- [ ] **Step 7: Real-session sanity check.** With the sidecar (via `tests/common::TestServer`) or a manual run, confirm a multi-turn session still triggers compaction at a sensible point and no request exceeds the window (B1's clamp guarantees validity; this checks the estimate isn't wildly off). Record the observed estimate-vs-API-usage delta.
- [ ] **Step 8: Full build/clippy/test/fmt** green with `llama-cpp-2` absent from `Cargo.toml`.
- [ ] **Step 9: Commit** — `refactor(inference): drop llama-cpp-2 for a chars/4 token estimate`

---

## Phase C — Robustness

### Task C1: Compaction fail-safe + circuit breaker

**Files:**
- Modify: `src-tauri/src/context/mod.rs` (`summarize_and_persist`, `maybe_compact`)
- Modify: `src-tauri/src/context/limits.rs` (`MAX_CONSECUTIVE_COMPACTION_FAILURES = 3`)
- Test: `src-tauri/src/context/mod.rs` tests

**Context:** `summarize_and_persist` (`context/mod.rs:383-425`) persists whatever the model returns — no rejection of empty/inflated/truncated summaries, no circuit breaker. A small local model can produce a bad summary and silently corrupt context.

**Interfaces:**
- `summarize_and_persist` returns `Ok(None)` (history untouched, no notice persisted) on rejection instead of persisting; it must surface WHY (an enum or a logged reason) so `maybe_compact` can count failures.
- A per-conversation `consecutive_compaction_failures` counter (persisted with context state, or in the settings/notice table) gates auto-compaction: at `>= MAX_CONSECUTIVE_COMPACTION_FAILURES`, the non-forced path NOOPs; a `force=true` success resets it to 0.

- [ ] **Step 1: Failing tests — the three rejections.** With a fake summarization (inject the model's returned text via the `base_url` client — or refactor `summarize_and_persist` to take the raw summary string for unit testing, whichever the implementer finds cleaner): (a) empty-after-strip summary → returns `Ok(None)`, no `context_notice` row persisted, history unchanged; (b) inflated — estimated post-compaction tokens ≥ pre-compaction → `Ok(None)`, not persisted; (c) truncated — the summarization `ChatOutcome.finish_reason == "length"` → `Ok(None)`, not persisted. Run — fail (all currently persist).
- [ ] **Step 2: Implement the guards** in `summarize_and_persist`: strip + empty-check; compute pre/post estimate via `token_estimate` and reject inflation; thread `finish_reason` from the `ChatOutcome` and reject `"length"`. On any rejection, return `Ok(None)` WITHOUT persisting. Run Step-1 — pass.
- [ ] **Step 3: Failing test — circuit breaker.** Three consecutive rejected auto-compactions → the next non-forced `maybe_compact` NOOPs (no summarization call attempted); a subsequent `force=true` that succeeds resets the counter so auto works again. Run — fails.
- [ ] **Step 4: Implement the breaker** in `maybe_compact`: track consecutive failures; skip the tier-2 attempt when the breaker is open and `!force`; reset on a successful (non-rejected) summarization. Persist the counter so it survives reloads (mirror how `ContextSettings`/notices persist). Run Step-3 — pass.
- [ ] **Step 5: Surface a warn state.** When the breaker is open, `usage.state` reflects a warning (e.g. `"compactionStalled"`) so the UI can show it. Add a minimal assertion.
- [ ] **Step 6: Build/clippy/test/fmt** green.
- [ ] **Step 7: Commit** — `feat(context): reject bad compactions and add a consecutive-failure circuit breaker`

### Task C2: Bound the Task subagent result

**Files:**
- Modify: `src-tauri/src/commands/agent.rs` (~`1248-1263`, the Task `persist_tool_result`)

**Context:** A subagent's `sub_final` is persisted as the parent's Task tool_result with no offload gate — a verbose answer floods the parent's window. Route it through the A4 staging helper.

- [ ] **Step 1: Failing test — over-threshold Task result offloads.** A subagent final answer whose `token_estimate` exceeds `DEFAULT_TOOL_OUTPUT_OFFLOAD_TOKENS` should, once persisted, present to the parent model as a reference line (not the whole text), with a `payloadRef` written. Construct the smallest test around the Task-result persistence path (may require a helper seam). Run — fails (currently persisted raw).
- [ ] **Step 2: Route through the helper.** Call the A4 `stage_and_prepare_tool_result` (or `context::payload::stage_tool_result` directly) on `sub_final` before persisting the parent Task tool_result, with `tool_name = "Task"`. Run Step-1 — pass.
- [ ] **Step 3: Build/clippy/test/fmt** green.
- [ ] **Step 4: Commit** — `fix(agent): offload oversized Task subagent results like any other tool output`

### Task C3: Log discarded surplus tool_calls

**Files:**
- Modify: `src-tauri/src/inference/http.rs` (`ToolCallAccum::finish`, ~`497-505`)

**Context:** `finish()` takes the lowest-index tool_call and silently discards any others. With `parallel_tool_calls:false`, >1 is a server anomaly worth surfacing.

- [ ] **Step 1: Failing test — surplus is logged, first is kept.** Feed `ToolCallAccum` two fragments at different indices; `finish()` returns the index-0 call AND the accumulator records/exposes that it discarded 1 (assert via a returned count or a captured log). Keep the return contract (first-only). Run — fails.
- [ ] **Step 2: Implement** — keep first-only; add `log::warn!` with the discarded count + names when `> 1` bucket is present. If a testable signal is needed, have `finish` also return/expose the discarded count. Run Step-1 — pass.
- [ ] **Step 3: Build/clippy/test/fmt** green.
- [ ] **Step 4: Commit** — `feat(inference): warn instead of silently dropping surplus tool_calls`

---

## Self-review checklist (run before dispatching Task 1)

- **Spec coverage:** A (deletions) ✓, B (max_tokens+escalation, window derive, API usage, drop tokenizer) ✓, C (fail-safe, Task bound, tool_calls log) ✓. The small SOTA refinements assigned to SP1 (adaptive escalation) land in B1; the rest are SP2.
- **No prompt-byte change:** confirmed — SP1 touches sampling params, accounting, deletions, internals only. The `<tools>`/`call_format_instructions` block and the summary prompt text are untouched (SP3).
- **Type consistency:** `token_estimate` is the single estimation seam (introduced B1, re-pointed B4); `context_window`/`SERVER_CTX_SIZE`/`OUTPUT_RESERVE_TOKENS` keep 16384 exact so no threshold shifts; `clamp_output_tokens` used at all three build sites.
- **Ordering:** A→B→C; within B, B4 (dependency removal) last; C2 depends on A4's helper.
