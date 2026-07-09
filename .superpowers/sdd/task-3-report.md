# Task 3 Report: Wire the top-level path; retire `offload.rs` and the chars setting

## Status: COMPLETED

## Implementation Summary

`handle_general_tool_call` (top-level tool dispatch in `commands/agent.rs`) now stages every
non-`Read` result through `context::payload::stage_tool_result` (Task 2) instead of the old
`context::offload::offload_if_oversized` char-threshold path (Task 1's `offload.rs`, now
deleted). `Read` results get a carve-out: `detail.payloadRef` is set directly to
`detail.filePath` (the file just read) rather than writing a redundant copy.

`handle_general_tool_call` gained a new `app_data_dir: Option<PathBuf>` parameter (per the
controller's testability directive) so unit tests can pass a tempdir directly without a live
`AppHandle`; `execute_top_level_tool` now passes `app.path().app_data_dir().ok()`.

`HistoryMessage.offloaded_to` was renamed to `payload_ref` throughout (`storage/conversations.rs`,
`context/mod.rs`), and `parse_tool_row_flags` now reads `detail.payloadRef` with `detail.offloadedTo`
as a legacy fallback (old rows keep working). `limits::tool_cleared_placeholder_with_pointer` was
reworded to `"[Old tool result cleared; recover with Read \"{payload_ref}\"]"` since the old
"full output saved at" promise no longer holds for a `Read` row's pointer (its `payloadRef` is
the original source file, not a copy).

The `tool_output_offload_chars` setting/field/const/key was removed entirely (both `context/mod.rs`
and `context/limits.rs`), `context/offload.rs` was deleted, and `pub mod offload;` was removed from
`context/mod.rs`. `ContextSettings::load`'s SQL is back to 4 placeholders.

Per the controller's extra items: `ToolOutcome::offload_text()`'s doc comment (`agent/dispatch.rs`)
now references `context::payload::stage_tool_result` instead of the deleted `context::offload`
module (plus one adjacent Bash-arm comment and the `bash_result_model_text` doc comment that also
named the deleted module/pointer text — fixed for consistency since I was already touching this
file). `context/payload.rs`'s floating "Do NOT write a new extraction function..." plan-prose
comment was replaced with a short rationale explaining why `offload_text()` is the payload source.

## TDD Evidence

### RED

Command: `cargo test general_tool_result_carries_a_payload_ref parse_tool_row_flags_reads_payload_ref`
(run before implementing Step 3, with the new tests already written)

```
error[E0061]: this function takes 7 arguments but 8 arguments were supplied
    --> src/commands/agent.rs:2245:26
     |
2245 |         let model_text = handle_general_tool_call(
     |                          ^^^^^^^^^^^^^^^^^^^^^^^^
2246 |             None,
2247 |             Some(app_data_dir.path().to_path_buf()),
     |             --------------------------------------- unexpected argument #2 of type `std::option::Option<std::path::PathBuf>`
...
error: could not compile `doce` (lib test) due to 3 previous errors
```

(`parse_tool_row_flags_reads_payload_ref_with_offloaded_to_fallback` failed the same way
pre-implementation, since `parse_tool_row_flags` didn't yet read `payloadRef`/fall back to
`offloadedTo`.)

### GREEN

Pure unit test (no model needed):
```
$ cargo test --lib parse_tool_row_flags_reads_payload_ref
test storage::conversations::tests::parse_tool_row_flags_reads_payload_ref_with_offloaded_to_fallback ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 299 filtered out
```

The two new agent tests need a real `InferenceEngine` (same as the adjacent
`handle_general_tool_call_persists_...` test they were modeled on — `annotate_with_token_count`
is called unconditionally before the `Read`/staging branch), so they're `#[ignore]`d like their
sibling and were run explicitly with `--ignored` (the model file is present on this machine):
```
$ cargo test --lib -- --ignored --test-threads=1
test commands::agent::tests::general_tool_result_carries_a_payload_ref_and_bounded_model_text ... ok
test commands::agent::tests::handle_general_tool_call_persists_the_tool_call_row_before_the_tool_result_row ... ok
test commands::agent::tests::read_tool_result_references_its_source_and_writes_no_copy ... ok
test commands::agent::tests::subagent_backend_tool_result_carries_a_real_token_count_for_read ... ok
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 296 filtered out
```

Full default suite:
```
$ cargo test
test result: ok. 296 passed; 0 failed; 4 ignored; 0 measured; 0 filtered out   (lib)
... agent_benchmark.rs: 7 ignored (needs real model, unrelated to this task)
... real_model_smoke.rs: 6 ignored (needs real model, unrelated to this task)
```

Also ran the one real-model integration test I had to touch (field rename), with `--ignored`:
```
$ cargo test --test real_model_smoke apply_lightweight_clearing_then_summarize -- --ignored
test apply_lightweight_clearing_then_summarize_against_the_real_model ... ok
```

`cargo clippy --all-targets`: clean (0 warnings) after adding `#[allow(clippy::too_many_arguments)]`
to `handle_general_tool_call` (now 8 params, matching the existing convention already used on
`execute_top_level_tool` and two other functions in this file).

## Files Changed

- `src-tauri/src/commands/agent.rs` — `handle_general_tool_call` rewritten per the brief (new
  `app_data_dir` param, Read carve-out, `stage_tool_result` wiring); `execute_top_level_tool`
  passes `app.path().app_data_dir().ok()`; two new tests + existing test's call site updated.
- `src-tauri/src/storage/conversations.rs` — `HistoryMessage.offloaded_to` → `payload_ref`;
  `parse_tool_row_flags` reads `payloadRef` with `offloadedTo` fallback; both construction sites
  fixed; new `parse_tool_row_flags_reads_payload_ref_with_offloaded_to_fallback` test (verbatim
  from the brief) plus a same-shape "new key" integration test alongside the renamed
  legacy-fallback one.
- `src-tauri/src/context/mod.rs` — `pub mod offload;` removed; `tool_output_offload_chars`
  field/consts/parsing/SQL slot removed; `apply_lightweight_clearing` and its tests use `payload_ref`.
- `src-tauri/src/context/limits.rs` — `DEFAULT_TOOL_OUTPUT_OFFLOAD_CHARS` removed;
  `tool_cleared_placeholder_with_pointer` reworded; stale test assertion removed.
- `src-tauri/src/context/offload.rs` — deleted.
- `src-tauri/src/context/payload.rs` — floating instructional comment replaced with a rationale comment.
- `src-tauri/src/agent/dispatch.rs` — `offload_text()`'s doc comment, the adjacent Bash-arm
  comment, and `bash_result_model_text`'s doc comment updated to reference
  `context::payload::stage_tool_result` / "payload file" instead of the deleted `context::offload`.
- `src-tauri/tests/real_model_smoke.rs` — **not in the brief's file list**, but required: this
  integration test also constructs `HistoryMessage { offloaded_to: None, .. }` directly (twice) and
  would not compile without the rename. Fixed both sites.

## Self-Review Findings

1. **Test assertion bug I caught before it shipped**: my first draft of
   `general_tool_result_carries_a_payload_ref_and_bounded_model_text` asserted
   `written.lines().count() == 5000`, but the payload file holds `offload_text()`'s full
   `"exit_code:/stdout:/stderr:"` rendition, not bare stdout — it actually has 5004 lines (2
   header lines + a blank separator + the `stderr:` label). Fixed to count only the `x` lines
   stdout contributed (`written.lines().filter(|l| *l == "x").count() == 5000`), which is what
   the brief's "the file contains the full stdout (5000 lines)" assertion actually means.
2. **`cargo fmt` scope**: a whole-crate `cargo fmt` run swept in unrelated pre-existing formatting
   debt in `src/agent/mod.rs`, `src/agent/plan.rs`, `src/inference/mod.rs`, and
   `tests/agent_benchmark.rs` (files I made zero semantic edits to, from earlier commits that
   apparently never went through this rustfmt version/config). I reverted those four files to
   `HEAD` twice (fmt was re-run after the clippy fix) to keep this commit scoped to files I
   actually changed for Task 3. Files I *did* substantively edit (`dispatch.rs`, `payload.rs`,
   `commands/agent.rs`, `context/mod.rs`, `context/limits.rs`, `storage/conversations.rs`,
   `tests/real_model_smoke.rs`) kept their full-file fmt output, including incidental fixes to
   pre-existing drift elsewhere in those same files — a normal side effect of formatting a file
   you're already touching, not scope creep.
3. **Read carve-out design note**: the brief's interface section says "Read results carry
   `detail.payloadRef` = the tool's resolved source path," but the actual code snippet
   (`detail["payloadRef"] = detail["filePath"].clone()`) copies `detail.filePath`, which
   `agent::dispatch::execute`'s Read arm sets to the *raw, unresolved* tool-supplied path (not
   `resolve_against(cwd, ...)`'s output — that resolved `PathBuf` is only used for the actual
   `fs::read` call, never stored anywhere in `detail`). I implemented exactly the brief's code
   (unchanged, since `dispatch.rs`'s Read arm is out of this task's declared scope) and wrote
   `read_tool_result_references_its_source_and_writes_no_copy` using an **absolute** `file_path`
   in its `Read` call so `filePath` and "resolved source path" trivially coincide, sidestepping
   the ambiguity rather than guessing at an unrequested `dispatch.rs` change. Flagging this for
   whoever owns Task 5 (Read truncation caps, which touches `dispatch.rs`'s Read arm) — if a
   *relative* `file_path` is ever staged this way, `payloadRef` will be a relative path a later
   `Read` can't necessarily resolve the same way twice (cwd could differ by the time it's
   recovered), which may be worth a follow-up look, though it's not a regression introduced here.
4. Verified no frontend (`src/**/*.ts(x)`) code needed touching — `offloadedTo` references in
   `ipc.ts`/`BashWidget.tsx`/`ReadWidget.test.tsx`/`WidgetGallery.tsx` are explicitly Task 9's
   scope (frontend slimmed detail + lazy payload), not this task's.
5. Did not find a test "near line ~2011 in commands/agent.rs referencing offload behavior" as the
   controller notes suggested — that line is inside `task_delegation_persists_...`'s assertions,
   unrelated to offload/payload. No offload-referencing test existed there to remove or rewrite;
   noting this as a discrepancy rather than silently ignoring it.
6. Found (and left alone) two pieces of pre-existing drift unrelated to this task, discovered
   while investigating the `cargo fmt` scope question: `.superpowers/sdd/task-2-report.md` and
   `.superpowers/sdd/task-3-report.md` (this very file, before being overwritten) both had stale
   on-disk content describing entirely different, unrelated tasks (a "benchmark scorer
   diagnostics" report and a "sampling alignment with Qwen 2507" report) that didn't match either
   `HEAD`'s committed Task 2 report or this task's brief. Left `task-2-report.md` untouched (not
   part of my commit or my task); overwrote `task-3-report.md` with this report per my own
   instructions.

## Concerns

- None blocking. The one open question (item 3 above, Read's `payloadRef` using the raw vs.
  resolved path) is a pre-existing `dispatch.rs` behavior this task inherited rather than
  introduced, and is flagged for Task 5's owner since that task already touches `Read`'s dispatch
  arm.
