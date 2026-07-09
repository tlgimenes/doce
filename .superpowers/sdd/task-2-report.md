# Task 2: Benchmark scorer diagnostics + metrics line

## Status: COMPLETED

## Changes Made

### Modified Files
- `src-tauri/tests/agent_benchmark.rs`

### Implementation Details

1. **Extended `tier4_score` function** (lines 571-598):
   - Changed signature from `(usize, usize)` to `(usize, usize, Vec<String>)`
   - Added failure tracking to capture why each file failed to be fixed
   - Failures are categorized as:
     - `marker still present` - when `// BUG:` comment wasn't removed
     - `fixed line missing` - when the corrected line `let result = {a} + {b};` isn't present
     - Combined message when both conditions fail
   - Seeder analysis verified: `let result = {a} - {b};` format with `a = i` and `b = i + 1`, corrected to `{a} + {b}`

2. **Updated first tier4 test** (`tier4_long_running_fixes_many_scattered_bugs`, lines 617-630):
   - Destructured tier4_score return value to capture failures
   - Added per-file failure output with `[tier4]` prefix
   - Added machine-greppable metrics line: `[metrics] score=N/20 turns=T elapsed_s=E seed=S`
   - Metrics use `run.turns_taken` and `run.elapsed.as_secs_f32()`
   - Seed reads from `DOCE_GEN_SEED` environment variable with fallback to `"entropy"`

3. **Updated second tier4 test** (`tier4_planned_long_running_fixes_many_scattered_bugs`, lines 651-665):
   - Applied identical changes as first tier4 test
   - Uses `[tier4_planned]` prefix for failure output
   - Shares same metrics line format for consistency

## Verification Results

✓ **Compilation**: `cargo test --test agent_benchmark --no-run` - CLEAN (12.62s)
✓ **Clippy**: `cargo clippy --tests` - CLEAN (no warnings)
✓ **Lib tests**: `cargo test --lib` - PASSED (244 passed; 0 failed)

## Diagnostic Output Examples

When a tier4 run completes, the output will now include:

```
  [tier4] bug_07: marker still present; fixed line missing
  [tier4] bug_14: fixed line missing
  [metrics] score=18/20 turns=42 elapsed_s=125.4 seed=12345
```

This enables:
- Per-file root cause analysis of failures
- Machine parsing of metrics via regex on `[metrics]` lines
- Distinction between operator fixes that worked but comments remained vs. actual unfixed bugs
- Reproducibility tracking via seed value in metrics line

## Commit

Ready for commit per task specification:
```bash
git add src-tauri/tests/agent_benchmark.rs
git commit -m "feat(bench): per-file failure reasons and seeded metrics line for tier4

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```
