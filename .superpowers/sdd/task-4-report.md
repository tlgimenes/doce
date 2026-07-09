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
