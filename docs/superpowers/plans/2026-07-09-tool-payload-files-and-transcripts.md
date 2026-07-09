# Tool Payload Files, Read Truncation, and Materialized Transcripts — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every data-tool result is written to a payload file; SQLite stores only what entered the model's context; the model recovers anything via Read — including a per-conversation transcript file of everything it ever saw.

**Architecture:** Three pieces in strict order. Piece 1: a `context/payload.rs` staging function writes every Bash/Grep/Glob/Write/Edit result to `<app_data>/tool-outputs/<conv>/<call_id>.txt` and thresholds (by tokens) whether `model_text` is the full result or a status reference line; `detail` shrinks to metadata + `payloadRef`. Piece 2: `fs::read` gains per-line and total byte caps; Read never writes files (its `payloadRef` is the source path). Piece 3: a `context/transcript.rs` module renders `[#seq role]` entries of `model_text` into `<app_data>/transcripts/<conv>.txt`, appended from a new single `storage::messages::insert` helper that replaces every hand-rolled `MAX(sequence)+1` site; healing regenerates the file from SQLite on mismatch.

**Tech Stack:** Rust (Tauri 2, rusqlite/tokio-rusqlite, serde_json, tempfile for tests), React/TypeScript frontend (vitest), spec at `docs/superpowers/specs/2026-07-09-tool-payload-files-and-transcripts-design.md`.

## Global Constraints

- Governing invariant (spec): **SQLite stores exactly what entered the model's context; files store the canonical payloads that might enter it.** No unbounded text may reach `model_text` on any path, including failure paths.
- Payload file write order: **file first, then SQLite row** — a crash may orphan a file, never a row referencing a missing file.
- Transcript files are **derived caches**: regenerable from SQLite at any time; append failures are logged and swallowed.
- Constants (spec, verbatim): threshold default = `CONTEXT_WINDOW_TOKENS / 16` (= 512) under settings key `context.toolOutputOffloadTokens`; `READ_MAX_LINE_CHARS = 2000`; `READ_MAX_BYTES = 8192`; `TRANSCRIPT_ARGS_CAP_CHARS = 2000`; `PREVIEW_CHARS = 500` (retained).
- Carve-outs (spec): **Read** never writes a payload copy; **Task**, **AskUserQuestion**, and plan-tool replies stay inline with no payload file.
- Rust tests: `cargo test` run from `src-tauri/`. Frontend tests: `npm test` (vitest) from repo root. Format Rust with `cargo fmt`, TS with `npm run format` (oxfmt — this repo does not use prettier). Lint TS with `npm run lint` (oxlint).
- Commit after every task. Do not commit failing tests.

---

### Task 1: Token-denominated threshold setting

**Files:**
- Modify: `src-tauri/src/context/limits.rs` (add constant; keep `DEFAULT_TOOL_OUTPUT_OFFLOAD_CHARS` until Task 3 removes its last consumer)
- Modify: `src-tauri/src/context/mod.rs:55-150` (ContextSettings)
- Test: inline `#[cfg(test)]` in both files (existing modules)

**Interfaces:**
- Produces: `ContextSettings.tool_output_offload_tokens: usize` (default 512), `ContextSettings::KEY_TOOL_OUTPUT_OFFLOAD_TOKENS = "context.toolOutputOffloadTokens"`, `limits::DEFAULT_TOOL_OUTPUT_OFFLOAD_TOKENS`. Task 3 consumes the field; Task 3 removes the old `tool_output_offload_chars` field and its key/default.

- [ ] **Step 1: Write the failing test**

In the `tests` module of `src-tauri/src/context/mod.rs`, add:

```rust
#[test]
fn tool_output_offload_tokens_parses_and_defaults() {
    use std::collections::HashMap;
    // Absent -> default.
    let s = ContextSettings::from_raw(&HashMap::new());
    assert_eq!(
        s.tool_output_offload_tokens,
        limits::DEFAULT_TOOL_OUTPUT_OFFLOAD_TOKENS
    );
    // Present and valid -> honored.
    let mut raw = HashMap::new();
    raw.insert(
        ContextSettings::KEY_TOOL_OUTPUT_OFFLOAD_TOKENS.to_string(),
        "1024".to_string(),
    );
    assert_eq!(ContextSettings::from_raw(&raw).tool_output_offload_tokens, 1024);
    // Zero/garbage -> default (same clamp discipline as the other keys).
    raw.insert(
        ContextSettings::KEY_TOOL_OUTPUT_OFFLOAD_TOKENS.to_string(),
        "0".to_string(),
    );
    assert_eq!(
        ContextSettings::from_raw(&raw).tool_output_offload_tokens,
        limits::DEFAULT_TOOL_OUTPUT_OFFLOAD_TOKENS
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run (from `src-tauri/`): `cargo test tool_output_offload_tokens_parses_and_defaults`
Expected: FAIL — `no field tool_output_offload_tokens` (compile error).

- [ ] **Step 3: Implement**

In `src-tauri/src/context/limits.rs`, below `DEFAULT_TOOL_OUTPUT_OFFLOAD_CHARS`:

```rust
/// 1/16 of `CONTEXT_WINDOW_TOKENS` (= 512 today). A tool result whose
/// model-facing text costs at most this many tokens is inlined whole;
/// anything larger becomes a status reference line pointing at its payload
/// file (2026-07-09 payload-files design). Token-denominated successor to
/// `DEFAULT_TOOL_OUTPUT_OFFLOAD_CHARS`.
pub const DEFAULT_TOOL_OUTPUT_OFFLOAD_TOKENS: usize = (CONTEXT_WINDOW_TOKENS / 16) as usize;
```

Add to the existing `budget_constants_stay_proportional_to_the_window` test:

```rust
assert_eq!(
    DEFAULT_TOOL_OUTPUT_OFFLOAD_TOKENS,
    (CONTEXT_WINDOW_TOKENS / 16) as usize
);
```

In `src-tauri/src/context/mod.rs`, extend `ContextSettings`: add field `pub tool_output_offload_tokens: usize,` after `tool_output_offload_chars`; add consts

```rust
pub const DEFAULT_TOOL_OUTPUT_OFFLOAD_TOKENS: usize = limits::DEFAULT_TOOL_OUTPUT_OFFLOAD_TOKENS;
pub const KEY_TOOL_OUTPUT_OFFLOAD_TOKENS: &'static str = "context.toolOutputOffloadTokens";
```

In `from_raw`, mirror the chars parsing:

```rust
let tool_output_offload_tokens = raw
    .get(Self::KEY_TOOL_OUTPUT_OFFLOAD_TOKENS)
    .and_then(|v| v.parse::<usize>().ok())
    .filter(|v| *v > 0)
    .unwrap_or(Self::DEFAULT_TOOL_OUTPUT_OFFLOAD_TOKENS);
```

and include it in the returned `Self { .. }`. In `load`, the SQL `WHERE key IN (?1, ?2, ?3, ?4)` becomes `(?1, ?2, ?3, ?4, ?5)` with `Self::KEY_TOOL_OUTPUT_OFFLOAD_TOKENS` appended to the params list.

- [ ] **Step 4: Run tests**

Run: `cargo test --lib context`
Expected: PASS (all context tests, including the new one).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/context/limits.rs src-tauri/src/context/mod.rs
git commit -m "feat(context): token-denominated tool-output threshold setting"
```

---

### Task 2: `context/payload.rs` — stage every tool result through a payload file

**Files:**
- Create: `src-tauri/src/context/payload.rs`
- Modify: `src-tauri/src/context/mod.rs` (add `pub mod payload;` next to `pub mod offload;` — offload is deleted in Task 3)

**Interfaces:**
- Consumes: nothing from other tasks (pure function; token counting injected).
- Produces (Task 3/4 call this):

```rust
pub struct StagedResult {
    pub model_text: String,          // what the model sees: full result or reference line
    pub payload_ref: Option<String>, // absolute payload-file path; None only on write failure
    pub detail: serde_json::Value,   // slimmed: bulk replaced by previews + byte counts
}

pub fn stage_tool_result(
    app_data_dir: &Path,
    conversation_id: &str,
    tool_call_id: &str,
    model_text: &str,
    detail: serde_json::Value,
    threshold_tokens: usize,
    count_tokens: impl Fn(&str) -> usize,
) -> StagedResult
```

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/context/payload.rs` containing only the test module first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// chars/4 — deterministic stand-in for the real tokenizer.
    fn fake_count(text: &str) -> usize {
        text.chars().count().div_ceil(4)
    }

    fn bash_detail(stdout: &str, stderr: &str) -> serde_json::Value {
        json!({
            "toolName": "Bash", "command": "x", "timeoutMs": null,
            "outcome": {"ok": true, "exitCode": 0, "stdout": stdout, "stderr": stderr},
        })
    }

    #[test]
    fn small_result_inlines_but_still_writes_the_payload_file() {
        let dir = tempfile::tempdir().unwrap();
        let staged = stage_tool_result(
            dir.path(), "conv1", "call1", "short output",
            json!({"toolName": "Grep", "matches": [], "outcome": {"ok": true}}),
            512, fake_count,
        );
        assert_eq!(staged.model_text, "short output");
        let path = staged.payload_ref.expect("payload file must always exist");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "short output");
        assert!(path.contains("conv1") && path.contains("call1"));
    }

    #[test]
    fn oversized_result_becomes_a_status_reference_line() {
        let dir = tempfile::tempdir().unwrap();
        let big = "line of output\n".repeat(500); // ~1875 fake tokens
        let staged = stage_tool_result(
            dir.path(), "conv1", "call2", &big,
            json!({"toolName": "Grep", "matches": ["a", "b"], "outcome": {"ok": true}}),
            512, fake_count,
        );
        let path = staged.payload_ref.clone().unwrap();
        assert!(staged.model_text.starts_with("Grep: 2 matches"));
        assert!(staged.model_text.contains(&path));
        assert!(staged.model_text.contains("Read"));
        assert!(!staged.model_text.contains("line of output"), "no content leaks into a reference line");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), big);
    }

    #[test]
    fn bash_payload_is_full_stdout_and_stderr_from_detail_and_detail_is_slimmed() {
        let dir = tempfile::tempdir().unwrap();
        let stdout = "s".repeat(10_000);
        let stderr = "e".repeat(3_000);
        let staged = stage_tool_result(
            dir.path(), "conv1", "call3",
            "tail-biased preview the model would have seen",
            bash_detail(&stdout, &stderr),
            512, fake_count,
        );
        let written = std::fs::read_to_string(staged.payload_ref.as_ref().unwrap()).unwrap();
        assert!(written.contains(&stdout) && written.contains(&stderr));
        // Slimmed detail: previews + byte counts, bulk gone.
        let out = &staged.detail["outcome"];
        assert!(out.get("stdout").is_none() && out.get("stderr").is_none());
        assert_eq!(out["stdoutBytes"], 10_000);
        assert_eq!(out["stderrBytes"], 3_000);
        assert_eq!(out["stdoutPreview"].as_str().unwrap().chars().count(), 2000);
        // Small model_text -> inlined even though the payload is big.
        assert_eq!(staged.model_text, "tail-biased preview the model would have seen");
    }

    #[test]
    fn oversized_bash_reference_line_carries_exit_code_and_sizes() {
        let dir = tempfile::tempdir().unwrap();
        let big_preview = "x".repeat(4_000); // ~1000 fake tokens > 512
        let staged = stage_tool_result(
            dir.path(), "conv1", "call4", &big_preview,
            bash_detail(&"s".repeat(10_000), ""),
            512, fake_count,
        );
        assert!(staged.model_text.starts_with("Bash: exit 0 — 10000 bytes stdout, 0 bytes stderr"));
        assert!(staged.model_text.contains("Read"));
    }

    #[test]
    fn write_failure_falls_back_to_a_bounded_preview_with_no_payload_ref() {
        // A file path in place of a directory forces create_dir_all to fail.
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("tool-outputs");
        std::fs::write(&blocker, "not a dir").unwrap();
        let big = "y".repeat(10_000);
        let staged = stage_tool_result(
            dir.path(), "conv1", "call5", &big,
            json!({"toolName": "Grep", "matches": [], "outcome": {"ok": true}}),
            512, fake_count,
        );
        assert!(staged.payload_ref.is_none());
        assert!(staged.model_text.contains("could not be saved"));
        assert!(staged.model_text.chars().count() < 700, "fallback must stay bounded");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib context::payload`
Expected: FAIL — `stage_tool_result` not found (compile error). Add `pub mod payload;` to `src-tauri/src/context/mod.rs` first so the module is reachable.

- [ ] **Step 3: Implement**

Above the test module in `src-tauri/src/context/payload.rs`:

```rust
//! Payload-file staging (2026-07-09 payload-files design): every
//! data-tool result is written to
//! `<app_data_dir>/tool-outputs/<conversation_id>/<tool_call_id>.txt` —
//! always, inline or not — and the token threshold only decides whether
//! `model_text` is the full result or a status reference line. Successor
//! to `offload_if_oversized`, which wrote a file only when oversized and
//! left the bulk duplicated in `detail`.

use std::path::Path;

/// How much of an oversized result survives inline when the payload file
/// could not be written (the bounded failure fallback).
const PREVIEW_CHARS: usize = 500;

/// Widget-facing preview length for slimmed `detail` fields.
const DETAIL_PREVIEW_CHARS: usize = 2000;

pub struct StagedResult {
    pub model_text: String,
    pub payload_ref: Option<String>,
    pub detail: serde_json::Value,
}

/// The canonical payload for this result: Bash's is the full untruncated
/// stdout+stderr living in `detail` (its `model_text` is already
/// tail-biased); every other tool's `model_text` IS the full result.
fn full_payload(model_text: &str, detail: &serde_json::Value) -> String {
    if detail["toolName"] == "Bash" {
        let stdout = detail["outcome"]["stdout"].as_str().unwrap_or("");
        let stderr = detail["outcome"]["stderr"].as_str().unwrap_or("");
        if stderr.is_empty() {
            stdout.to_string()
        } else {
            format!("{stdout}\n--- stderr ---\n{stderr}")
        }
    } else {
        model_text.to_string()
    }
}

/// The status line an over-threshold result is replaced with: cheap
/// metadata that answers "did it work / how big" without a Read round-trip.
fn reference_line(detail: &serde_json::Value, payload_bytes: usize, path: &str) -> String {
    let tool = detail["toolName"].as_str().unwrap_or("Tool");
    let stats = match tool {
        "Bash" => {
            let exit = &detail["outcome"]["exitCode"];
            let stdout_b = detail["outcome"]["stdoutBytes"].as_u64().unwrap_or(0);
            let stderr_b = detail["outcome"]["stderrBytes"].as_u64().unwrap_or(0);
            format!("exit {exit} — {stdout_b} bytes stdout, {stderr_b} bytes stderr")
        }
        "Grep" | "Glob" => {
            let n = detail["matches"].as_array().map(|a| a.len()).unwrap_or(0);
            format!("{n} matches")
        }
        _ => format!("{payload_bytes} bytes of output"),
    };
    format!("{tool}: {stats} → Read \"{path}\" to view")
}

/// Replaces bulk text fields in `detail` with bounded previews + byte
/// counts (the spec's "detail becomes pure metadata"). Only Bash carries
/// bulk in `detail` today; other tools' detail passes through unchanged.
fn slim_detail(mut detail: serde_json::Value) -> serde_json::Value {
    if detail["toolName"] == "Bash" {
        if let Some(outcome) = detail["outcome"].as_object_mut() {
            for (bulk, preview_key, bytes_key) in [
                ("stdout", "stdoutPreview", "stdoutBytes"),
                ("stderr", "stderrPreview", "stderrBytes"),
            ] {
                if let Some(text) = outcome.remove(bulk).and_then(|v| v.as_str().map(String::from)) {
                    outcome.insert(bytes_key.to_string(), serde_json::json!(text.len()));
                    outcome.insert(
                        preview_key.to_string(),
                        serde_json::json!(text.chars().take(DETAIL_PREVIEW_CHARS).collect::<String>()),
                    );
                }
            }
        }
    }
    detail
}

pub fn stage_tool_result(
    app_data_dir: &Path,
    conversation_id: &str,
    tool_call_id: &str,
    model_text: &str,
    detail: serde_json::Value,
    threshold_tokens: usize,
    count_tokens: impl Fn(&str) -> usize,
) -> StagedResult {
    let payload = full_payload(model_text, &detail);
    let detail = slim_detail(detail);

    let dir = app_data_dir.join("tool-outputs").join(conversation_id);
    let write_result = std::fs::create_dir_all(&dir)
        .and_then(|()| {
            let path = dir.join(format!("{tool_call_id}.txt"));
            std::fs::write(&path, &payload).map(|()| path)
        });

    match write_result {
        Ok(path) => {
            let path_string = path.to_string_lossy().to_string();
            let model_text = if count_tokens(model_text) <= threshold_tokens {
                model_text.to_string()
            } else {
                reference_line(&detail, payload.len(), &path_string)
            };
            StagedResult { model_text, payload_ref: Some(path_string), detail }
        }
        Err(e) => {
            // Invariant: unbounded text never enters the window, even here.
            let model_text = if count_tokens(model_text) <= threshold_tokens {
                model_text.to_string()
            } else {
                let preview: String = model_text.chars().take(PREVIEW_CHARS).collect();
                format!("{preview}…\n[full output could not be saved: {e}]")
            };
            StagedResult { model_text, payload_ref: None, detail }
        }
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib context::payload`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/context/payload.rs src-tauri/src/context/mod.rs
git commit -m "feat(context): payload-file staging for every data-tool result"
```

---

### Task 3: Wire the top-level path; retire `offload.rs` and the chars setting

**Files:**
- Modify: `src-tauri/src/commands/agent.rs:1003-1041` (`handle_general_tool_call`)
- Modify: `src-tauri/src/storage/conversations.rs:29-66` (`HistoryMessage.offloaded_to` → `payload_ref`; `parse_tool_row_flags`)
- Modify: `src-tauri/src/context/mod.rs` (clearing uses renamed field; remove chars setting; remove `pub mod offload;`)
- Modify: `src-tauri/src/context/limits.rs` (remove `DEFAULT_TOOL_OUTPUT_OFFLOAD_CHARS`; reword pointer placeholder)
- Delete: `src-tauri/src/context/offload.rs`
- Test: existing tests in `commands/agent.rs` (`handle_general_tool_call_persists_...` around line 2143) plus new ones below

**Interfaces:**
- Consumes: `stage_tool_result` (Task 2), `ContextSettings.tool_output_offload_tokens` (Task 1).
- Produces: persisted `detail.payloadRef` (string path) on every non-Read general tool result; Read results carry `detail.payloadRef` = the tool's resolved source path. `HistoryMessage.payload_ref: Option<String>` (renamed from `offloaded_to`), parsed from `detail.payloadRef` with `detail.offloadedTo` as legacy fallback. `limits::tool_cleared_placeholder_with_pointer(path)` now reads `"[Old tool result cleared; recover with Read \"{path}\"]"`.

- [ ] **Step 1: Write the failing tests**

In `commands/agent.rs`'s test module (near the existing `handle_general_tool_call_persists_the_tool_call_row_before_the_tool_result_row` test, which shows how to build a test conn/engine — follow its arrangement exactly):

```rust
#[tokio::test]
async fn general_tool_result_carries_a_payload_ref_and_bounded_model_text() {
    // Arrange identically to handle_general_tool_call_persists_... (test
    // conn via storage::test_connection wrapped in tokio_rusqlite, no app),
    // then execute a Bash call producing >512 tokens of output:
    //   {"name": "Bash", "arguments": {"command": "yes x | head -5000"}}
    // Assert on the persisted tool_result row:
    //   - detail.payloadRef is a path that exists on disk
    //   - the file contains the full stdout (5000 lines)
    //   - model_text starts with "Bash: exit 0" and contains the path
    //   - detail.outcome.stdout is absent; stdoutPreview/stdoutBytes present
}

#[tokio::test]
async fn read_tool_result_references_its_source_and_writes_no_copy() {
    // Execute a Read call on a real temp file; assert:
    //   - detail.payloadRef equals the resolved source path
    //   - no file was created under <app_data>/tool-outputs/
    //   - model_text is fs::read's numbered output, unstaged
}
```

(Write them as real tests, not comments — the sketches above name every assertion; the arrangement boilerplate is copied from the adjacent existing test.)

In `storage/conversations.rs` tests:

```rust
#[test]
fn parse_tool_row_flags_reads_payload_ref_with_offloaded_to_fallback() {
    let (_, new_key) = parse_tool_row_flags(r#"{"payloadRef": "/p/new.txt"}"#);
    assert_eq!(new_key.as_deref(), Some("/p/new.txt"));
    let (_, legacy) = parse_tool_row_flags(r#"{"offloadedTo": "/p/old.txt"}"#);
    assert_eq!(legacy.as_deref(), Some("/p/old.txt"));
    let (_, both) = parse_tool_row_flags(r#"{"payloadRef": "/p/new.txt", "offloadedTo": "/p/old.txt"}"#);
    assert_eq!(both.as_deref(), Some("/p/new.txt"), "payloadRef wins");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test general_tool_result_carries_a_payload_ref parse_tool_row_flags_reads_payload_ref`
Expected: FAIL (payloadRef never stamped; parse ignores the new key).

- [ ] **Step 3: Implement**

**`handle_general_tool_call`** — replace lines 1006-1027 (the offload block and `detail["offloadedTo"]` stamp) with:

```rust
let settings = crate::context::ContextSettings::load(conn)
    .await
    .unwrap_or_else(|_| crate::context::ContextSettings::from_raw(&Default::default()));

let (model_text, detail) = if call.name == "Read" {
    // Carve-out: never write a copy of a file we just read — the payload
    // reference IS the source. fs::read's own caps (Task 5) bound the text.
    let mut detail = outcome.detail.clone();
    detail["payloadRef"] = detail["filePath"].clone();
    (outcome.model_text.clone(), detail)
} else {
    match app.and_then(|a| a.path().app_data_dir().ok()) {
        Some(app_data_dir) => {
            let staged = crate::context::payload::stage_tool_result(
                &app_data_dir,
                parent_conversation_id,
                tool_call_id,
                &outcome.model_text,
                outcome.detail.clone(),
                settings.tool_output_offload_tokens,
                |text| engine.count_tokens(text).unwrap_or(usize::MAX),
            );
            let mut detail = staged.detail;
            detail["payloadRef"] = serde_json::json!(staged.payload_ref);
            (staged.model_text, detail)
        }
        None => (outcome.model_text.clone(), outcome.detail.clone()),
    }
};
```

(The `None` arm is the unit-test path with no app handle; tests that need staging pass a tempdir-backed app_data_dir — mirror how the existing offload test at line ~2011 handles this, or refactor `handle_general_tool_call` to take `app_data_dir: Option<PathBuf>` the way the existing code comments say offload did for testability. Prefer the parameter: change the signature to accept `app_data_dir: Option<std::path::PathBuf>`, have `execute_top_level_tool` pass `app.path().app_data_dir().ok()`, and the tests pass `Some(tempdir)`. Update the two existing callers/tests accordingly.)

Note on `count_tokens` failure: `usize::MAX` forces the reference line — a result we cannot measure is treated as oversized, never inlined. On the Read arm, `unwrap_or(usize::MAX)` never runs (no staging).

**`storage/conversations.rs`** — rename field and update parsing:

```rust
// HistoryMessage: rename `offloaded_to` -> `payload_ref` (update doc comment
// to say detail.payloadRef with detail.offloadedTo as the legacy fallback).
pub payload_ref: Option<String>,
```

```rust
fn parse_tool_row_flags(content: &str) -> (bool, Option<String>) {
    let parsed: Option<serde_json::Value> = serde_json::from_str(content).ok();
    let plan = parsed
        .as_ref()
        .and_then(|v| v.get("plan"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let payload_ref = parsed
        .as_ref()
        .and_then(|v| v.get("payloadRef").or_else(|| v.get("offloadedTo")))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    (plan, payload_ref)
}
```

Fix the two construction sites of `HistoryMessage` in this file and every use of `.offloaded_to` in `context/mod.rs` (`apply_lightweight_clearing` lines 264-306 and its tests) — mechanical rename, `cargo build` finds them all.

**Retire the chars setting and offload module:**
- Delete `src-tauri/src/context/offload.rs`; remove `pub mod offload;` from `context/mod.rs`.
- Remove `tool_output_offload_chars` field, `KEY_TOOL_OUTPUT_OFFLOAD_CHARS`, `DEFAULT_TOOL_OUTPUT_OFFLOAD_CHARS` (both files), its `from_raw` parsing, its slot in `load`'s SQL (back to 4 placeholders), and the `assert!(DEFAULT_TOOL_OUTPUT_OFFLOAD_CHARS >= 1500)` line in the limits test. An existing user-set `context.toolOutputOffloadChars` row is simply never read again (spec: dropped, not converted).
- Reword the pointer placeholder in `limits.rs` (it now also covers Read rows whose ref is a source path, so "full output saved at" is wrong):

```rust
pub fn tool_cleared_placeholder_with_pointer(payload_ref: &str) -> String {
    format!("[Old tool result cleared; recover with Read \"{payload_ref}\"]")
}
```

Update the offload-era doc comments on this function and on `HistoryMessage` to reference `context::payload::stage_tool_result`.

- [ ] **Step 4: Run the full Rust suite**

Run: `cargo test`
Expected: PASS. Existing offload tests are gone with the module; existing clearing tests compile against `payload_ref`; the two new agent tests and the flags test pass.

- [ ] **Step 5: Commit**

```bash
git add -A src-tauri/src
git commit -m "feat(agent): stage top-level tool results through payload files; retire offload"
```

---

### Task 4: Subagent staging + honest Bash marker

**Files:**
- Modify: `src-tauri/src/commands/agent.rs:669-676` (SubagentBackend struct), `:730-782` (its `execute_tool`), `:907-914` (construction site)
- Modify: `src-tauri/src/agent/tools/bash.rs:54-58` (marker text) and its tests

**Interfaces:**
- Consumes: `stage_tool_result` (Task 2).
- Produces: subagent tool_result rows carry `payloadRef` exactly like top-level rows; `truncate_tail_biased`'s marker no longer claims the output is "preserved in the conversation transcript".

- [ ] **Step 1: Write the failing test**

In `commands/agent.rs` tests, next to the existing SubagentBackend test that calls `backend.execute_tool("call1", call)` (line ~2132) — same arrangement, new assertions:

```rust
#[tokio::test]
async fn subagent_tool_result_carries_a_payload_ref() {
    // Same test-conn arrangement as the existing execute_tool test, plus a
    // tempdir passed as the backend's app_data_dir. Execute a Bash call,
    // then SELECT the subagent's tool_result row and assert
    // detail.payloadRef exists on disk and holds the command's stdout.
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test subagent_tool_result_carries_a_payload_ref`
Expected: FAIL — SubagentBackend has no `app_data_dir` field (compile error).

- [ ] **Step 3: Implement**

Add the field to the struct:

```rust
struct SubagentBackend<'a> {
    engine: &'a InferenceEngine,
    conn: &'a tokio_rusqlite::Connection,
    subagent_id: &'a str,
    cwd: Option<&'a Path>,
    threshold: u32,
    plan_state: crate::agent::plan::PlanState,
    /// Payload staging root (2026-07-09 payload-files design) — resolved by
    /// the spawn site, which holds the AppHandle this backend deliberately
    /// doesn't. None only in unit tests that don't exercise staging.
    app_data_dir: Option<std::path::PathBuf>,
}
```

Construction site (line ~907) gains `app_data_dir: app.path().app_data_dir().ok(),`. Existing test constructions gain `app_data_dir: None` (or `Some(tempdir)` in the new test).

In `SubagentBackend::execute_tool`, between the `annotate_with_token_count` line (768) and `persist_tool_call_and_result` (769), insert the same staging block as Task 3's, with `parent_conversation_id` → `self.subagent_id`, `app.and_then(...)` → `self.app_data_dir.as_deref()`, and the same Read carve-out. Pass the staged `model_text`/`detail` into `persist_tool_call_and_result` instead of `outcome.model_text`/`outcome.detail`, and return `ToolExecution::Result(staged_model_text)`. The `ContextSettings::load(self.conn)` call mirrors Task 3's.

In `bash.rs`, change the marker line to:

```rust
"{}\n... [{omitted} bytes omitted — full output saved to this call's payload file]\n{}",
```

and update the bash test that asserts on the old marker text (grep for `preserved in the conversation transcript` under `src-tauri/`; the dispatch doc comment at `dispatch.rs:342-347` mentions offload — update it to name `stage_tool_result`).

- [ ] **Step 4: Run tests**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands/agent.rs src-tauri/src/agent/tools/bash.rs src-tauri/src/agent/dispatch.rs
git commit -m "feat(agent): payload staging on the subagent path; honest Bash marker"
```

---

### Task 5: Read truncation caps (Piece 2)

**Files:**
- Modify: `src-tauri/src/agent/tools/fs.rs:14-31` (`read`) and its tests
- Modify: `src-tauri/src/agent/dispatch.rs:218-244` (Read arm: `detail.outcome.content` → bounded preview)

**Interfaces:**
- Consumes: nothing new.
- Produces: `fs::read` output is bounded by `READ_MAX_LINE_CHARS = 2000` per line and `READ_MAX_BYTES = 8192` total, with a continue-offset marker. Read's `detail.outcome` carries `contentPreview` (2000 chars) + `contentBytes` instead of full `content` — frontend types updated in Task 9.

- [ ] **Step 1: Write the failing tests**

In `fs.rs` tests:

```rust
#[test]
fn read_clamps_single_long_lines() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("long.txt");
    std::fs::write(&p, format!("{}\nshort", "a".repeat(5000))).unwrap();
    let out = read(&p, None, None).unwrap();
    let first_line = out.lines().next().unwrap();
    assert!(first_line.len() < 2100, "long line must be clamped");
    assert!(first_line.ends_with("… [line truncated]"));
    assert!(out.contains("short"), "later lines still served");
}

#[test]
fn read_caps_total_bytes_with_a_continue_offset() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("big.txt");
    // 1000 lines x ~30 bytes ≈ 30KB > READ_MAX_BYTES.
    std::fs::write(&p, "0123456789012345678901234\n".repeat(1000)).unwrap();
    let out = read(&p, None, None).unwrap();
    assert!(out.len() <= 8192 + 200, "body bounded (allow marker slack)");
    // The marker names the exact offset to continue from: the number of
    // lines already emitted (offset is a skip count).
    let emitted = out.lines().count() - 1; // minus the marker line
    assert!(out.trim_end().ends_with(&format!("continue with offset={emitted}]")));
    // And that offset actually continues where this read stopped.
    let next = read(&p, Some(emitted), None).unwrap();
    assert!(next.starts_with(&format!("{:>6}\t", emitted + 1)));
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib agent::tools::fs`
Expected: FAIL (no clamping today).

- [ ] **Step 3: Implement**

Replace `read` in `fs.rs`:

```rust
/// Per-line clamp: a single pathological line (minified JS, one-line JSONL
/// record) must not blow through the total cap on its own.
pub const READ_MAX_LINE_CHARS: usize = 2000;
/// Total output cap: ~2k tokens of the 8192-token window. The marker names
/// the exact `offset` to continue from, so paging never needs guesswork.
pub const READ_MAX_BYTES: usize = 8192;

/// `Read` (FR-009): matches Claude Code's own tool — 1-indexed line
/// numbers, `cat -n`-style, with optional offset/limit for large files.
/// Not sandboxed to any workspace (FR-009 explicitly: "without restricting
/// these actions to the opened workspace folder"). Output is bounded
/// (2026-07-09 payload-files design): long lines are clamped and the total
/// is capped with an honest continue-from marker, because Read results are
/// never payload-staged — this truncation is the only thing standing
/// between a huge file and the model's context window.
pub fn read(path: &Path, offset: Option<usize>, limit: Option<usize>) -> Result<String, ToolError> {
    let content = fs::read_to_string(path)?;
    let start = offset.unwrap_or(0);
    let take = limit.unwrap_or(2000);

    let mut out = String::new();
    let mut emitted = 0usize;
    for (i, line) in content.lines().enumerate().skip(start).take(take) {
        let clamped: String = if line.chars().count() > READ_MAX_LINE_CHARS {
            let head: String = line.chars().take(READ_MAX_LINE_CHARS).collect();
            format!("{head}… [line truncated]")
        } else {
            line.to_string()
        };
        let rendered = format!("{:>6}\t{clamped}\n", i + 1);
        if out.len() + rendered.len() > READ_MAX_BYTES {
            let continue_from = start + emitted;
            out.push_str(&format!(
                "[capped at {} bytes — continue with offset={continue_from}]\n",
                out.len()
            ));
            return Ok(out);
        }
        out.push_str(&rendered);
        emitted += 1;
    }
    Ok(out)
}
```

Note on the continue offset: it must be an absolute skip count from the file start (`start + emitted`), because `offset` is a skip count. The test computes `emitted` from the output alone with `offset=None` (start = 0), so both agree there. Treat the test's `read(&p, Some(emitted), None)` round-trip assertion as the source of truth for off-by-one: if it fails, the marker's arithmetic is wrong, not the test.

In `dispatch.rs`'s Read `Ok(content)` arm, replace `"content": content` with:

```rust
"contentPreview": content.chars().take(2000).collect::<String>(),
"contentBytes": content.len(),
```

(keep `"truncated": truncated`). Grep `src/` for `outcome.content` users — `ReadWidget` renders it; that frontend change is Task 9, and until then the widget shows nothing for new rows, which is acceptable mid-plan on a feature branch.

- [ ] **Step 4: Run tests**

Run: `cargo test`
Expected: PASS (fix any dispatch test asserting on `outcome.content`).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/agent/tools/fs.rs src-tauri/src/agent/dispatch.rs
git commit -m "feat(tools): bounded Read — per-line clamp and total cap with continue-offset marker"
```

---

### Task 6: `context/transcript.rs` — render, append, regenerate, heal

**Files:**
- Create: `src-tauri/src/context/transcript.rs`
- Modify: `src-tauri/src/context/mod.rs` (`pub mod transcript;`)

**Interfaces:**
- Consumes: nothing from other tasks (plain `rusqlite::Connection` + paths).
- Produces (Tasks 7-8 call these):

```rust
pub const TRANSCRIPT_ARGS_CAP_CHARS: usize = 2000;

pub fn transcript_path(transcript_dir: &Path, conversation_id: &str) -> PathBuf; // <dir>/<conv>.txt

/// One rendered entry, ending in exactly one blank line.
pub fn render_entry(
    seq: i64, role: &str, content_type: &str,
    tool_name: Option<&str>, body: &str,
) -> String;

/// Best-effort append; errors are returned for the caller to log-and-drop.
pub fn append_entry(transcript_dir: &Path, conversation_id: &str, entry: &str) -> std::io::Result<()>;

/// Full rebuild from SQLite: render every row, write temp, atomic rename.
pub fn regenerate(conn: &rusqlite::Connection, transcript_dir: &Path, conversation_id: &str) -> rusqlite::Result<()>;

/// Regenerates unless the file's last "[#seq" equals MAX(sequence).
pub fn heal_if_stale(conn: &rusqlite::Connection, transcript_dir: &Path, conversation_id: &str) -> rusqlite::Result<()>;
```

Header formats (fixed contract, pinned by the golden test): `[#{seq} user]`, `[#{seq} assistant]`, `[#{seq} assistant → {tool}]` (tool_call), `[#{seq} {tool} result]` (tool_result), `[#{seq} error]`, `[#{seq} context-notice]`, fallback `[#{seq} {role} {content_type}]`. Body is capped at `TRANSCRIPT_ARGS_CAP_CHARS` for `tool_call` rows only (Write/Edit args embed whole files); all other bodies are `model_text`-bounded already.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::test_connection;

    fn insert_row(conn: &rusqlite::Connection, conv: &str, seq: i64, role: &str,
                  ct: &str, content: &str, tool: Option<&str>, model_text: Option<&str>) {
        conn.execute(
            "INSERT INTO messages (id, conversation_id, role, content_type, content, tool_name, created_at, sequence, model_text) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7, ?8)",
            rusqlite::params![uuid::Uuid::now_v7().to_string(), conv, role, ct, content, tool, seq, model_text],
        ).unwrap();
    }

    fn seed_conversation(conn: &rusqlite::Connection, id: &str) {
        conn.execute(
            "INSERT INTO conversations (id, workspace_id, spawned_by_conversation_id, title, created_at, updated_at) \
             VALUES (?1, NULL, NULL, 'T', 0, 0)",
            [id],
        ).unwrap();
    }

    #[test]
    fn golden_entry_formats() {
        assert_eq!(render_entry(1, "user", "text", None, "hello"), "[#1 user]\nhello\n\n");
        assert_eq!(
            render_entry(2, "assistant", "tool_call", Some("Bash"), r#"{"command":"ls"}"#),
            "[#2 assistant → Bash]\n{\"command\":\"ls\"}\n\n"
        );
        assert_eq!(
            render_entry(3, "tool", "tool_result", Some("Bash"), "ok"),
            "[#3 Bash result]\nok\n\n"
        );
        assert_eq!(render_entry(4, "assistant", "error", None, "boom"), "[#4 error]\nboom\n\n");
        // tool_call bodies are capped; others are not.
        let big = "x".repeat(5000);
        let capped = render_entry(5, "assistant", "tool_call", Some("Write"), &big);
        assert!(capped.len() < 2200 && capped.contains("… [args truncated]"));
    }

    #[test]
    fn regenerate_then_append_matches_regenerate_from_scratch() {
        let conn = test_connection();
        let dir = tempfile::tempdir().unwrap();
        seed_conversation(&conn, "c1");
        insert_row(&conn, "c1", 0, "user", "text", "hi", None, None);
        insert_row(&conn, "c1", 1, "assistant", "text", "hello", None, None);
        regenerate(&conn, dir.path(), "c1").unwrap();
        // Now append row 2 both ways and compare byte-for-byte.
        insert_row(&conn, "c1", 2, "tool", "tool_result", "{}", Some("Bash"), Some("done"));
        append_entry(dir.path(), "c1", &render_entry(2, "tool", "tool_result", Some("Bash"), "done")).unwrap();
        let appended = std::fs::read_to_string(transcript_path(dir.path(), "c1")).unwrap();
        regenerate(&conn, dir.path(), "c1").unwrap();
        let rebuilt = std::fs::read_to_string(transcript_path(dir.path(), "c1")).unwrap();
        assert_eq!(appended, rebuilt);
    }

    #[test]
    fn heal_regenerates_on_missing_stale_or_torn_files() {
        let conn = test_connection();
        let dir = tempfile::tempdir().unwrap();
        seed_conversation(&conn, "c1");
        insert_row(&conn, "c1", 0, "user", "text", "hi", None, None);
        // Missing file -> created.
        heal_if_stale(&conn, dir.path(), "c1").unwrap();
        let p = transcript_path(dir.path(), "c1");
        assert!(p.exists());
        let healthy = std::fs::read_to_string(&p).unwrap();
        // Fresh file + no new rows -> untouched (same content).
        heal_if_stale(&conn, dir.path(), "c1").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), healthy);
        // Torn tail -> rebuilt.
        std::fs::write(&p, format!("{healthy}[#gar")).unwrap();
        heal_if_stale(&conn, dir.path(), "c1").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), healthy);
        // Stale (missing latest row) -> rebuilt.
        insert_row(&conn, "c1", 1, "assistant", "text", "hello", None, None);
        heal_if_stale(&conn, dir.path(), "c1").unwrap();
        assert!(std::fs::read_to_string(&p).unwrap().contains("[#1 assistant]"));
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib context::transcript`
Expected: FAIL (module doesn't exist until you add the stubs + `pub mod transcript;`).

- [ ] **Step 3: Implement**

```rust
//! Materialized conversation transcripts (2026-07-09 payload-files design):
//! a per-conversation text file of exactly what the model saw, one
//! `[#seq role]` entry per message row. A DERIVED, REGENERABLE cache of
//! SQLite — never authoritative, so every consistency question is answered
//! by `regenerate`. Entry bodies are `model_text` (bounded by payload
//! staging), so the file can never hand the model an unbounded line.

use std::path::{Path, PathBuf};

/// Write/Edit tool_call args embed whole files; cap them in the transcript.
pub const TRANSCRIPT_ARGS_CAP_CHARS: usize = 2000;

pub fn transcript_path(transcript_dir: &Path, conversation_id: &str) -> PathBuf {
    transcript_dir.join(format!("{conversation_id}.txt"))
}

pub fn render_entry(
    seq: i64,
    role: &str,
    content_type: &str,
    tool_name: Option<&str>,
    body: &str,
) -> String {
    let header = match (content_type, tool_name) {
        ("tool_call", Some(tool)) => format!("[#{seq} assistant → {tool}]"),
        ("tool_result", Some(tool)) => format!("[#{seq} {tool} result]"),
        ("error", _) => format!("[#{seq} error]"),
        ("context_notice", _) => format!("[#{seq} context-notice]"),
        ("text", _) | ("rich_text", _) => format!("[#{seq} {role}]"),
        (other, _) => format!("[#{seq} {role} {other}]"),
    };
    let body = if content_type == "tool_call" && body.chars().count() > TRANSCRIPT_ARGS_CAP_CHARS {
        let head: String = body.chars().take(TRANSCRIPT_ARGS_CAP_CHARS).collect();
        format!("{head}… [args truncated]")
    } else {
        body.to_string()
    };
    format!("{header}\n{body}\n\n")
}

pub fn append_entry(
    transcript_dir: &Path,
    conversation_id: &str,
    entry: &str,
) -> std::io::Result<()> {
    std::fs::create_dir_all(transcript_dir)?;
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(transcript_path(transcript_dir, conversation_id))?;
    f.write_all(entry.as_bytes())
}

/// The per-row body: what the model saw. tool rows use `model_text`
/// (falling back to `content` for legacy rows persisted before model_text
/// existed); everything else uses `content`.
fn row_body(content_type: &str, content: &str, model_text: Option<&str>) -> String {
    match content_type {
        "tool_call" | "tool_result" => model_text.unwrap_or(content).to_string(),
        _ => content.to_string(),
    }
}

pub fn regenerate(
    conn: &rusqlite::Connection,
    transcript_dir: &Path,
    conversation_id: &str,
) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare(
        "SELECT sequence, role, content_type, content, tool_name, model_text \
         FROM messages WHERE conversation_id = ?1 ORDER BY sequence ASC",
    )?;
    let entries = stmt
        .query_map([conversation_id], |row| {
            let seq: i64 = row.get(0)?;
            let role: String = row.get(1)?;
            let content_type: String = row.get(2)?;
            let content: String = row.get(3)?;
            let tool_name: Option<String> = row.get(4)?;
            let model_text: Option<String> = row.get(5)?;
            Ok(render_entry(
                seq, &role, &content_type, tool_name.as_deref(),
                &row_body(&content_type, &content, model_text.as_deref()),
            ))
        })?
        .collect::<rusqlite::Result<Vec<String>>>()?;

    // Derived cache: IO failures must not fail the caller's DB work.
    let _ = std::fs::create_dir_all(transcript_dir);
    let tmp = transcript_dir.join(format!("{conversation_id}.txt.tmp"));
    if std::fs::write(&tmp, entries.concat()).is_ok() {
        let _ = std::fs::rename(&tmp, transcript_path(transcript_dir, conversation_id));
    }
    Ok(())
}

/// Last entry seq actually in the file, or None if missing/unparseable.
fn last_file_seq(path: &Path) -> Option<i64> {
    let content = std::fs::read_to_string(path).ok()?;
    let idx = content.rfind("[#")?;
    let rest = &content[idx + 2..];
    let end = rest.find(' ')?;
    rest[..end].parse().ok()
}

pub fn heal_if_stale(
    conn: &rusqlite::Connection,
    transcript_dir: &Path,
    conversation_id: &str,
) -> rusqlite::Result<()> {
    let max_seq: Option<i64> = conn.query_row(
        "SELECT MAX(sequence) FROM messages WHERE conversation_id = ?1",
        [conversation_id],
        |row| row.get(0),
    )?;
    let file_seq = last_file_seq(&transcript_path(transcript_dir, conversation_id));
    if file_seq != max_seq {
        regenerate(conn, transcript_dir, conversation_id)?;
    }
    Ok(())
}
```

One subtlety the torn-tail test pins: a trailing `[#gar` makes `rfind("[#")` land on garbage, `parse` fails → `None != max_seq` → regenerate. Exactly the wanted behavior, no special-casing.

- [ ] **Step 4: Run tests**

Run: `cargo test --lib context::transcript`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/context/transcript.rs src-tauri/src/context/mod.rs
git commit -m "feat(context): materialized transcript render/append/regenerate/heal"
```

---

### Task 7: `storage::messages::insert` — one choke point for every message insert

**Files:**
- Create: `src-tauri/src/storage/messages.rs`
- Modify: `src-tauri/src/storage/mod.rs` (`pub mod messages;`)
- Modify: every hand-rolled `MAX(sequence)+1` insert site. Enumerate them with:
  `grep -rn "COALESCE(MAX(sequence)" src-tauri/src` — expected sites: `commands/agent.rs` (persist_tool_call ~line 235, persist_tool_result ~line 299, and any sibling persist helpers the grep reveals), `scheduler/worker.rs` (~line 137), `storage/conversations.rs` (~line 387, `persist_context_notice` and the user/assistant insert helpers), `agent/subagent.rs` (~line 54 seed message).

**Interfaces:**
- Consumes: `transcript::{render_entry, append_entry}`, `row_body` logic (Task 6 — expose the body choice by having `insert` decide it the same way; see code).
- Produces (Task 8 relies on the transcript side-effect; all existing callers rely on identical DB semantics):

```rust
pub struct NewMessage<'a> {
    pub conversation_id: &'a str,
    pub role: &'a str,
    pub content_type: &'a str,
    pub content: &'a str,
    pub tool_name: Option<&'a str>,
    pub tool_call_id: Option<&'a str>,
    pub model_text: Option<&'a str>,
    pub created_at: i64,
    pub duration_ms: Option<i64>,
    pub token_count: Option<i64>,
}

/// Allocates MAX(sequence)+1, inserts, best-effort-appends the transcript
/// entry. Returns the allocated sequence. `transcript_dir: None` (tests,
/// callers without an AppHandle) skips the append — heal_if_stale
/// regenerates on next conversation open, so a skipped append is never
/// corruption, only staleness.
pub fn insert(
    conn: &rusqlite::Connection,
    transcript_dir: Option<&std::path::Path>,
    msg: &NewMessage,
) -> rusqlite::Result<i64>;
```

- [ ] **Step 1: Write the failing tests**

In `storage/messages.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::test_connection;

    fn seed_conversation(conn: &rusqlite::Connection, id: &str) {
        conn.execute(
            "INSERT INTO conversations (id, workspace_id, spawned_by_conversation_id, title, created_at, updated_at) \
             VALUES (?1, NULL, NULL, 'T', 0, 0)",
            [id],
        ).unwrap();
    }

    fn msg<'a>(ct: &'a str, content: &'a str) -> NewMessage<'a> {
        NewMessage {
            conversation_id: "c1", role: "user", content_type: ct, content,
            tool_name: None, tool_call_id: None, model_text: None,
            created_at: 0, duration_ms: None, token_count: None,
        }
    }

    #[test]
    fn allocates_sequences_and_appends_transcript() {
        let conn = test_connection();
        let dir = tempfile::tempdir().unwrap();
        seed_conversation(&conn, "c1");
        assert_eq!(insert(&conn, Some(dir.path()), &msg("text", "one")).unwrap(), 0);
        assert_eq!(insert(&conn, Some(dir.path()), &msg("text", "two")).unwrap(), 1);
        let t = std::fs::read_to_string(dir.path().join("c1.txt")).unwrap();
        assert_eq!(t, "[#0 user]\none\n\n[#1 user]\ntwo\n\n");
    }

    #[test]
    fn tool_rows_render_model_text_not_content() {
        let conn = test_connection();
        let dir = tempfile::tempdir().unwrap();
        seed_conversation(&conn, "c1");
        let m = NewMessage {
            role: "tool", content_type: "tool_result",
            content: r#"{"toolName":"Bash","big":"detail"}"#,
            tool_name: Some("Bash"), tool_call_id: Some("tc1"),
            model_text: Some("what the model saw"),
            ..msg("tool_result", "")
        };
        insert(&conn, Some(dir.path()), &m).unwrap();
        let t = std::fs::read_to_string(dir.path().join("c1.txt")).unwrap();
        assert_eq!(t, "[#0 Bash result]\nwhat the model saw\n\n");
    }

    #[test]
    fn none_transcript_dir_skips_the_append_but_inserts() {
        let conn = test_connection();
        seed_conversation(&conn, "c1");
        assert_eq!(insert(&conn, None, &msg("text", "one")).unwrap(), 0);
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib storage::messages`
Expected: FAIL (module missing).

- [ ] **Step 3: Implement the helper**

```rust
//! The single insert path for `messages` rows (2026-07-09 payload-files
//! design): sequence allocation, the INSERT itself, and the transcript
//! append live together so the transcript file and the table cannot drift
//! via a forgotten call site — previously 7+ sites each hand-rolled
//! `COALESCE(MAX(sequence), -1) + 1`.

use crate::context::transcript;
use rusqlite::Connection;
use uuid::Uuid;

pub struct NewMessage<'a> {
    pub conversation_id: &'a str,
    pub role: &'a str,
    pub content_type: &'a str,
    pub content: &'a str,
    pub tool_name: Option<&'a str>,
    pub tool_call_id: Option<&'a str>,
    pub model_text: Option<&'a str>,
    pub created_at: i64,
    pub duration_ms: Option<i64>,
    pub token_count: Option<i64>,
}

pub fn insert(
    conn: &Connection,
    transcript_dir: Option<&std::path::Path>,
    msg: &NewMessage,
) -> rusqlite::Result<i64> {
    let seq: i64 = conn.query_row(
        "SELECT COALESCE(MAX(sequence), -1) + 1 FROM messages WHERE conversation_id = ?1",
        [msg.conversation_id],
        |row| row.get(0),
    )?;
    conn.execute(
        "INSERT INTO messages (id, conversation_id, role, content_type, content, tool_name, \
         created_at, sequence, tool_call_id, model_text, duration_ms, token_count) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        rusqlite::params![
            Uuid::now_v7().to_string(), msg.conversation_id, msg.role, msg.content_type,
            msg.content, msg.tool_name, msg.created_at, seq, msg.tool_call_id,
            msg.model_text, msg.duration_ms, msg.token_count,
        ],
    )?;
    if let Some(dir) = transcript_dir {
        let body = match msg.content_type {
            "tool_call" | "tool_result" => msg.model_text.unwrap_or(msg.content),
            _ => msg.content,
        };
        let entry = transcript::render_entry(seq, msg.role, msg.content_type, msg.tool_name, body);
        // Derived cache: an append failure is staleness, not corruption.
        let _ = transcript::append_entry(dir, msg.conversation_id, &entry);
    }
    Ok(seq)
}
```

- [ ] **Step 4: Run helper tests**

Run: `cargo test --lib storage::messages`
Expected: PASS.

- [ ] **Step 5: Migrate every insert site**

For each grep hit from the Files section: replace the two-statement `MAX(sequence)+1` + `INSERT` with a `storage::messages::insert(conn, transcript_dir, &NewMessage { ... })` call carrying exactly the columns that site set before (absent columns are `None`). Rules:

- **Async wrappers in `commands/agent.rs`** (`persist_tool_call`, `persist_tool_result`, plus siblings): these run inside `conn.call(move |conn| ...)` closures. Each enclosing function gains a `transcript_dir: Option<std::path::PathBuf>` parameter, cloned into the closure. Resolve it once at the call sites that hold an `AppHandle`: `app.map(|a| a.path().app_data_dir().ok().map(|d| d.join("transcripts"))).flatten()`. `persist_tool_result` keeps its `already_paired` EXISTS check before calling the helper — idempotency stays where it is; only the alloc+insert moves.
- **`scheduler/worker.rs`** (~line 137): the worker inserts the assistant text row and bumps `conversations.updated_at` in one transaction. Call the helper inside that same transaction (`&tx` derefs to `&Connection` — `rusqlite::Transaction` implements `Deref<Target = Connection>`, so `messages::insert(&tx, ...)` works). Pass the worker's transcript dir if it holds an `AppHandle`; otherwise `None` (healing covers it — note which one you did in the commit message).
- **`storage/conversations.rs`** (`persist_context_notice` ~line 381, user/assistant insert helpers): same mechanical replacement; these are sync `&Connection` functions — add a `transcript_dir: Option<&Path>` parameter and thread it from callers.
- **`agent/subagent.rs`** (~line 54, the seed user message): pass `None` — the subagent's first heal regenerates.

After each file's migration: `cargo test` must stay green (the existing idempotency and ordering tests in `commands/agent.rs` are the safety net — they now execute through the helper).

- [ ] **Step 6: Verify no hand-rolled site remains**

Run: `grep -rn "COALESCE(MAX(sequence)" src-tauri/src --include='*.rs' | grep -v "storage/messages.rs"`
Expected: no output (test fixtures excepted — inspect any hit).

Run: `cargo test`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add -A src-tauri/src
git commit -m "refactor(storage): single messages::insert choke point with transcript append"
```

---

### Task 8: Heal on open, system-prompt transcript line, transcript-aware clearing

**Files:**
- Modify: `src-tauri/src/agent/plan.rs:74-157` (`plan_system_message` gains `transcript_path: Option<&str>`)
- Modify: `src-tauri/src/commands/agent.rs` (top-level seed + subagent seed at ~line 889 pass the path; heal_if_stale on agent start)
- Modify: `src-tauri/src/commands/conversations.rs` (heal_if_stale where history loads for chat)
- Modify: `src-tauri/src/context/mod.rs:264-306` (`apply_lightweight_clearing` gains transcript context) and `src-tauri/src/context/limits.rs` (new placeholder)

**Interfaces:**
- Consumes: `transcript::{heal_if_stale, transcript_path}` (Task 6), `HistoryMessage.payload_ref` (Task 3).
- Produces: `limits::tool_cleared_placeholder_transcript(path: &str, seq: i64) -> String` = `"[Old tool result cleared; see entry #{seq} in the transcript at \"{path}\" — Read it to recover]"`. New signature `apply_lightweight_clearing(history, keep_n, transcript_path: Option<&str>)`. `plan_system_message(cwd, allow_task, transcript_path)`.

- [ ] **Step 1: Write the failing tests**

In `context/mod.rs` tests (extend the existing clearing tests — they construct `HistoryMessage` directly):

```rust
#[test]
fn cleared_rows_without_payload_ref_cite_their_transcript_entry() {
    // Build 4 tool_result HistoryMessages (seq 0-3), none with payload_ref,
    // then: apply_lightweight_clearing(&mut history, 2, Some("/t/c1.txt"));
    // Assert history[0] and history[1] contents equal
    // limits::tool_cleared_placeholder_transcript("/t/c1.txt", 0) and (…, 1).
}

#[test]
fn cleared_rows_with_payload_ref_still_cite_the_payload_file() {
    // Same shape, payload_ref = Some("/p/x.txt") on the cleared rows;
    // assert placeholder equals tool_cleared_placeholder_with_pointer("/p/x.txt")
    // regardless of the transcript being available.
}
```

In `agent/plan.rs` tests:

```rust
#[test]
fn system_prompt_names_the_transcript_when_given() {
    let with = plan_system_message(None, true, Some("/t/c1.txt"));
    assert!(with.contains("/t/c1.txt"));
    assert!(with.contains("transcript"));
    let without = plan_system_message(None, true, None);
    assert!(!without.contains("transcript"));
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test cleared_rows_with cleared_rows_without system_prompt_names_the_transcript`
Expected: FAIL (compile errors on both new signatures).

- [ ] **Step 3: Implement**

`limits.rs`:

```rust
/// Tier-1 placeholder for a cleared row with no payload file of its own
/// (Task/plan/AskUserQuestion rows, legacy rows): the transcript entry is
/// the recovery route instead.
pub fn tool_cleared_placeholder_transcript(transcript_path: &str, seq: i64) -> String {
    format!(
        "[Old tool result cleared; see entry #{seq} in the transcript at \"{transcript_path}\" — Read it to recover]"
    )
}
```

`apply_lightweight_clearing` — new parameter and placeholder cascade (replace the placeholder match inside the final loop; the tuple gathering must also carry each row's `sequence`):

```rust
pub fn apply_lightweight_clearing(
    history: &mut [HistoryMessage],
    keep_n: usize,
    transcript_path: Option<&str>,
) -> usize {
    let tool_rows: Vec<(usize, bool, Option<String>, i64)> = history
        .iter()
        .enumerate()
        .filter(|(_, m)| m.content_type == "tool_call" || m.content_type == "tool_result")
        .map(|(i, m)| (i, m.plan, m.payload_ref.clone(), m.sequence))
        .collect();

    // plan_to_clear / regular_to_clear selection: byte-for-byte identical to
    // the current implementation at context/mod.rs:272-292 (only the tuple
    // arity changes from 3 to 4 — the added `sequence` rides along unused
    // until the placeholder choice below).

    let mut cleared = 0;
    for (i, _, payload_ref, sequence) in &tool_rows {
        if plan_to_clear.contains(i) || regular_to_clear.contains(i) {
            let placeholder = match (payload_ref, transcript_path) {
                (Some(path), _) => limits::tool_cleared_placeholder_with_pointer(path),
                (None, Some(tp)) => limits::tool_cleared_placeholder_transcript(tp, *sequence),
                (None, None) => TOOL_CLEARED_PLACEHOLDER.to_string(),
            };
            history[*i].chat.content = MessageContent::Text(placeholder);
            cleared += 1;
        }
    }
    cleared
}
```

Callers found by `cargo build` after the signature change (expected: `compute_usage_via_conn` at mod.rs:243, the maybe_compact path, and tests). Each caller that has a conversation id + app_data_dir derives the path with `transcript::transcript_path(&app_data_dir.join("transcripts"), conversation_id).display().to_string()`; callers without one pass `None`.

`plan_system_message` — append, when `transcript_path` is `Some(p)`:

```rust
"\n\n# Transcript\nThis conversation's transcript — everything so far, including content no longer in your context — is at \"{p}\". Read it to recall earlier work."
```

Seed sites: the top-level agent seed in `commands/agent.rs` (grep `plan_system_message(`) passes its conversation's transcript path; the subagent seed (line ~889) passes the **subagent's own** path (`subagent_id`), never the parent's — this is the spec's isolation stance, enforce it in a comment there.

Heal-on-open: at the top of the agent-turn entry point in `commands/agent.rs` (where history first loads for a conversation — grep `load_history_annotated`/`load_history_via_conn` callers in `commands/`) and its chat-mode counterpart in `commands/conversations.rs`, insert one `conn.call` that runs `transcript::heal_if_stale(conn, &transcript_dir, &conversation_id)` before the load, swallowing errors (`let _ =`). One call per user-visible entry point, not per turn inside the loop.

- [ ] **Step 4: Run the full suite**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A src-tauri/src
git commit -m "feat(context): transcript in system prompt, heal-on-open, restorable clearing for every row"
```

---

### Task 9: Frontend — slimmed detail shapes and lazy payload loading

**Files:**
- Modify: `src/lib/ipc.ts` (`BashDetail.outcome`: `stdout/stderr` → `stdoutPreview/stderrPreview/stdoutBytes/stderrBytes`; `ReadDetail.outcome`: `content` → `contentPreview/contentBytes`; both details gain `payloadRef?: string | null`)
- Modify: `src/views/chat/tool-widgets/BashWidget.tsx`, `src/views/chat/tool-widgets/ReadWidget.tsx` (render previews; `ViewFullOutput` gets `payloadRef ?? offloadedTo`)
- Test: `src/views/chat/tool-widgets/BashWidget.test.tsx`, `ReadWidget.test.tsx`

**Interfaces:**
- Consumes: the Rust detail shapes from Tasks 3-5 (`stdoutPreview`, `stderrPreview`, `stdoutBytes`, `stderrBytes`, `contentPreview`, `contentBytes`, `payloadRef`). `ViewFullOutput` already lazy-loads any path via the existing `read_attached_file` IPC — no new command needed.
- Produces: widgets that render instantly from bounded metadata and load bulk on demand. Legacy rows (old `stdout`/`content`/`offloadedTo` keys) must still render.

- [ ] **Step 1: Write the failing tests**

Extend `BashWidget.test.tsx` (follow its existing render/query patterns):

```tsx
it("renders the preview fields and offers the payload file", () => {
  render(
    <BashWidget
      detail={{
        toolName: "Bash", command: "cargo test", timeoutMs: null,
        payloadRef: "/data/tool-outputs/c1/tc1.txt",
        outcome: {
          ok: true, exitCode: 0,
          stdoutPreview: "running 214 tests…", stdoutBytes: 48213,
          stderrPreview: "", stderrBytes: 0,
        },
      }}
    />,
  );
  expect(screen.getByTestId("bash-stdout")).toHaveTextContent("running 214 tests…");
  expect(screen.getByTestId("view-full-output-button")).toBeInTheDocument();
});

it("still renders legacy rows with inline stdout and offloadedTo", () => {
  render(
    <BashWidget
      detail={{
        toolName: "Bash", command: "ls", timeoutMs: null,
        offloadedTo: "/old/offload.txt",
        outcome: { ok: true, exitCode: 0, stdout: "a.txt", stderr: "" },
      }}
    />,
  );
  expect(screen.getByTestId("bash-stdout")).toHaveTextContent("a.txt");
  expect(screen.getByTestId("view-full-output-button")).toBeInTheDocument();
});
```

Mirror the same two cases for `ReadWidget` with `contentPreview`/legacy `content`.

- [ ] **Step 2: Run to verify failure**

Run: `npm test -- BashWidget`
Expected: FAIL (type errors / missing rendering).

- [ ] **Step 3: Implement**

`ipc.ts` — make the outcome fields a union of new + legacy (all optional), e.g. for Bash:

```ts
outcome?: {
  ok: boolean;
  error?: string;
  exitCode?: number;
  /** New rows (payload-files design): bounded previews + byte counts. */
  stdoutPreview?: string;
  stderrPreview?: string;
  stdoutBytes?: number;
  stderrBytes?: number;
  /** Legacy rows persisted before the payload-files design. */
  stdout?: string;
  stderr?: string;
};
payloadRef?: string | null;
offloadedTo?: string | null;
```

`BashWidget.tsx` — resolve once at the top of the success branch:

```tsx
const stdout = detail.outcome.stdoutPreview ?? detail.outcome.stdout ?? "";
const stderr = detail.outcome.stderrPreview ?? detail.outcome.stderr ?? "";
const payloadPath = detail.payloadRef ?? detail.offloadedTo;
```

Rendering below is unchanged (the existing `truncatedLines` cap still applies to previews); the footer becomes `{payloadPath && <ViewFullOutput path={payloadPath} />}`. Same pattern in `ReadWidget` with `contentPreview ?? content`.

- [ ] **Step 4: Run frontend checks**

Run: `npm test` then `npm run lint` then `npm run format`
Expected: tests PASS, no lint errors, formatter clean.

- [ ] **Step 5: Commit**

```bash
git add src/lib/ipc.ts src/views/chat/tool-widgets/
git commit -m "feat(ui): render slimmed tool detail with lazy payload loading"
```

---

### Task 10: End-to-end verification

**Files:**
- No new code. Possibly touch: `tests/e2e/specs/` if an existing spec asserts on old detail shapes.

- [ ] **Step 1: Full test suites**

Run (from `src-tauri/`): `cargo test`
Run (from repo root): `npm test`
Expected: all PASS.

- [ ] **Step 2: E2E suite**

Run: `npm run test:e2e`
Expected: PASS. The subagent spec (`tests/e2e/specs/subagent.spec.ts`) is the SC-008 regression guard — the parent transcript must carry only the Task result. If a spec asserts on `detail.outcome.stdout`, update it to the preview fields.

- [ ] **Step 3: Manual smoke (dev app)**

Run: `npm run tauri dev`, open a conversation in agent mode, and verify the full loop the spec promises:
1. Ask for something producing big output ("run `find / -name '*.plist' 2>/dev/null | head -3000`"). The widget shows exit code + previews instantly; "View full output" loads the payload file; `<app_data>/tool-outputs/<conv>/` contains the file.
2. `<app_data>/transcripts/<conv>.txt` exists and its entries match what the model saw (reference line, not 3000 paths).
3. Keep the conversation going past `TOOL_KEEP_N` tool calls; ask the model about the earlier output — it should Read the payload file or transcript back (cleared placeholder cites a real path).
4. Delete `<app_data>/transcripts/<conv>.txt`, reopen the conversation, send a message — the file regenerates (healing).

- [ ] **Step 4: Final commit / cleanup**

`git status` must be clean except intentional changes. Squash-fix any review debris, then:

```bash
git log --oneline main..HEAD   # confirm one commit per task, coherent messages
```

---

## Self-Review Notes (already applied)

- **Spec coverage:** Piece 1 → Tasks 1-4; Piece 2 → Task 5; Piece 3 → Tasks 6-8; frontend consequence → Task 9; spec's testing section → distributed per task + Task 10. The spec's GC-on-conversation-deletion item has **no task**: no deletion command exists in the codebase today (verified by grep); the spec's own wording ("wherever conversations are deleted, today or future") defers it. When deletion is built, GC `transcripts/<id>.txt` + `tool-outputs/<id>/` alongside.
- **Type consistency:** `payload_ref`/`payloadRef` naming is uniform Rust/JSON; `stage_tool_result` signature identical in Tasks 2, 3, 4; `render_entry` signature identical in Tasks 6, 7; placeholder function names identical in Tasks 3, 8.
- **Known judgment calls for the implementer:** (a) Task 3 prefers passing `app_data_dir` as a parameter over `Option<&AppHandle>` for testability — follow it; (b) Task 7's worker transcript_dir may be `None` if the worker holds no AppHandle — healing makes this safe; state the choice in the commit; (c) Read's `payloadRef` uses `detail.filePath` (the model-supplied path, possibly relative) — acceptable because the same cwd resolution applies when the model Reads it back.
