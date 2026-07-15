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
Record the three `score=N/20` lines; take the **median**. Call it `NEW`.

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
- Seeds are stochastic; a 1-point median difference is noise. Look for a consistent, multi-point gap before calling a
  regression.
