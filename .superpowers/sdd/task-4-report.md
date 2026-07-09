# Task 4 Report: §4.11 Correctness Fixes

## Status
COMPLETE

## Commit Hash
5c32a56

## Verification Summary
Applied n_threads to InferenceEngine context params; usage measurement now uses plan prompt instead of flat system prompt. All 244 lib tests pass; clippy clean.

## Changes Made

### 1. InferenceEngine n_threads Fix
- Added `n_threads: i32` field to `InferenceEngine` struct
- Modified `load()` to store the parameter: `Ok(Self { backend, model, n_threads })`
- Applied it in `generate()` using builder methods:
  ```rust
  let ctx_params = LlamaContextParams::default()
      .with_n_ctx(NonZeroU32::new(CONTEXT_WINDOW_TOKENS))
      .with_n_threads(self.n_threads)
      .with_n_threads_batch(self.n_threads);
  ```
- Verified both methods exist in llama-cpp-2 0.1.150 registry

### 2. Context Usage Measurement Fix
- Updated `emit_context_usage_update()` in `src-tauri/src/commands/agent.rs`
- Replaced `&system_message(cwd)` with fresh `PlanState::default()` + `plan_system_message()`
- Added explanatory comment noting this matches top-level loop's actual seed prompt (~300 token difference)

### 3. Dead Code Suppression
- Added `#[allow(dead_code)]` to `system_message()` function (still used by tests)

## Test Results
- `cargo test --lib`: 244 passed, 0 failed, 2 ignored
- `cargo clippy --lib --tests`: No warnings

## Concerns
None. Both thread-count parameters are now genuinely applied, and usage measurement aligns with actual rendering behavior.

## Fix: dead system_message

### Status
COMPLETE

### Commit Hash
c11db85

### Verification Summary
Deleted dead `system_message()` function; rewrote its 2 tests to exercise live `plan_system_message()` path. All 244 lib tests pass; clippy clean; 0 build warnings.

### Changes Made

#### 1. Deleted Dead Function
- Removed `system_message()` function (lines 1026-1041) including its `#[allow(dead_code)]` annotation
- Function was never called in production (only via tests); `plan_system_message()` is the live path

#### 2. Rewrote Tests for Live Path
- Renamed `system_message_appends_the_cwd_line_when_known()` → `plan_system_message_appends_the_cwd_line_when_known()`
  - Now exercises: `plan_system_message(&mut crate::agent::plan::PlanState::default(), Some(path))`
  - Verifies suffix: `"You are currently working in the directory: /Users/tester/code/doce"`
  - Verifies prompt body matches `PlanState::default().system_prompt()`

- Renamed `system_message_is_unchanged_when_no_cwd_is_known()` → `plan_system_message_is_unchanged_when_no_cwd_is_known()`
  - Now exercises: `plan_system_message(&mut crate::agent::plan::PlanState::default(), None)`
  - Verifies output equals `PlanState::default().system_prompt()` (no cwd suffix)

### Test Results
- `cargo test --lib`: 244 passed, 0 failed, 2 ignored
- `cargo clippy --lib --tests`: No warnings
- `cargo build --lib`: 0 warnings

### Notes
- Removed misleading doc comment that described `system_message()` as "the live path"
- Tests now verify the actual code path used in production
- No functionality changed; only removed dead code and aligned tests with live implementation
