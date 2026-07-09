# Task 5 report: schema-shaped argument validation before dispatch

## Status
Done.

## Commit
3490c73 — feat(agent): schema-shaped argument validation before every dispatch

## Test summary
`cargo test --lib`: 246 passed, 0 failed, 2 ignored (includes both new tests:
`missing_required_arguments_get_a_schema_shaped_error_before_dispatch`,
`wrong_type_arguments_get_named`, plus the two pre-existing hint-text tests,
both still green). `cargo clippy --lib --tests`: clean, no warnings.

## What was done
- TDD: added the brief's two failing tests first. Confirmed the predicted
  pre-implementation split — `missing_required_arguments_get_a_schema_shaped_error_before_dispatch`
  passed by luck (Grep's and Edit's own arms already named the missing keys),
  while `wrong_type_arguments_get_named` failed (`Read` with `file_path: 42`
  fell into the "missing" branch via `.as_str()` returning `None`, and the old
  message never said "string").
- Added `REQUIRED_STRING_ARGS` (the static table from the brief, verbatim) and
  `validate_required_args` (also verbatim), wired as the first thing
  `execute()` does, before the `match` on tool name.
- Deleted the per-arm missing-argument `let-else` blocks for all 6 tools
  (Read, Write, Edit, Bash, Glob, Grep) — every one of their required string
  keys is now covered by `REQUIRED_STRING_ARGS`, so by the time an arm runs,
  presence+string-type is already guaranteed; replaced each with a direct
  `.get(key).and_then(|v| v.as_str()).unwrap_or_default()` extraction (a
  defensive fallback to `""`, never actually reachable, rather than an
  `.unwrap()` that could panic if the invariant were ever violated).
  `wrong_key_hint` itself is unchanged and still used — now called only from
  inside `validate_required_args`, generalized from 3 tools (Read/Write/Edit)
  to all 6.
- AskUserQuestion and Task were left untouched, per the brief (not in the
  required-args table; they have their own non-string/optional-rich shapes
  and handling elsewhere).

## Detail-shape decision (per the judgment note)
Chose: let the validator's generic detail shape
(`{"toolName", "arguments", "outcome": {"ok": false, "error": ...}}`) apply
uniformly to every tool's validation failure, rather than preserving each
arm's old bespoke shape (e.g. `{"toolName": "Read", "filePath": null, ...}`).
Reasoning: checked both the Rust test suite and the frontend
(`src/lib/ipc.ts`'s `parseToolResultDetail`, `MessageContent.tsx`'s
`ToolWidget` router, and every widget's `.test.tsx`) for any assertion tied to
the specific missing-argument detail shape. None exists — the two existing
Rust tests for this path (`missing_required_argument_returns_a_clear_error`,
`read_with_the_wrong_key_name_gets_a_hint_not_a_bare_missing_argument_error`)
only assert on `model_text`, and no frontend test constructs a Read/Write/
Edit/Bash/Glob/Grep call with a missing required argument. `parseToolResultDetail`
routes purely on the `toolName` string (it does not validate the rest of the
shape at runtime), so the generic detail still reaches the right widget
(e.g. `ReadWidget`) — that widget just renders `undefined` for the
now-absent `filePath` field on this error path instead of `null`, which is
not observably different in the UI (both render as blank) and is not
covered by any test. This kept the change to one shape, built once in
`execute()`, instead of a per-tool match duplicating each arm's old JSON —
the minimal-churn option that kept every existing test green.

## Concerns
- Cosmetic only: on the validation-failure path, widgets like
  `ReadWidget`/`WriteWidget`/`EditDiffWidget` will show a blank/undefined
  file path instead of the previous explicit `null`-rendered-as-blank —
  visually identical today, but if a future frontend test starts asserting
  on that field for this specific error path, the detail shape would need
  reconsidering (a per-tool match rebuilding the old bespoke shape, using
  the validator only for the `model_text`/hint logic).
- `cargo fmt --check` reports pre-existing violations in `dispatch.rs`,
  `agent/mod.rs`, and `agent/plan.rs` unrelated to this change (confirmed by
  diffing before/after my edit — the only fmt-flagged spot inside my diff was
  the brief's own verbatim test code and the pre-existing `wrong_key_hint`
  signature, neither touched by this change). The repo does not appear to
  enforce `cargo fmt` as a gate, so left as-is rather than reformatting
  unrelated code.

## Fix: widget-safe validation details
**Commit:** 010ece2 — fix(agent): widget-safe per-tool details for validation failures

**Rationale:** The generic validation-failure detail shape 
(`{"toolName", "arguments", "outcome": {...}}`) was reaching the frontend's
`SearchResultsWidget` (for Glob/Grep), which unconditionally reads 
`detail.matches.length`. When a required argument was missing, `matches` was
absent entirely, causing a frontend TypeError that crashed the React tree
(no ErrorBoundary). Reachable in production via a model omitting `pattern` on Grep.

**Fix applied:** Replaced the generic shape with per-tool minimal shapes:
- **Glob/Grep:** include `matches: []` (empty array) to prevent 
  SearchResultsWidget from crashing on `.length` access
- **Grep additionally:** include `truncated: false, skippedOversized: 0`
  (required for the widget's conditional rendering)
- **Read/Write/Edit/Bash:** include their required fields as `null` when missing
  (e.g., `filePath: null` for Read, `command: null` for Bash)
- **Other tools:** fall back to the generic shape with `arguments`

Each shape now echoes what the caller sent where available, ensuring all
widgets receive the fields they unconditionally access.

**Regression test added:**
`validation_failure_details_stay_widget_safe()` — verifies that Glob/Grep 
validation failures carry `matches: []` (array, not absent) and that Read 
includes `filePath` (even if `null`).

**Verification:**
- `cargo test --lib`: 247 passed, 0 failed, 2 ignored (includes new regression test)
- `cargo clippy --lib --tests`: clean, no warnings

## Fix: string-only echo
**Commit:** f156fb6 — fix(agent): echo only string args into validation-failure details

**Rationale:** The validation-failure closure echoed raw argument values from the model's call into the detail JSON. When a required-string argument was passed with the wrong type (e.g., an object `{"pattern": {"nested": "*.rs"}}`), the closure included that object verbatim. The detail object reached the frontend and rendered as a JSX child → React threw "Objects are not valid as a React child" → the React tree unmounted.

**Fix applied:** Changed the echo closure to filter out non-string values:
```rust
let a = |key: &str| {
    call.arguments
        .get(key)
        .filter(|v| v.is_string())
        .cloned()
        .unwrap_or(serde_json::Value::Null)
};
```

Non-string arguments now echo as `null` instead of raw objects, which render safely in React widgets.

**Regression test extended:**
`validation_failure_details_stay_widget_safe()` now includes a case verifying that object-valued arguments are echoed as null:
```rust
let result = execute(
    &call("Glob", serde_json::json!({"pattern": {"nested": "*.rs"}})),
    None,
);
assert!(result.detail["pattern"].is_null(), "non-string args must echo as null, got {}", result.detail);
```

**Verification:**
- `cargo test --lib`: 247 passed, 0 failed, 2 ignored (new test case confirmed)
- `cargo clippy --lib --tests`: clean, no warnings
