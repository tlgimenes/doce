# Task 2 Report: `context/payload.rs` — Payload-file staging

## Implementation Summary

Created a new module `context/payload.rs` that stages every tool result through a payload file on disk while deciding whether the model sees the full result or a status reference line based on token count.

## TDD Evidence

### RED (Test Failures)
```
$ cargo test --lib context::payload

error[E0425]: cannot find function `stage_tool_result` in this scope
  --> src/context/payload.rs:25:22
   |
25 |         let staged = stage_tool_result(
   |                      ^^^^^^^^^^^^^^^^^ not found in this scope
...
error: could not compile `doce` (lib test) due to 5 previous errors
```

### GREEN (Test Passes)
```
$ cargo test --lib context::payload

running 5 tests
test context::payload::tests::write_failure_falls_back_to_a_bounded_preview_with_no_payload_ref ... ok
test context::payload::tests::small_result_inlines_but_still_writes_the_payload_file ... ok
test context::payload::tests::oversized_result_becomes_a_status_reference_line ... ok
test context::payload::tests::oversized_bash_reference_line_carries_exit_code_and_sizes ... ok
test context::payload::tests::bash_payload_is_full_stdout_and_stderr_from_detail_and_detail_is_slimmed ... ok

test result: ok. 5 passed; 0 failed; 0 ignored
```

## Files Changed

1. **Created:** `src-tauri/src/context/payload.rs` (276 lines)
   - `StagedResult` struct
   - `reference_line()` function — generates status line metadata for oversized results
   - `slim_detail()` function — replaces bulk Bash output in detail with previews + byte counts
   - `stage_tool_result()` function — orchestrates payload writing and decision logic
   - 5 passing unit tests covering:
     - Small results that inline but still write payload files
     - Oversized results that become reference lines
     - Bash-specific handling (full output from detail, slimmed detail)
     - Oversized Bash reference lines with exit codes and sizes
     - Write-failure fallback with bounded preview

2. **Modified:** `src-tauri/src/context/mod.rs`
   - Added `pub mod payload;` declaration

## Self-Review Findings

1. **Correct use of `offload_text()`**: Leveraged the existing `ToolOutcome::offload_text()` method as specified in the brief, which:
   - For Bash: reconstructs the full output from `detail.outcome` (stdout + stderr)
   - For other tools: borrows `model_text`
   - This is exactly the right payload source.

2. **Proper detail slimming**: The `slim_detail()` function correctly replaces bulk stdout/stderr in Bash outcomes with:
   - Byte counts (`stdoutBytes`, `stderrBytes`)
   - Bounded previews (`stdoutPreview`, `stderrPreview`) at `DETAIL_PREVIEW_CHARS` (2000 chars)
   - Non-Bash details pass through unchanged

3. **Invariant enforcement**: The implementation maintains the invariant that unbounded text never enters the model's window, even in the write-failure fallback (bounded to `PREVIEW_CHARS` + error message).

4. **Path handling**: Payload files are written to `<app_data_dir>/tool-outputs/<conversation_id>/<tool_call_id>.txt` with absolute paths returned in `payload_ref`.

5. **Test coverage**: All five test scenarios from the brief pass, exercising:
   - Normal flow (inline small, file large)
   - Bash-specific flow (detail slimming)
   - Error path (write failure fallback)

## Concerns

**Re: `offload_text()` doc comment:** The brief notes that the doc comment references `context::offload` (which is deleted in Task 3) and suggests updating it to reference this module. However, since `offload_if_oversized` still exists until Task 3, the current doc comment is still truthful — it correctly describes what `offload_text()` does for the old module. Left unchanged to avoid breaking its truth until the cutover. Task 3 should address this update when retiring `offload.rs`.

## Verification

- All 5 context::payload tests pass
- Full test suite passes (298 tests)
- Code formatted with cargo fmt
- Commit: `7c787a6` with message "feat(context): payload-file staging for every data-tool result"
