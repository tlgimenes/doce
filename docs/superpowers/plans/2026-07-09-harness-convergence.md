# Harness Convergence Implementation Plan (report P2-P10 + new findings)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement every remaining proposal from `docs/reports/2026-07-09-small-model-context-tooling.md` (P2-P4, P6-P10, §4.11) plus two failure modes discovered after the report shipped (seed variance dominating single-run benchmarks; regex-blind self-verification).

**Architecture:** Five phases, each independently shippable and each gated on the tier4 benchmark protocol established in Phase 0. Phases: (0) measurement, (1) small knobs, (2) convergence guardrails, (3) context spend, (4) KV-cache session + stable-prefix prompt, (5) window rebalance. The engine work (Phase 4) lands last because its correctness gate depends on everything before it.

**Tech Stack:** Rust (llama-cpp-2 0.1.150, tauri 2), the existing `agent_benchmark` tiers as the regression instrument.

**Spec:** `docs/reports/2026-07-09-small-model-context-tooling.md` (+ its §5 seven-run ladder). Audit reference: `.superpowers/sdd/harness-audit.md`.

## Global Constraints

- Every phase ends with the benchmark gate: `for s in 11 22 33; do DOCE_GEN_SEED=$s cargo test --test agent_benchmark tier4_planned -- --ignored --nocapture --test-threads=1; done` (from `src-tauri/`, real GGUF required, ~10-30 min/run) — **median score must be ≥ the previous phase's median** (record each phase's numbers in the progress ledger). Runs can exceed shell-tool timeouts: launch detached (`nohup ... > log 2>&1 &`) and poll the log.
- Baseline reality (measured 2026-07-09, post-`cbe5c42`): tier4 single-run scores vary wildly by seed (trace-verified ~20/20 in one run; 0/20 in another where all 20 operator-edits landed but `// BUG:` comment-removal was skipped and the model's verification grep used an unescaped `+`). No conclusions from single runs.
- All cargo commands run from `/Users/gimenes/code/doce/src-tauri`.
- Formatter for any TS file is oxfmt (`npm run format:check`), never prettier. Rust: `cargo clippy --lib --tests` must stay clean.
- Commit messages: conventional prefixes, ending with `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.
- `CONTEXT_WINDOW_TOKENS` stays 8192 until Phase 5; all Phase 0-4 work must not change it.

---

## Phase 0 — Measurement first (P10 + scorer diagnostics)

### Task 1: Deterministic generation seed

**Files:**
- Modify: `src-tauri/src/inference/mod.rs` (seed derivation in `generate()`)

**Interfaces:**
- Produces: env var `DOCE_GEN_SEED` (u32) — when set and parseable, every `generate()` call uses it verbatim; otherwise the existing per-call nanosecond seed. Pure helper `fn generation_seed() -> u32`.

- [ ] **Step 1: Write the failing test** (append to `inference/mod.rs`'s tests module):

```rust
    #[test]
    fn generation_seed_honors_the_env_var_and_falls_back_to_entropy() {
        // Serial-unsafe env mutation is confined to this one test.
        std::env::set_var("DOCE_GEN_SEED", "42");
        assert_eq!(generation_seed(), 42);
        std::env::set_var("DOCE_GEN_SEED", "not-a-number");
        let a = generation_seed();
        let b = generation_seed();
        // Entropy fallback: not asserted unequal (could collide), just valid.
        let _ = (a, b);
        std::env::remove_var("DOCE_GEN_SEED");
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib generation_seed 2>&1 | tail -3`
Expected: FAIL to compile — `generation_seed` not found.

- [ ] **Step 3: Implement** — in `inference/mod.rs`, above `impl InferenceEngine`:

```rust
/// The sampler seed for one `generate()` call: `DOCE_GEN_SEED` (set by the
/// benchmark protocol for reproducible runs — single-run agent benchmarks
/// were observed swinging 0/20..20/20 on seed alone) or per-call entropy.
fn generation_seed() -> u32 {
    std::env::var("DOCE_GEN_SEED")
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0)
        })
}
```

and replace the existing seed block inside `generate()` (`let seed = SystemTime::now()...unwrap_or(0);`) with `let seed = generation_seed();`.

- [ ] **Step 4: Verify** — `cargo test --lib 2>&1 | tail -2` all pass; `cargo clippy --lib 2>&1 | tail -1` clean.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/inference/mod.rs
git commit -m "feat(inference): DOCE_GEN_SEED for reproducible benchmark runs

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

### Task 2: Benchmark scorer diagnostics + metrics line

**Files:**
- Modify: `src-tauri/tests/agent_benchmark.rs` (`tier4_score` + both tier4 tests' reporting)

**Interfaces:**
- Produces: per-file failure reasons on stdout (`bug_07: marker still present` / `bug_07: fixed line missing`), and one machine-greppable metrics line per run: `[metrics] score=N/20 turns=T elapsed_s=E seed=S`.

- [ ] **Step 1: Extend `tier4_score` to return reasons.** Change its signature and body (current version returns `(usize, usize)` around `tests/agent_benchmark.rs:575`):

```rust
/// Per-file grading for tier 4: fixed = the `// BUG:` marker is gone AND
/// the corrected line is present. Returns (fixed_count, total, failures)
/// where each failure names the file and which criterion failed — a 0/20
/// where every operator was actually fixed but the comments remained
/// (observed for real) must be diagnosable from the output alone.
fn tier4_score(dir: &Path) -> (usize, usize, Vec<String>) {
    let mut fixed = 0;
    let mut failures = Vec::new();
    let total = 20;
    for i in 0..total {
        let path = dir.join(format!("bug_{i:02}.txt"));
        let content = std::fs::read_to_string(&path).unwrap_or_default();
        let marker_gone = !content.contains("// BUG:");
        let line_ok = content.contains(&format!("let result = {i} + {};", i + 1));
        if marker_gone && line_ok {
            fixed += 1;
        } else {
            failures.push(format!(
                "bug_{i:02}: {}{}",
                if marker_gone { "" } else { "marker still present; " },
                if line_ok { "" } else { "fixed line missing" }
            ));
        }
    }
    (fixed, total, failures)
}
```

(Adapt the corrected-line check to whatever expression the current scorer uses — read it first; the shape above matches the seeder's `let result = {a} - {b};` with `a=i, b=i+1`.)

- [ ] **Step 2: Update both tier4 call sites** to destructure the triple, print each failure line, and print the metrics line:

```rust
    let (fixed, total, failures) = tier4_score(dir.path());
    for f in &failures {
        println!("  [tier4] {f}");
    }
    println!(
        "[metrics] score={fixed}/{total} turns={} elapsed_s={:.1} seed={}",
        run.turns,
        run.elapsed.as_secs_f32(),
        std::env::var("DOCE_GEN_SEED").unwrap_or_else(|_| "entropy".into())
    );
```

- [ ] **Step 3: Verify compile** — `cargo test --test agent_benchmark --no-run 2>&1 | tail -2` clean; `cargo clippy --tests 2>&1 | tail -1` clean.

- [ ] **Step 4: Establish the Phase-0 baseline** — run the three-seed gate (Global Constraints) detached; record `[metrics]` lines + failure reasons in the progress ledger as the baseline medians all later phases compare against.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/tests/agent_benchmark.rs
git commit -m "feat(bench): per-file failure reasons and seeded metrics line for tier4

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

## Phase 1 — Small knobs (P4 + §4.11)

### Task 3: Sampling alignment with Qwen's recommendation

**Files:**
- Modify: `src-tauri/src/inference/mod.rs` (`generate()`'s sampler chain)

**Interfaces:**
- Produces: chain = grammar → penalties(64, **1.0**, 0.0, **1.0**) → top_k(**20**) → top_p(**0.8**, 1) → **min_p(0.0, 1)** → temp(0.7) → dist(seed). (Repeat-penalty retired to 1.0 = off; presence-penalty 1.0 takes over repetition control per Qwen 2507 guidance; top-k/top-p per the model card.)

- [ ] **Step 1: Implement** — replace the `chain.extend([...])` block in `generate()`:

```rust
        // Qwen3-*-2507's own recommended sampling (model card): temp 0.7,
        // top-p 0.8, top-k 20, min-p 0 — with presence-penalty for
        // repetition control instead of repeat-penalty (repeat-penalty
        // taxes the tokens JSON repeats BY DESIGN — braces, quotes, key
        // names — and inside an active grammar it can only distort
        // argument content).
        chain.extend([
            LlamaSampler::penalties(64, 1.0, 0.0, 1.0),
            LlamaSampler::top_k(20),
            LlamaSampler::top_p(0.8, 1),
            LlamaSampler::min_p(0.0, 1),
            LlamaSampler::temp(0.7),
            LlamaSampler::dist(seed),
        ]);
```

(Confirm `LlamaSampler::min_p`'s exact signature in `~/.cargo/registry/src/*/llama-cpp-2-0.1.150/src/sampling.rs` before writing — if it's absent in this crate version, drop that line; min-p 0 is a no-op anyway.)

- [ ] **Step 2: Verify** — `cargo test --lib 2>&1 | tail -2`; `cargo clippy --lib 2>&1 | tail -1`.

- [ ] **Step 3: Benchmark gate** (three-seed protocol) — median must not regress vs Phase 0 baseline. If it regresses, revert ONLY the penalties line (keep top-k/top-p/min-p) and re-gate; record both configurations' metrics in the ledger.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/inference/mod.rs
git commit -m "feat(inference): align sampling with Qwen 2507 guidance; retire repeat-penalty

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

### Task 4: §4.11 correctness fixes (dead n_threads; usage-prompt mismatch)

**Files:**
- Modify: `src-tauri/src/inference/mod.rs` (store + apply `n_threads`)
- Modify: `src-tauri/src/commands/agent.rs` (`emit_context_usage_update` measures with the plan seed prompt)

**Interfaces:**
- Consumes: `InferenceEngine::load(path, n_threads)` already receives the value (hardcoded 4 at call sites).
- Produces: `InferenceEngine` gains field `n_threads: i32`; `generate()`'s ctx params apply it. `emit_context_usage_update` gains no new signature — internally builds the prompt via a fresh `PlanState`'s `plan_system_message`.

- [ ] **Step 1: n_threads.** In `InferenceEngine::load`, store the param (`Self { backend, model, n_threads }` — add the struct field); in `generate()`:

```rust
        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(CONTEXT_WINDOW_TOKENS))
            .with_n_threads(self.n_threads)
            .with_n_threads_batch(self.n_threads);
```

Fix `load`'s doc comment to state the param is now genuinely applied. Consider raising the call sites' literal `4` to `(num_cpus)`-style later — out of scope here; keep 4.

- [ ] **Step 2: usage prompt.** In `emit_context_usage_update` (commands/agent.rs), replace the `&system_message(cwd)` argument with the plan engine's actual seed prompt:

```rust
    let mut seed_state = crate::agent::plan::PlanState::default();
    let system_prompt = plan_system_message(&mut seed_state, cwd);
    if let Ok(usage) =
        crate::context::compute_usage(conn, engine, conversation_id, &skills_dir, &system_prompt).await
```

with a comment noting this matches what the top-level loop actually renders (the flat `SYSTEM_PROMPT` understated usage by ~300 tokens).

- [ ] **Step 3: Verify** — full `cargo test --lib` + clippy. No benchmark gate needed (no generation-path behavior change beyond thread count; spot-check tokens/sec in one tier1 run and note it in the ledger).

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/inference/mod.rs src-tauri/src/commands/agent.rs
git commit -m "fix(inference): apply n_threads for real; measure usage against the plan prompt

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

## Phase 2 — Convergence guardrails (P9 + the regex-blindness finding)

### Task 5: JSON-schema argument validation before dispatch

**Files:**
- Modify: `src-tauri/src/agent/dispatch.rs` (validation table + check in `execute`)

**Interfaces:**
- Produces: `fn validate_required_args(call: &ToolCall) -> Option<String>` — `Some(error_text)` when a required argument is missing/mistyped, naming every missing key and the expected shape; wired as the first thing `execute()` does. Generalizes `wrong_key_hint` from 3 tools to all.

- [ ] **Step 1: Failing tests** (dispatch.rs tests module):

```rust
    #[test]
    fn missing_required_arguments_get_a_schema_shaped_error_before_dispatch() {
        let result = execute(&call("Grep", serde_json::json!({})), None);
        assert!(result.model_text.starts_with("Error:"));
        assert!(result.model_text.contains("pattern"), "must name the missing key");

        let result = execute(&call("Edit", serde_json::json!({"file_path": "/a"})), None);
        assert!(result.model_text.contains("old_string"));
        assert!(result.model_text.contains("new_string"));
    }

    #[test]
    fn wrong_type_arguments_get_named() {
        let result = execute(
            &call("Read", serde_json::json!({"file_path": 42})),
            None,
        );
        assert!(result.model_text.starts_with("Error:"));
        assert!(result.model_text.contains("file_path"));
        assert!(result.model_text.contains("string"));
    }
```

- [ ] **Step 2: Run to verify failure** — the Grep case currently produces its own arm's error (may pass by luck); the Edit and Read-type cases fail. `cargo test --lib missing_required 2>&1 | tail -3`.

- [ ] **Step 3: Implement** — a static table + checker at the top of `execute()`:

```rust
/// (tool, required string-typed args). The NVIDIA SLM-agents "simple
/// format checks" applied at the boundary: a malformed call becomes a
/// one-turn correction naming exactly what's missing, instead of each
/// tool arm improvising (the model was observed repeating a wrong key
/// six times without self-correcting when the error didn't name it).
const REQUIRED_STRING_ARGS: &[(&str, &[&str])] = &[
    ("Read", &["file_path"]),
    ("Write", &["file_path", "content"]),
    ("Edit", &["file_path", "old_string", "new_string"]),
    ("Bash", &["command"]),
    ("Glob", &["pattern"]),
    ("Grep", &["pattern"]),
];

fn validate_required_args(call: &ToolCall) -> Option<String> {
    let (_, required) = REQUIRED_STRING_ARGS
        .iter()
        .find(|(name, _)| *name == call.name)?;
    let problems: Vec<String> = required
        .iter()
        .filter_map(|key| match call.arguments.get(*key) {
            None => {
                let hint = wrong_key_hint(&call.arguments, key, &["file", "path", "filepath", "filename", "text", "cmd"]);
                Some(format!("missing required \"{key}\" (a string){hint}"))
            }
            Some(v) if !v.is_string() => Some(format!("\"{key}\" must be a string")),
            Some(_) => None,
        })
        .collect();
    if problems.is_empty() {
        None
    } else {
        Some(format!(
            "Error: invalid {} arguments: {}. Re-issue the call with the corrected arguments.",
            call.name,
            problems.join("; ")
        ))
    }
}
```

Wire into `execute()` before the match:

```rust
    if let Some(error) = validate_required_args(call) {
        return ToolOutcome {
            detail: json!({"toolName": call.name, "arguments": call.arguments, "outcome": {"ok": false, "error": error}}),
            model_text: error,
        };
    }
```

Then delete the now-redundant per-arm missing-argument `let-else` blocks ONLY where behavior is identical (keep arms whose messages carry extra hints if any would be lost — compare each; the `wrong_key_hint` text must not regress, it's in the validator now).

- [ ] **Step 4: Verify** — full `cargo test --lib` (existing per-arm error tests must still pass — adapt their expected strings if the wording moved, keeping the key names asserted); clippy.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/agent/dispatch.rs
git commit -m "feat(agent): schema-shaped argument validation before every dispatch

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

### Task 6: Zero-match Grep regex-literalness hint

**Files:**
- Modify: `src-tauri/src/agent/dispatch.rs` (Grep zero-match arm)
- Test: same file

**Interfaces:**
- Produces: when Grep matches nothing AND the pattern contains an unescaped regex metacharacter (`+ * ? ( ) [ ] { } |`), the model_text appends a hint naming the metacharacter and the escaped form.

- [ ] **Step 1: Failing test:**

```rust
    #[test]
    fn zero_match_grep_with_unescaped_metachars_hints_at_escaping() {
        // Observed for real: the model verified its work by grepping for
        // "compute a + b" — `+` quantifies the space, matches nothing —
        // and trusted the empty result, reporting false success (0/20).
        let dir = tempdir().unwrap();
        stdfs::write(dir.path().join("f.txt"), "compute a + b now\n").unwrap();

        let result = execute(
            &call(
                "Grep",
                serde_json::json!({"pattern": "compute a + b", "path": dir.path().to_str().unwrap()}),
            ),
            None,
        );
        assert!(result.model_text.contains("No matches found"));
        assert!(
            result.model_text.contains("\\+"),
            "must show the escaped form, got: {:?}",
            result.model_text
        );
    }
```

- [ ] **Step 2: Verify failure** — currently prints a bare "No matches found".

- [ ] **Step 3: Implement** — in dispatch.rs near `wrong_key_hint`:

```rust
/// A zero-match Grep whose pattern contains unescaped regex
/// metacharacters is ambiguous between "nothing matches" and "the
/// pattern doesn't mean what you think" — name the suspicion instead of
/// letting an empty result read as verification.
fn regex_literalness_hint(pattern: &str) -> Option<String> {
    let mut chars = pattern.chars().peekable();
    let mut prev_backslash = false;
    while let Some(c) = chars.next() {
        if prev_backslash {
            prev_backslash = false;
            continue;
        }
        if c == '\\' {
            prev_backslash = true;
            continue;
        }
        if "+*?()[]{}|".contains(c) {
            return Some(format!(
                " Note: your pattern contains '{c}', a regex metacharacter — if you meant the literal character, escape it as '\\{c}' and search again."
            ));
        }
    }
    None
}
```

and in the Grep arm's empty-match model_text construction append `regex_literalness_hint(pattern).unwrap_or_default()` after "No matches found" (before the truncation/skip notices).

- [ ] **Step 4: Verify** — full suite + clippy; the existing zero-match test asserting exact `"No matches found"` equality must be relaxed to `contains` if it breaks.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/agent/dispatch.rs
git commit -m "feat(agent): hint at regex escaping on suspicious zero-match greps

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

### Task 7: Subagents run the plan engine

**Files:**
- Modify: `src-tauri/src/commands/agent.rs` (`SubagentBackend` gains `plan_state`; `execute_top_level_tool`'s Task branch seeds it)

**Interfaces:**
- Consumes: `PlanState`, `plan_system_message`, `ToolCallMode::Require`, `PlanToolReply` (all existing).
- Produces: `SubagentBackend { .., plan_state: crate::agent::plan::PlanState }`; its `generate` swaps `messages[0]` with `plan_system_message(&mut self.plan_state, self.cwd)` and uses `ToolCallMode::Require`; its `execute_tool` handles plan tools first (persisting under the subagent's own conversation with the `"plan": true` marker, `app: None`, no ActivePlans/events — subagents have no tracker), mapping `Finish` to `ToolExecution::Finish`.

- [ ] **Step 1: Failing test** — extend the commands::agent tests:

```rust
    #[tokio::test]
    async fn subagent_plan_rows_persist_under_the_subagent_conversation_with_the_plan_marker() {
        let conn = crate::storage::test_async_connection().await;
        seed_conversation(&conn, "sub-1").await;

        persist_plan_tool(
            None,
            &conn,
            "sub-1",
            "tc1",
            &crate::agent::ToolCall {
                name: "CreatePlan".to_string(),
                arguments: serde_json::json!({"goal": "g", "steps": ["a"]}),
            },
            "Plan created with 1 steps.",
        )
        .await;
        let (_, content_type, _, content) = latest_message(&conn, "sub-1").await;
        assert_eq!(content_type, "tool_result");
        let detail: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(detail["plan"], true);
    }
```

(This pins the persistence path the wiring below reuses; the wiring itself is exercised by the benchmark + a compile-time check that `SubagentBackend` has the same plan-first shape as `RealBackend`.)

- [ ] **Step 2: Implement** — mirror `RealBackend`'s three changes onto `SubagentBackend`: add the `plan_state` field (constructed `PlanState::default()` at the Task branch's `SubagentBackend { ... }` literal); replace its `generate` body with the prompt-swap + `Require` version (identical to `RealBackend::generate` but no ActivePlans and using `self.cwd`); rewrite its `execute_tool` to offer `handle_plan_tool` first — `Reply` → `persist_plan_tool(None, ...)` under `self.subagent_id` then `ToolExecution::Result`; `Finish` → persist then `ToolExecution::Finish`; `None` → the existing dispatch path. Also change the Task branch's `sub_messages` seed from `ChatMessage::system(SYSTEM_PROMPT)` to the plan seed prompt (`plan_system_message(&mut plan_state, cwd)` computed before the backend literal takes ownership).

- [ ] **Step 3: Verify** — full `cargo test --lib` + clippy; then ONE detached tier4 run whose task is delegated via `Task` is ideal but no such tier exists — instead run the three-seed tier4 gate (subagent path unused there, so this is a no-regression check) and note in the ledger that subagent-specific benchmarking is future work.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/commands/agent.rs
git commit -m "feat(agent): subagents run the two-state plan engine

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

## Phase 3 — Context spend (P7 + P8-remaining)

### Task 8: Bash output cap (tail-biased) at the tool

**Files:**
- Modify: `src-tauri/src/agent/tools/bash.rs` (truncation) and `src-tauri/src/agent/dispatch.rs` (notice text)

**Interfaces:**
- Produces: `pub const BASH_OUTPUT_MAX_BYTES: usize = 65536;` — stdout and stderr independently capped: first 20 lines + last 200 lines + `"... [{omitted} bytes omitted — full output offloaded]"` marker when over. The generic offload still runs afterward and stores the FULL output (pass the untruncated text to the offload call; the capped text is what enters `model_text`).

- [ ] **Step 1: Failing test** (bash.rs tests):

```rust
    #[test]
    fn oversized_stdout_is_tail_biased_truncated() {
        let result = run("i=0; while [ $i -lt 20000 ]; do echo line-$i; i=$((i+1)); done", None, None).unwrap();
        assert!(result.stdout.len() <= BASH_OUTPUT_MAX_BYTES + 1024);
        assert!(result.stdout.contains("line-0\n"), "head preserved");
        assert!(result.stdout.contains("line-19999"), "tail preserved");
        assert!(result.stdout.contains("bytes omitted"));
    }
```

- [ ] **Step 2: Run to verify failure** — `cargo test --lib oversized_stdout 2>&1 | tail -3` (compile-RED: `BASH_OUTPUT_MAX_BYTES` and the truncation don't exist).

- [ ] **Step 3: Implement** in `bash.rs`:

```rust
/// Independent cap for each of stdout/stderr entering the model-facing
/// result. Tail-biased: for failures and long build/test logs the signal
/// is at the END; the head is kept for context (the same reasoning as
/// Grep's caps — an unbounded String from a chatty command is both a
/// memory and a context-budget hazard).
pub const BASH_OUTPUT_MAX_BYTES: usize = 65536;
const HEAD_KEEP_LINES: usize = 20;
const TAIL_KEEP_LINES: usize = 200;

fn truncate_tail_biased(text: &str) -> String {
    if text.len() <= BASH_OUTPUT_MAX_BYTES {
        return text.to_string();
    }
    let lines: Vec<&str> = text.lines().collect();
    let head: Vec<&str> = lines.iter().take(HEAD_KEEP_LINES).copied().collect();
    let tail_start = lines.len().saturating_sub(TAIL_KEEP_LINES).max(HEAD_KEEP_LINES);
    let tail: Vec<&str> = lines[tail_start..].to_vec();
    let kept: usize = head.iter().chain(tail.iter()).map(|l| l.len() + 1).sum();
    let omitted = text.len().saturating_sub(kept);
    format!(
        "{}
... [{omitted} bytes omitted -- full output offloaded]
{}",
        head.join("
"),
        tail.join("
")
    )
}
```

applied where `run()` builds its result: `stdout: truncate_tail_biased(&stdout), stderr: truncate_tail_biased(&stderr)`. IMPORTANT: dispatch's Bash arm must hand the offload step the ORIGINAL untruncated text — check `handle_general_tool_call`'s flow; if the offload only ever sees `model_text`, store the full text in `detail.outcome.stdout` before truncation so nothing is lost (restorable-compression rule).

- [ ] **Step 4: Full suite + clippy.** Existing bash tests asserting exact stdout stay green (all under the cap).

- [ ] **Step 5: Commit** as `feat(agent): tail-biased Bash output cap`.

### Task 9: Restorable clearing (offload pointers) + plan-row clearing

**Files:**
- Modify: `src-tauri/src/context/mod.rs` (`apply_lightweight_clearing`), `src-tauri/src/context/limits.rs` (placeholder text)

**Interfaces:**
- Produces: tier-1 clearing replaces an old tool result with `"[Old tool result cleared; full output saved at {path} — Read it to recover]"` when the row's detail carries `offloadedTo`, else the existing placeholder. Plan-marked rows (`"plan": true` in detail) are cleared beyond the most recent 2 regardless of `TOOL_KEEP_N` (they re-state nothing the state prompt doesn't already carry).

- [ ] **Step 1: Failing tests** (context/mod.rs tests): (a) a cleared row with `offloadedTo` in its detail JSON gets the pointer text (seed a history row whose content JSON includes `"offloadedTo": "/tmp/x.txt"`); (b) plan rows older than the last 2 clear even when regular `TOOL_KEEP_N` would keep them; (c) tier-2's summarization input always pins the FIRST user message (the task statement) — extend `summarize_and_persist`'s test to assert the first user message survives outside the summarized span (OpenHands keep-first behavior; the report's P7 point that generative compression by a 4B is the riskiest link, so it must never eat the task statement).
- [ ] **Step 2: Implement** inside `apply_lightweight_clearing` (it already walks tool rows; parse each row's content JSON once for the `offloadedTo`/`plan` flags — pointer text for offloaded rows, aggressive clearing for plan rows) and inside `summarize_and_persist` (exclude the first user-role message from the summarized span, same as the most-recent-`PROTECTED_RECENT_MESSAGES` exclusion).
- [ ] **Step 3: Full suite + clippy.**
- [ ] **Step 4: Benchmark gate** (three seeds — clearing changes what the model sees late in long tasks), then commit as `feat(context): restorable clearing pointers; clear plan rows; pin the task statement`.

### Task 10: Plan recitation at the context tail

**Files:**
- Modify: `src-tauri/src/agent/plan.rs` (pure `recitation_text`), `src-tauri/src/commands/agent.rs` + `src-tauri/tests/agent_benchmark.rs` (inject in both plan-engine `generate`s)

**Interfaces:**
- Produces: `pub fn recitation_text(&self) -> Option<String>` on `PlanState` — `None` without a plan; else e.g. `"Plan status — goal: {goal}\n[x] step0 …\n[>] step3 (current)\n[ ] step4 …\n(3/7 done)"`. Both plan-engine hosts append `ChatMessage::user(recitation)` as the LAST message of the local clone before rendering (in-memory only, never persisted).

- [ ] **Step 1: Failing test** (plan.rs): create a 3-step plan, mark one done, set Executing{1}; assert the text contains the goal, a `[x]`, a `[>]` on the current step, and the `1/3` counter; assert `None` for a fresh state.
- [ ] **Step 2: Implement** on `PlanState`:

```rust
    /// The live plan restated for the context TAIL — Manus's recitation
    /// trick: on long tasks the system prompt drifts into the
    /// lost-in-the-middle zone; a compact checklist at the end of the
    /// context keeps the global plan inside the model's recent attention
    /// span. `None` when no plan exists (trivial turns pay nothing).
    pub fn recitation_text(&self) -> Option<String> {
        if !self.has_plan() {
            return None;
        }
        let done = self.plan.steps.iter().filter(|s| s.done).count();
        let current = match self.state {
            LoopState::Executing { step_index } => Some(step_index),
            LoopState::Planning => None,
        };
        let mut lines = vec![format!("Plan status -- goal: {}", self.plan.goal)];
        for (i, step) in self.plan.steps.iter().enumerate() {
            let mark = if step.done {
                "[x]"
            } else if current == Some(i) {
                "[>]"
            } else {
                "[ ]"
            };
            lines.push(format!("{mark} {}", step.description));
        }
        lines.push(format!("({done}/{} done)", self.plan.steps.len()));
        Some(lines.join("
"))
    }
```

- [ ] **Step 3: Inject** — in `RealBackend::generate` and the benchmark's `PlanExecBackend::generate`, after the `messages[0]` swap: `if let Some(recitation) = self.plan_state.recitation_text() { messages.push(ChatMessage::user(recitation)); }` (the local clone only — never persisted, never in the canonical history).
- [ ] **Step 4: Full suite + clippy; three-seed benchmark gate** (this one should HELP the medians — record). **Step 5: commit** as `feat(agent): recite the live plan at the context tail`.

---

## Phase 4 — KV-cache session + stable-prefix prompt (P2 + P3-remaining)

### Task 11: `PromptSession` — persistent context with prefix reuse

**Files:**
- Create: session type inside `src-tauri/src/inference/mod.rs`
- Modify: `src-tauri/src/commands/agent.rs`, `src-tauri/tests/agent_benchmark.rs` (hosts hold a session per turn)

**Interfaces:**
- Produces:

```rust
pub struct PromptSession<'m> {
    ctx: LlamaContext<'m>,
    /// The token sequence currently materialized in the KV cache.
    cached: Vec<LlamaToken>,
}

impl InferenceEngine {
    pub fn new_session(&self) -> Result<PromptSession<'_>, InferenceError>;
}

impl PromptSession<'_> {
    /// Like `InferenceEngine::generate`, but reuses the KV prefix shared
    /// with the previous call: tokenizes `prompt`, finds the longest
    /// common prefix with `self.cached`, truncates the KV past it
    /// (`clear_kv_cache_seq(Some(0), Some(common as u32), None)`), decodes
    /// only the suffix, then samples as before. Sampled tokens are
    /// appended to `cached` so the NEXT call's prefix includes this
    /// call's own output (the agent loop re-feeds it verbatim).
    pub fn generate(&mut self, engine: &InferenceEngine, prompt: &str, max_tokens: i32,
                    tool_calls: ToolCallMode, on_token: impl FnMut(&str),
                    should_cancel: impl FnMut() -> bool) -> Result<String, InferenceError>;
}
```

- Pure helper `fn common_prefix_len(a: &[LlamaToken], b: &[LlamaToken]) -> usize` (unit-tested).
- `InferenceEngine::generate` stays as-is (chat path, summarization, subagents until migrated) — implemented BY creating a throwaway session internally, so there is exactly one decode implementation.

- [ ] **Step 1: Failing unit test** for `common_prefix_len` (equal, disjoint, one-is-prefix cases) — compile-RED, implement.
- [ ] **Step 2: Implement `PromptSession`** by extracting `generate()`'s body: context creation moves to `new_session`; tokenize/prefill/sample stays but prefill starts at `common_prefix_len(&tokens, &self.cached)` after `self.ctx.clear_kv_cache_seq(Some(0), Some(common as u32), None)` (check the exact llama-cpp-2 signature in `context/kv_cache.rs:80` — the range semantics are `[p0, p1)`; pass `common..` as the removal range). Positions for the suffix batch continue from `common`. `InferenceEngine::generate` becomes `self.new_session()?.generate(self, prompt, ...)`.
- [ ] **Step 3: Wire the hosts:** `send_agent_message` creates one session after taking the engine guard and passes `&mut session` into `RealBackend` (add the field; `generate` uses `self.session.generate(self.engine, ...)`). Same for the benchmark's two backends. Borrow note: the session borrows the engine immutably (`LlamaContext<'m>` ties to the model); `RealBackend` already holds `&'a InferenceEngine` — hold `session: &'a mut PromptSession<'a>`... if the borrow checker fights the double-lifetime, fall back to constructing the session INSIDE the backend (field `session: PromptSession<'a>`), which only needs `engine: &'a InferenceEngine`.
- [ ] **Step 4: Real-model equivalence smoke** (add to `tests/real_model_smoke.rs`, `#[ignore]`): two sequential `session.generate` calls where the second's prompt extends the first's (same fixed seed via `DOCE_GEN_SEED`); assert the second call's output equals a fresh-context `engine.generate` of the same prompt (same seed) — proving prefix reuse changes nothing semantically. Print both calls' wall-clock; the second must be visibly faster on the shared prefix.
- [ ] **Step 5: Full suite + clippy + three-seed tier4 gate.** Record per-run `elapsed_s` vs Phase 3 — the expected win is large (each turn re-decodes only the newest tool exchange). **Step 6: commit** as `feat(inference): persistent PromptSession with KV prefix reuse`.

### Task 12: Stable-prefix prompt architecture

**Files:**
- Modify: `src-tauri/src/agent/plan.rs` (unified prompt + tail state message), `src-tauri/src/commands/agent.rs`, `src-tauri/tests/agent_benchmark.rs`

**Interfaces:**
- Produces: `pub const PLAN_SYSTEM_PROMPT: &str` — ONE immutable system prompt containing the union `<tools>` block (all Planning + Executing tools incl. FinishTask/StepDone/RefuseStep) and both rules sections; `PlanState::state_tail(&mut self) -> String` — the per-turn tail message: mode banner + (for Executing) goal + current step + the refusal context when present; folds Task 10's recitation into itself (one tail message total). `system_prompt()` is deleted; hosts render `[PLAN_SYSTEM_PROMPT+cwd, ...history..., tail]` — messages[0] NEVER changes within a turn (with Task 11, the KV prefix now survives every state transition).
- Grammar-level state gating compensates for the union tool list: extend `tool_call_grammar_sampler` with an optional `allowed_names: Option<&[&str]>` that, when set, constrains the `name` field to an enum (generate the GBNF alternation directly: `name-value ::= "\"CreatePlan\"" | "\"AddStep\"" | ...`); hosts pass the current state's tool set each call. (Sampler is rebuilt per call already — no cache impact.)

- [ ] **Step 1: Failing tests:** plan.rs — `state_tail` contains "PLANNING" for a fresh state; contains the goal + step text + recitation checklist for Executing; refusal reason appears once then is consumed. inference — a unit test for the name-enum GBNF string builder (pure string assembly).
- [ ] **Step 2: Implement** (prompt merge, `state_tail`, grammar enum, host wiring: replace the `messages[0]` swap with tail-append; the tail replaces Task 10's separate recitation push).
- [ ] **Step 3: Full suite + clippy.**
- [ ] **Step 4: THE gate that matters:** three-seed tier4 — this restructure touches the mechanism the 20/20 design validated; a median regression vs Phase 3 reverts the task (keep the branch/commit for later study) and the plan proceeds without it. Record cache-hit effect: elapsed_s should drop further vs Task 11 alone (no more full re-prefill on state flips).
- [ ] **Step 5: Commit** as `feat(agent): stable-prefix plan prompt with tail state + grammar-gated tools`.

---

## Phase 5 — Window rebalance (P6)

### Task 13: 16K window + proportional constants

**Files:**
- Modify: `src-tauri/src/inference/mod.rs` (`CONTEXT_WINDOW_TOKENS`), `src-tauri/src/context/limits.rs` (re-derived constants + guard test)

**Interfaces:**
- Produces: `CONTEXT_WINDOW_TOKENS: u32 = 16384`; `SUMMARY_MAX_TOKENS = (CONTEXT_WINDOW_TOKENS / 16) as i32` (=1024); `DEFAULT_TOOL_OUTPUT_OFFLOAD_CHARS = 2000`; comments rewritten as current fractions; a test asserting the ratios hold so the next window change trips it.

- [ ] **Step 1: Failing test** (limits.rs):

```rust
    #[test]
    fn budget_constants_stay_proportional_to_the_window() {
        assert_eq!(SUMMARY_MAX_TOKENS, (CONTEXT_WINDOW_TOKENS / 16) as i32);
        assert!(DEFAULT_TOOL_OUTPUT_OFFLOAD_CHARS >= 1500);
        assert!(AGENT_TURN_MAX_OUTPUT_TOKENS >= CONTEXT_WINDOW_TOKENS / 16);
    }
```

- [ ] **Step 2: Implement** the three constant changes + comment rewrites.
- [ ] **Step 3: Memory + throughput check:** one detached tier1 + tier4 run; watch RSS (`ps -o rss -p <pid>`) stays under ~6 GB and note tokens/sec (KV at 16K roughly doubles cache memory; with Task 11's session there is exactly ONE live context).
- [ ] **Step 4: Three-seed tier4 gate** vs Phase 4. **Step 5: commit** as `feat(context): 16K window with proportionally re-derived budgets`.

---

## Execution notes

- Ledger every phase's `[metrics]` lines — the plan's whole premise is that medians, not single runs, decide.
- Long benchmark runs exceed shell-tool timeouts: always `nohup ... > /tmp/log 2>&1 &` and poll.
- If any phase's gate regresses and the cause isn't obvious from the failure reasons (Task 2's diagnostics), stop and investigate with the trace before proceeding — the §5 ladder showed every regression names its cause within one read of the trace.
