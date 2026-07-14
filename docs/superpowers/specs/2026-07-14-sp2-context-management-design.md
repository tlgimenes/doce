# SP2 — SOTA Context Management (design spec)

> Sub-project 2 of the harness-simplification program
> (`2026-07-14-harness-simplification-design.md`). SP1 (cleanup & budgeting)
> is complete and landed. SP2 is **un-gated** (changes NO prompt bytes) and
> fully autonomous. Prompt-engineering work is SP3 (benchmark-gated), separate.

**Goal:** Bring doce's context management to parity with claude-code / qwen-code
on the dimensions SP1 left open: measure the window from the server's
*authoritative* token usage (not an estimate), always let the model use the
maximum output the window allows, and preserve the file the agent was working
on across a compaction.

**Architecture:** Three focused changes to the existing `context` + `inference`
seams SP1 established — no new subsystems, no schema migration, no prompt-byte
changes, and `run_loop`'s Require-invariant stays byte-untouched.

**Tech stack:** Rust (Tauri backend), the llama-server sidecar's OpenAI-compatible
`/v1/chat/completions` (SSE, authoritative `usage` trailer), `chars/4`
estimation as the gap-filler only.

---

## Global Constraints (inherited from the program spec)

- **Single model.** Qwen3.5 (Hermes). Dialect pinned to `HermesJson`.
- **No prompt-byte changes.** SP2 changes NO system-prompt or summarization-prompt
  bytes — it stays un-gated. (SP3 owns all prompt-byte work, behind the benchmark gate.)
- **`run_loop` is sacred.** The Require-invariant (a Require-mode turn with no
  tool call = retriable correction, never done) MUST stay byte-untouched. No
  change to `run_loop`'s body or the `AgentBackend` Require/`requires_tool_call`
  semantics. (This is why adaptive in-loop escalation, B1b, is NOT in SP2 — see
  "Always-max-output" below, which achieves the same end without touching the loop.)
- **API `usage` is authoritative.** `http::ChatOutcome.usage = Some((prompt_tokens,
  completion_tokens))` from the SSE trailer is the truth; `chars/4` estimation is a
  conservative gap-filler used only where no observed usage exists yet.
- **Formatter is `cargo fmt`** (Rust). `bindings.ts` is git-ignored, never committed.
- **In-place on `main`.** A parallel FRONTEND session commits `src/views/**`; every
  SP2 commit is scoped to `src-tauri/**` only, built on current HEAD.

---

## Decisions locked (from brainstorming)

1. **Always max output** (user directive: "should always have max output"). Agent-turn
   generation always requests the maximum output that fits the window, not a fixed
   2048 cap. This removes truncation at the root, so **B1b adaptive escalation is
   dropped entirely** — you can't truncate-then-retry if you were already at max.
2. **Restore a single most-recent file, fresh content** (user directive: "not
   reference files"). After a tier-2 summary, re-read the one most-recently-`Read`
   file from disk at CURRENT contents and inject the actual content — never a path
   reference line.
3. **API-usage-authoritative measure** (B3). The compaction-trigger measure uses the
   last observed `usage.prompt_tokens` as its base, `chars/4` only for the unmeasured delta.

## Scope: what is NOT in SP2 (YAGNI — considered and deferred)

- **Micro-compaction time/char triggers.** doce's token-threshold two-tier compaction
  already fires correctly; sub-threshold time/char triggers are marginal complexity.
  Deferred until a real need appears.
- **Session-wide disk-write budget + payload GC.** The tool-output payload files
  (`<app_data_dir>/tool-outputs/<conv>/`) grow unbounded over a long session. This is a
  disk-cleanup concern, not context-management SOTA. Deferred to a housekeeping task.
- **Idempotent truncation sentinel.** With always-max-output, mid-turn truncation is
  rare; the SP1 fail-safe already rejects a truncated *summary*. Deferred.

---

## FR-1: Always-max-output for agent turns

**Problem.** `clamp_output_tokens(AGENT_TURN_MAX_OUTPUT_TOKENS=2048, window, prompt_est)`
caps every agent turn's `max_tokens` at 2048 even when the window has thousands of free
tokens. A long final answer or a large single code edit hits that cap and truncates
(`finish_reason == "length"`), and the truncated text silently becomes the turn's output.

**Design.** Make the output ceiling the *window itself*, so the existing clamp returns
`window − prompt_est − margin` (the maximum that structurally fits) rather than a flat
2048. `clamp_output_tokens` already floors at `MIN_OUTPUT_TOKENS=512` and guarantees
`prompt + max_tokens ≤ window`; only the `ceiling` argument changes.

- Add `pub const AGENT_TURN_OUTPUT_CEILING: u32 = CONTEXT_WINDOW_TOKENS;` (= 16384) in
  `limits.rs`, documented as "always-max-output: agent turns request as much output as
  fits the window; the clamp shrinks it to `window − prompt − margin`, and the sidecar's
  `OUTPUT_RESERVE_TOKENS` (4096) beyond `CONTEXT_WINDOW_TOKENS` absorbs chat-template
  overhead, so `prompt + max_tokens ≤ CONTEXT_WINDOW_TOKENS < SERVER_CTX_SIZE` always."
- At BOTH request build sites (`commands/agent.rs` RealBackend ~743 and SubagentBackend
  ~910), pass `AGENT_TURN_OUTPUT_CEILING` (not `AGENT_TURN_MAX_OUTPUT_TOKENS`) as the
  clamp ceiling. Everything else (the `prompt_est` sum, `req.max_tokens = Some(...)`) is unchanged.
- **Keep `AGENT_TURN_MAX_OUTPUT_TOKENS = 2048`** as the RESERVE input to the two plan
  hosts' `threshold` computations and `STATE_TAIL_RESERVE` envelope (it is the *reserve*
  those budgets subtract, a conservative floor — raising the actual wire `max_tokens`
  does not require inflating that reserve, since the clamp already guarantees the
  envelope holds against the real `prompt_est`). Add a doc note distinguishing the two
  roles: `AGENT_TURN_MAX_OUTPUT_TOKENS` = the budgeting reserve; `AGENT_TURN_OUTPUT_CEILING`
  = the wire ceiling.
- **Summarization is unaffected.** The tier-2 summary call keeps `SUMMARY_MAX_TOKENS`
  (~1024) — we WANT a small summary; this FR is agent turns only.

**Tests.** `clamp_output_tokens(AGENT_TURN_OUTPUT_CEILING, window, small_prompt)` returns
`window − prompt − margin` (large), not 2048; with a near-full prompt it floors at
`MIN_OUTPUT_TOKENS`; `prompt_est + result ≤ window` in every regime. A build-site test (or
the existing regime tests re-pointed) asserts the wire `max_tokens` now scales with free window.

**Failure mode addressed:** a 3000-token final answer that previously truncated at 2048
now completes; a turn with a 15000-token prompt still fits (output floored, no over-window request).

**Interaction with FR-2.** Always-max-output relies on `prompt_est` not badly *under*-counting
(if the estimate undercounts the real prompt, `real_prompt + max_tokens` could push toward
`SERVER_CTX_SIZE`). Two things keep it safe: the `OUTPUT_RESERVE_TOKENS` (4096) gap between
`CONTEXT_WINDOW_TOKENS` (16384) and `SERVER_CTX_SIZE` (20480) plus the 1024 margin give ~5120
tokens of slack against under-counting, and FR-2's authoritative `prompt_tokens` closes the
gap directly once a turn has been observed. FR-2 should therefore land with or before FR-1.

## FR-2: API-usage-authoritative measure base (B3)

**Problem.** `usage_from_history` / `usage_from_fitted_messages` / the subagent `measure`
re-estimate the ENTIRE prompt with `chars/4` every turn, even though the server already
returned the exact `prompt_tokens` it decoded on the previous turn
(`http::ChatOutcome.usage`, already captured into `TurnOutcome.usage` at `agent.rs:51`).
The estimate can drift from the truth (chat-template overhead, tokenizer specifics), and
the drift compounds the compaction-trigger decision.

**Design.** Introduce an authoritative-usage seam that prefers the last observed
`prompt_tokens` and adds only the estimated delta of messages appended since that observation.

- **In-memory per-conversation observation.** Add `pub struct LastObservedUsage(pub
  Mutex<HashMap<String, ObservedUsage>>)` app state (mirror `CompactionFailures`/`ActiveGenerations`),
  where `ObservedUsage { prompt_tokens: u32, at_message_seq: i64 }` records the server's
  `prompt_tokens` and the history length (message seq / count) it corresponded to. No DB
  schema change — session-scoped, restart resets to pure estimation, which is safe.
- **Record.** After each successful agent `generate` whose `TurnOutcome.usage` is
  `Some((p, _))`, store `ObservedUsage { prompt_tokens: p, at_message_seq: <current tail
  seq> }` for the conversation.
- **Measure.** A new `pub fn authoritative_prompt_tokens(observed: Option<ObservedUsage>,
  history: &[…], estimate_fn) -> u32`:
  - If `observed` is `None` (first turn, or post-restart): fall back to the current full
    `chars/4` estimate over `to_openai_messages` (unchanged behavior).
  - Else: `observed.prompt_tokens + estimate(messages appended after
    observed.at_message_seq)` — the authoritative base plus the estimated tail delta.
  - This is a PURE function (the unit-test surface): given an observed base and a set of
    newly-appended messages, it returns base + est(new). Test the None-fallback, the
    exact-match (no new messages → returns base), and the base+delta cases.
- **Wire it into the trigger only.** `usage_from_history` (and the subagent `measure`,
  and `usage_from_fitted_messages` where it feeds the compaction TRIGGER) consult
  `authoritative_prompt_tokens` instead of a bare full estimate. The output-clamp
  `prompt_est` at the build sites MAY stay a pure estimate (it sizes the NEXT prompt,
  which has no observed usage yet — see FR-3 note on shape alignment) — but PREFER
  routing it through the same seam for consistency where an observation exists.
- **Invalidation.** A compaction (summary replaces history) changes the prompt shape
  wholesale, so the observation is stale: CLEAR the conversation's `LastObservedUsage`
  entry inside `maybe_compact`'s `Persisted` arm (next to the `CompactionFailures` reset),
  forcing a fresh full estimate until the next real `generate` re-observes.

**Tests.** Pure `authoritative_prompt_tokens` (None→full-estimate; no-delta→base;
base+delta). Invalidation: after a `Persisted` compaction the entry is cleared (unit-test
the clear alongside the existing breaker-reset test shape).

**Failure mode addressed:** a conversation whose `chars/4` estimate under-counts by 15%
(dense non-ASCII / heavy tool-JSON) no longer trips compaction late (or early) — the
trigger tracks the server's real decoded size within one turn's estimated delta.

## FR-3: Restore the most-recent file after compaction

**Problem.** Tier-2 compaction replaces older history — including the `Read` result the
agent was actively working from — with a summary. The summary mentions the file by name
but drops its contents, so the next turn often re-`Read`s it (a wasted round-trip) or, worse,
edits from memory.

**Design.** After a summary is persisted, find the single most-recently-`Read` file among
the messages the summary replaced, re-read it fresh from disk, and inject its CURRENT
content as a post-summary context note (actual content, never a reference line — per the
"not reference files" directive).

- **Find the file.** Scan the summarized (older) span for the last `Read` tool result
  (its `detail.resolvedPath`, the same resolved absolute path SP1's staging uses). If none,
  no-op.
- **Re-read fresh.** Read the file from disk NOW (current contents, not the stale
  pre-summary snapshot) via the same capped `fs::read` path `Read` uses (so its size caps
  apply). If the file no longer exists / is unreadable, no-op (the summary still names it).
- **Inject bounded.** Persist the content as a post-summary context row (the same
  `context_notice` mechanism the summary uses, a distinct `kind: "restoredFile"`), bounded
  by `DEFAULT_TOOL_OUTPUT_OFFLOAD_TOKENS` (~1024) — if the file exceeds the cap, restore a
  head+tail window with a truncation note (never a bare reference). It splices into history
  right AFTER the summary, so the model sees: summary, then "Current contents of
  `<path>`:\n<content>".
- **Ordering & idempotency.** Exactly one restored-file note per compaction, immediately
  after the summary notice. A subsequent compaction re-evaluates from the new most-recent Read.
- **No prompt-byte impact.** The wrapper text ("Current contents of `<path>`:") is a
  context-notice body, not a system/summarization PROMPT string — SP2 stays un-gated. (It
  is data injected into history, the same category as a tool result, not a prompt template.)

**Tests.** Pure helper `most_recent_read_path(summarized_span) -> Option<PathBuf>` (last
Read's resolvedPath; None when no Read). The bounded-injection helper: content under cap →
full; over cap → head+tail + truncation note, never a reference line. Missing-file → no-op.

**Failure mode addressed:** agent Reads `foo.rs`, works for 20 turns, compaction fires; the
next turn still has `foo.rs`'s current contents inline and edits correctly without re-Reading.

## FR-4: Align build-site `prompt_est` shape (SP1 final-review minor)

The SP1 final review flagged that the two output-clamp build sites estimate `prompt_est`
as `Σ token_estimate(m.text())` (per-message rendered) while the compaction trigger uses
`token_estimate(json(to_openai_messages(...)))` (server-decoded shape). It is not a
correctness bug (the conservative window absorbs the delta), but it is an inconsistency.
With FR-2 routing the trigger through the authoritative seam, align the build-site
`prompt_est` to the same `to_openai_messages` shape (or, where an observation exists, the
authoritative base) so both the clamp and the trigger reason about the same number.
Document the choice explicitly if any divergence remains intentional.

**Tests.** A build-site `prompt_est` test asserts the estimated shape matches the trigger's
shape for the same message list.

---

## Non-goals / invariants to preserve

- `run_loop` body and Require-invariant: byte-untouched.
- No system-prompt / summarization-prompt byte changes (SP2 is un-gated).
- No DB schema migration (all new state is in-memory app state).
- The SP1 tool-result payload-file offload mechanism (A4/C2 staging) is KEPT and unchanged
  — "not reference files" applies to file RESTORATION (FR-3 injects content), not to the
  tool-result offload, which continues to reference-line over-threshold outputs.
- The sidecar (`ServerState`/`launch_args`) and `SERVER_CTX_SIZE`/`OUTPUT_RESERVE_TOKENS`
  coupling: unchanged.

## Test strategy

Every FR's decision logic is a PURE function (the unit-test surface), mirroring SP1's
`evaluate_summary`/`breaker_open`/`clamp_output_tokens` pattern: `authoritative_prompt_tokens`,
`most_recent_read_path`, the bounded-injection sizing, and the clamp-ceiling regime. The
wiring (app-state record/clear, post-summary splice) is exercised where the existing
`maybe_compact` precedent is — controller/manual validation for the DB-live paths, since
`maybe_compact` needs a live DB + sidecar (same testability boundary SP1 documented).

## Commit discipline

Each FR is its own task/commit, `src-tauri/**`-scoped, TDD (failing pure-helper test first),
`cargo build`/`clippy --all-targets`/`test`/`fmt` green, `bindings.ts` unchanged, the
parallel session's `src/views/**` files untouched.
