# Agent benchmark → hard-asserted task tests

**Date:** 2026-07-11
**Status:** Approved (user-approved in session; tier-4 floor and variant policy decided by user)

## Problem

`src-tauri/tests/agent_benchmark.rs` was built as a print-and-compare
benchmark for the 2026-07 context-management redesign: tiers 3–5 printed
scores instead of asserting, so two runs could be compared by eye across an
architecture change. That comparison mission closed on 2026-07-09 ("good
enough"). What remains useful is a regression suite — and a bench that never
fails can't gate anything.

## Decision

Convert the file into a hard pass/fail regression suite and drop the bench
identity entirely.

- **Every tier asserts.** Tier 3 asserts `cargo build` succeeds on the
  refactored fixture (stderr tail in the panic message). Tier 4 — both flat
  and planned variants — asserts **20/20** bugs fixed, graded against ground
  truth, with per-file failures in the panic message. Tier 5 asserts the
  surgical-edit check (target line fixed, every other line byte-identical).
  Tiers 1–2 already asserted and are unchanged.
- **Aspirational floor, red today.** The last gate run (binary a66f010)
  scored 2/20 on tier4_planned with two diagnosed open defects — the
  reference-line doom loop and the plan-nudge contradiction. The 20/20
  assert makes those tests the definition of done for that follow-up work.
- **All variants stay.** Flat tests cover the plain `run_loop` path (still
  used by subagents); `_planned` variants cover the production plan loop and
  remain directly comparable to their flat counterparts.
- **Rename:** file → `tests/agent_tasks.rs`; run command →
  `cargo test --test agent_tasks -- --ignored --nocapture --test-threads=1`.
  Internal names lose the bench vocabulary (`BenchBackend` → `FlatBackend`,
  `BenchmarkRun` → `TaskRun`, `run_benchmark_task` → `run_flat_task`,
  `run_planned_benchmark_task` → `run_planned_task`,
  `stage_bench_tool_result` → `stage_general_tool_result`, wiring test →
  `staging_wiring_replaces_oversized_result_with_reference_line`).
  Source doc-comments pointing at the old path (`context/mod.rs`,
  `agent/plan.rs`) updated; historical plans/specs/reports left as written.

## Non-goals

- No harness changes; the backends still mirror production wiring exactly.
- No seed pinning: runs stay stochastic (`DOCE_GEN_SEED` respected). The
  three-seed gate protocol remains a convention around the suite.
- The metric `println!`s stay for diagnosing red runs under `--nocapture`.
