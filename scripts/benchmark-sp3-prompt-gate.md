# Benchmark gate: SP3 prompt engineering (main vs sp3-prompt-engineering)

The acceptance bar for SP3: **do the reworked prompts make tool-call/task quality no worse (ideally better) than
main's prompts, on the SAME model?** Unlike the cutover gate (which A/B'd two *models*), this A/Bs two *prompt sets* on
the one shipped model (Qwen3.5-4B via the sidecar). Runs on your machine (GPU-heavy, ~tens of minutes).

## What changed on the branch (all benchmark-gated)
`sp3-prompt-engineering` (5 commits off `main`):
- (a) deleted `ToolDialect` + the redundant hand-written `<tools>` block & call-format teaching — tools now come solely
  from the server's `--jinja` template (the API `tools` array). **Biggest prompt-byte change / likeliest regression source.**
- (b) structured `<state_snapshot>` compaction summary prompt (was a one-sentence prompt).
- (c) `AGENTS.md` project-instructions ingestion (only bites when the task workspace has an `AGENTS.md` — the benchmark
  tasks don't, so this is a no-op for the gate; it's here for completeness).
- (d) fixed the self-contradicting `# Tools` section ("one or more functions" → "exactly one tool per response") and
  sharpened the `Read` tool description.

The **`_planned` tiers** drive production's exact prompt + plan machine — those are the fair A/B. Scoring tier:
**tier4_planned** (`score=N/20` in the `[metrics]` line).

## Prereqs (same as the cutover runbook)
- Qwen3.5 model on disk: `~/Library/Application Support/app.doce.desktop/models/qwen3.5-4b-q4_k_m.gguf`
  (see `benchmark-cutover-gate.md` §1 to download; sha256 `00fe7986...ef11a4`).
- Sidecar binary built at `src-tauri/binaries/llama-server-aarch64-apple-darwin`.
- Real-model tests run single-threaded (`--test-threads=1`).

## 0. The seed reproduces — read this before designing a run

**As of `bench-determinism` (2026-07-15), a fixed `DOCE_GEN_SEED` + fixed code produces a
byte-identical run.** Same score, same turn count, same tool calls, same prompt on every turn.
Measured: two seed-11 runs, both `score=20/20 turns=50`, with all 50 per-turn prompt digests
identical. Before that fix the same seed on identical code swung `0/20 / 10/20 / 20/20` — the
`seed=N` in the metrics line pinned the *sampler* while two random values (the `tempfile`
scratch-dir path, and `run_loop`'s per-tool-call UUIDv7) re-rolled the *prompt bytes* every run.

What this changes for you:

- **One run per seed, not three.** A repeat of the same seed adds no information; it will return
  the identical number.
- **`turns` is now a real signal.** It's noiseless and far sharper than `score`, which sits at
  its 20/20 ceiling and can therefore only detect regressions. Compare turn counts.
- **A same-seed re-run that does NOT reproduce is a BUG, not noise.** It means the environment
  moved (toolchain, model file, sidecar binary). Investigate before trusting any number.
- **You still need multiple seeds.** Determinism removed *measurement* noise, not trajectory
  luck. Any prompt change alters the prompt bytes and therefore re-rolls the trajectory, so one
  seed is still one sample. Keep the 3-seed protocol below — each seed is now a clean sample
  instead of a noisy one, which is what makes a 3-seed comparison meaningful at all.
- **Same machine only.** The prompt embeds `$TMPDIR` and `$HOME`. Numbers are comparable within
  a machine, never across machines.

### Debugging a run that won't reproduce
Every turn prints `[prompt] <label> turn=N bytes=… fnv1a=…` over the exact JSON sent to the
server. Diff the two runs' `[prompt]` streams:
```bash
diff <(grep '\[prompt\]' run1.log) <(grep '\[prompt\]' run2.log)
```
- **First line differs** → an input is still run-to-run random. Hunt for a fresh path, id, or
  timestamp reaching the prompt.
- **Agree to turn N, then diverge** → the prompt is reproducible; the residual is llama.cpp's
  own (batching/threading/float non-associativity). That wall has *not* been hit here (the
  sidecar already runs `-np 1` with a fresh server per test), but if it ever is, the levers are
  pinned `--n-threads`, `--parallel 1`, or temp-0 sampling.

See `.superpowers/sdd/bench-determinism-report.md` for the full diagnosis.

## 1. NEW prompts (branch)
```bash
cd <repo-root>
git checkout sp3-prompt-engineering
cd src-tauri
QWEN35="$HOME/Library/Application Support/app.doce.desktop/models/qwen3.5-4b-q4_k_m.gguf"
for s in 11 22 33; do
  DOCE_GEN_SEED=$s DOCE_BENCH_MODEL="$QWEN35" cargo test --release --test agent_tasks -- \
    --ignored --test-threads=1 --nocapture tier4_planned 2>&1 | grep '\[metrics\]'
done
```
Record the three `score=N/20` **and `turns=N`** lines; take the **median**. Call it `NEW`.
(One run per seed — see §0. Re-running a seed returns the identical number.)

## 2. BASELINE prompts (main)
```bash
cd <repo-root>
git checkout main
cd src-tauri
QWEN35="$HOME/Library/Application Support/app.doce.desktop/models/qwen3.5-4b-q4_k_m.gguf"
for s in 11 22 33; do
  DOCE_GEN_SEED=$s DOCE_BENCH_MODEL="$QWEN35" cargo test --release --test agent_tasks -- \
    --ignored --test-threads=1 --nocapture tier4_planned 2>&1 | grep '\[metrics\]'
done
```
Median = `BASE`.

(Same model, same seeds, only the prompt bytes differ between the two checkouts — so any score delta is attributable to
the prompt rework.)

## 3. Decide
- **`NEW >= BASE`** → PASS. Merge the branch to `main`:
  ```bash
  git checkout main && git merge --no-ff sp3-prompt-engineering
  ```
  SP3 is done; the goal (SOTA context management + prompt engineering) is met.
- **`NEW < BASE`** (a real regression beyond seed noise) → bisect. Component (a) is the biggest change and the first
  suspect. Re-run tier4_planned at each commit (`85982a3` a, `4d4b02a` b, `d4c0432` d) to find which one regressed, then
  revise or drop just that commit and re-gate. (b)/(c)/(d) are small and independently revertable.

## Notes
- Optionally also eyeball **tier5_planned** (harder multi-step) and the non-scored planned tiers for gross breakage
  (e.g. the model no longer emitting valid tool calls — which would show as `score=0` and would implicate (a)'s removal
  of the hand-written call-format teaching, though the `--jinja` template should supply it).
- Seeds still select among trajectories; a 1-point median difference is not a regression. Look for a consistent,
  multi-point gap. What is *no longer* true: that a repeat of the same seed can disagree with itself (see §0).

---

## ⚠️ 2026-07-15 — THIS RUNBOOK'S NUMBERS ARE VOID AS A BASELINE

The gate PASSED and SP3 merged (`2285471`) on these numbers:
`BASE 20/20, 0/20, 20/20` vs `NEW 20/20, 20/20, 20/20` (medians 20 vs 20).

**But an audit then found the benchmark was not measuring production.** It sent no
`max_tokens`, so the model got a server-unbounded ~6,900 output tokens, while
production clamps every agent turn via
`clamp_output_tokens(AGENT_TURN_OUTPUT_CEILING, CONTEXT_WINDOW_TOKENS, prompt_est)`
— ~1,792 at the loop threshold, floor 512. Near-4×, exactly where tier4 lives. It
also re-implemented `measure` (chars/4 forever, discarding the server's
authoritative `usage`), so its compaction trigger diverged from production at turn 2.

Consequence: the gate was structurally blind to prompt-INFLATING regressions — and
`AGENTS.md` ingestion (SP3 c) and SP4's `# Memories` block are exactly that. In
production they shrink the output budget; the old gate could not see it.

Fixed in `16f206f` (benchmark now calls production's own helpers). **Any score taken
before `16f206f` is not comparable to one taken after. Do not use the table above as
a baseline.**

### ⚠️ SUPERSEDED — tier4_planned @ `a108dbf`, measured on the NOISY harness
Production Qwen3.5-4B (sha256 `00fe7986...ef11a4`), real output clamp:

| seed | score | turns |
|---|---|---|
| 11 | 20/20 | 42 |
| 22 | 20/20 | 80 |
| 33 | 20/20 | 71 |
| **median** | **20/20** | 3/3 perfect, no truncation |

**These numbers were taken before the harness was reproducible, and each is one draw from a
noisy process — not a property of the code.** Seed 11 on identical code (commits whose diff
provably does not touch the benchmark's code path) was separately measured at both
`20/20 turns=42` and `10/20 turns=43`, with an observed spread of `0/20 / 10/20 / 20/20`
across essentially-identical code. The cause was prompt-byte
variance, not sampling: the random `tempfile` scratch-dir path (which lands in `messages[0]`
via `plan_system_message`'s cwd + transcript lines) and `run_loop`'s per-tool-call
`Uuid::now_v7()` (on the wire twice per call, plus in every payload reference line) re-rolled
the prompt on every run. `DOCE_GEN_SEED` fixed the sampler and nothing else.

Do not use this table as a baseline, and do not read a delta against it as a prompt effect.

---

## 2026-07-15 — the reproducible baseline

Fixed in `tests/agent_tasks.rs` (`ScratchDir` + `StableToolCallIds`; both stand-ins are
length-preserving, so token counts and difficulty are unchanged — see
`.superpowers/sdd/bench-determinism-report.md` for the arithmetic). Production code, the task,
the 20 bugs, the scoring, the tool set and every prompt are untouched.

### tier4_planned @ `bench-determinism`
Production Qwen3.5-4B (sha256 `00fe7986...ef11a4`), on the reproducible harness:

| seed | score | turns | reproduces? |
|---|---|---|---|
| 11 | 20/20 | 50 | ✅ **3/3 runs identical** — all 50 per-turn prompt digests and the full tool-call trace byte-identical |
| 22 | 20/20 | 54 | control: a different trajectory, so the seed still steers the run |
| 33 | — | — | not measured |

Compare future prompt work against THIS, and prefer **turns** to **score**: tier 4 is at its
20/20 ceiling so score can only detect regressions, while turns is now noiseless and moves
(50 vs 54 across two seeds).

A same-seed re-run must return its exact line. If it doesn't, the environment changed — fix
that before reading any result.

Seed 11's `turns=42` in the superseded table above is **not** comparable to `turns=50` here:
the fix necessarily changed the prompt *bytes* (a fixed scratch-dir name and fixed tool-call
ids are still different bytes, though deliberately the same *length* — so difficulty is
unchanged; see the report for the arithmetic). The old 42 was one draw from a noisy process
regardless.
