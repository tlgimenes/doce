# SP2 — SOTA Context Management Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Measure the window from the server's authoritative token usage, always let agent turns use the maximum output the window allows, and restore the most-recent file's contents after a compaction.

**Architecture:** Three focused changes to the existing `context` + `inference` seams SP1 established. No new subsystems, no DB schema migration, no prompt-byte changes, `run_loop` byte-untouched.

**Tech Stack:** Rust (Tauri backend), llama-server sidecar OpenAI API (authoritative `usage` SSE trailer), `chars/4` gap-filler.

## Global Constraints

- **No prompt-byte changes.** SP2 changes NO system-prompt / summarization-prompt bytes (stays un-gated). Injected history content (restored file, context notices) is DATA, not a prompt template — allowed.
- **`run_loop` sacred.** No change to `run_loop`'s body or `AgentBackend` Require/`requires_tool_call` semantics. B1b adaptive escalation is NOT done (superseded by always-max-output).
- **API `usage` authoritative.** `ChatOutcome.usage = Some((prompt_tokens, completion_tokens))` is truth; `chars/4` only as gap-filler.
- **No DB schema migration.** All new state is in-memory app state (mirror `CompactionFailures`).
- **Single model** (Qwen3.5, Hermes; dialect pinned `HermesJson`). **`cargo fmt`.** `bindings.ts` git-ignored, never committed.
- **In-place on `main`.** Parallel FRONTEND session commits `src/views/**`; every SP2 commit is `src-tauri/**`-scoped, built on current HEAD, uses the commit's ACTUAL parent for review packages. Never touch the parallel session's files.

---

## Task order & interaction

FR-2 (authoritative usage) lands FIRST — it de-risks FR-1 (always-max-output relies on the prompt estimate not badly under-counting). Then FR-1+FR-4 (share the build sites). Then FR-3 (restore file).

---

### Task 1 (FR-2): API-usage-authoritative measure base

**Files:**

- Modify: `src-tauri/src/context/mod.rs` (add `ObservedUsage`, `LastObservedUsage`, `authoritative_prompt_tokens`; wire into `usage_from_history`)
- Modify: `src-tauri/src/lib.rs` (`.manage(LastObservedUsage::default())`)
- Modify: `src-tauri/src/commands/agent.rs` (record observation after a successful `generate`; thread `State`)
- Modify: `src-tauri/src/commands/context.rs` (thread `State` where `usage_from_history`/`maybe_compact` is called manually)
- Test: unit tests in `context::tests`

**Interfaces:**

- Produces: `pub struct ObservedUsage { pub prompt_tokens: u32, pub at_len: usize }`; `pub struct LastObservedUsage(pub std::sync::Mutex<std::collections::HashMap<String, ObservedUsage>>)`; `pub fn authoritative_prompt_tokens(observed: Option<&ObservedUsage>, all_openai_msgs: &[serde_json::Value], estimate: impl Fn(&str) -> u32) -> u32`.
- `at_len` = the number of OpenAI-shaped messages that were present when the server reported `prompt_tokens` (so the "new tail" is `all_openai_msgs[at_len..]`).

- [ ] **Step 1: Failing unit test for `authoritative_prompt_tokens`**

```rust
#[test]
fn authoritative_prompt_tokens_falls_back_to_full_estimate_when_unobserved() {
    let msgs = vec![serde_json::json!({"role":"user","content":"hello world"})];
    // None observed => full estimate over the serialized messages
    let est = |s: &str| (s.chars().count() / 4) as u32;
    let full = est(&serde_json::to_string(&msgs).unwrap());
    assert_eq!(authoritative_prompt_tokens(None, &msgs, est), full);
}

#[test]
fn authoritative_prompt_tokens_returns_base_when_no_new_messages() {
    let msgs = vec![serde_json::json!({"role":"user","content":"x"})];
    let observed = ObservedUsage { prompt_tokens: 500, at_len: 1 };
    let est = |s: &str| (s.chars().count() / 4) as u32;
    // at_len == msgs.len(): nothing appended since observation => base only
    assert_eq!(authoritative_prompt_tokens(Some(&observed), &msgs, est), 500);
}

#[test]
fn authoritative_prompt_tokens_adds_estimated_delta_for_new_messages() {
    let msgs = vec![
        serde_json::json!({"role":"user","content":"x"}),
        serde_json::json!({"role":"assistant","content":"a longer reply here"}),
    ];
    let observed = ObservedUsage { prompt_tokens: 500, at_len: 1 };
    let est = |s: &str| (s.chars().count() / 4) as u32;
    let delta = est(&serde_json::to_string(&msgs[1..]).unwrap());
    assert_eq!(authoritative_prompt_tokens(Some(&observed), &msgs, est), 500 + delta);
}

#[test]
fn authoritative_prompt_tokens_falls_back_when_history_shrank_below_observation() {
    // A compaction (or reload) left fewer messages than at_len: the observation
    // is stale/inapplicable => full estimate, never underflow.
    let msgs = vec![serde_json::json!({"role":"user","content":"x"})];
    let observed = ObservedUsage { prompt_tokens: 9999, at_len: 5 };
    let est = |s: &str| (s.chars().count() / 4) as u32;
    let full = est(&serde_json::to_string(&msgs).unwrap());
    assert_eq!(authoritative_prompt_tokens(Some(&observed), &msgs, est), full);
}
```

Run: `cargo test -p <crate> authoritative_prompt_tokens` — Expected: FAIL (fn undefined).

- [ ] **Step 2: Implement `ObservedUsage` / `LastObservedUsage` / `authoritative_prompt_tokens`**

```rust
/// The server's last authoritative prompt-token count for a conversation and
/// the history length it corresponded to. In-memory (session-scoped): a
/// restart resets to pure `chars/4` estimation, which is safe.
#[derive(Debug, Clone)]
pub struct ObservedUsage {
    pub prompt_tokens: u32,
    pub at_len: usize,
}

pub struct LastObservedUsage(pub std::sync::Mutex<std::collections::HashMap<String, ObservedUsage>>);
impl Default for LastObservedUsage {
    fn default() -> Self { Self(std::sync::Mutex::new(std::collections::HashMap::new())) }
}

/// Prefers the server's authoritative `prompt_tokens` as the base and adds
/// only the estimated `chars/4` delta of messages appended since that
/// observation. Falls back to a full estimate when unobserved, or when the
/// history has shrunk to at-or-below the observed length (a compaction/reload
/// invalidated the base) — never underflows.
pub fn authoritative_prompt_tokens(
    observed: Option<&ObservedUsage>,
    all_openai_msgs: &[serde_json::Value],
    estimate: impl Fn(&str) -> u32,
) -> u32 {
    let full = |slice: &[serde_json::Value]| {
        estimate(&serde_json::to_string(slice).unwrap_or_default())
    };
    match observed {
        Some(o) if o.at_len <= all_openai_msgs.len() => {
            o.prompt_tokens + full(&all_openai_msgs[o.at_len..])
        }
        _ => full(all_openai_msgs),
    }
}
```

Run: Expected PASS.

- [ ] **Step 3: Wire into `usage_from_history` (the compaction trigger base)**

`usage_from_history` gains an `observed: Option<&ObservedUsage>` param. Replace its `tokens_used` computation:

```rust
    let openai = crate::inference::http::to_openai_messages(&messages);
    let tokens_used = authoritative_prompt_tokens(observed, &openai, crate::inference::token_estimate);
```

Thread `observed` from its callers (`compute_usage`, `maybe_compact`, the subagent `measure` path if it routes here). Where a caller has no `LastObservedUsage` handle (pure/test callers), pass `None` (identical to today's behavior).

- [ ] **Step 4: Register `LastObservedUsage` app state + record observations**

- `lib.rs`: `.manage(LastObservedUsage::default())` next to `.manage(CompactionFailures(...))`.
- `commands/agent.rs`: after a successful agent `generate` whose `TurnOutcome.usage` is `Some((p, _))`, record `ObservedUsage { prompt_tokens: p, at_len: <count of OpenAI-shaped messages sent this turn> }` for the conversation. The turn's message list is in hand at the generate call site; `at_len = to_openai_messages(&sent_messages).len()`. Store under the conversation id. (Record for the TOP-LEVEL conversation; the subagent path may record under `subagent_id` if it consults the seam — otherwise leave subagent on pure estimate, note which.)
- Thread `State<'_, LastObservedUsage>` into `send_agent_message` and the manual `compact_conversation`/`get_context_usage` commands that call `usage_from_history`/`maybe_compact` (framework-injected; no `bindings.ts` change — same as `CompactionFailures`).

- [ ] **Step 5: Invalidate on compaction**

In `maybe_compact`'s `SummaryResult::Persisted` arm (next to the `CompactionFailures` reset), clear the conversation's `LastObservedUsage` entry:

```rust
    observed_usage.0.lock().unwrap().remove(conversation_id);
```

so the next turn re-estimates fully until a fresh `generate` re-observes. Add `observed_usage: &LastObservedUsage` to `maybe_compact`'s params.

- [ ] **Step 6: Unit-test the invalidation-shaped logic + run suite**

Add a test asserting a stale observation whose `at_len` exceeds the current message count falls back (already covered by Step 1's fourth test). Run `cargo test`, `cargo clippy --all-targets`, `cargo fmt`. Confirm `bindings.ts` unchanged.

- [ ] **Step 7: Commit**

```
git add src-tauri/src/context/mod.rs src-tauri/src/lib.rs src-tauri/src/commands/agent.rs src-tauri/src/commands/context.rs
git commit -m "feat(context): measure the window from the server's authoritative prompt_tokens"
```

---

### Task 2 (FR-1 + FR-4): always-max-output ceiling + align build-site prompt_est shape

**Files:**

- Modify: `src-tauri/src/context/limits.rs` (add `AGENT_TURN_OUTPUT_CEILING`)
- Modify: `src-tauri/src/commands/agent.rs` (both build sites: ceiling swap + prompt_est shape align)
- Test: `limits::tests` + a build-site prompt_est test

**Interfaces:**

- Consumes: `clamp_output_tokens` (unchanged), `CONTEXT_WINDOW_TOKENS`, `to_openai_messages`, `token_estimate`.
- Produces: `pub const AGENT_TURN_OUTPUT_CEILING: u32 = CONTEXT_WINDOW_TOKENS;`.

- [ ] **Step 1: Failing test — the ceiling yields max-fit, not 2048**

```rust
#[test]
fn agent_output_ceiling_lets_output_fill_the_free_window() {
    // small prompt => output should be ~window - prompt - margin, NOT capped at 2048
    let window = CONTEXT_WINDOW_TOKENS;
    let out = clamp_output_tokens(AGENT_TURN_OUTPUT_CEILING, window, 1000);
    assert!(out > AGENT_TURN_MAX_OUTPUT_TOKENS, "expected max-fit output, got {out}");
    assert_eq!(out, window - (1000 + 1024.max(window / 20)));
    // still structural: prompt + max_tokens <= window
    assert!(1000 + out <= window);
}

#[test]
fn agent_output_ceiling_floors_at_min_when_prompt_nearly_fills_window() {
    let window = CONTEXT_WINDOW_TOKENS;
    let out = clamp_output_tokens(AGENT_TURN_OUTPUT_CEILING, window, window - 100);
    assert_eq!(out, MIN_OUTPUT_TOKENS);
}
```

Run: Expected FAIL (`AGENT_TURN_OUTPUT_CEILING` undefined).

- [ ] **Step 2: Add the constant**

```rust
/// The output-token CEILING for agent turns under the always-max-output
/// policy: agent turns request as much output as fits the window, so the
/// ceiling is the window itself and `clamp_output_tokens` returns
/// `window - prompt_est - margin` (shrinking only when the prompt is large).
/// `SERVER_CTX_SIZE - CONTEXT_WINDOW_TOKENS` (= OUTPUT_RESERVE_TOKENS, 4096)
/// stays as slack beyond the clamp's own `window`, so `prompt + max_tokens <=
/// CONTEXT_WINDOW_TOKENS < SERVER_CTX_SIZE` holds even if `prompt_est` slightly
/// under-counts. Distinct from `AGENT_TURN_MAX_OUTPUT_TOKENS`, which remains the
/// conservative RESERVE the plan-host `threshold`/`STATE_TAIL_RESERVE` budgets subtract.
pub const AGENT_TURN_OUTPUT_CEILING: u32 = CONTEXT_WINDOW_TOKENS;
```

Run: Expected PASS.

- [ ] **Step 3: Swap the ceiling at both build sites + align prompt_est shape**

At `commands/agent.rs` RealBackend (~743) and SubagentBackend (~910):

- Change the `prompt_est` computation from `messages.iter().map(|m| token_estimate(&m.text())).sum()` to the OpenAI shape the trigger uses:

```rust
        let prompt_est = crate::inference::token_estimate(
            &serde_json::to_string(&crate::inference::http::to_openai_messages(&messages))
                .unwrap_or_default(),
        );
```

- Change the clamp ceiling from `AGENT_TURN_MAX_OUTPUT_TOKENS` to `AGENT_TURN_OUTPUT_CEILING`:

```rust
        req.max_tokens = Some(crate::context::limits::clamp_output_tokens(
            crate::context::limits::AGENT_TURN_OUTPUT_CEILING,
            crate::inference::CONTEXT_WINDOW_TOKENS,
            prompt_est,
        ));
```

Update the adjacent comment to describe always-max-output.

- [ ] **Step 4: Verify + commit**

Run `cargo test` (incl the 2 new + existing clamp regime tests), `cargo clippy --all-targets`, `cargo fmt`. Confirm the existing `clamp_output_tokens` structural tests still hold. `bindings.ts` unchanged.

```
git add src-tauri/src/context/limits.rs src-tauri/src/commands/agent.rs
git commit -m "feat(inference): always request the max output that fits the window"
```

---

### Task 3 (FR-3): restore the most-recent file after compaction

**Files:**

- Modify: `src-tauri/src/context/mod.rs` (pure `most_recent_read_path`; bounded-injection sizing; splice after summary in `summarize_and_persist`/`maybe_compact`)
- Modify: `src-tauri/src/context/limits.rs` (a `restored_file_note(path, content, truncated)` body builder, if a shared helper is cleaner)
- Test: `context::tests`

**Interfaces:**

- Produces: `pub fn most_recent_read_path(summarized: &[HistoryMessage]) -> Option<String>` (the last `Read` result's `detail.resolvedPath` in the summarized span; `None` if no `Read`); `pub fn bounded_restore_body(path: &str, content: &str, cap_tokens: usize, estimate: impl Fn(&str) -> u32) -> String` (full content under cap, else head+tail window + truncation note — NEVER a bare reference line).

- [ ] **Step 1: Failing tests for the pure helpers**

```rust
#[test]
fn most_recent_read_path_returns_the_last_read_resolved_path() {
    let span = vec![
        history_read_result("/a.rs"),
        history_bash_result(),
        history_read_result("/b.rs"),
    ];
    assert_eq!(most_recent_read_path(&span).as_deref(), Some("/b.rs"));
}

#[test]
fn most_recent_read_path_is_none_without_a_read() {
    let span = vec![history_bash_result()];
    assert_eq!(most_recent_read_path(&span), None);
}

#[test]
fn bounded_restore_body_returns_full_content_under_cap() {
    let est = |s: &str| (s.chars().count() / 4) as u32;
    let body = bounded_restore_body("/a.rs", "fn main() {}", 1000, est);
    assert!(body.contains("/a.rs"));
    assert!(body.contains("fn main() {}"));
    assert!(!body.to_lowercase().contains("read \"")); // never a reference line
}

#[test]
fn bounded_restore_body_head_tail_windows_over_cap_content() {
    let est = |s: &str| (s.chars().count() / 4) as u32;
    let big = "L\n".repeat(5000);
    let body = bounded_restore_body("/a.rs", &big, 10, est);
    assert!(est(&body) <= 10 + /*note slack*/ 64);
    assert!(body.contains("truncated")); // a truncation note, not a reference
    assert!(!body.to_lowercase().contains("read \""));
}
```

(Provide `history_read_result(path)` / `history_bash_result()` test builders producing `HistoryMessage`s whose `chat` is a `ToolResult` with `detail.toolName == "Read"` / `"Bash"` and `detail.resolvedPath` set — match the shape SP1's staging writes.)

Run: Expected FAIL.

- [ ] **Step 2: Implement the pure helpers**

`most_recent_read_path`: iterate the span in reverse, return the first message whose tool-result detail has `toolName == "Read"`, reading its `resolvedPath` (fallback `filePath`). `bounded_restore_body`: if `estimate(content) <= cap`, return ``format!("Current contents of `{path}`:\n{content}")``; else take a head window + tail window whose combined estimate fits `cap`, joined by `\n… [{n} lines truncated] …\n`, prefixed the same way. Never emit a "Read … to view" reference.

Run: Expected PASS.

- [ ] **Step 3: Splice the restored file after the summary**

In `summarize_and_persist`'s `Accept` arm (right after `persist_notice` for the summary), before returning `Persisted`:

- `let restored = most_recent_read_path(&to_summarize);`
- if `Some(path)`: re-read the file fresh from disk NOW via the same capped read `Read` uses (`crate::agent::dispatch`'s fs read or `std::fs::read_to_string` with the Read cap); on Ok, `persist_notice` a SECOND context row `kind: "restoredFile"` whose body is `bounded_restore_body(&path, &content, DEFAULT_TOOL_OUTPUT_OFFLOAD_TOKENS, token_estimate)`, ordered immediately after the summary so `load_history_annotated` splices it right after the summary. On read error / missing file: no-op (summary alone).
- Exactly one restored-file note per compaction.

- [ ] **Step 4: Verify + commit**

Run `cargo test` (incl the new pure-helper tests), `cargo clippy --all-targets`, `cargo fmt`. Confirm the restored-file body is CONTENT, never a reference line. `bindings.ts` unchanged. Note in the report: `restoredFile` context-notice bodies are injected DATA (not prompt-template bytes), so SP2 stays un-gated.

```
git add src-tauri/src/context/mod.rs src-tauri/src/context/limits.rs
git commit -m "feat(context): restore the most-recent file's contents after a compaction"
```

---

## Self-Review checklist (controller, before dispatch)

- Spec coverage: FR-1 (Task 2), FR-2 (Task 1), FR-3 (Task 3), FR-4 (Task 2 Step 3). ✓
- Type consistency: `ObservedUsage { prompt_tokens: u32, at_len: usize }` used identically in Tasks 1 record/measure/invalidate. `AGENT_TURN_OUTPUT_CEILING` referenced only in Task 2. `most_recent_read_path`/`bounded_restore_body` signatures match between Task 3 steps.
- No prompt-byte change: verify each task's diff touches no system/summarization prompt string literal. Restored-file/context-notice bodies are data.
- `run_loop` untouched: no task edits `agent/mod.rs` run_loop.
- Each commit `src-tauri/**`-scoped; review package uses the commit's ACTUAL parent (parallel FRONTEND session interleaves).
