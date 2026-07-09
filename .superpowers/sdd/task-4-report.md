# Task 4 Report: Subagent staging + honest Bash marker

## Status
COMPLETE

## Commit Hash
f3cf835

## Summary

Wired `context::payload::stage_tool_result` onto `SubagentBackend::execute_tool`
(`src-tauri/src/commands/agent.rs`), mirroring the top-level staging block that
Task 3 added to `handle_general_tool_call` — including the `Read` carve-out
(payloadRef points at the source file, no duplicate written) and the
`detail["payloadRef"]` stamp on staged results. Also fixed
`truncate_tail_biased`'s (`src-tauri/src/agent/tools/bash.rs`) two marker
sites, which previously claimed the omitted bytes were "preserved in the
conversation transcript" — false since Task 3 retired the old
duplicate-in-`detail` offload design. Both now read "full output saved to
this call's payload file".

`src-tauri/src/agent/dispatch.rs` needed no changes: its doc comments
(`offload_text`'s doc, the Bash arm's comment) already name
`context::payload::stage_tool_result` — that update landed in Task 3's commit
`101e8aa`, ahead of this task's re-baseline note.

## Changes

### `src-tauri/src/commands/agent.rs`

1. `SubagentBackend<'a>` gained a new field:
   ```rust
   /// Payload staging root (2026-07-09 payload-files design) — resolved by
   /// the spawn site, which holds the AppHandle this backend deliberately
   /// doesn't. None only in unit tests that don't exercise staging.
   app_data_dir: Option<std::path::PathBuf>,
   ```
2. `execute_top_level_tool`'s construction site (the only production
   construction) now passes `app_data_dir: app.path().app_data_dir().ok()`.
3. `SubagentBackend::execute_tool` — between the token-count annotation and
   `persist_tool_call_and_result` — now stages the outcome exactly like
   `handle_general_tool_call`: loads `ContextSettings` from `self.conn`, takes
   the `Read` carve-out (`detail["payloadRef"] = detail["filePath"].clone()`,
   `model_text` unstaged) for `Read` calls, and otherwise calls
   `context::payload::stage_tool_result` with `self.subagent_id` as the
   staging conversation id and `self.app_data_dir.as_deref()` as the root,
   stamping `detail["payloadRef"]` on the result. `persist_tool_call_and_result`
   and the returned `ToolExecution::Result` now carry the staged
   `model_text`/`detail` rather than the raw `outcome` fields.
4. Existing `SubagentBackend` test construction (the Read/token-count test)
   gained `app_data_dir: None`.
5. New test `subagent_tool_result_carries_a_payload_ref` (model-dependent,
   `#[ignore]`d, next to the existing SubagentBackend test).

### `src-tauri/src/agent/tools/bash.rs`

Both `truncate_tail_biased` marker format strings (the line-window branch and
the byte-fallback branch) changed their trailing clause from `"full output
preserved in the conversation transcript"` to `"full output saved to this
call's payload file"`. The `"bytes omitted -- "` prefix and the `--` ASCII
separator (matching this file's existing convention, not the brief's em-dash
rendering artifact) were left untouched, per the brief's "only the trailing
clause changes" instruction.

## TDD evidence

**RED**: With the `app_data_dir` field temporarily removed from the struct
(and the new test/construction sites already referencing it), `cargo test
subagent_tool_result_carries_a_payload_ref --no-run` failed to compile:

```
error[E0609]: no field `app_data_dir` on type `&mut commands::agent::SubagentBackend<'_>`
   --> src/commands/agent.rs:781:24
error[E0560]: struct `SubagentBackend<'_>` has no field named `app_data_dir`
   --> src/commands/agent.rs:957:9
... (2 more E0560 at the two test construction sites)
```

**GREEN**: After restoring the field and implementing the staging block, the
test passes (model-dependent, run with `--ignored`):

```
test commands::agent::tests::subagent_tool_result_carries_a_payload_ref ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 300 filtered out
```

Note: the test's first draft called `Bash` directly against a fresh
`PlanState::default()` and failed at runtime (not a compile error) with
"payloadRef must be a path" — `PlanState::handle_plan_tool`'s Planning-state
arm rejects `Bash` (`(_, other) => "Error: {other} is not available in the
current phase"`; only `Read`/`Grep`/`Glob`/`AskUserQuestion` pass through in
Planning). Fixed by driving the plan machine into `Executing` state through
two prior `execute_tool` calls (`CreatePlan` then `ResumeExecution`) — the
same path a real subagent turn takes — rather than reaching into
`PlanState`'s private `refusal_context` field (which blocked a `..Default::
default()` struct-update-syntax shortcut with an E0451 privacy error).

## Test runs

- `cargo test --lib agent::tools::bash::` — 25/25 pass (includes all four F3
  hard-cap tests, whose `bytes omitted` assertions still pass since only the
  trailing clause changed).
- `cargo test` (full suite, default) — 296 passed, 0 failed, 5 ignored
  (model-dependent, expected to be skipped by default).
- `cargo test --lib commands::agent:: -- --ignored --test-threads=1` — all 5
  model-dependent tests in this module pass, including the new one. (Running
  them with the default parallel test runner hits a pre-existing,
  unrelated `Backend("BackendAlreadyInitialized")` failure from loading the
  GGUF model concurrently in multiple threads — confirmed pre-existing by
  running the same command against `git stash`; not introduced by this
  change.)
- `cargo fmt -- --check` on the two changed files only
  (`rustfmt --check --edition 2021 src/commands/agent.rs
  src/agent/tools/bash.rs`) — clean. (Whole-repo `cargo fmt -- --check` shows
  pre-existing formatting debt in `agent/mod.rs`, `agent/plan.rs`,
  `inference/mod.rs`, `tests/agent_benchmark.rs` — confirmed identical before
  and after this change via `git stash`; none of it is in files this task
  touched.)
- `cargo clippy --all-targets` — no warnings.

## Files changed

- `/Users/gimenes/code/doce/src-tauri/src/commands/agent.rs`
- `/Users/gimenes/code/doce/src-tauri/src/agent/tools/bash.rs`

(`src-tauri/src/agent/dispatch.rs` required no edits — see Summary.)

## Self-review

- Staging block is a faithful mirror of Task 3's `handle_general_tool_call`
  block: same settings load, same Read carve-out, same `payloadRef` stamping,
  same fallback to unstaged `outcome` fields when `app_data_dir` is `None`.
- The subagent's staging directory keys off `self.subagent_id` (its own
  conversation id), matching the brief's `parent_conversation_id ->
  self.subagent_id` instruction and keeping subagent payload files under
  their own conversation's directory, isolated from the parent's — consistent
  with this file's existing isolation guarantee (only the final answer
  crosses from subagent to parent transcript).
- No other `SubagentBackend` construction sites existed besides the one
  production site and the one pre-existing test site — grepped to confirm.
- Confirmed via grep that no other source under `src-tauri/` still contains
  the stale "preserved in the conversation transcript" wording.
- Did not touch the two `.superpowers/sdd/task-2-report.md` /
  `task-3-report.md` files that appeared modified in `git status` at the
  start of this session (other in-flight task agents' own report files) —
  out of this task's scope, left untouched, not staged.

## Concerns

- None blocking. Minor note: the new test needed to drive the plan machine
  through `CreatePlan`/`ResumeExecution` first (two extra `execute_tool`
  calls) because `Bash` isn't reachable from the default `Planning` state —
  this is a slightly heavier arrangement than the brief's one-paragraph
  sketch implied, but it's the only way to exercise `Bash` staging through
  the real public API without reaching into `PlanState`'s private fields.
- The parallel-test-runner model-loading failure
  (`BackendAlreadyInitialized`) is pre-existing test-infra debt affecting all
  `#[ignore]`d model-dependent tests in this module, not something this task
  introduced or is in scope to fix; documented here so it isn't mistaken for
  a regression.
