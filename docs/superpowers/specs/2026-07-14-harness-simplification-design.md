# SOTA Context Management & Prompt Engineering — Program Spec

**Status:** design (approved to spec 2026-07-14)
**Goal:** Bring doce's agent harness to **state-of-the-art context management and prompt engineering, comparable to claude-code and qwen-code** — now that the llama-server sidecar's OpenAI-compatible API owns generation, tool-calling, and token usage.

**Architecture:** doce is a Tauri v2 + Rust local coding agent. Post-cutover, generation and tool-calling go through a llama-server sidecar via `/v1/chat/completions` (streaming SSE, structured `tool_calls`, `tool_choice:"required"`). The in-process llama-cpp-2 model is currently loaded vocab-only for token counting; this program removes it (the server reports authoritative usage) and adds the context/prompt machinery that closes the gap to claude-code/qwen-code.

**Tech Stack:** Rust (tokio, serde, reqwest/SSE), Tauri v2, SQLite (message store), the llama-server sidecar.

## Global Constraints

- **Single model.** doce ships one tier (`qwen3.5-4b-q4_k_m`); cross-dialect detection (MiniCPM/Hermes) is unneeded — Qwen3.5 is Hermes-style. Any dialect selection may be pinned to `HermesJson`.
- **Benchmark-gate prompt changes.** Any change to the bytes of the system prompt or summarization prompt is benchmark-gated (project rule): it runs through the tier-ladder A/B (`scripts/benchmark-cutover-gate.md`) before landing. This confines the prompt-engineering sub-project behind the gate; the cleanup/budgeting/context sub-projects change no prompt bytes.
- **Formatter is oxfmt, not prettier** (frontend). `bindings.ts` is git-ignored (never committed).
- **API `usage` is authoritative.** The server returns `(prompt_tokens, completion_tokens)` in the SSE trailer (`http::ChatOutcome.usage`); local estimation is a conservative gap-filler only (first send + pre-send delta), never used to *skip* a budget check.
- **Each sub-project is its own cycle.** Brainstorm → spec → plan → subagent-driven build, in the sequence below. This document details **SP1** (ready to plan) and outlines SP2–SP4 (detailed in their own cycles).

---

## Program decomposition

Four sequenced sub-projects. **SP1** is the foundation and is fully specified here; **SP2/SP3** build on it (SP3 is benchmark-gated); **SP4** is a large, largely-independent axis that comes last.

| # | Sub-project | Gated? | Depends on |
|---|---|---|---|
| SP1 | Harness cleanup & budgeting soundness | no | — |
| SP2 | SOTA context management | no | SP1 |
| SP3 | Prompt engineering | **benchmark** | SP1 (B4) |
| SP4 | Auto-memory subsystem | no | SP1–SP3 land first |

**Sequence:** SP1 → (SP2 ‖ SP3, SP3 through the gate) → SP4.

### Background: what the cutover left behind

Recon (three parallel maps, 2026-07-14) established doce is already substantially qwen-code-like: two-tier compaction (non-LLM stale-tool clearing → LLM summarization), spill-to-file + re-read pointers for big tool outputs (`context::payload`), per-tool output caps, keep-first + protected-recent. So SP1 is *close the gaps + delete the vestige + fix post-cutover divergences*; SP2–SP4 add the SOTA techniques doce still lacks. Key anchors carried into the sub-projects below.

---

## SP1 — Harness cleanup & budgeting soundness (ready to plan)

**Resolved decisions:** (1) Full accounting — API `usage` authoritative, local counting estimate-only. (2) Context window derived from the launch `--ctx-size`, one source of truth. (3) Drop llama-cpp-2 for a chars/4 heuristic.

### Phase A — Safe deletions (no behavior change)

- **A1. Delete the dead two-state plan machine** (`agent/plan.rs`, self-declared dead `plan.rs:150-157`, zero production callers): `LoopState`, `handle_plan_tool`, `state_tail`, `allowed_tool_names`, `recitation_text`, `PLAN_TOOL_NAMES`, the `*_ALLOWED_TOOLS[_NO_*]` consts, `REFUSAL_*` consts, `PlanState` fields `state`/`refusal_context`/`refusal_count`, and their tests (~`plan.rs:749-1425`). **Keep** `Plan`/`PlanStep`/`next_undone_step`/`has_plan` and the single-mode Todo surface. Grep-confirm zero non-test callers per symbol.
- **A2. Purge stale docs & dangling refs** describing the deleted grammar/parse world (`ToolExecution` `mod.rs:194-208`, `TurnOutcome` field docs, `requires_tool_call` doc; dangling `plan_system_prompt` refs at `plan.rs:11,435`, `mod.rs:27`).
- **A3. Fix the doubled `#[test]`** at `dispatch.rs:1361/1363`.
- **A4. Unify the duplicated staging block** (top-level `commands/agent.rs:1309-1354` + subagent `940-985`) into one helper (sets up C2).

### Phase B — Budgeting soundness (Full accounting)

- **B1. Restore an output-token cap + adaptive escalation.** Add `max_tokens: Option<u32>` to `ChatRequest` (skip-serialize when `None`), sized by the clamp `min(ceiling, max(MIN_OUTPUT, window − prompt_est − margin))`, `margin = max(1024, 5% window)`, so `prompt + max_tokens ≤ window` is structural. Ceilings: `AGENT_TURN_MAX_OUTPUT_TOKENS` (2048) for turns, `SUMMARY_MAX_TOKENS` (revived; 1024 now, raised in SP3) for summarization. **Adaptive escalation:** on `finish_reason:"length"`, retry once with an escalated ceiling before falling back to the "keep it brief" correction (qwen-code's `ESCALATED_MAX_TOKENS`).
- **B2. Derive `context_window()` from the launch arg** (`--ctx-size` at `server.rs:70`, minus an output reserve) instead of the standalone `CONTEXT_WINDOW_TOKENS = 16384`. `context_window()` stays the single chokepoint.
- **B3. Consume the API `usage`** — wire `TurnOutcome.usage` (`agent/mod.rs:68`, carried-but-ignored) so turn-to-turn "tokens used" is the server's `prompt_tokens`. Pre-send estimate for turn N+1 = `last_api_prompt_tokens + estimate(added content)`; first send = whole-prompt local estimate.
- **B4. Drop llama-cpp-2; chars/4 estimate.** Replace `count_tokens` with chars/4 (ASCII `len/4`, higher ratio for multibyte; calibrate for Qwen3.5). Delete the vocab-only startup load, `render_chat_prompt`, the `count_tokens` tokenizer body, the counting-only dialect render path (`render_tool_use`/`render_tool_result` go dead), and the `llama-cpp-2` dep from `Cargo.toml`. `engine.dialect()` disappears → pin the prompt's `dialect` param to `HermesJson` until SP3 removes it. Highest-risk step; land last within B, tests re-pointed to the chars/4 estimator + a real-session sanity check.

### Phase C — Robustness

- **C1. Compaction fail-safe + circuit breaker** (`context/mod.rs:383-522`): reject empty / inflated (post ≥ pre) / truncated (`finish_reason:"length"`) summaries → history untouched, increment consecutive-failure counter; after 3, auto-compaction NOOPs until a `force=true` compaction succeeds + resets; surface a warn state.
- **C2. Bound the Task subagent result** — route `sub_final` (`commands/agent.rs:1248-1263`) through the unified `stage_tool_result` (A4) so an over-threshold subagent answer offloads like any other tool output.
- **C3. `>1 tool_calls` policy** — keep documented first-only (`http.rs:497-505`) but `log::warn` the discarded count/names instead of silent drop.

**SP1 acceptance:** `cargo build`/`clippy --all-targets`/`test`/`fmt` green per phase; `llama-cpp-2` gone from `Cargo.toml`; run-loop Require-invariant tests (`mod.rs:594-716`) unchanged; new tests for the compaction guards + Task offloading; a real sidecar session shows compaction still triggers sensibly on API usage + chars/4. No prompt bytes change.

---

## SP2 — SOTA context management (outline; own cycle, builds on SP1)

- **Restore-recent-files after compaction** (qwen-code `postCompactAttachments`): after summarization, re-read the last N *touched files* fresh from disk and reattach as one post-summary user turn, instead of keeping the last 10 turns verbatim-and-stale. The agent resumes on *current* file contents. Bounded (N files, workspace-root-filtered), token-cost folded into the inflation guard.
- **Microcompaction time/char triggers**: extend the tier-1 stale-tool clearing (`apply_lightweight_clearing`) beyond keep-N with a stale-age trigger (clear tool results older than X minutes) and a total-tool-result-char budget, à la qwen-code's `toolResultsThresholdMinutes` / `toolResultsTotalCharsThreshold`.
- **Session-wide disk-write budget** for offloaded tool outputs (`context::payload`): cap total on-disk spill per session with a synchronously-reserved-before-write counter (race-safe under parallel tool calls); graceful degradation + rollback on write failure.
- **Idempotent truncation sentinel**: stable prefix so a re-truncated output passes through unchanged (no nested headers / duplicate spill).

## SP3 — Prompt engineering (outline; own cycle, **benchmark-gated**)

- **Remove the double tool-description**: delete the `<tools>` block + `call_format_instructions` from `build_single_mode_system_prompt` (`plan.rs:194-207`); cascade-delete `SINGLE_MODE_TOOL_LINES`, the `*_TOOL_LINE` consts, `ToolDialect::call_format_instructions`, and the `dialect` prompt-param chain (OnceLock 4→2 cells). `http::tool_def` becomes the single schema authority; with B4's render path dead, this **fully deletes `ToolDialect`**.
- **9-section `<state_snapshot>` summarization prompt** (primary intent, key concepts, files+code, errors+fixes, problem-solving, all user messages, pending tasks, current work, next step) replacing the one-line `SUMMARIZATION_PROMPT`; raise summary `max_tokens` (~4096).
- **Project-instructions file** (a `CLAUDE.md`/`AGENTS.md` equivalent — confirmed absent today): discover up the directory tree, `@import` resolution, pin the concatenated result into the system prompt *outside* the compaction pool so a hard user constraint is never summarized away (qwen-code's `userMemory` model). Bill it every turn, keep it user-curated/small.
- **System-prompt + tool-description quality pass**: review and upgrade the agent system prompt and tool descriptions to claude-code/qwen-code caliber (tool-use guidance, worked constraints, recitation/todo framing).
- **Gate:** all of SP3 runs through the tier-ladder A/B; planned-tier median must not regress. Co-evaluates with the cutover benchmark gate.

## SP4 — Auto-memory subsystem (outline; own cycle, last)

- Disk-backed long-term memory modeled on qwen-code's `memory/*` (extract/recall/forget/dream) + doce's existing MEMORY.md-style index: after sessions, extract durable facts (user prefs, corrections, project context) into small per-project markdown files; recall/inject relevant memories at session start into a bounded block *outside* compaction; a periodic dedup/prune pass; manual `remember`/`forget`. Lazy-load memory bodies via a read pointer (index in the prompt, bodies read on demand), protected from microcompaction so the model doesn't silently "forget" a just-loaded memory. Large, independent; specified in full in its own cycle once SP1–SP3 land.

---

## Testing strategy

- SP1/SP2/C add/keep Rust unit tests; the run-loop Require-invariant suite is non-negotiable coverage that must stay green. B4 re-points threshold/compaction tests to the chars/4 estimator + a real-session sanity check.
- SP3 is validated by the benchmark A/B, not unit tests.
- SP4 gets its own test surface (extraction/recall/forget round-trips) in its cycle.
- Each phase ends `cargo build`/`clippy --all-targets`/`test`/`fmt` green.

## Risks & mitigations

- **B4 (dependency removal) is the highest-risk change** — touches startup + every budget decision. Mitigation: land after B1–B3 green; API-usage (B3) is the authoritative signal so a coarser local estimate only affects *when* compaction triggers, never the correctness of the sent request (B1's clamp guarantees `prompt + output ≤ window` regardless of estimate error).
- **chars/4 mis-estimation** shifts compaction timing → calibrate for Qwen3.5; hard-limit reactive tier is the safety net.
- **SP3 depends on the benchmark verdict** — if the cutover gate regresses, SP3's prompt cleanup is the first lever, so they co-evaluate.
- **Scope** — SP4 is large; keeping it a separate final cycle prevents it from blocking the high-value SP1–SP3 wins.

## Open questions

None blocking SP1. chars/4 calibration for Qwen3.5 is resolved during B4 (measure real prompts' API `prompt_tokens` vs char count). SP2–SP4 design details are deferred to their own brainstorm cycles.
