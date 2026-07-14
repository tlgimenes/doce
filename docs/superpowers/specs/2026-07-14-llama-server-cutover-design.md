# llama-server cutover: OpenAI-compatible sidecar inference

**Date:** 2026-07-14
**Status:** design approved in conversation; benchmark-gated. This REPLACES
the in-process `llama-cpp-2` inference path (hand-rolled grammar + dialect
tool-call parsing) with a bundled `llama-server` sidecar spoken to over the
OpenAI-compatible HTTP/SSE API. Acceptance test: tool-call quality on the
agent-task benchmark, measured against the current (broken) state.

Grounded in a 7-agent research + adversarial-critique workflow
(`wf_0bccb992-377`); every technical claim below traces to a verified
finding.

## Why

Tool calls are poor because we drive the model with llama.cpp's **core C API
only**. Everything that makes tool-calling work — Jinja template application,
per-model tool-call parsers, reasoning extraction — lives in llama.cpp's
`common/chat.cpp`, which the `llama-cpp-2` crate does not bind. So doce
hand-rolled substitutes (`inference/dialect.rs`, a GBNF tool grammar,
`strip_think_blocks`, a pre-opened `<think>`).

That substitute stack is actively wrong for our target model. **Qwen3.5's
real tool-call format is XML** —
`<tool_call><function=NAME><parameter=P>value</parameter></function></tool_call>`
— but `dialect.rs` hard-codes the assumption that Qwen == Hermes-JSON
(`<tool_call>{"name","arguments"}</tool_call>`) and its `detect()` keys
MiniCPM on `<function name=` (space + `name=`), so Qwen3.5's `<function=`
(no space) is misclassified as Hermes. We are literally parsing the model's
tool calls with the wrong parser.

`llama-server --jinja` *is* the `common/chat.cpp` layer: it applies the
model's own trained template and matching server-side parser, and returns
structured `tool_calls`. Routing through it fixes the root cause.

## Goals

1. Replace in-process generation with HTTP calls to a bundled, supervised
   `llama-server` sidecar (OpenAI `/v1/chat/completions`, streaming).
2. Converge on a single registry model, **Qwen3.5-4B** (spike-gated; see
   below), and remove the Settings model picker.
3. Preserve every externally-observable behavior: the `AgentBackend` seam,
   the four inference events, the context-usage gauge, the plan/todo tracker,
   token accounting, and cancellation semantics (finally implemented for
   real).
4. Delete the obsolete hand-rolled stack (`dialect.rs`, tool grammar,
   `strip_think_blocks`, `PromptSession`/KV machinery, the `<tools>` block +
   call-format teaching in the system prompt).

## Non-goals

- Notarized distribution / hardened-runtime signing. doce runs local/ad-hoc
  for now; the bundled server is ad-hoc signed. Notarization is future work,
  called out where the build touches it.
- Multi-model hot-swap UX. One model, one server invocation.
- Per-conversation KV slot affinity as an optimization. With `-np 1` (see
  §Context) there is one slot; switching conversations cold-restarts the
  prefix cache — parity with today's per-turn cold session, not worse.
- YaRN long-context (>256K). `rope_scaling` stays null (native 262144).

## The model decision (spike-gated)

Qwen3.5-4B is a **Gated DeltaNet hybrid** — the same architecture family as
MiniCPM5-1B, which produced gibberish on this M1 Pro ("tensor API disabled
for pre-M5 and pre-A19 devices"). The premise "Qwen works where MiniCPM
doesn't" is therefore unproven for 3.5.

**Phase 0 gates the whole migration on an empirical hardware coherence
check.** Build a recent `llama-server`, run Qwen3.5-4B with one real
tool-call prompt on this machine:

- **Coherent** → proceed on Qwen3.5-4B.
- **Gibberish** (GDN Metal wall) → fall back to **Qwen3-4B-Thinking-2507**
  (standard attention, proven on this Metal, thinking + tool-calling, Hermes
  tool format which the server also handles via `--jinja`), and ship the
  migration on that. Everything downstream of Phase 0 is model-agnostic
  except the registry entry and default sampling.

### Verified model pins

Primary (pending Phase 0):
- Repo/file: `unsloth/Qwen3.5-4B-GGUF` / `Qwen3.5-4B-Q4_K_M.gguf`
- URL: `https://huggingface.co/unsloth/Qwen3.5-4B-GGUF/resolve/main/Qwen3.5-4B-Q4_K_M.gguf`
- sha256: `00fe7986ff5f6b463e62455821146049db6f9313603938a70800d1fb69ef11a4`
- size: `2740937888` (2.74 GB)
- thinking-by-default (emits `<think>`), native context 262144, XML tool
  template.
- sampling (coding preset, doce is a coding agent): `temperature=0.6,
  top_p=0.95, top_k=20, min_p=0.0, presence_penalty=0.0`. Set
  `enable_thinking` **explicitly** (sources conflict on the small-model
  default) via `chat_template_kwargs`.

Fallback:
- `Qwen3-4B-Thinking-2507` — already the registry fallback entry; keep its
  existing URL + sha256.

## Architecture

```
send_agent_message ─┐
                    │  AgentBackend trait (agent/mod.rs:248-262)  ← stable seam
RealBackend ────────┤  measure / threshold / compact / generate / execute_tool
SubagentBackend ────┘
                    │
                    ▼
          LlamaServerClient (NEW: inference/http.rs)
          reqwest POST /v1/chat/completions (stream=true)
          SSE deltas → content / reasoning_content / tool_calls / usage
                    │  HTTP localhost
                    ▼
          ServerSupervisor (NEW: inference/server.rs)
          spawns/​health-gates/​reaps the bundled sidecar
                    │  process
                    ▼
          llama-server  --jinja --reasoning-format deepseek
                        -m <active.gguf> --host 127.0.0.1 --port <ephemeral>
                        -np 1 --ctx-size <N> -ngl 999
```

The `AgentBackend` trait is the insertion seam. The cutover replaces the
*bodies* of `RealBackend::generate` and `SubagentBackend::generate` with an
HTTP call, and replaces `run_loop`'s `parse_response` with reading the
server's structured `tool_calls`. `run_loop`'s control flow (turn cap,
futile-repetition breaker) stays. `dispatch.rs` and `PlanState` host logic
(Todo/FinishTask/Task/AskUserQuestion routing) are transport-agnostic and
untouched.

## Verified server facts (llama.cpp master, mid-2026)

**Launch flags.** `--jinja` is mandatory when sending `tools` (server throws
`"tools param requires --jinja flag"` otherwise); default-on for the server
but pass explicitly for portability. `--reasoning-format deepseek` extracts
`<think>` into a separate `reasoning_content` field (streamed as
`delta.reasoning_content`). `--host 127.0.0.1` (loopback ONLY — never
`0.0.0.0`). `-np 1` (one slot). `--ctx-size N` explicit (never `-c 0`, which
loads the model's 262144 and allocates a multi-GB KV cache → OOM on 8GB
Macs). Prompt-prefix caching (`cache_prompt`) defaults ON.

**Request.** OpenAI shape. `tools`: array of
`{"type":"function","function":{"name","description","parameters"}}`.
`tool_choice` is a **string only** — `"auto"|"none"|"required"`; the named
object form is NOT supported on this endpoint. `parallel_tool_calls`:
default is template-capability-dependent — **must set `false` explicitly**
every turn. `stream:true` + `stream_options:{include_usage:true}`.
`chat_template_kwargs:{enable_thinking:true}`. Sampling passthrough:
`temperature/top_p/top_k/min_p/presence_penalty`.

**Streaming SSE.** `data: <chunk>\n\n` framing, terminates with
`data: [DONE]\n\n`. Each chunk:
`{"choices":[{"delta":{...},"finish_reason":null,"index":0}],"system_fingerprint":"b<build>",...}`.
`delta` carries `content`, `reasoning_content`, and `tool_calls[]` fragments
(accumulate by `index`; `function.name` then `function.arguments` string
fragments). `usage` appears ONLY as a final separate chunk with empty
`choices[]` when `include_usage` is set, then `[DONE]`.

**The arguments quirk.** `function.arguments` SHOULD be (and on current
master IS) a JSON **string**, but a build window (PR #18675 → fixed #20213)
regressed it to a parsed **object**. The client MUST accept either: if
string → `serde_json::from_str`; if object → use directly. Wrap in a
try/catch that feeds a correction, and record `system_fingerprint` to know
which build shipped.

## The design, by phase

Each phase produces working, independently-testable software.

### Phase 0 — hardware coherence spike (GATE)

Build `llama-server` locally (minimal: `-DGGML_METAL=ON -DLLAMA_CURL=OFF`),
download Qwen3.5-4B, run one tool-call prompt via curl on this M1 Pro.
Assert the output is coherent (a well-formed `<tool_call>` / structured
`tool_calls`, not repetition/gibberish). Record the verdict in the ledger.
Outcome selects the registry model for Phase 6. **No app code changes** —
this is a decision gate.

### Phase 1 — reproducible server build + bundling

- A build script (`scripts/build-llama-server.sh`) that clones/pins llama.cpp
  at a fixed tag (**≥ b8020**; the research built and validated **b9993**),
  configures with `-DBUILD_SHARED_LIBS=OFF -DGGML_METAL=ON
  -DGGML_METAL_EMBED_LIBRARY=ON -DLLAMA_CURL=OFF -DLLAMA_BUILD_SERVER=ON
  -DLLAMA_USE_PREBUILT_UI=OFF -DCMAKE_OSX_DEPLOYMENT_TARGET=13.0`, and
  produces a **single** `llama-server` exe (all ggml/llama libs static-linked
  in; only always-present system frameworks remain dynamic). This fixes the
  official prebuilt's macOS-26-minimum blocker (doce's floor is 13.0) and
  collapses ~10 dylibs into one file.
- Place the exe as a Tauri sidecar: `bundle.externalBin` +
  target-triple name `llama-server-aarch64-apple-darwin`, with the
  `shell:allow-execute` capability scoped to it.

### Phase 2 — HTTP inference client (`inference/http.rs`)

`LlamaServerClient` with an async `chat(messages, tools, tool_choice,
sampling, cancel) -> ChatOutcome` that POSTs `/v1/chat/completions`
(stream=true), parses SSE, and returns either a tool call or final text plus
`usage`. Maps `ChatMessage` → OpenAI messages (system/user/assistant/tool),
the 9 tools → an OpenAI `tools` array, `ToolCallMode` → `tool_choice`
(`Require→"required"`, `Allow→"auto"`, `Forbid→omit tools`). Tolerant
argument parsing (string-or-object). Streams `content`/`reasoning_content`
deltas into the existing `on_token`→`agent-generation-piece` path. Tested
against `wiremock` (already a dev-dep) with canned SSE, including the
arguments-as-object variant and a `[DONE]`-with-usage tail.

### Phase 3 — server supervisor (`inference/server.rs`)

Spawn the sidecar via `tauri-plugin-shell` pointed at the active model,
pick a free ephemeral port, poll `GET /health` until `200`, expose the base
URL. Lifecycle:
- **Not spawned** until an active model exists (mirrors today's lazy engine
  load). Spawn + health-gate **after** download completes (the model-install
  auto-activate path must trigger a spawn).
- **Kill on** `RunEvent::ExitRequested`. Because `panic="abort"` disables
  unwinding (Drop won't run on crash), also write a **PID+port file** and
  **reap any orphan on startup** (llama-server does NOT exit on stdin EOF, and
  an orphan keeps the 2.74 GB model + KV resident → a second launch would
  load a second copy → fatal on 8 GB). Project memory already notes orphaned
  doce binaries wedge the single-instance lock; the sidecar compounds this.
- **Restart** with a new `-m` on model switch (`set_active_model`).
- A **`server-status` event** (`starting`/`loading`/`ready`/`error`) so the
  UI can show a boot state between "installed" and the first response (4B
  spawn + mmap + Metal warmup + `/health` takes seconds).

### Phase 4 — agent-loop cutover

Rewrite `RealBackend::generate` / `SubagentBackend::generate` to call
`LlamaServerClient`. Replace `parse_response` with reading structured
`tool_calls`. Harness-invariant guards (critical — see §Tool-call invariant):
`tool_choice:"required"` **and** `parallel_tool_calls:false` every
production turn; an empty-`tool_calls` required turn is a **retriable
error, never `Done`**; a `>1` tool_calls array is a correction, not
first-only. Re-home reasoning/empty-answer recovery off
`finish_reason=="length"` + `usage` (not `strip_think_blocks`). Wire real
cancellation: a `tokio_util::sync::CancellationToken` from
`send_agent_message`, `tokio::select!`-ed against the reqwest SSE stream
(replacing every `|| false` stub).

### Phase 5 — tokenizer retention

`count_tokens` backs 20+ budgeting/offload/metering sites synchronously and
guarantees an exact match with what generation decodes; an HTTP `/tokenize`
round-trip cannot satisfy the sync hot paths, and the completion `usage`
gives no pre-flight number. **Resolution: keep `llama-cpp-2` as a
vocab-only tokenizer** — load the GGUF with `vocab_only` (cheap; no weights,
no context, no Metal) purely for `count_tokens`. This is the deliberate
exception to "delete `llama-cpp-2`": the *inference* path is cut over; exact
local tokenization stays. `context_window()` is driven from the server's
`/props` `n_ctx` (per-slot), reconciled with the local
`CONTEXT_WINDOW_TOKENS` budget anchor. (If vocab-only loading proves
awkward, the fallback is the `tokenizers` crate over Qwen's `tokenizer.json`
— accept minor count drift, which the budget reserves already absorb.)

### Phase 6 — registry convergence + picker removal

Single registry entry (Phase-0-selected model, all tiers). Remove the
Settings "Model" card, `list_available_models` + `AvailableModel`, and
`set_active_model` (picker-only; onboarding auto-activates via direct DB
writes). Regenerate `bindings.ts` via the ignored
`export_typescript_bindings` test. Onboarding download/verify path stays;
`start_model_install` now also triggers the server spawn (Phase 3).

### Phase 7 — delete the dead stack

Delete `inference/dialect.rs` whole. From `inference/mod.rs`:
`hermes_tool_call_grammar`, `tool_call_schema`, `NAME_KV_*`,
`tool_call_grammar_sampler`, `generation_seed` + sampler chain,
`PromptSession`/`new_session`/`common_prefix_len`/`prefill_chunks_from`/
`BATCH_CAPACITY`, the `<think>` injection, `strip_think_blocks`,
`render_chat_prompt`'s hand-rendering, and the `json_schema_to_grammar` /
`encoding_rs` imports. Unwind `ToolDialect` from `AgentBackend::dialect`,
`parse_response`, `single_mode_system_prompt`/`plan_system_message`
signatures, and drop `call_format_instructions()` + the `<tools>` block from
`build_single_mode_system_prompt`. Remove `llama-cpp-2` **metal** feature,
`gbnf`, and `hf-chat-template` from Cargo.toml (the latter two are already
unused). Delete the dormant two-mode plan machine (`UNION_TOOL_LINES`,
`build_plan_system_prompt`, PlanState transitions) that the single-mode
transition left behind.

### Phase 8 — test + benchmark harness rebuild

Re-point `tests/agent_tasks.rs` and `tests/real_model_smoke.rs` off
`InferenceEngine::load`/`new_session`/`render_chat_prompt` onto the HTTP
path (wiremock for unit; a real spawned server for smoke). Rebuild the
benchmark gate (project memory: benchmark-gate prompt/inference changes) to
score real agent runs through the server. **Acceptance:** tool-call quality
on the agent-task benchmark beats the pre-cutover baseline.

## The tool-call invariant (explicit)

Today, Require mode is a **non-lazy GBNF grammar**: a disallowed tool name is
literally unsamplable and a second call is structurally impossible. The
server's `tool_choice:"required"` is softer — it forces ≥1 call but not
*which* tool, and the tools API generally cannot also carry a custom
top-level grammar (`"Cannot use both json_schema and grammar"`). We
consciously trade the token-level guarantee for: (1) `required` +
`parallel_tool_calls:false`; (2) a **filtered `tools` array** as the
name-restriction (in single mode the set is a stable 9/7, so this preserves
prefix caching); (3) dispatch validation + a **content-fallback parser**
(scan `content` for a stray Qwen-XML/Hermes-JSON call before treating an
empty `tool_calls` as anything); (4) empty-`tool_calls` = retry. `FinishTask`
remains the ONLY legitimate terminator. Phase 0/4 must empirically confirm
the server restricts the sampled name to the provided tools (so the filtered
array is a real name-enum, not merely advisory).

## Critic blockers → resolutions (traceability)

- **Model swap smuggled into transport change** → explicit: Phase 0 gates
  the model, Phase 6 swaps the registry, Phase 7 deletes `dialect.rs` only
  after the single model is committed.
- **`count_tokens` exactness vs delete-llama-cpp-2** → Phase 5 keeps
  `llama-cpp-2` vocab-only; not a full crate delete.
- **Prebuilt requires macOS 26 vs floor 13.0** → Phase 1 builds from source
  with `CMAKE_OSX_DEPLOYMENT_TARGET=13.0`.
- **`--host 0.0.0.0` leak** → loopback `127.0.0.1` only (§server facts).
- **n_ctx ÷ n_parallel** → `-np 1` + explicit `--ctx-size ≥ 16384 +
  reserves`; `context_window()` from `/props`.
- **Orphan reaping under `panic="abort"`** → Phase 3 PID/port file +
  startup reap (Drop is not relied on).
- **Cancellation greenfield** → Phase 4 CancellationToken + `select!`.
- **Arguments string-vs-object** → tolerant parser (§arguments quirk).
- **`required` ≠ exactly-one** → `parallel_tool_calls:false` + defensive
  multi/empty handling (§invariant).
- **Qwen param typing (XML strings)** → verify empirically what the server
  returns for numeric/array/bool params against the pinned GGUF; keep a
  normalization step if needed (Phase 4).
- **thinking-default conflict** → set `enable_thinking` explicitly.
- **Stable-prefix vs `tools` in prefix** → single-mode tool set is stable,
  so the templated prefix stays byte-stable across turns; do not vary the
  advertised tool set or `chat_template_kwargs` per turn.
- **First-run/boot UX** → Phase 3 `server-status` event.
- **Benchmark/test rebuild** → Phase 8, and it is the acceptance gate.

## What stays unchanged

`dispatch.rs`, `PlanState` host routing (Todo/FinishTask/Task/
AskUserQuestion), the four events (`agent-generation-piece`, `plan-update`,
`context-usage-update`, `agent-message-persisted`), `PlanTracker` UI, the
download/verify pipeline, and the frontend event contract. `reqwest`
(stream+json) and `wiremock` are already in the dep tree.

## Risks

1. **GDN Metal wall on Qwen3.5** — the Phase 0 gate exists precisely for
   this; fallback is pre-agreed.
2. **From-source build infra** in a Rust/Tauri/npm repo (CMake + Metal
   toolchain, pinned tag, per-triple artifact). Version drift between the
   pinned server and later Qwen3.5 template fixes is an ongoing maintenance
   cost.
3. **Softened tool-call guarantee** (§invariant) — mitigated by
   required+no-parallel+filtered-tools+dispatch-validation+content-fallback,
   verified by the benchmark.
4. **Notarization deferred** — fine for local/ad-hoc; a real blocker only if
   doce is later distributed (adds Developer-ID re-sign + hardened runtime +
   `allow-jit` for Metal + notarizing the nested Mach-O).

## Verification

- **Unit:** SSE parser (content/reasoning/tool_calls deltas, usage tail,
  arguments-as-object); `tool_choice`/`parallel_tool_calls` mapping;
  supervisor spawn/health/reap (mocked); tolerant argument parsing;
  vocab-only `count_tokens` parity with a known string.
- **Integration:** a real spawned server answering a scripted agent task;
  cancellation aborts an in-flight stream and frees the slot.
- **Benchmark (gate):** agent-task tool-call quality vs the pre-cutover
  baseline, on the Phase-0-selected model. This governs merge.
