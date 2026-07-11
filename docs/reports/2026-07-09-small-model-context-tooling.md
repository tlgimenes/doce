# Context & Tooling for High Convergence on Small Local Models

**Date:** 2026-07-09 · **Model under study:** Qwen3-4B-Instruct-2507 (Q4_K_M, llama.cpp/Metal)
**Inputs:** full harness audit (`.superpowers/sdd/harness-audit.md`, ~90 file:line refs), field research (sources at end), and one applied experiment (native tool-call format, benchmarked same-day).

---

## 1. Executive summary

doce's harness is already unusually well-adapted to small models — the two-state Plan/Execute engine (benchmarked 2-4/20 → **20/20** on the 20-scattered-bugs task), grammar-constrained decoding, tiered compaction, tool-output offload, and a dozen empirically-earned prompt/error-text mitigations are exactly the moves the field recommends. The remaining gaps cluster into three themes:

1. **Latency is dominated by re-work, not generation.** Every `generate()` builds a fresh llama context and re-prefills the entire prompt (`inference/mod.rs:363-396`). Agent loops are ~100:1 input:output (Manus's production number); doce pays that input cost from scratch every turn — O(N²) cumulative prefill over an N-turn task.
2. **Several choices fight the model's training distribution** instead of riding it: the bespoke tool-call JSON (fixed today — see §5), sampling params off Qwen's recommendation, a repeat-penalty that taxes JSON syntax, and an 8K window on a 262K-native model.
3. **Context spend has no per-token ROI discipline yet**: uncapped Bash output, plan-machinery rows accumulating in history, stale constants sized for the old 2048-token window, and generative (lossy) summarization where structural (restorable) compression would be safer for a 4B.

**Ranked recommendations** (impact × effort, details in §4):

| #   | Proposal                                                                                      | Impact                   | Effort | Status                                                                                                                 |
| --- | --------------------------------------------------------------------------------------------- | ------------------------ | ------ | ---------------------------------------------------------------------------------------------------------------------- |
| P1  | Speak the model's trained tool format (Hermes `<tool_call>`)                                  | Convergence: high        | S      | **Applied** — and it surfaced a 6-mode failure ladder, all fixed structurally (§5)                                     |
| P2  | KV-cache reuse: persistent context, incremental decode                                        | Latency: dominant        | M-L    | Proposed                                                                                                               |
| P3  | Cache-stable prompt architecture (stable prefix, state at the tail)                           | Latency + convergence    | M      | **Half-applied**: grammar Require mode (the masking half) shipped during §5; the stable-prefix restructure remains     |
| P4  | Sampling alignment (top-k 20, top-p 0.8, min-p 0; retire repeat-penalty for presence-penalty) | Convergence: medium-high | S      | Proposed                                                                                                               |
| P5  | Per-intent output caps (256 is too small for Write/final answers)                             | Convergence: medium      | S      | **Applied** — demonstrated live by run 3's truncation failure; 1024, constant-wired                                    |
| P6  | Rebalance the context budget (8K → 16-32K; re-tune stale constants)                           | Capability: high         | S-M    | Proposed                                                                                                               |
| P7  | Restorable compression over lossy summarization; Bash output cap; clear plan rows             | Convergence: medium      | M      | Proposed                                                                                                               |
| P8  | Recitation: re-state the live plan at the context tail                                        | Convergence: medium      | S      | **Partially applied** in tool-result form (decision-moment nudges, §5 run 5); tail recitation of the full plan remains |
| P9  | Subagents get the PlanState engine; JSON-schema argument validation pre-dispatch              | Convergence: medium      | S-M    | Proposed                                                                                                               |
| P10 | Fixed-seed mode + benchmark expansion into a harness-regression suite                         | Measurement              | S      | Proposed — §5 is the demonstration of its value                                                                        |

Plus two structural robustness protocols that emerged from §5, now in `run_loop`: truncated tool calls trigger a correction turn instead of silently ending the task, and "done" is itself a grammar-constrained tool call (`FinishTask` → `ToolExecution::Finish`), which is what makes Require-mode decoding safe in every plan state.

Plus four small correctness fixes found by the audit (§4.11): the dead `n_threads` parameter, the 256-literal duplication, and two usage-measurement mismatches.

---

## 2. Where doce stands today (audit snapshot)

- **Window:** `CONTEXT_WINDOW_TOKENS = 8192` (raised from 2048), fixed; model's `n_ctx_train` is 262,144.
- **Prompts:** flat `SYSTEM_PROMPT` ~345 tok; `PLANNING_SYSTEM_PROMPT` ~636 tok; `executing_system_prompt` ~363 tok + goal/step. System prompt is **swapped wholesale per state** each generation.
- **Loop:** `run_loop` measures (full render+tokenize) → compacts if over threshold → generates (fresh context, full re-prefill) → parses first tool call → executes → appends result. Top-level 200-turn cap, subagent 30.
- **Sampling:** grammar (lazy) → repeat-penalty 1.1/64 → top-k 40 → top-p 0.9 → temp 0.7 → random seed. Max 256 output tokens per generation, hardcoded at 4 call sites.
- **Context pipeline:** thresholds 0.5/0.75/0.9 of window; tier 1 clears old tool results to a 12-token placeholder (keep last 2); tier 2 summarizes all but the last 10 messages via the same 4B model (256-token summary) and splices on reload; per-turn `fit_turn_to_budget` (drop-oldest + render-recheck); tool results >500 chars offloaded to disk with a 500-char preview + `Read` pointer.
- **Empirical scar tissue (all "confirmed against the real model"):** wrong-arg-key loops (×6 without self-correction), whitespace-glob giving-up, run-on double tool calls, the `role: "tool"` template trap, goal-less step hallucination, the 64GB Grep slurp, oversized-result budget starvation, mid-turn window blowouts, 0/20-with-confident-false-success (→ the "a claim is not proof" verification rule), 7 confirmed Bash-denylist bypasses.

The scar tissue is worth emphasizing: doce has been doing evidence-driven harness engineering all along. The proposals below extend that practice; none of them contradict an observed finding.

---

## 3. What the field converged on

**Manus (production agent, ~50 tool calls/task, ~100:1 input:output):** the KV-cache hit rate is _the_ production metric. Stable prompt prefixes (one token of drift invalidates everything after it), append-only context, deterministic JSON serialization, **mask tools instead of removing them** (logit-level constraints; prefix-grouped tool names), the **filesystem as restorable memory** (drop content, keep the path — never irreversibly), **recitation** (rewrite the todo list at the tail of context to pull attention onto the plan), **keep errors in context** (models update priors on observed failures), and avoid few-shot ruts (structured variation).

**Anthropic (context engineering for agents):** find the _smallest set of high-signal tokens_; compaction as the first lever but treat it carefully; **just-in-time context** (lightweight references, load on demand) over pre-loading; sub-agents with clean windows returning condensed summaries.

**OpenHands condenser:** without condensation, per-turn cost grows quadratically; an event-count-triggered LLM summary of the middle (keep first 4, keep recent) makes it linear at <½ the cost. **Aider's repo-map:** a budgeted, ranked symbol map as just-in-time codebase context.

**Small-model agent research (NVIDIA's SLM-agents position paper + surveys + fine-tuning studies):** SLMs (1-12B) are sufficient — often superior — for agentic workloads _when the harness constrains them_: decomposed tasks, schema-constrained outputs via guided decoding, format checks at the boundary, and heterogeneous architectures. Fine-tuned 3B models reach >95% schema compliance where zero-shot frontier models sit at 80-85%. The practical flywheel: log your agent's real tool calls → cluster → fine-tune per task type.

**Qwen3-4B-Instruct-2507 specifics:** 262K native context; recommended sampling **temp 0.7, top-p 0.8, top-k 20, min-p 0**; natively trained on Hermes-style tool calling (`<tools>` signatures in the system message, `<tool_call>{"name","arguments"}</tool_call>` emission, `<tool_response>` feedback).

Everything above maps cleanly onto doce; §4 does the mapping.

---

## 4. Proposals

### P1. Speak the model's trained tool format — **applied**

**Was:** a bespoke `{"tool_call": {...}}` JSON convention taught in prose, grammar-triggered on `{"tool_call"`. Tool _results_ were already in Qwen's `<tool_response>` format (a previously-earned finding); tool _calls_ and signatures were not.
**Now:** all three system prompts declare tools as JSON function signatures inside `<tools></tools>` with Qwen's own instruction wording; assistant history replays calls as `<tool_call>\n{"name",...}\n</tool_call>`; the lazy grammar triggers on `<tool_call>` and constrains the full tagged shape (closing tag completes the grammar — which also forecloses the observed run-on-second-call failure at the sampler level, not just in the parser); `parse_response` reads the tagged format with the legacy shape kept as fallback.
**Why it matters:** a 4B reproduces its fine-tuning format with far higher fidelity than an in-context convention; every re-teaching token was also pure context overhead. Benchmark results in §5.
**Bonus available cheaply:** the tool schemas now in the prompts are real JSON Schemas — the same objects can drive P9's argument validation and, later, per-tool grammar constraints.

### P2. KV-cache reuse — the dominant latency lever

**Finding:** every `generate()` = new `LlamaContext` + full re-prefill (`inference/mod.rs:363-396`). Turn N re-processes everything turns 1..N-1 already processed. Manus calls the cached-vs-uncached difference 10× on cost; locally it's pure wall-clock — on an M-series running a 4B, prefill of a 6K-token prompt is seconds, per turn, mostly redundant.
**Proposal:** hold one `LlamaContext` per active conversation turn (the engine already serializes generations behind a lock, so one live context is compatible with the concurrency model):

1. Keep the context across `run_loop` turns; on each turn, tokenize only the _delta_ (new tool results + the swap-affected suffix) and decode from the first divergent token, llama.cpp-server-style longest-common-prefix reuse.
2. The compaction path (splice/drop) truncates the KV to the divergence point rather than discarding it.
3. Fall back to full re-prefill whenever the prefix diverges before the system prompt (state swap — see P3, which exists to make that never happen).
   **Effort:** M-L (the trickiest part is KV truncation on compaction; `llama-cpp-2` exposes `kv_cache_seq_rm`-family APIs). **Expected effect:** per-turn latency approaches O(new tokens); a 30-turn task stops costing ~30 full prefills.

### P3. Cache-stable prompt architecture

**Finding:** `RealBackend::generate` swaps `messages[0]` per state (Planning ↔ Executing, and per step-index) — under P2 this would invalidate the _entire_ cache on every state transition, and it already costs measure/generate coherence today (audit §7.4-7.5). Manus's rule: the prefix must be immutable; vary behavior at the _tail_ or via masking.
**Proposal:** restructure the two-state prompting:

1. **One immutable system prompt** containing both roles' instructions and the union `<tools>` block (single source for all tool schemas — also de-duplicates the three hand-maintained copies P1 left behind).
2. **State goes to the tail:** append a short, state-bearing message as the last context item each turn — "You are now PLANNING…" / "You are now EXECUTING step 3 of 7: {step}. Overall goal: {goal}" (this doubles as P8's recitation).
3. **Enforce state-gating at the sampler, not the prompt:** `PlanState` already rejects out-of-state tools textually; add grammar-level masking of `name` to the current state's tool set (the grammar is rebuilt per generation anyway today, and a name-enum per state is a tiny grammar change) — Manus's "mask, don't remove."
   **Caveat:** the current per-state full-prompt swap is exactly what the 20/20 benchmark validated. Do this _after_ P10's regression suite exists, and gate it on reproducing 20/20. **Effort:** M.

### P4. Sampling alignment

**Finding vs. Qwen's recommendation for this exact checkpoint:** doce top-k 40 (rec: 20), top-p 0.9 (rec: 0.8), no min-p (rec: 0), plus `penalties(64, 1.1, …)` — a repeat-penalty over the last 64 tokens. Repeat-penalty is known to hurt structured output: it taxes the very tokens JSON repeats by design (`{`, `"`, `:`, key names), and inside a grammar it can only redistribute mass among grammar-legal tokens — i.e. it distorts _argument content_. Qwen's own guidance for repetition control on 2507 is presence-penalty (0-2), not repeat-penalty.
**Proposal:** temp 0.7 / top-k 20 / top-p 0.8 / min-p 0; replace repeat-penalty with presence-penalty ~1.0 (tune via benchmark); consider dropping penalties entirely while the grammar is active. **Effort:** S — but benchmark before/after (P10), since the current chain's "greedy degenerates into loops" note means penalties exist for a documented reason.

### P5. Per-intent output caps

**Finding:** every agent generation is capped at 256 tokens (hardcoded ×4). A `Write` call's `content` argument or a multi-file final answer simply cannot fit — generation truncates mid-JSON, the parse falls back to "final answer," and the loop ends with garbage. This failure is silent and its frequency is invisible today.
**Proposal:** raise the tool-call cap to ~1024 (grammar guarantees well-formedness to completion; EOG ends it early when short) and give final answers ~512; keep the summary cap separate. Longer-term (with P2), the cap can be generous because unused budget costs nothing when there's no re-prefill. Wire all call sites to `limits::` constants (audit §7.11). **Effort:** S.

### P6. Rebalance the context budget

**Finding:** 8192 of 262,144 available. KV memory for Qwen3-4B at 16K ≈ 1.2-1.6 GB fp16 (halvable with q8 KV quantization) — comfortably within a 16GB machine alongside the ~2.5GB Q4 weights. Meanwhile two constants still assume the old 2048 window (their comments say so): `SUMMARY_MAX_TOKENS = 256` (now 3.1% of window, was 12.5% — summaries got relatively 4× lossier) and `tool_output_offload_chars = 500` (~1.5%, was ~24% — moderate results now offload aggressively, each costing a `Read` round-trip to recover).
**Proposal:** raise the window to 16K (measure Metal throughput; 32K if prefill under P2 stays acceptable), re-derive the dependent constants as fractions (`SUMMARY_MAX_TOKENS` ≈ 5-6% of window; offload threshold ≈ 2000-4000 chars), and add a startup assertion that they stay proportional. Note the counterweight: bigger windows worsen lost-in-the-middle on small models — which is why P6 ships together with P7's spend discipline and P8's recitation, not instead of them. **Effort:** S-M.

### P7. Restorable compression over lossy summarization

**Findings:** (a) tier-2 asks the same 4B to summarize its own history — for a small model this is the riskiest link in the chain (hallucinated summaries poison everything after; Manus: "compression risks irreversible information loss; keep it restorable"); (b) `Bash` output is uncapped in `model_text` — only the generic 500-char offload catches it, and that silently no-ops if `app_data_dir()` fails (audit §7.10); (c) plan-machinery rows (2+ per step) accumulate in model history with near-zero informational value — the state prompt re-states the plan anyway (audit §7.8); (d) tier-1 already is restorable compression (placeholder + full row still in DB) — the pattern just isn't applied everywhere.
**Proposal:**

1. **Cap Bash output at the tool** (tail-biased: last ~200 lines + first 20 + byte count), with the full output offloaded — same shape as the Grep fix; kills the unbounded-String risk too.
2. **Prefer structural clearing to generative summary:** extend tier 1 to replace old cleared tool results with _offload pointers_ ("result saved at {path}; Read to recover") instead of a bare placeholder — restorable by construction. Demote tier-2 summarization to last resort (or scope it to prose-only turns), keep first-message + recent-K pinned like OpenHands.
3. **Clear plan rows aggressively:** superseded `ResumeExecution`/`StepDone` exchanges older than the last one collapse to placeholders in-memory (they already carry `"plan": true` — reuse the marker server-side).
   **Effort:** M.

### P8. Recitation — the plan at the tail

**Finding:** Manus's highest-leverage attention trick (rewrite the todo at the tail of context) and doce's `PlanState` are made for each other. Today the plan lives only in the _system prompt_ (top of context, the lost-in-the-middle danger zone as history grows) and in hidden tool rows.
**Proposal:** each turn, append/replace a short tail message: goal, checklist with ✓/current/pending, current step. This is the state-bearing tail message of P3 — one mechanism serves both. ~40-80 tokens/turn for measurably better goal adherence on long tasks; the 20-bug benchmark is the perfect instrument to prove it. **Effort:** S (given P3's structure; still S standalone).

### P9. Convergence guardrails at the tool boundary

1. **Subagents get the engine that won.** `Task` subagents still run the flat ReAct loop + prompt that scored 0/20-with-false-confidence on decomposable work (audit §7.9). Give `SubagentBackend` its own `PlanState`. **Effort:** S (the state machine is already a lib type).
2. **Validate arguments against the JSON Schemas before dispatch** (NVIDIA's "simple format checks"): P1's schemas make this nearly free — on mismatch, return the same style of corrective error text as `wrong_key_hint`, generalized ("missing required \"file_path\"; expected {schema}"). This turns every malformed call into a one-turn recovery instead of a wandering failure. **Effort:** S-M.
3. **Keep errors in context** (Manus) — doce already does this well (errors feed back as tool results; healing writes interrupted markers). Preserve under P7: never clear the _most recent_ error row.

### P10. Measure what convergence means

1. **Fixed-seed mode** for `generate()` (audit: seed is per-call subsec nanos) — benchmark comparisons are currently noisier than they need to be.
2. **Promote the benchmark to a harness-regression suite:** it already has 5 tiers and produced the 20/20 evidence; add per-run metrics worth tracking over time — turns-to-completion, wall-clock, prefill tokens (P2's KV hit rate once it exists), schema-violation count, wrong-state tool-call count, truncated-generation count (P5's silent failure made visible).
3. Run it on every harness-affecting change (it's `#[ignore]`d and needs the real GGUF — a manual gate is fine; the discipline is what matters).

### 4.11 Small correctness fixes (do anytime)

- `n_threads` param is dead — `generate()` never applies it (`inference/mod.rs:209-216` vs `:363`); wire `.with_n_threads()` or delete the parameter.
- The 256 literal ×4 call sites → reference `AGENT_TURN_MAX_OUTPUT_TOKENS` (subsumed by P5).
- `emit_context_usage_update` measures with the flat prompt while the plan prompt is live (~270-token understatement; audit §7.4).
- Stale `limits.rs` percentage comments (subsumed by P6).

---

## 5. Applied experiment: native tool format (P1) — a seven-run failure ladder

The format switch was applied and benchmarked the same day. What followed is the most instructive part of this report: **six distinct small-model failure modes surfaced one at a time**, each with a structural fix, each fix validated by the next run exposing the _next_ mode. `tier1_planned` (trivial task) passed immediately (2 turns, 7.0s, correct no-plan behavior); `tier4_planned` (20 scattered bugs, historical baseline **20/20** on the bespoke format) told the real story:

| Run | Configuration                                                                                                                  | tier4 result                                                                                                                                | What it exposed                                                                                                                                                                                                                                                                                     |
| --- | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1-2 | Native format; behavioral guidance moved into `<tools>` description fields                                                     | 0/20, 3/20 — confident false success                                                                                                        | **Discipline collapse.** Rules inside the schema block get skimmed as signature boilerplate (that's what `<tools>` is in training data): plans bundled 18 files into one step; zero verification before answering. Per-file mechanics were flawless — the format itself worked.                     |
| 3   | Granularity + verification rules promoted to headed prose sections                                                             | 0/20 at turn 2                                                                                                                              | **Prose rules bit** (model emitted a clean 20-step CreatePlan!) → which blew the 256-token output cap: generation truncated mid-JSON, the unclosed call silently became the "final answer".                                                                                                         |
| 4   | Cap → 1024 (wired to the constant); truncated calls get a correction turn                                                      | 1/20 at turn 6                                                                                                                              | Bundling recurred (same prompt that made 20 steps in run 3 — prompt-only rules are stochastic) **and** the model emitted `StepDone(summary="...")` as plain prose, ending the task.                                                                                                                 |
| 5   | Grammar **Require** mode while Executing; granularity/verification nudges moved into the tool _results_ at the decision moment | 0/20 at turn 23                                                                                                                             | **The nudge mechanism works**: model read the CreatePlan result's warning and AddStep'd all 20 per-file steps. Then, after 20 identical calls, a few-shot rut: it answered the bare text "ResumeExecution" — a pseudo-call in Planning, where plain text was still legal (final answers needed it). |
| 6   | **`FinishTask` tool** + Require mode in BOTH states (free text unsamplable everywhere in the plan loop)                        | 0/20 — honest refusal                                                                                                                       | Free-text failure class closed permanently. Exposed a benchmark-only parity gap: no cwd anchor in the prompt → model globbed the filesystem root, found nothing, and — with perfect mechanics — RefuseStep'd, revised, and honestly reported the task impossible. No false success.                 |
| 7   | cwd line (production parity) + "omit path" tool-description guidance                                                           | **stopped externally at turn 53 — 9+ of 20 files fixed, on a clean 20/20 trajectory** (Read→Edit→StepDone per file, correct absolute paths) | —                                                                                                                                                                                                                                                                                                   |

**Distilled lessons (each now embodied in code):**

1. **Native format for shapes, prose for behavior.** The `<tools>` block is signature data to the model; behavioral rules put there are ignored. Headed prose sections bind — sometimes.
2. **"Sometimes" isn't enough: prompt-level rules fail stochastically; structural enforcement holds.** Every prompt-only rule broke in at least one run. What held: grammar-Required decoding (Manus's "required" mode), "done" as a tool call (`FinishTask` + the `ToolExecution::Finish` loop protocol), correction turns for truncated calls, and nudges delivered _in tool results at the decision moment_ (fresh-context recitation) rather than in the distant system prompt.
3. **The output cap is a convergence parameter, not just a cost knob** — P5 was demonstrated, not hypothesized: well-granulated plans structurally require >256 tokens.
4. **Small models degrade under repetition** (the AddStep×20 → bare-text rut) — exactly Manus's few-shot-rut warning, observed live; grammar enforcement is the antidote.
5. **A failing benchmark that names the failure is worth more than a passing one.** Each run's trace pinpointed the next fix in minutes. This is P10's argument made flesh.

Net implementation state from the ladder (beyond P1 itself): P5 shipped (1024 cap, constant-wired), the Require-mode half of P3 shipped, the recitation-style decision-moment nudges of P8 shipped in tool-result form, and the run_loop gained two robustness protocols (truncation recovery, `Finish`). A full clean tier4 rerun is pending (run 7 was killed externally mid-flight while on trajectory).

---

## 6. Suggested sequencing

1. **Now (S, independent):** P4 sampling + P5 caps + 4.11 fixes + P10 fixed-seed — each benchmark-gated.
2. **Next (M):** P7 spend discipline (Bash cap, pointer-clearing, plan-row clearing) + P8 recitation + P9 (subagent PlanState, schema validation).
3. **Then (M-L, the big one):** P2 persistent context + P3 stable-prefix restructure together (they only pay off together), gated on reproducing tier4's 20/20.
4. **Finally:** P6 window raise, once P2 makes big windows cheap and P7/P8 keep them disciplined.

## Sources

- [Manus — Context Engineering for AI Agents: Lessons from Building Manus](https://manus.im/blog/Context-Engineering-for-AI-Agents-Lessons-from-Building-Manus)
- [Anthropic — Effective context engineering for AI agents](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents)
- [NVIDIA Research — Small Language Models are the Future of Agentic AI](https://research.nvidia.com/labs/lpr/slm-agents/) ([paper](https://arxiv.org/pdf/2506.02153))
- [Small Language Models for Agentic Systems: A Survey](https://arxiv.org/abs/2510.03847)
- [SLMs for Efficient Agentic Tool Calling (fine-tuning study)](https://arxiv.org/html/2512.15943v1)
- [OpenHands — Context Condensation for More Efficient AI Agents](https://www.openhands.dev/blog/openhands-context-condensensation-for-more-efficient-ai-agents) · [Condenser docs](https://docs.openhands.dev/sdk/guides/context-condenser)
- [Qwen/Qwen3-4B-Instruct-2507 model card (sampling & tool-calling guidance)](https://huggingface.co/Qwen/Qwen3-4B-Instruct-2507)
- [llama.cpp — KV cache reuse with llama-server](https://github.com/ggml-org/llama.cpp/discussions/13606) · [server README (cache_prompt, grammars)](https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md)
- [LangChain — Context Engineering for Agents](https://www.langchain.com/blog/context-engineering-for-agents)
- Internal: `.superpowers/sdd/harness-audit.md` (full audit), `src-tauri/tests/agent_benchmark.rs` (tier evidence)
