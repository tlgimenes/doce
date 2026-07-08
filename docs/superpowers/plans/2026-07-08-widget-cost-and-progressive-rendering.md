# Widget Cost Badges + Progressive Rendering Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add real token/byte cost badges to the `Read`/`Bash`/`Grep`/`Glob` tool-call widgets, and make `Bash`/`Task` render a live "pending/running" state instead of staying invisible until they finish.

**Architecture:** Backend: one shared helper (`context::annotate_with_token_count`) tags a tool result's `detail` JSON with its real tokenizer count before persistence; a `tool_call` row is now persisted *before* execution (not bundled with the result afterward) for the general tool path and for `Task`, mirroring `AskUserQuestion`'s existing early-persist pattern — this is what makes a live pending state possible at all. Frontend: two new detail-shape parsers read a still-pending `tool_call` row's raw arguments into a "pending" version of the widget's own detail type; `Workspace.tsx`'s existing `pendingQuestion` derivation generalizes to cover `Bash`/`Task` too.

**Tech Stack:** Rust (Tauri backend, `src-tauri/`), TypeScript/React (frontend, `src/`), Vitest, `cargo test`.

## Global Constraints

- Cost badges apply only to `Read`/`Bash`/`Grep`/`Glob` — not `Write`/`Edit`/`Task`/`AskUserQuestion` (design doc's explicit scope decision).
- Pending/progressive rendering applies only to `Bash`/`Task` — not `Read`/`Write`/`Edit`/`Glob`/`Grep` (execution too fast to matter).
- Token counts use the real model tokenizer (`InferenceEngine::count_tokens`), never a client-side estimate.
- No new aggregate/rollup cost widget (explicitly deferred).
- Every new/changed function gets a real test — see each task's Step 1/2.

Every task's requirements implicitly include this section.

---

## Task 1: Backend — token-count annotation helper

**Files:**
- Modify: `src-tauri/src/context/mod.rs`
- Test: `src-tauri/src/context/mod.rs` (inline `#[cfg(test)] mod tests`, already present at line 487)

**Interfaces:**
- Produces: `pub fn annotate_with_token_count(engine: &InferenceEngine, outcome: ToolOutcome) -> ToolOutcome` — used by Task 2 and Task 3.

- [ ] **Step 1: Write the failing tests**

Add to `src-tauri/src/context/mod.rs`, inside the existing `#[cfg(test)] mod tests { ... }` block (after the last existing test, before the closing `}`):

```rust
    #[test]
    fn wants_token_count_is_true_only_for_the_four_size_variable_tools() {
        assert!(wants_token_count("Read"));
        assert!(wants_token_count("Bash"));
        assert!(wants_token_count("Grep"));
        assert!(wants_token_count("Glob"));
        assert!(!wants_token_count("Write"));
        assert!(!wants_token_count("Edit"));
        assert!(!wants_token_count("Task"));
        assert!(!wants_token_count("AskUserQuestion"));
    }

    #[test]
    fn merge_token_count_inserts_the_field_into_an_object_detail() {
        let detail = serde_json::json!({"toolName": "Read", "filePath": "/tmp/x.txt"});
        let merged = merge_token_count(detail, 312);
        assert_eq!(merged["tokenCount"], 312);
        assert_eq!(merged["filePath"], "/tmp/x.txt");
    }
```

Also add near the top of the file, alongside the other `use crate::...` lines (after line 21's `use crate::inference::{ChatMessage, InferenceEngine, MessageContent};`):

```rust
use crate::agent::dispatch::ToolOutcome;
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test --lib context::tests::wants_token_count_is_true_only_for_the_four_size_variable_tools context::tests::merge_token_count_inserts_the_field_into_an_object_detail`
Expected: FAIL with `cannot find function 'wants_token_count'` / `cannot find function 'merge_token_count'` (compile error).

- [ ] **Step 3: Write the implementation**

Add to `src-tauri/src/context/mod.rs`, right before the `#[cfg(test)]` line (i.e., after `fit_to_budget`'s closing brace and before the tests module):

```rust
/// Names the four tool results whose size varies enough for a cost badge
/// to be worth showing (`Write`/`Edit`/`Task`/`AskUserQuestion` are small
/// and roughly fixed-cost, so a badge there would just be noise) — see
/// the widget-cost-and-progressive-rendering design doc's scope decision.
fn wants_token_count(tool_name: &str) -> bool {
    matches!(tool_name, "Read" | "Bash" | "Grep" | "Glob")
}

/// Merges a computed token count into `detail`'s `tokenCount` field — pure
/// JSON manipulation, split out from the token-counting itself so it's
/// unit-testable without a loaded model.
fn merge_token_count(mut detail: serde_json::Value, token_count: usize) -> serde_json::Value {
    if let Some(obj) = detail.as_object_mut() {
        obj.insert("tokenCount".to_string(), serde_json::json!(token_count));
    }
    detail
}

/// Annotates a tool result with its real token cost — the same tokenizer
/// `fit_to_budget`/the context usage gauge already use, not a client-side
/// estimate, since the whole point is that this number has to match the
/// real budget math. Applied only to the four tool results whose size
/// varies enough to matter (`wants_token_count`); every other tool's
/// `detail` passes through unchanged. Called right after
/// `dispatch::execute()` returns, before persistence, from every call site
/// that already holds an `&InferenceEngine` for this exact reason
/// (`context::fit_turn_to_budget`). A tokenization failure leaves `detail`
/// unannotated rather than failing the whole tool result over a
/// UI-only concern.
pub fn annotate_with_token_count(engine: &InferenceEngine, outcome: ToolOutcome) -> ToolOutcome {
    let tool_name = outcome
        .detail
        .get("toolName")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !wants_token_count(tool_name) {
        return outcome;
    }
    let Ok(token_count) = engine.count_tokens(&outcome.model_text) else {
        return outcome;
    };
    ToolOutcome {
        model_text: outcome.model_text,
        detail: merge_token_count(outcome.detail, token_count),
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test --lib context::tests::wants_token_count_is_true_only_for_the_four_size_variable_tools context::tests::merge_token_count_inserts_the_field_into_an_object_detail`
Expected: `test result: ok. 2 passed`

- [ ] **Step 5: Full backend sanity check**

Run: `cd src-tauri && cargo check --all-targets && cargo test --lib && cargo clippy --all-targets`
Expected: all clean, all existing tests still pass (this task only adds new, unused-so-far code).

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/context/mod.rs
git commit -m "feat(context): add real token-count annotation for tool results"
```

---

## Task 2: Backend — wire token-count annotation into `SubagentBackend`

**Files:**
- Modify: `src-tauri/src/commands/agent.rs`

**Interfaces:**
- Consumes: `context::annotate_with_token_count(engine, outcome) -> ToolOutcome` (Task 1).

This is the simpler of the two call sites — `SubagentBackend::execute_tool` has no offload logic to interleave with, just a single `dispatch::execute` call followed by persistence.

- [ ] **Step 1: Write the failing test**

Add to `src-tauri/src/commands/agent.rs`'s `#[cfg(test)] mod tests { ... }` block (near the other persistence-shape tests, e.g. after `persist_user_turn_with_rich_content_...`):

```rust
    #[tokio::test]
    async fn subagent_backend_tool_result_carries_a_real_token_count_for_read() {
        let conn = crate::storage::test_async_connection().await;
        seed_conversation(&conn, "sub").await;
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("notes.txt"), "hello world").unwrap();

        let engine = crate::inference::InferenceEngine::load(&test_model_path(), 4)
            .expect("model should load");
        let mut backend = SubagentBackend {
            engine: &engine,
            conn: &conn,
            subagent_id: "sub",
            cwd: Some(dir.path()),
            threshold: 1024,
        };
        use crate::agent::AgentBackend;
        let call = crate::agent::ToolCall {
            name: "Read".to_string(),
            arguments: serde_json::json!({"file_path": "notes.txt"}),
        };
        backend.execute_tool("call1".to_string(), call).await;

        let (_, _, _, content) = latest_message(&conn, "sub").await;
        let detail: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert!(detail["tokenCount"].as_u64().is_some());
    }
```

This test needs a real loaded model (matching this file's existing `#[ignore]`'d-elsewhere convention for anything touching `InferenceEngine::load`) — check whether `test_model_path()` already exists in this test module; if not, add it right above this test:

```rust
    fn test_model_path() -> std::path::PathBuf {
        let home = std::env::var("HOME").expect("HOME must be set");
        std::path::PathBuf::from(home).join(
            "Library/Application Support/app.doce.desktop/models/qwen3-4b-instruct-2507-q4_k_m.gguf",
        )
    }
```

Mark the test `#[ignore]` (needs the real installed model, same convention as `tests/agent_benchmark.rs`):

```rust
    #[tokio::test]
    #[ignore]
    async fn subagent_backend_tool_result_carries_a_real_token_count_for_read() {
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd src-tauri && cargo test --lib subagent_backend_tool_result_carries_a_real_token_count_for_read -- --ignored --nocapture`
Expected: FAIL — `detail["tokenCount"]` is `null` (the field doesn't exist yet).

- [ ] **Step 3: Write the implementation**

In `src-tauri/src/commands/agent.rs`, find `SubagentBackend::execute_tool` (currently):

```rust
    async fn execute_tool(&mut self, tool_call_id: String, call: ToolCall) -> String {
        // 004-tool-call-widgets: the subagent's own tool activity persists
        // under its own conversation row -- never the parent's --
        // preserving 001's existing FR-015/SC-008 isolation guarantee
        // (only its final answer, inserted by the caller, ever reaches the
        // parent's transcript). No live-refresh event (`app: None`) -- it
        // isn't rendered by any current view, so there's no consumer to
        // notify.
        let outcome = dispatch::execute(&call, self.cwd);
        persist_tool_call_and_result(
            None,
            self.conn,
            self.subagent_id,
            &tool_call_id,
            &call.name,
            call.arguments.clone(),
            &outcome.model_text,
            outcome.detail.clone(),
        )
        .await;
        outcome.model_text
    }
```

Change the `let outcome = ...` line to annotate before persisting:

```rust
        let outcome = dispatch::execute(&call, self.cwd);
        let outcome = crate::context::annotate_with_token_count(self.engine, outcome);
        persist_tool_call_and_result(
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd src-tauri && cargo test --lib subagent_backend_tool_result_carries_a_real_token_count_for_read -- --ignored --nocapture`
Expected: `test result: ok. 1 passed`

- [ ] **Step 5: Full backend sanity check**

Run: `cd src-tauri && cargo check --all-targets && cargo test --lib && cargo clippy --all-targets`
Expected: all clean.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/commands/agent.rs
git commit -m "feat(agent): annotate subagent tool results with token count"
```

---

## Task 3: Backend — early `tool_call` persist + token annotation for the general top-level tool path

**Files:**
- Modify: `src-tauri/src/commands/agent.rs`

**Interfaces:**
- Consumes: `context::annotate_with_token_count` (Task 1), existing `persist_tool_call`/`persist_tool_result`.
- Produces: `async fn handle_general_tool_call(app: Option<&AppHandle>, conn: &tokio_rusqlite::Connection, engine: &InferenceEngine, parent_conversation_id: &str, cwd: Option<&std::path::Path>, tool_call_id: &str, call: &ToolCall) -> String` — used by `execute_top_level_tool` (this task) and directly by the test below.

This extracts the general (non-`Task`, non-`AskUserQuestion`) branch of `execute_top_level_tool` into its own helper, taking `app: Option<&AppHandle>` instead of the enclosing function's mandatory `&AppHandle` — mirroring `handle_ask_user_question`'s own existing precedent for testability without a live Tauri app. `execute_top_level_tool` itself still isn't unit-testable (the `Task` branch needs a real loaded model), but this extracted piece now is.

- [ ] **Step 1: Write the failing test**

Add to `src-tauri/src/commands/agent.rs`'s test module:

```rust
    #[tokio::test]
    async fn handle_general_tool_call_persists_the_tool_call_row_before_the_tool_result_row() {
        let conn = crate::storage::test_async_connection().await;
        seed_conversation(&conn, "c1").await;
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("notes.txt"), "hello world").unwrap();

        let engine = crate::inference::InferenceEngine::load(&test_model_path(), 4)
            .expect("model should load");
        let call = ToolCall {
            name: "Read".to_string(),
            arguments: serde_json::json!({"file_path": "notes.txt"}),
        };

        let model_text = handle_general_tool_call(
            None,
            &conn,
            &engine,
            "c1",
            Some(dir.path()),
            "call1",
            &call,
        )
        .await;

        assert!(model_text.contains("hello world"));

        // `all_messages` (already defined in this test module, near
        // `task_delegation_persists_...`) returns `Vec<(content_type,
        // tool_name)>`, ordered by sequence — enough to confirm the
        // tool_call row landed before the tool_result row.
        let rows = all_messages(&conn, "c1").await;
        assert_eq!(rows.len(), 2, "expected exactly a tool_call row and a tool_result row");
        assert_eq!(rows[0].0, "tool_call");
        assert_eq!(rows[1].0, "tool_result");

        // `latest_message` (already defined in this test module) returns
        // (role, content_type, tool_name, content) for the newest row —
        // which, after the two inserts above, is the tool_result row.
        let (_, content_type, _, content) = latest_message(&conn, "c1").await;
        assert_eq!(content_type, "tool_result");
        let result_detail: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert!(
            result_detail["tokenCount"].as_u64().is_some(),
            "Read is one of the four annotated tools"
        );
    }
```

Mark it `#[ignore]` (needs the real installed model):

```rust
    #[tokio::test]
    #[ignore]
    async fn handle_general_tool_call_persists_the_tool_call_row_before_the_tool_result_row() {
```

Both `all_messages` and `latest_message` already exist in this test module (`all_messages` near `task_delegation_persists_...`, `latest_message` near `seed_conversation`) — verified directly against their current definitions, not assumed.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd src-tauri && cargo test --lib handle_general_tool_call_persists_the_tool_call_row_before_the_tool_result_row -- --ignored --nocapture`
Expected: FAIL with `cannot find function 'handle_general_tool_call'` (compile error).

- [ ] **Step 3: Write the implementation**

In `src-tauri/src/commands/agent.rs`, find `execute_top_level_tool`'s general-tool branch (currently, inside the function, right after the `AskUserQuestion` early return):

```rust
    if call.name != "Task" {
        let outcome = dispatch::execute(&call, cwd);

        // 010-context-window-management/US3 (FR-011/FR-012): an oversized
        // result gets truncated to a preview + a `Read`-able pointer before
        // it ever enters the model's context -- the persisted `detail`
        // still carries the full outcome for the transcript UI (widgets
        // decide for themselves whether to show a "view full output"
        // affordance), only `model_text` (what the model actually sees) is
        // substituted.
        let settings = crate::context::ContextSettings::load(conn)
            .await
            .unwrap_or_else(|_| crate::context::ContextSettings::from_raw(&Default::default()));
        let (model_text, offloaded_to) = match app.path().app_data_dir() {
            Ok(app_data_dir) => crate::context::offload::offload_if_oversized(
                &app_data_dir,
                parent_conversation_id,
                &tool_call_id,
                &outcome.model_text,
                settings.tool_output_offload_chars,
            )
            .unwrap_or_else(|_| (outcome.model_text.clone(), None)),
            Err(_) => (outcome.model_text.clone(), None),
        };

        let mut detail = outcome.detail.clone();
        detail["offloadedTo"] = serde_json::json!(offloaded_to);

        persist_tool_call_and_result(
            Some(app),
            conn,
            parent_conversation_id,
            &tool_call_id,
            &call.name,
            call.arguments.clone(),
            &model_text,
            detail,
        )
        .await;
        emit_context_usage_update(app, conn, engine, parent_conversation_id, cwd).await;
        return model_text;
    }
```

Replace it with a call to the new helper:

```rust
    if call.name != "Task" {
        let model_text = handle_general_tool_call(
            Some(app),
            conn,
            engine,
            parent_conversation_id,
            cwd,
            &tool_call_id,
            &call,
        )
        .await;
        emit_context_usage_update(app, conn, engine, parent_conversation_id, cwd).await;
        return model_text;
    }
```

Then add the new helper function right after `execute_top_level_tool`'s closing brace (before the `AskUserQuestion` handler or wherever the next function starts — place it immediately after `execute_top_level_tool`):

```rust
/// Handles a single non-`Task`, non-`AskUserQuestion` tool call for the
/// top-level loop. Persists the `tool_call` row *before* executing —
/// mirrors `handle_ask_user_question`'s existing early-persist pattern —
/// so a slow tool (e.g. a long-running `Bash` command) is visible as "in
/// flight" the moment it starts, not only once it's already finished.
/// `app: Option<&AppHandle>` (not mandatory, unlike the enclosing
/// `execute_top_level_tool`) specifically so this is unit-testable without
/// a live Tauri app.
async fn handle_general_tool_call(
    app: Option<&AppHandle>,
    conn: &tokio_rusqlite::Connection,
    engine: &InferenceEngine,
    parent_conversation_id: &str,
    cwd: Option<&std::path::Path>,
    tool_call_id: &str,
    call: &ToolCall,
) -> String {
    persist_tool_call(
        app,
        conn,
        parent_conversation_id,
        tool_call_id,
        &call.name,
        call.arguments.clone(),
    )
    .await;

    let outcome = dispatch::execute(call, cwd);
    let outcome = crate::context::annotate_with_token_count(engine, outcome);

    // 010-context-window-management/US3 (FR-011/FR-012): an oversized
    // result gets truncated to a preview + a `Read`-able pointer before it
    // ever enters the model's context -- the persisted `detail` still
    // carries the full outcome for the transcript UI, only `model_text`
    // (what the model actually sees) is substituted.
    let settings = crate::context::ContextSettings::load(conn)
        .await
        .unwrap_or_else(|_| crate::context::ContextSettings::from_raw(&Default::default()));
    let (model_text, offloaded_to) = match app.and_then(|a| a.path().app_data_dir().ok()) {
        Some(app_data_dir) => crate::context::offload::offload_if_oversized(
            &app_data_dir,
            parent_conversation_id,
            tool_call_id,
            &outcome.model_text,
            settings.tool_output_offload_chars,
        )
        .unwrap_or_else(|_| (outcome.model_text.clone(), None)),
        None => (outcome.model_text.clone(), None),
    };

    let mut detail = outcome.detail.clone();
    detail["offloadedTo"] = serde_json::json!(offloaded_to);

    persist_tool_result(
        app,
        conn,
        parent_conversation_id,
        tool_call_id,
        &call.name,
        &model_text,
        detail,
    )
    .await;

    model_text
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd src-tauri && cargo test --lib handle_general_tool_call_persists_the_tool_call_row_before_the_tool_result_row -- --ignored --nocapture`
Expected: `test result: ok. 1 passed`

- [ ] **Step 5: Full backend sanity check, including the real-model benchmark's fast tiers**

Run: `cd src-tauri && cargo check --all-targets && cargo test --lib && cargo clippy --all-targets`
Expected: all clean.

Run: `cd src-tauri && cargo test --test agent_benchmark tier1_single_tool_call -- --ignored --nocapture --test-threads=1`
Expected: still passes — this exercises the real top-level tool-call path end-to-end (though via `BenchBackend`, not `execute_top_level_tool` directly, it's the closest available regression signal for the general tool-call flow).

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/commands/agent.rs
git commit -m "refactor(agent): persist a tool_call row before execution, not after"
```

---

## Task 4: Backend — early `tool_call` persist for the `Task` branch

**Files:**
- Modify: `src-tauri/src/commands/agent.rs`

**Interfaces:**
- Consumes: existing `persist_tool_call`/`persist_tool_result`.

Unlike Task 3, this doesn't unlock new unit-test coverage on its own — `execute_top_level_tool`'s `Task` branch needs a real loaded model for the nested `run_loop` call regardless of how persistence is split, matching this file's existing, already-documented constraint. The existing persistence-shape test (`task_delegation_persists_a_complete_status_on_the_parent_and_keeps_subagent_activity_isolated`) gets updated to simulate the new two-call pattern instead of the old bundled one, so it still documents (and would catch a regression in) the exact shape `execute_top_level_tool` is expected to produce.

- [ ] **Step 1: Update the existing test to simulate the new call pattern**

In `src-tauri/src/commands/agent.rs`'s test module, find:

```rust
        // What execute_top_level_tool persists on the PARENT once the
        // delegation itself completes (FR-010).
        persist_tool_call_and_result(
            None,
            &conn,
            "parent",
            "call2",
            "Task",
            serde_json::json!({"prompt": "go read the file"}),
            "the file says hi",
            serde_json::json!({
                "toolName": "Task", "prompt": "go read the file",
                "subagentConversationId": "sub", "state": "complete",
            }),
        )
        .await;
```

Replace with two separate calls, matching what `execute_top_level_tool`'s `Task` branch will do after this task's Step 3:

```rust
        // What execute_top_level_tool now persists on the PARENT: the
        // tool_call row immediately (before spawn_subagent/run_loop), the
        // tool_result row once the delegation completes (FR-010).
        persist_tool_call(
            None,
            &conn,
            "parent",
            "call2",
            "Task",
            serde_json::json!({"prompt": "go read the file"}),
        )
        .await;
        persist_tool_result(
            None,
            &conn,
            "parent",
            "call2",
            "Task",
            "the file says hi",
            serde_json::json!({
                "toolName": "Task", "prompt": "go read the file",
                "subagentConversationId": "sub", "state": "complete",
            }),
        )
        .await;
```

- [ ] **Step 2: Run the test to verify it still passes as-is (sanity check before touching production code)**

Run: `cd src-tauri && cargo test --lib task_delegation_persists_a_complete_status_on_the_parent_and_keeps_subagent_activity_isolated`
Expected: `test result: ok. 1 passed` — `persist_tool_call` + `persist_tool_result` back-to-back produce the same final DB state as `persist_tool_call_and_result` did (they're already its own internal implementation), so this passes immediately. This step exists to prove the test itself is still valid before Step 3 changes production code.

- [ ] **Step 3: Update `execute_top_level_tool`'s `Task` branch**

In `src-tauri/src/commands/agent.rs`, find (in `execute_top_level_tool`, after the `subagent_id` is obtained and before `run_loop` is called):

```rust
    let Some(prompt) = call.arguments.get("prompt").and_then(|v| v.as_str()) else {
        return "Error: Task requires a prompt argument".to_string();
    };
    let prompt = prompt.to_string();

    let parent_id = parent_conversation_id.to_string();
```

Insert an early `persist_tool_call` right after `let prompt = prompt.to_string();`:

```rust
    let Some(prompt) = call.arguments.get("prompt").and_then(|v| v.as_str()) else {
        return "Error: Task requires a prompt argument".to_string();
    };
    let prompt = prompt.to_string();

    persist_tool_call(
        Some(app),
        conn,
        parent_conversation_id,
        &tool_call_id,
        "Task",
        serde_json::json!({ "prompt": prompt }),
    )
    .await;

    let parent_id = parent_conversation_id.to_string();
```

Then find the final bundled persist call at the end of the `Task` branch:

```rust
    persist_tool_call_and_result(
        Some(app),
        conn,
        parent_conversation_id,
        &tool_call_id,
        "Task",
        serde_json::json!({ "prompt": prompt }),
        &sub_final,
        serde_json::json!({
            "toolName": "Task",
            "prompt": prompt,
            "subagentConversationId": subagent_id,
            "state": "complete",
        }),
    )
    .await;

    sub_final
}
```

Replace it with just the result half (the call half is already persisted above):

```rust
    persist_tool_result(
        Some(app),
        conn,
        parent_conversation_id,
        &tool_call_id,
        "Task",
        &sub_final,
        serde_json::json!({
            "toolName": "Task",
            "prompt": prompt,
            "subagentConversationId": subagent_id,
            "state": "complete",
        }),
    )
    .await;

    sub_final
}
```

- [ ] **Step 4: Run the persistence-shape test again to confirm it still passes**

Run: `cd src-tauri && cargo test --lib task_delegation_persists_a_complete_status_on_the_parent_and_keeps_subagent_activity_isolated`
Expected: `test result: ok. 1 passed`

- [ ] **Step 5: Full backend sanity check**

Run: `cd src-tauri && cargo check --all-targets && cargo test --lib && cargo clippy --all-targets`
Expected: all clean.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/commands/agent.rs
git commit -m "refactor(agent): persist Task's tool_call row before spawning the subagent"
```

---

## Task 5: Frontend — extend `ipc.ts` types and add pending-detail parsers

**Files:**
- Modify: `src/lib/ipc.ts`
- Create: `src/lib/ipc.test.ts`

**Interfaces:**
- Produces: `parsePendingBashCallDetail(content: string): BashDetail | null`, `parsePendingTaskCallDetail(content: string): TaskDetail | null` — used by Task 9 (`Workspace.tsx`).
- Produces: `ReadDetail.tokenCount?: number`, `BashDetail.tokenCount?: number`, `BashDetail.outcome?: BashOutcome` (now optional), `GrepDetail.tokenCount?: number`, `GlobDetail.tokenCount?: number` — used by Task 6, 7, 8.

- [ ] **Step 1: Write the failing tests**

Create `src/lib/ipc.test.ts`:

```typescript
import { describe, it, expect } from "vitest";
import { parsePendingBashCallDetail, parsePendingTaskCallDetail } from "./ipc";

describe("parsePendingBashCallDetail", () => {
  it("parses a pending Bash tool_call row's arguments into an outcome-less BashDetail", () => {
    const content = JSON.stringify({ arguments: { command: "cargo test", timeoutMs: 5000 } });
    const detail = parsePendingBashCallDetail(content);
    expect(detail).toEqual({
      toolName: "Bash",
      command: "cargo test",
      timeoutMs: 5000,
    });
  });

  it("defaults timeoutMs to null when absent", () => {
    const content = JSON.stringify({ arguments: { command: "ls" } });
    const detail = parsePendingBashCallDetail(content);
    expect(detail?.timeoutMs).toBeNull();
  });

  it("returns null when command is missing", () => {
    const content = JSON.stringify({ arguments: {} });
    expect(parsePendingBashCallDetail(content)).toBeNull();
  });

  it("returns null on malformed JSON", () => {
    expect(parsePendingBashCallDetail("not json")).toBeNull();
  });
});

describe("parsePendingTaskCallDetail", () => {
  it("parses a pending Task tool_call row's arguments into a running TaskDetail", () => {
    const content = JSON.stringify({ arguments: { prompt: "go investigate the bug" } });
    const detail = parsePendingTaskCallDetail(content);
    expect(detail).toEqual({
      toolName: "Task",
      prompt: "go investigate the bug",
      subagentConversationId: "",
      state: "running",
    });
  });

  it("returns null when prompt is missing", () => {
    const content = JSON.stringify({ arguments: {} });
    expect(parsePendingTaskCallDetail(content)).toBeNull();
  });

  it("returns null on malformed JSON", () => {
    expect(parsePendingTaskCallDetail("not json")).toBeNull();
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run src/lib/ipc.test.ts`
Expected: FAIL — `parsePendingBashCallDetail`/`parsePendingTaskCallDetail` are not exported from `./ipc`.

- [ ] **Step 3: Write the implementation**

In `src/lib/ipc.ts`, update `BashDetail` (currently):

```typescript
export interface BashDetail {
  toolName: "Bash";
  command: string | null;
  timeoutMs: number | null;
  outcome: BashOutcome;
  /** 010-context-window-management/US3: set when this result was large
   * enough to be offloaded to disk — the model saw only a preview, but the
   * full stdout/stderr is still readable from this path. */
  offloadedTo?: string | null;
}
```

Replace with (`outcome` now optional — absent means "still running," fed by `parsePendingBashCallDetail`; `tokenCount` added):

```typescript
export interface BashDetail {
  toolName: "Bash";
  command: string | null;
  timeoutMs: number | null;
  /** Absent while the command is still running — see BashWidget's pending
   * branch, fed by `parsePendingBashCallDetail`. */
  outcome?: BashOutcome;
  /** 010-context-window-management/US3: set when this result was large
   * enough to be offloaded to disk — the model saw only a preview, but the
   * full stdout/stderr is still readable from this path. */
  offloadedTo?: string | null;
  /** Real tokenizer count of this result's model-facing text — see
   * `context::annotate_with_token_count` on the backend. Only ever set for
   * Read/Bash/Grep/Glob (the four tools whose size varies enough to make a
   * cost badge worth showing). */
  tokenCount?: number;
}
```

Add `tokenCount?: number` to `ReadDetail`, `GrepDetail`, `GlobDetail` (each gets the same field, same doc comment). `ReadDetail` currently ends:

```typescript
export interface ReadDetail {
  toolName: "Read";
  filePath: string | null;
  offset: number | null;
  limit: number | null;
  outcome: ReadOutcome;
  /** 010-context-window-management/US3: set when this result was large
   * enough to be offloaded to disk — the model saw only a preview, but the
   * full content is still readable from this path. */
  offloadedTo?: string | null;
}
```

Add after `offloadedTo`:

```typescript
  /** Real tokenizer count of this result's content — see
   * `context::annotate_with_token_count` on the backend. */
  tokenCount?: number;
}
```

`GlobDetail` currently:

```typescript
export interface GlobDetail {
  toolName: "Glob";
  pattern: string | null;
  path: string | null;
  matches: string[];
}
```

Add `tokenCount?: number;` before the closing brace, with the same doc comment as above.

`GrepDetail` currently:

```typescript
export interface GrepDetail {
  toolName: "Grep";
  pattern: string | null;
  path: string | null;
  glob: string | null;
  matches: GrepMatch[];
}
```

Add `tokenCount?: number;` before the closing brace, same doc comment.

Then, right after `parseAskUserQuestionCallDetail`'s closing brace, add the two new parsers:

```typescript
/** Parses a still-*pending* `Bash` tool_call row's `content` (shape
 * `{"arguments": {command, timeoutMs}}`) into an outcome-less `BashDetail`
 * — `BashWidget` treats a missing `outcome` as "still running." Returns
 * `null` on any parse failure or missing `command`. */
export function parsePendingBashCallDetail(content: string): BashDetail | null {
  try {
    const parsed = JSON.parse(content) as { arguments?: Record<string, unknown> };
    const args = parsed?.arguments;
    if (!args || typeof args.command !== "string") {
      return null;
    }
    return {
      toolName: "Bash",
      command: args.command,
      timeoutMs: typeof args.timeoutMs === "number" ? args.timeoutMs : null,
    };
  } catch {
    return null;
  }
}

/** Parses a still-*pending* `Task` tool_call row's `content` (shape
 * `{"arguments": {prompt}}`) into a `state: "running"` `TaskDetail` —
 * `TaskWidget` already has a dedicated running-state render branch for
 * this value, previously never actually produced (the backend only ever
 * persisted `tool_result` once the subagent had already finished, so
 * `state` was always `"complete"`). `subagentConversationId` isn't known
 * yet at this point (the subagent hasn't been spawned) — empty string is
 * safe since `TaskWidget` never renders it. Returns `null` on any parse
 * failure or missing `prompt`. */
export function parsePendingTaskCallDetail(content: string): TaskDetail | null {
  try {
    const parsed = JSON.parse(content) as { arguments?: Record<string, unknown> };
    const args = parsed?.arguments;
    if (!args || typeof args.prompt !== "string") {
      return null;
    }
    return {
      toolName: "Task",
      prompt: args.prompt,
      subagentConversationId: "",
      state: "running",
    };
  } catch {
    return null;
  }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npx vitest run src/lib/ipc.test.ts`
Expected: `Test Files 1 passed`, all cases pass.

- [ ] **Step 5: Full frontend sanity check**

Run: `npx tsc -b && npx vitest run`
Expected: `tsc -b` clean; full suite passes (making `BashDetail.outcome` optional could surface a type error anywhere that assumed it was always present outside `BashWidget.tsx` — if `tsc -b` fails, find and fix each such call site before moving on, don't work around the type error by re-widening the field).

- [ ] **Step 6: Commit**

```bash
git add src/lib/ipc.ts src/lib/ipc.test.ts
git commit -m "feat(ipc): add pending tool_call parsers and tokenCount fields"
```

---

## Task 6: Frontend — `ReadWidget` cost badge

**Files:**
- Modify: `src/views/chat/tool-widgets/ReadWidget.tsx`
- Modify: `src/views/chat/tool-widgets/ReadWidget.test.tsx`
- Create: `src/lib/formatByteCount.ts`
- Create: `src/lib/formatByteCount.test.ts`

**Interfaces:**
- Consumes: `ReadDetail.tokenCount` (Task 5), `formatTokenCount` (`src/lib/formatTokenCount.ts`, already exists).
- Produces: `formatByteCount(bytes: number): string` — used here and by Task 8 (`BashWidget`).

- [ ] **Step 1: Write the failing tests**

Create `src/lib/formatByteCount.test.ts`:

```typescript
import { describe, it, expect } from "vitest";
import { formatByteCount } from "./formatByteCount";

describe("formatByteCount", () => {
  it("shows the exact byte count under 1000 bytes", () => {
    expect(formatByteCount(0)).toBe("0B");
    expect(formatByteCount(42)).toBe("42B");
    expect(formatByteCount(999)).toBe("999B");
  });

  it("shows one decimal KB past 1000 bytes", () => {
    expect(formatByteCount(1000)).toBe("1.0KB");
    expect(formatByteCount(1500)).toBe("1.5KB");
    expect(formatByteCount(15600)).toBe("15.6KB");
  });
});
```

Add to `src/views/chat/tool-widgets/ReadWidget.test.tsx`, after the existing "does not show the affordance..." test, before the closing `});`:

```typescript
  it("shows a byte/token cost badge when tokenCount is present", () => {
    const detail: ReadDetail = {
      toolName: "Read",
      filePath: "/tmp/notes.txt",
      offset: null,
      limit: null,
      outcome: { ok: true, content: "hello world", truncated: false },
      tokenCount: 312,
    };
    render(<ReadWidget detail={detail} />);
    expect(screen.getByTestId("read-widget")).toHaveTextContent("312 tok");
  });

  it("shows no cost badge when tokenCount is absent", () => {
    const detail: ReadDetail = {
      toolName: "Read",
      filePath: "/tmp/notes.txt",
      offset: null,
      limit: null,
      outcome: { ok: true, content: "hello world", truncated: false },
    };
    render(<ReadWidget detail={detail} />);
    expect(screen.getByTestId("read-widget")).not.toHaveTextContent("tok");
  });
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run src/lib/formatByteCount.test.ts src/views/chat/tool-widgets/ReadWidget.test.tsx`
Expected: FAIL — `formatByteCount` module doesn't exist yet; the two new `ReadWidget` cases fail (no badge rendered).

- [ ] **Step 3: Write the implementation**

Create `src/lib/formatByteCount.ts`:

```typescript
/** Formats a byte count as a compact, human-readable size ("1.5KB" past
 * 1000 bytes, otherwise the exact count) — mirrors `formatTokenCount`'s
 * own convention, since the two are shown together on a tool-call widget's
 * cost badge. */
export function formatByteCount(bytes: number): string {
  if (bytes >= 1000) {
    return `${(bytes / 1000).toFixed(1)}KB`;
  }
  return `${bytes}B`;
}
```

In `src/views/chat/tool-widgets/ReadWidget.tsx`, add imports at the top:

```typescript
import { formatByteCount } from "@/lib/formatByteCount";
import { formatTokenCount } from "@/lib/formatTokenCount";
```

Find the success-state render block (currently):

```tsx
  return (
    <div className="rounded-lg border border-border bg-card p-3 text-sm" data-testid="read-widget">
      <p className="font-mono text-xs text-muted-foreground">
        Read <span>{detail.filePath}</span>
      </p>
      {detail.outcome.truncated && (
```

Replace the `<p>` block:

```tsx
  return (
    <div className="rounded-lg border border-border bg-card p-3 text-sm" data-testid="read-widget">
      <p className="font-mono text-xs text-muted-foreground">
        Read <span>{detail.filePath}</span>
        {detail.tokenCount != null && (
          <span>
            {" "}
            · {formatByteCount(detail.outcome.content.length)} ·{" "}
            {formatTokenCount(detail.tokenCount)} tok
          </span>
        )}
      </p>
      {detail.outcome.truncated && (
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npx vitest run src/lib/formatByteCount.test.ts src/views/chat/tool-widgets/ReadWidget.test.tsx`
Expected: all pass.

- [ ] **Step 5: Full frontend sanity check**

Run: `npx tsc -b && npx vitest run`
Expected: clean, all pass.

- [ ] **Step 6: Commit**

```bash
git add src/lib/formatByteCount.ts src/lib/formatByteCount.test.ts src/views/chat/tool-widgets/ReadWidget.tsx src/views/chat/tool-widgets/ReadWidget.test.tsx
git commit -m "feat(widgets): show a byte/token cost badge on ReadWidget"
```

---

## Task 7: Frontend — `SearchResultsWidget` cost badge (Glob/Grep)

**Files:**
- Modify: `src/views/chat/tool-widgets/SearchResultsWidget.tsx`
- Modify: `src/views/chat/tool-widgets/SearchResultsWidget.test.tsx`

**Interfaces:**
- Consumes: `GlobDetail.tokenCount`/`GrepDetail.tokenCount` (Task 5), `formatTokenCount`.

Only a token badge here, not bytes — Glob/Grep results are match lists, not a single content blob, so there's no single meaningful "byte size" the way there is for Read/Bash.

- [ ] **Step 1: Write the failing tests**

Add to `src/views/chat/tool-widgets/SearchResultsWidget.test.tsx`, before the closing `});`:

```typescript
  it("shows a token cost badge when tokenCount is present", () => {
    const detail: GlobDetail = {
      toolName: "Glob",
      pattern: "*.rs",
      path: "/tmp/project",
      matches: ["/tmp/project/a.rs"],
      tokenCount: 42,
    };
    render(<SearchResultsWidget detail={detail} />);
    expect(screen.getByTestId("search-widget")).toHaveTextContent("42 tok");
  });

  it("shows no cost badge when tokenCount is absent", () => {
    const detail: GlobDetail = {
      toolName: "Glob",
      pattern: "*.rs",
      path: "/tmp/project",
      matches: ["/tmp/project/a.rs"],
    };
    render(<SearchResultsWidget detail={detail} />);
    expect(screen.getByTestId("search-widget")).not.toHaveTextContent("tok");
  });
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run src/views/chat/tool-widgets/SearchResultsWidget.test.tsx`
Expected: FAIL — no badge rendered yet.

- [ ] **Step 3: Write the implementation**

In `src/views/chat/tool-widgets/SearchResultsWidget.tsx`, add the import:

```typescript
import { formatTokenCount } from "@/lib/formatTokenCount";
```

Find the header line (currently):

```tsx
      <p className="mb-1 font-mono text-xs text-muted-foreground">
        {detail.toolName} {detail.pattern}
      </p>
```

Replace with:

```tsx
      <p className="mb-1 font-mono text-xs text-muted-foreground">
        {detail.toolName} {detail.pattern}
        {detail.tokenCount != null && <span> · {formatTokenCount(detail.tokenCount)} tok</span>}
      </p>
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npx vitest run src/views/chat/tool-widgets/SearchResultsWidget.test.tsx`
Expected: all pass.

- [ ] **Step 5: Full frontend sanity check**

Run: `npx tsc -b && npx vitest run`
Expected: clean, all pass.

- [ ] **Step 6: Commit**

```bash
git add src/views/chat/tool-widgets/SearchResultsWidget.tsx src/views/chat/tool-widgets/SearchResultsWidget.test.tsx
git commit -m "feat(widgets): show a token cost badge on SearchResultsWidget"
```

---

## Task 8: Frontend — `BashWidget` cost badge and pending/running state

**Files:**
- Modify: `src/views/chat/tool-widgets/BashWidget.tsx`
- Modify: `src/views/chat/tool-widgets/BashWidget.test.tsx`

**Interfaces:**
- Consumes: `BashDetail.tokenCount`, `BashDetail.outcome` (now optional) (Task 5), `formatTokenCount`.

- [ ] **Step 1: Write the failing tests**

Add to `src/views/chat/tool-widgets/BashWidget.test.tsx`, before the closing `});`:

```typescript
  it("shows a token cost badge in the status row when tokenCount is present", () => {
    const detail: BashDetail = {
      toolName: "Bash",
      command: "cargo test --lib",
      timeoutMs: null,
      outcome: { ok: true, exitCode: 0, stdout: "ok", stderr: "" },
      tokenCount: 89,
    };
    render(<BashWidget detail={detail} />);
    expect(screen.getByTestId("bash-status")).toHaveTextContent("89 tok");
  });

  // --- pending/running state (no outcome yet) ---

  it("renders a pending state (command shown, no outcome) as Running…", () => {
    const detail: BashDetail = {
      toolName: "Bash",
      command: "cargo test --test agent_benchmark tier4_planned",
      timeoutMs: null,
    };
    render(<BashWidget detail={detail} />);
    expect(screen.getByTestId("bash-status")).toHaveTextContent(/running/i);
    expect(screen.getByTestId("bash-command")).toHaveTextContent(
      "cargo test --test agent_benchmark tier4_planned",
    );
    expect(screen.queryByTestId("bash-stdout")).not.toBeInTheDocument();
  });
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run src/views/chat/tool-widgets/BashWidget.test.tsx`
Expected: FAIL — no cost badge; the pending test throws (`detail.outcome.ok` on `undefined`).

- [ ] **Step 3: Write the implementation**

In `src/views/chat/tool-widgets/BashWidget.tsx`, add the import:

```typescript
import { formatTokenCount } from "@/lib/formatTokenCount";
```

Find the top of the component:

```tsx
export default function BashWidget({ detail }: BashWidgetProps) {
  if (!detail.outcome.ok) {
```

Insert a new pending branch before the existing failure check:

```tsx
export default function BashWidget({ detail }: BashWidgetProps) {
  if (!detail.outcome) {
    return (
      <div className="overflow-hidden rounded-lg border border-border" data-testid="bash-widget">
        <div
          className="flex items-center justify-between border-b border-border px-3 py-1.5 font-mono text-xs text-sky-600 dark:text-sky-400"
          data-testid="bash-status"
        >
          <span>Running…</span>
        </div>
        <pre
          className="overflow-x-auto whitespace-pre-wrap break-words bg-card px-3 py-2 font-mono text-xs"
          data-testid="bash-command"
        >
          $ {detail.command}
        </pre>
      </div>
    );
  }

  if (!detail.outcome.ok) {
```

Find the success-state status row (currently):

```tsx
        data-testid="bash-status"
      >
        <span>{succeeded ? "Success" : `Failed (exit ${exitCode})`}</span>
        <span>exit {exitCode}</span>
      </div>
```

Replace the second `<span>`:

```tsx
        data-testid="bash-status"
      >
        <span>{succeeded ? "Success" : `Failed (exit ${exitCode})`}</span>
        <span>
          exit {exitCode}
          {detail.tokenCount != null && ` · ${formatTokenCount(detail.tokenCount)} tok`}
        </span>
      </div>
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npx vitest run src/views/chat/tool-widgets/BashWidget.test.tsx`
Expected: all pass, including all pre-existing tests (unchanged behavior for the success/failure branches beyond the added badge).

- [ ] **Step 5: Full frontend sanity check**

Run: `npx tsc -b && npx vitest run`
Expected: clean, all pass.

- [ ] **Step 6: Commit**

```bash
git add src/views/chat/tool-widgets/BashWidget.tsx src/views/chat/tool-widgets/BashWidget.test.tsx
git commit -m "feat(widgets): add BashWidget pending state and cost badge"
```

---

## Task 9: Frontend — generalize `Workspace.tsx`'s pending-tool-call rendering to Bash/Task

**Files:**
- Modify: `src/views/workspace/Workspace.tsx`
- Modify: `src/views/workspace/Workspace.test.tsx`

**Interfaces:**
- Consumes: `parsePendingBashCallDetail`, `parsePendingTaskCallDetail` (Task 5), `BashWidget` (Task 8), `TaskWidget` (unchanged).

- [ ] **Step 1: Write the failing tests**

Add to `src/views/workspace/Workspace.test.tsx`, right after the existing `'shows the pending question widget...'` test (same `describe` block, same file):

```typescript
  it("shows a pending Bash widget (not \"Working…\") when the latest message is an unfinished Bash tool_call", async () => {
    vi.mocked(commands.listMessages).mockResolvedValue([
      {
        id: "u1",
        conversationId: "conv-1",
        role: "user",
        contentType: "text",
        content: "run the tests",
        toolName: null,
        createdAt: 1,
        durationMs: null,
        tokenCount: null,
      },
      {
        id: "tc1",
        conversationId: "conv-1",
        role: "assistant",
        contentType: "tool_call",
        content: JSON.stringify({ arguments: { command: "cargo test --lib" } }),
        toolName: "Bash",
        createdAt: 2,
        durationMs: null,
        tokenCount: null,
      },
    ]);
    vi.mocked(commands.sendAgentMessage).mockReturnValue(new Promise(() => {}));

    render(<Workspace conversationId="conv-1" />);

    const status = await screen.findByTestId("bash-status");
    expect(status).toHaveTextContent(/running/i);
    expect(screen.getByTestId("bash-command")).toHaveTextContent("cargo test --lib");
    expect(screen.queryByTestId("agent-thinking")).not.toBeInTheDocument();
  });

  it("shows a pending Task widget when the latest message is an unfinished Task tool_call", async () => {
    vi.mocked(commands.listMessages).mockResolvedValue([
      {
        id: "u1",
        conversationId: "conv-1",
        role: "user",
        contentType: "text",
        content: "investigate the bug",
        toolName: null,
        createdAt: 1,
        durationMs: null,
        tokenCount: null,
      },
      {
        id: "tc1",
        conversationId: "conv-1",
        role: "assistant",
        contentType: "tool_call",
        content: JSON.stringify({ arguments: { prompt: "find the root cause" } }),
        toolName: "Task",
        createdAt: 2,
        durationMs: null,
        tokenCount: null,
      },
    ]);
    vi.mocked(commands.sendAgentMessage).mockReturnValue(new Promise(() => {}));

    render(<Workspace conversationId="conv-1" />);

    const status = await screen.findByTestId("task-status");
    expect(status).toHaveTextContent(/running/i);
    expect(screen.queryByTestId("agent-thinking")).not.toBeInTheDocument();
  });

  it("does not show a pending Bash widget once the tool_result has landed (latest message is the result, not the call)", async () => {
    vi.mocked(commands.listMessages).mockResolvedValue([
      {
        id: "tc1",
        conversationId: "conv-1",
        role: "assistant",
        contentType: "tool_call",
        content: JSON.stringify({ arguments: { command: "cargo test --lib" } }),
        toolName: "Bash",
        createdAt: 1,
        durationMs: null,
        tokenCount: null,
      },
      {
        id: "tr1",
        conversationId: "conv-1",
        role: "tool",
        contentType: "tool_result",
        content: JSON.stringify({
          toolName: "Bash",
          command: "cargo test --lib",
          outcome: { ok: true, exitCode: 0, stdout: "ok", stderr: "" },
        }),
        toolName: "Bash",
        createdAt: 2,
        durationMs: null,
        tokenCount: null,
      },
    ]);

    render(<Workspace conversationId="conv-1" />);

    await screen.findByTestId("bash-widget");
    const statuses = screen.getAllByTestId("bash-status");
    expect(statuses).toHaveLength(1);
    expect(statuses[0]).not.toHaveTextContent(/running/i);
  });
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run src/views/workspace/Workspace.test.tsx`
Expected: FAIL — the two new pending-widget tests find nothing (`pendingQuestion` only recognizes `AskUserQuestion` today).

- [ ] **Step 3: Write the implementation**

In `src/views/workspace/Workspace.tsx`, update the imports (currently):

```typescript
import AskUserQuestionWidget from "@/views/chat/tool-widgets/AskUserQuestionWidget";
import {
  commands,
  events,
  parseAskUserQuestionCallDetail,
  type Message,
  type RichMessageContent,
} from "@/lib/ipc";
```

Replace with:

```typescript
import AskUserQuestionWidget from "@/views/chat/tool-widgets/AskUserQuestionWidget";
import BashWidget from "@/views/chat/tool-widgets/BashWidget";
import TaskWidget from "@/views/chat/tool-widgets/TaskWidget";
import {
  commands,
  events,
  parseAskUserQuestionCallDetail,
  parsePendingBashCallDetail,
  parsePendingTaskCallDetail,
  type Message,
  type RichMessageContent,
} from "@/lib/ipc";
```

Find the `pendingQuestion` derivation (currently):

```typescript
  const pendingQuestion = useMemo(() => {
    const latest = messages[messages.length - 1];
    if (latest?.contentType === "tool_call" && latest.toolName === "AskUserQuestion") {
      return parseAskUserQuestionCallDetail(latest.content);
    }
    return null;
  }, [messages]);
```

Replace with a broader `pendingToolCall` that still exposes `pendingQuestion` under its original name (every existing reference to `pendingQuestion` elsewhere in this file — the composer-disable check, the autoscroll effect's dependency array, etc. — stays unchanged):

```typescript
  // Generalizes the same "latest message is an unpaired tool_call" signal
  // AskUserQuestion has always used (sequence ordering guarantees a
  // tool_result can only ever land immediately after its tool_call, so
  // this is a reliable "still in flight" check for any tool) to also cover
  // Bash/Task — the two tools slow enough for a live pending state to
  // matter (010-context-window-management follow-up: widget cost badges +
  // progressive rendering design doc).
  const pendingToolCall = useMemo(() => {
    const latest = messages[messages.length - 1];
    if (latest?.contentType !== "tool_call") return null;
    if (latest.toolName === "AskUserQuestion") {
      const detail = parseAskUserQuestionCallDetail(latest.content);
      return detail ? { kind: "question" as const, detail } : null;
    }
    if (latest.toolName === "Bash") {
      const detail = parsePendingBashCallDetail(latest.content);
      return detail ? { kind: "bash" as const, detail } : null;
    }
    if (latest.toolName === "Task") {
      const detail = parsePendingTaskCallDetail(latest.content);
      return detail ? { kind: "task" as const, detail } : null;
    }
    return null;
  }, [messages]);
  const pendingQuestion = pendingToolCall?.kind === "question" ? pendingToolCall.detail : null;
```

Now find every other reference to `pendingQuestion` in the file and replace it with `pendingToolCall` for the "is something pending" checks, while keeping `pendingQuestion` for the question-specific render. Search: `grep -n "pendingQuestion" src/views/workspace/Workspace.tsx` and, for each match outside the derivation itself:
- `[messages, pendingQuestion, scheduleScrollToTranscriptBottom, showThinking]` (the autoscroll effect's deps) → change `pendingQuestion` to `pendingToolCall`.
- `if ((!content.trim() && !richContent) || sendInFlight || pendingQuestion) return false;` (the send-blocking check) → change `pendingQuestion` to `pendingToolCall`.
- `disabled={sendInFlight || pendingQuestion !== null}` (the composer's disabled prop) → change `pendingQuestion` to `pendingToolCall`.

Finally, find the render block (currently):

```tsx
            {pendingQuestion ? (
              <div
                className="mb-6"
                data-testid="chat-message"
                role="group"
                aria-label="doce replied"
              >
                <AskUserQuestionWidget detail={pendingQuestion} />
              </div>
            ) : (
              showThinking && (
                <p className="text-sm text-muted-foreground" data-testid="agent-thinking">
                  Working…
                </p>
              )
            )}
```

Replace with:

```tsx
            {pendingToolCall ? (
              <div
                className="mb-6"
                data-testid="chat-message"
                role="group"
                aria-label="doce replied"
              >
                {pendingToolCall.kind === "question" && (
                  <AskUserQuestionWidget detail={pendingToolCall.detail} />
                )}
                {pendingToolCall.kind === "bash" && <BashWidget detail={pendingToolCall.detail} />}
                {pendingToolCall.kind === "task" && <TaskWidget detail={pendingToolCall.detail} />}
              </div>
            ) : (
              showThinking && (
                <p className="text-sm text-muted-foreground" data-testid="agent-thinking">
                  Working…
                </p>
              )
            )}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npx vitest run src/views/workspace/Workspace.test.tsx`
Expected: all pass, including the original AskUserQuestion pending test (unchanged behavior).

- [ ] **Step 5: Full frontend sanity check**

Run: `npx tsc -b && npx vitest run`
Expected: clean, all pass.

- [ ] **Step 6: Commit**

```bash
git add src/views/workspace/Workspace.tsx src/views/workspace/Workspace.test.tsx
git commit -m "feat(workspace): render a live pending state for Bash and Task tool calls"
```

---

## Task 10: Frontend — `WidgetGallery` updates

**Files:**
- Modify: `src/views/design-system/WidgetGallery.tsx`

**Interfaces:**
- Consumes: everything from Tasks 5–9 (`tokenCount` fields, `BashDetail.outcome` optionality).

No new test file — `WidgetGallery` has no dedicated test today (it's a hand-populated reference page, not behavior under test); this task is verified by `tsc -b` (type-correctness of the new sample data) and a manual look in the running app.

- [ ] **Step 1: Populate `tokenCount` in the existing Read/Bash/Grep examples**

In `src/views/design-system/WidgetGallery.tsx`, find the Read "Success" example:

```tsx
          <Example label="Success">
            <ReadWidget
              detail={{
                toolName: "Read",
                filePath: "src/agent/dispatch.rs",
                offset: null,
                limit: null,
                outcome: { ok: true, content: "pub fn execute(...", truncated: false },
              }}
            />
          </Example>
```

Add `tokenCount: 312,` right after `outcome: { ... },`:

```tsx
          <Example label="Success">
            <ReadWidget
              detail={{
                toolName: "Read",
                filePath: "src/agent/dispatch.rs",
                offset: null,
                limit: null,
                outcome: { ok: true, content: "pub fn execute(...", truncated: false },
                tokenCount: 312,
              }}
            />
          </Example>
```

Find the Bash "Success (exit 0)" example:

```tsx
          <Example label="Success (exit 0)">
            <BashWidget
              detail={{
                toolName: "Bash",
                command: "cargo test --lib",
                timeoutMs: null,
                outcome: { ok: true, exitCode: 0, stdout: "test result: ok. 202 passed", stderr: "" },
              }}
            />
          </Example>
```

Add `tokenCount: 89,` after the `outcome` line:

```tsx
          <Example label="Success (exit 0)">
            <BashWidget
              detail={{
                toolName: "Bash",
                command: "cargo test --lib",
                timeoutMs: null,
                outcome: { ok: true, exitCode: 0, stdout: "test result: ok. 202 passed", stderr: "" },
                tokenCount: 89,
              }}
            />
          </Example>
```

Find the Glob "with matches" example:

```tsx
          <Example label="Glob, with matches">
            <SearchResultsWidget
              detail={{
                toolName: "Glob",
                pattern: "bug_*.txt",
                path: ".",
                matches: ["bug_00.txt", "bug_01.txt", "bug_02.txt"],
              }}
            />
          </Example>
```

Add `tokenCount: 24,`:

```tsx
          <Example label="Glob, with matches">
            <SearchResultsWidget
              detail={{
                toolName: "Glob",
                pattern: "bug_*.txt",
                path: ".",
                matches: ["bug_00.txt", "bug_01.txt", "bug_02.txt"],
                tokenCount: 24,
              }}
            />
          </Example>
```

Find the Grep "with matches" example:

```tsx
          <Example label="Grep, with matches">
            <SearchResultsWidget
              detail={{
                toolName: "Grep",
                pattern: "// BUG:",
                path: ".",
                glob: null,
                matches: [
                  { path: "bug_00.txt", lineNumber: 1, line: "// BUG: this should compute a + b" },
                  { path: "bug_01.txt", lineNumber: 1, line: "// BUG: this should compute a + b" },
                ],
              }}
            />
          </Example>
```

Add `tokenCount: 51,`:

```tsx
          <Example label="Grep, with matches">
            <SearchResultsWidget
              detail={{
                toolName: "Grep",
                pattern: "// BUG:",
                path: ".",
                glob: null,
                matches: [
                  { path: "bug_00.txt", lineNumber: 1, line: "// BUG: this should compute a + b" },
                  { path: "bug_01.txt", lineNumber: 1, line: "// BUG: this should compute a + b" },
                ],
                tokenCount: 51,
              }}
            />
          </Example>
```

- [ ] **Step 2: Add pending examples for Bash and Task**

Find the Bash `Section`'s closing (right after the "Dispatch failure (denylisted)" example, before `</Section>`):

```tsx
          <Example label="Dispatch failure (denylisted)">
            <BashWidget
              detail={{
                toolName: "Bash",
                command: "rm -rf ~",
                timeoutMs: null,
                outcome: { ok: false, error: "command rejected: matches a catastrophic pattern" },
              }}
            />
          </Example>
        </Section>
```

Add a new `Example` between the last one and `</Section>`:

```tsx
          <Example label="Dispatch failure (denylisted)">
            <BashWidget
              detail={{
                toolName: "Bash",
                command: "rm -rf ~",
                timeoutMs: null,
                outcome: { ok: false, error: "command rejected: matches a catastrophic pattern" },
              }}
            />
          </Example>
          <Example label="Pending (still running)">
            <BashWidget
              detail={{
                toolName: "Bash",
                command: "cargo test --test agent_benchmark tier4_planned -- --ignored --nocapture",
                timeoutMs: null,
              }}
            />
          </Example>
        </Section>
```

Find the Task `Section`'s "Running" example (already exists — it's the one thing that already demonstrates this state, since `state: "running"` was always a valid value even though production never produced it):

```tsx
          <Example label="Running">
            <TaskWidget
              detail={{
                toolName: "Task",
                prompt: "Investigate why tier4 scores 0/20 and report the root cause",
                subagentConversationId: "design-system-preview",
                state: "running",
              }}
            />
          </Example>
```

No change needed here — leave as-is (it already correctly demonstrates the now-real "running" state).

- [ ] **Step 3: Run the frontend sanity check**

Run: `npx tsc -b && npx vitest run`
Expected: `tsc -b` clean, full suite passes.

- [ ] **Step 4: Manual verification**

Start (or use the already-running) dev app, press `⌘D` to open the widget gallery, and confirm: the Read/Bash/Glob/Grep "Success"-style examples now show a `· N tok` (and, for Read, `· N.NKB ·`) badge; Bash's new "Pending (still running)" example shows a sky-blue "Running…" status with the command line visible and no stdout/stderr section.

- [ ] **Step 5: Commit**

```bash
git add src/views/design-system/WidgetGallery.tsx
git commit -m "feat(design-system): show cost badges and pending states in the widget gallery"
```

---

## Self-Review Notes

- **Spec coverage**: Section 1 (cost badges) → Tasks 1, 2, 3 (backend), 5, 6, 7, 8 (frontend). Section 2 (progressive rendering) → Tasks 3, 4 (backend persist-split), 5, 8, 9 (frontend). Section 3 (gallery updates) → Task 10. All three design-doc sections are covered.
- **Design doc correction carried through**: the plan reflects the corrected Section 2 (real backend change needed — Tasks 3 and 4), not the original "no backend changes" draft.
- **Type consistency checked**: `BashDetail.outcome` becomes optional in Task 5 and every later task (6 doesn't touch Bash, 8, 9, 10) treats it consistently as optional; `parsePendingBashCallDetail`/`parsePendingTaskCallDetail` (Task 5) return exactly the types `Workspace.tsx` (Task 9) consumes; `handle_general_tool_call` (Task 3)'s signature matches its call site in `execute_top_level_tool` exactly.
- **No placeholders**: every step has complete code, not a description of code.
