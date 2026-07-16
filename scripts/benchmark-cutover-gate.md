# Benchmark gate: llama-server cutover vs pre-cutover (Task 8.2)

The acceptance bar for the llama-server cutover: **does Qwen3.5-4B via the sidecar make better tool calls than the
pre-cutover in-process Qwen3-4B?** This runs on your machine (GPU-heavy, ~tens of minutes per tier-4 run). Nothing
here has been run yet — it's the runbook you asked me to set up.

Everything runs the `tier0`–`tier5` agent-task ladder in `src-tauri/tests/agent_tasks.rs` (all `#[ignore]`). The
`_planned` tiers drive PRODUCTION's exact prompt + plan state machine — **those are the fair A/B** (the flat tiers
shifted slightly in the cutover: Write/Edit→Update). The scoring tiers are **tier4 / tier4_planned** (fix N scattered
bugs → `score=N/20` in the `[metrics]` line).

---

## 0. Prerequisites (already true on this machine)
- Sidecar binary built: `src-tauri/binaries/llama-server-aarch64-apple-darwin` (the tests spawn it automatically).
- Pre-cutover model on disk: `~/Library/Application Support/app.doce.desktop/models/qwen3-4b-thinking-2507-q4_k_m.gguf`.
- `#[ignore]` real-model tests MUST run single-threaded: `--test-threads=1` (llama.cpp's backend init is a
  process-wide singleton — parallel real-model tests flake).

## 1. Download the cutover model (Qwen3.5-4B, 2.74 GB)
```bash
MODELS="$HOME/Library/Application Support/app.doce.desktop/models"
mkdir -p "$MODELS"
curl -L --fail -o "$MODELS/qwen3.5-4b-q4_k_m.gguf" \
  "https://huggingface.co/unsloth/Qwen3.5-4B-GGUF/resolve/main/Qwen3.5-4B-Q4_K_M.gguf"
# verify integrity (must print the expected sha256):
shasum -a 256 "$MODELS/qwen3.5-4b-q4_k_m.gguf"
# expected: 00fe7986ff5f6b463e62455821146049db6f9313603938a70800d1fb69ef11a4
```
(Alternatively: launch the app — onboarding auto-downloads it, since the registry now points here.)

## 2. Run the POST-cutover gate (current HEAD, Qwen3.5 via the sidecar)
From repo root, on the current branch:
```bash
cd src-tauri
QWEN35="$HOME/Library/Application Support/app.doce.desktop/models/qwen3.5-4b-q4_k_m.gguf"
# Warm, scored run of the whole ladder. --release matters (speed). Capture the log.
DOCE_BENCH_MODEL="$QWEN35" cargo test --release --features bench --test agent_tasks -- \
  --ignored --test-threads=1 --nocapture 2>&1 | tee /tmp/cutover-qwen35.log
```
Sampling is stochastic (temp=0.6), so run the **3-seed protocol**: pin the sampler via `DOCE_GEN_SEED` (now wired
into every request — the `[metrics] ... seed=` line is truthful) and take the median of three fixed seeds. This is the
old gate's protocol, restored:
```bash
for s in 11 22 33; do
  DOCE_GEN_SEED=$s DOCE_BENCH_MODEL="$QWEN35" cargo test --release --features bench --test agent_tasks -- \
    --ignored --test-threads=1 --nocapture tier4_planned 2>&1 | grep '\[metrics\]'
done
```
Each line reads `[metrics] score=N/20 turns=… elapsed_s=… seed=11|22|33`. Take the **median score** across the three
seeds. Higher `N` = better tool-call quality/task completion. (Omit `DOCE_GEN_SEED` for entropy-seeded runs; then
`seed=entropy` and runs are not reproducible.)

## 3. Run the PRE-cutover baseline (frozen @ `14392af`, in-process Qwen3-4B)
`14392af` is the last commit before generation moved to the client — the old hand-rolled in-process path. Run it in a
throwaway git worktree so your current tree is untouched:
```bash
cd <repo-root>
git worktree add /tmp/doce-precutover 14392af
cd /tmp/doce-precutover/src-tauri
QWEN34="$HOME/Library/Application Support/app.doce.desktop/models/qwen3-4b-thinking-2507-q4_k_m.gguf"
for s in 11 22 33; do
  DOCE_GEN_SEED=$s DOCE_BENCH_MODEL="$QWEN34" cargo test --release --features bench --test agent_tasks -- \
    --ignored --test-threads=1 --nocapture tier4_planned 2>&1 | grep '\[metrics\]'
done
# cleanup when done:
cd <repo-root> && git worktree remove /tmp/doce-precutover
```
(The old path is in-process — it does NOT spawn a sidecar; it loads the GGUF directly. It uses the model you point
`DOCE_BENCH_MODEL` at. Qwen3-4B-Thinking is the closest stand-in for "what the app ran before.")

## 4. How to read the result (the acceptance bar)
- Compare the **median `score=N/20`** of `tier4_planned` across the 3 post-cutover runs vs the 3 baseline runs.
- **PASS (cutover justified):** post-cutover median ≥ baseline median (ideally clearly higher). The cutover's thesis
  is that structured tool calls from the server beat the old text-parsed Hermes calls — expect fewer "made a garbled
  tool call / ended the task as garbage" failures.
- Also sanity-check the cheaper tiers (`tier1_planned`, `tier2`, `tier3`) pass reliably — they should be near-100%.
- Watch the failure *reasons* in `--nocapture` output, not just the score: "marker still present" (operator fixed but
  comment left / verification missed) vs "malformed tool call" tell you different things. The cutover specifically
  targets the malformed-tool-call class.

## Caveats / known limitations (so the numbers are read honestly)
- **Seed reproducibility (now wired both sides).** `DOCE_GEN_SEED` pins the sampler on both configs: post-cutover it
  flows through `ChatRequest`'s new optional `seed` field (`http.rs`, honored per-request by llama-server), and at the
  `14392af` baseline it feeds the in-process `generation_seed()`. Production never sets the env, so it stays
  entropy-seeded (`seed:None` omitted from the request). The two stacks are different engines, so a given seed does
  NOT produce bit-identical sampling across configs — it only makes each config reproducible *within itself*, which is
  all the median-of-three protocol needs. Sanity check: if two runs at the SAME seed diverge in score, the server
  build isn't honoring the per-request seed — fall back to adding `--seed $DOCE_GEN_SEED` in
  `inference::server::launch_args` (server-global seed) and re-run.
- **Two variables move at once** (model Qwen3-4B→Qwen3.5 AND path in-process→server). That's the cutover as shipped;
  it's a full-system A/B, not an isolated one. To isolate the *path*, run step 2 with `DOCE_BENCH_MODEL=$QWEN34`
  (Qwen3-4B through the sidecar) and compare to step 3 (Qwen3-4B in-process) — same model, path-only delta.
- **Use the `_planned` tiers for the verdict.** The flat tiers' tool set shifted in Task 8.1; their pre/post numbers
  aren't directly comparable. The planned tiers mirror production exactly.
- Output-token cap (`max_tokens`) was dropped in the cutover (both turns and summaries are bounded only by the 20480
  server ctx). Not expected to change scores, but noted.

## What "done" looks like
Record the medians + verdict in `.superpowers/sdd/progress.md` (Task 8.2) and a short results note, then the SDD final
whole-branch review runs and the cutover is complete. If the gate is *worse*, the first lever is the deferred
benchmark-gated prompt cleanup (remove the now-redundant `<tools>`/`call_format_instructions` from the single-mode
prompt — the model may be double-fed tools and confused).
