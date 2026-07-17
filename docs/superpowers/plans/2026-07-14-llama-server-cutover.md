# llama-server Cutover Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace doce's in-process `llama-cpp-2` generation with a bundled `llama-server` sidecar spoken to over the OpenAI-compatible HTTP/SSE API, converging on a single model, so tool calls are driven by the model's own trained template instead of doce's mis-matched hand-rolled parser.

**Architecture:** The `AgentBackend` trait (`agent/mod.rs:248-262`) is the stable seam. A new `LlamaServerClient` (`inference/http.rs`) POSTs `/v1/chat/completions` (streaming); a new `ServerSupervisor` (`inference/server.rs`) spawns/health-gates/reaps the sidecar. The agent loop, `dispatch.rs`, `PlanState`, the four events, and the frontend contract are preserved. `llama-cpp-2` is retained **vocab-only** for exact `count_tokens`; everything else in the in-process stack (`dialect.rs`, GBNF grammar, `PromptSession`, `strip_think_blocks`) is deleted.

**Tech Stack:** Rust (Tauri v2, tokio, `reqwest` stream+json, `tauri-plugin-shell`, `tokio-util` CancellationToken, `serde_json`, `llama-cpp-2` vocab-only), TypeScript/React frontend, `wiremock` (dev) for server mocking, llama.cpp `llama-server` (built from source, Metal), Qwen GGUF.

## Global Constraints

- **MODEL** (resolved by Task 0, used verbatim in Task 6.1): default `unsloth/Qwen3.5-4B-GGUF` / `Qwen3.5-4B-Q4_K_M.gguf`, URL `https://huggingface.co/unsloth/Qwen3.5-4B-GGUF/resolve/main/Qwen3.5-4B-Q4_K_M.gguf`, sha256 `00fe7986ff5f6b463e62455821146049db6f9313603938a70800d1fb69ef11a4`, size `2740937888`. **Fallback if Task 0 fails coherence:** `Qwen3-4B-Thinking-2507` (keep its existing registry URL/sha256).
- **llama.cpp pin:** a fixed release tag **≥ b8020** (validated: **b9993**). Record the exact tag in the build script.
- **Server launch flags (every spawn):** `--jinja --reasoning-format deepseek --host 127.0.0.1 --port <ephemeral> -np 1 --ctx-size 20480 -ngl 999 -m <active.gguf>`. Never `--host 0.0.0.0`. Never `--ctx-size 0`.
- **Per-request (every production turn):** `stream:true`, `stream_options:{include_usage:true}`, `cache_prompt:true`, `chat_template_kwargs:{enable_thinking:true}`, and for Require mode `tool_choice:"required"` **AND** `parallel_tool_calls:false`.
- **Sampling default (coding preset):** `temperature=0.6, top_p=0.95, top_k=20, min_p=0.0, presence_penalty=0.0`.
- **Tolerant tool arguments:** accept `function.arguments` as a JSON **string** (→ `serde_json::from_str`) OR an **object** (→ use directly).
- **Harness invariant:** an empty-`tool_calls` Require turn is a retriable error, NEVER `LoopStep::Done`. A `>1` tool_calls array is a correction, not first-only. `FinishTask` is the only terminator.
- **Loopback only; no notarization** (ad-hoc signing) for now.
- **Formatting:** Rust via `cargo fmt`; frontend via **oxfmt** (NOT prettier — `npm run format`). Lint: `cargo clippy`, `oxlint`.
- **Workflow:** in-place on `main` (no worktrees). Ledger at `.superpowers/sdd/progress.md`. Commit per task. Commit messages end with `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`.
- **Benchmark gate:** the cutover is not "done" until agent-task tool-call quality beats the pre-cutover baseline (Task 8.2).

---

### Task 0: Hardware coherence spike (GATE)

**Files:**

- Create: `scratch only` — no repo files. Records verdict in the SDD ledger.

**Interfaces:**

- Produces: the decision `MODEL = Qwen3.5-4B` (coherent) or `MODEL = Qwen3-4B-Thinking-2507` (gibberish), consumed by Task 6.1.

- [ ] **Step 1: Confirm the spike binary + model are present**

Run: `SP=/private/tmp/claude-501/-Users-gimenes-code-doce/*/scratchpad/llama-spike; ls -la $SP/llama.cpp/build/bin/llama-server $SP/Qwen3.5-4B-Q4_K_M.gguf`
Expected: both exist; the GGUF sha256 equals `00fe7986…f11a4`.

- [ ] **Step 2: Launch the server on the model**

```bash
$SP/llama.cpp/build/bin/llama-server --jinja --reasoning-format deepseek \
  --host 127.0.0.1 --port 18080 -np 1 --ctx-size 8192 -ngl 999 \
  -m $SP/Qwen3.5-4B-Q4_K_M.gguf > /tmp/spike-server.log 2>&1 &
# poll until ready
until curl -sf http://127.0.0.1:18080/health >/dev/null; do sleep 1; done; echo READY
```

Expected: `READY` within ~30s; the log shows the model loaded with the `qwen3_5`/`qwen3.5` arch (NOT an "unsupported arch" error).

- [ ] **Step 3: Send one real tool-call prompt**

```bash
curl -s http://127.0.0.1:18080/v1/chat/completions -H 'Content-Type: application/json' -d '{
  "messages":[{"role":"user","content":"Read the file /etc/hostname and tell me what it contains."}],
  "tools":[{"type":"function","function":{"name":"Read","description":"Read a file","parameters":{"type":"object","properties":{"file_path":{"type":"string"}},"required":["file_path"]}}}],
  "tool_choice":"required","parallel_tool_calls":false,
  "temperature":0.6,"top_p":0.95,"top_k":20
}' | tee /tmp/spike-toolcall.json
```

Expected (COHERENT): `choices[0].message.tool_calls[0].function.name == "Read"` with `arguments` naming `file_path` = `/etc/hostname` (arguments may be a string or object). GIBBERISH = repeated tokens, empty/garbled name, or no tool call.

- [ ] **Step 4: Record the verdict and select MODEL**

Note in the ledger: `Task 0: <coherent|gibberish>; MODEL=<qwen3.5-4b|qwen3-4b-thinking-2507>; arguments encoding=<string|object>; server build=<system_fingerprint>`. Kill the spike server (`kill %1`). If gibberish, ALSO run Steps 2-3 against `Qwen3-4B-Thinking-2507` (download it) to confirm the fallback is coherent before committing to it.

- [ ] **Step 5: Commit the ledger note** (no code changes; ledger is git-ignored — the "commit" here is just recording the decision in this plan's context and the ledger file).

---

### Task 1.1: Reproducible llama-server build script

**Files:**

- Create: `scripts/build-llama-server.sh`
- Create: `scripts/README-llama-server.md` (documents the pin + how to rebuild)

**Interfaces:**

- Produces: a single self-contained `src-tauri/binaries/llama-server-aarch64-apple-darwin` executable consumed by Task 1.2.

- [ ] **Step 1: Write the build script**

```bash
#!/usr/bin/env bash
# Builds a self-contained llama-server for macOS arm64 (Metal, embedded shaders,
# static ggml/llama libs) targeting doce's minimum macOS. Pin is deliberate.
set -euo pipefail
LLAMA_TAG="b9993"   # >= b8020 for Qwen3.5; validated by research wf_0bccb992-377
TRIPLE="aarch64-apple-darwin"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BUILD_DIR="${TMPDIR:-/tmp}/doce-llama-build"
DEST="$ROOT/src-tauri/binaries/llama-server-$TRIPLE"

rm -rf "$BUILD_DIR"; mkdir -p "$BUILD_DIR"
git clone --depth 1 --branch "$LLAMA_TAG" https://github.com/ggml-org/llama.cpp "$BUILD_DIR/llama.cpp"
cmake -B "$BUILD_DIR/build" -S "$BUILD_DIR/llama.cpp" \
  -DCMAKE_BUILD_TYPE=Release \
  -DBUILD_SHARED_LIBS=OFF \
  -DGGML_METAL=ON -DGGML_METAL_EMBED_LIBRARY=ON \
  -DLLAMA_CURL=OFF -DLLAMA_BUILD_SERVER=ON -DLLAMA_USE_PREBUILT_UI=OFF \
  -DLLAMA_BUILD_TESTS=OFF -DLLAMA_BUILD_EXAMPLES=OFF \
  -DCMAKE_OSX_DEPLOYMENT_TARGET=13.0
cmake --build "$BUILD_DIR/build" -j --config Release --target llama-server
mkdir -p "$ROOT/src-tauri/binaries"
cp "$BUILD_DIR/build/bin/llama-server" "$DEST"
echo "== verifying single-exe (only system dylibs) =="
otool -L "$DEST"
echo "== deployment target (expect 13.0) =="
otool -l "$DEST" | grep -A3 LC_BUILD_VERSION | grep minos
echo "BUILT: $DEST"
```

- [ ] **Step 2: Make it executable and run it**

Run: `chmod +x scripts/build-llama-server.sh && ./scripts/build-llama-server.sh`
Expected: `BUILT:` line; `otool -L` shows only `/System/.../*.framework`, `libc++`, `libobjc`, `libSystem` (no `@rpath/libggml*`, no libcurl); `minos 13.0`.

- [ ] **Step 3: Verify the built server runs and serves the model**

Run the Task-0 Step-2 launch line against this binary + the model; confirm `/health` 200.
Expected: READY.

- [ ] **Step 4: Add binaries dir to .gitignore reasoning + commit the script**

Add `src-tauri/binaries/*.gguf` ignore if needed; the sidecar binary itself is committed OR produced at build time — for now commit the script + README and gitignore the built binary (`src-tauri/binaries/llama-server-*`).

```bash
git add scripts/build-llama-server.sh scripts/README-llama-server.md .gitignore
git commit -m "build: reproducible self-contained llama-server (Metal, static, target 13.0)"
```

---

### Task 1.2: Bundle llama-server as a Tauri sidecar

**Files:**

- Modify: `src-tauri/tauri.conf.json` (add `bundle.externalBin`)
- Modify: `src-tauri/capabilities/default.json` (shell execute scope) — verify exact path first
- Modify: `src-tauri/Cargo.toml` (ensure `tauri-plugin-shell` dep)
- Modify: `src-tauri/src/lib.rs` (register `tauri_plugin_shell::init()`)

**Interfaces:**

- Consumes: `src-tauri/binaries/llama-server-aarch64-apple-darwin` (Task 1.1).
- Produces: a resolvable sidecar named `llama-server`, spawned in Task 3.1.

- [ ] **Step 1: Add the sidecar to tauri.conf.json bundle**

```json
"bundle": {
  "externalBin": ["binaries/llama-server"]
}
```

(Tauri appends the target triple automatically; the file on disk is `binaries/llama-server-aarch64-apple-darwin`.)

- [ ] **Step 2: Add shell plugin + capability**

In `Cargo.toml` add (if absent) `tauri-plugin-shell = "2"`. In `lib.rs`, `.plugin(tauri_plugin_shell::init())`. In the capability JSON add `"shell:allow-execute"` (and `shell:allow-spawn` if the version splits them) scoped to the `llama-server` sidecar.

- [ ] **Step 3: Smoke-test the sidecar resolves**

Run: `cargo test --lib -- --nocapture` (no new test yet — just confirm build compiles with the plugin) then `cargo build`.
Expected: compiles.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/tauri.conf.json src-tauri/capabilities src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/lib.rs
git commit -m "build: register llama-server tauri sidecar + shell plugin"
```

---

### Task 2.1: OpenAI request/response types + message mapping

**Files:**

- Create: `src-tauri/src/inference/http.rs` (types + mapping; client body follows in 2.2/2.3)
- Modify: `src-tauri/src/inference/mod.rs` (`pub mod http;`)
- Test: inline `#[cfg(test)]` in `http.rs`

**Interfaces:**

- Consumes: `ChatMessage`/`MessageContent` (`inference/mod.rs:90-136`), `ToolCallMode` (`mod.rs:120-125`), the 9 tool schemas (`agent/plan.rs`).
- Produces:
  - `fn to_openai_messages(&[ChatMessage]) -> Vec<serde_json::Value>`
  - `fn tools_array(names: &[&str]) -> Vec<serde_json::Value>` (OpenAI function defs for the given tool names)
  - `fn tool_choice_for(mode: ToolCallMode) -> Option<&'static str>` (`Require→Some("required")`, `Allow→Some("auto")`, `Forbid→None`)
  - `struct ChatRequest` (serde Serialize) with the Global-Constraint per-request fields.

- [ ] **Step 1: Write failing tests for message + tool_choice mapping**

```rust
#[test]
fn maps_roles_and_tool_messages() {
    let msgs = vec![
        ChatMessage::system("sys"),
        ChatMessage::user("hi"),
        ChatMessage::tool_use("call_1", "Read", serde_json::json!({"file_path":"/x"})),
        ChatMessage::tool_result("call_1", "Read", "contents"),
    ];
    let out = to_openai_messages(&msgs);
    assert_eq!(out[0]["role"], "system");
    assert_eq!(out[1]["role"], "user");
    // assistant tool_use -> assistant message with tool_calls
    assert_eq!(out[2]["role"], "assistant");
    assert_eq!(out[2]["tool_calls"][0]["function"]["name"], "Read");
    // tool_result -> role "tool" with tool_call_id
    assert_eq!(out[3]["role"], "tool");
    assert_eq!(out[3]["tool_call_id"], "call_1");
}

#[test]
fn tool_choice_maps_modes() {
    assert_eq!(tool_choice_for(ToolCallMode::Require), Some("required"));
    assert_eq!(tool_choice_for(ToolCallMode::Allow), Some("auto"));
    assert_eq!(tool_choice_for(ToolCallMode::Forbid), None);
}

#[test]
fn tools_array_emits_valid_openai_function() {
    let t = tools_array(&["Read"]);
    assert_eq!(t[0]["type"], "function");
    assert_eq!(t[0]["function"]["name"], "Read");
    assert!(t[0]["function"]["parameters"]["properties"]["file_path"].is_object());
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib inference::http`
Expected: FAIL (functions not defined).

- [ ] **Step 3: Implement the mapping**

Map `MessageContent::Text` → `{role, content}`; `ToolUse{id,name,input}` → assistant `{role:"assistant","tool_calls":[{"id":id,"type":"function","function":{"name":name,"arguments":input.to_string()}}]}`; `ToolResult{tool_use_id,content,...}` → `{role:"tool","tool_call_id":tool_use_id,"content":content}`. `tools_array` builds function defs; the 9 tool JSON schemas already exist as text in `plan.rs` — port them into structured `serde_json::json!` builders keyed by name (Read/Update/Bash/Grep/Glob/Todo/Task/AskUserQuestion/FinishTask), reusing `dispatch.rs` `REQUIRED_STRING_ARGS`/`LEGAL_TOOL_ARGS` (dispatch.rs:159-194) as the source of truth for arg names/required-ness.

- [ ] **Step 4: Run tests to verify pass**

Run: `cargo test --lib inference::http`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/inference/http.rs src-tauri/src/inference/mod.rs
git commit -m "feat(inference): OpenAI request types + ChatMessage/tool mapping"
```

---

### Task 2.2: Tolerant SSE stream parser

**Files:**

- Modify: `src-tauri/src/inference/http.rs`
- Test: inline `#[cfg(test)]`

**Interfaces:**

- Produces:
  - `enum ChatChunk { Content(String), Reasoning(String), ToolCallFragment{index:u32, name:Option<String>, args:String}, Usage{prompt:u32, completion:u32}, Done }`
  - `fn parse_sse_line(line: &str) -> Option<Vec<ChatChunk>>`
  - `struct ToolCallAccum` with `fn push_fragment(&mut self, ChatChunk)` and `fn finish(self) -> Option<(String, serde_json::Value)>` implementing the **tolerant string-or-object arguments** rule.

- [ ] **Step 1: Write failing tests (content, reasoning, tool_calls by index, usage tail, args-as-object)**

```rust
#[test]
fn parses_content_and_reasoning_deltas() {
    let l = r#"data: {"choices":[{"delta":{"content":"hel"},"index":0}]}"#;
    assert!(matches!(parse_sse_line(l).unwrap()[0], ChatChunk::Content(ref s) if s=="hel"));
    let r = r#"data: {"choices":[{"delta":{"reasoning_content":"think"},"index":0}]}"#;
    assert!(matches!(parse_sse_line(r).unwrap()[0], ChatChunk::Reasoning(ref s) if s=="think"));
}

#[test]
fn accumulates_tool_call_fragments_by_index() {
    let mut acc = ToolCallAccum::default();
    acc.push_fragment(ChatChunk::ToolCallFragment{index:0,name:Some("Read".into()),args:String::new()});
    acc.push_fragment(ChatChunk::ToolCallFragment{index:0,name:None,args:"{\"file_path\":".into()});
    acc.push_fragment(ChatChunk::ToolCallFragment{index:0,name:None,args:"\"/x\"}".into()});
    let (name, args) = acc.finish().unwrap();
    assert_eq!(name, "Read");
    assert_eq!(args["file_path"], "/x");
}

#[test]
fn tolerates_arguments_as_object() {
    // some builds stream arguments as a parsed object fragment, not a string
    let l = r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"name":"Read","arguments":{"file_path":"/x"}}}]}}]}"#;
    let chunks = parse_sse_line(l).unwrap();
    let mut acc = ToolCallAccum::default();
    for c in chunks { acc.push_fragment(c); }
    assert_eq!(acc.finish().unwrap().1["file_path"], "/x");
}

#[test]
fn parses_usage_tail_and_done() {
    let u = r#"data: {"choices":[],"usage":{"prompt_tokens":12,"completion_tokens":5}}"#;
    assert!(matches!(parse_sse_line(u).unwrap()[0], ChatChunk::Usage{prompt:12,completion:5}));
    assert!(matches!(parse_sse_line("data: [DONE]").unwrap()[0], ChatChunk::Done));
}
```

- [ ] **Step 2: Run to verify failure.** Run: `cargo test --lib inference::http`. Expected: FAIL.

- [ ] **Step 3: Implement.** Strip the `data: ` prefix; `[DONE]` → `[Done]`. Parse JSON; from `choices[0].delta` emit `Content`/`Reasoning`; for each `tool_calls[]` fragment emit `ToolCallFragment` (arguments may arrive as a string OR object — if object, serialize it to a string fragment). `ToolCallAccum::finish` concatenates arg strings per index and parses: try `serde_json::from_str`; if that fails but the whole thing is already valid JSON value, use it; on failure return `None` (caller treats as malformed → correction). Usage chunk (`choices:[]` + `usage`) → `Usage`.

- [ ] **Step 4: Run to verify pass.** Expected: PASS.

- [ ] **Step 5: Commit.** `git commit -m "feat(inference): tolerant SSE parser (content/reasoning/tool_calls/usage)"`

---

### Task 2.3: The streaming chat client

**Files:**

- Modify: `src-tauri/src/inference/http.rs`
- Test: inline `#[cfg(test)]` using `wiremock`

**Interfaces:**

- Consumes: 2.1 types, 2.2 parser, `tokio_util::sync::CancellationToken`.
- Produces:
  - `struct LlamaServerClient { base_url: String, http: reqwest::Client }`
  - `struct ChatOutcome { tool_call: Option<(String, serde_json::Value)>, text: String, reasoning: String, finish_reason: String, usage: Option<(u32,u32)> }`
  - `async fn chat(&self, req: ChatRequest, on_piece: impl FnMut(&str), cancel: &CancellationToken) -> Result<ChatOutcome, InferenceError>`

- [ ] **Step 1: Write failing wiremock test (server streams SSE → client returns a tool call)**

```rust
#[tokio::test]
async fn chat_returns_tool_call_from_sse() {
    let server = wiremock::MockServer::start().await;
    let body = concat!(
        "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"hmm\"},\"index\":0}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",\"function\":{\"name\":\"Read\",\"arguments\":\"{\\\"file_path\\\":\\\"/x\\\"}\"}}]},\"index\":0}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\",\"index\":0}]}\n\n",
        "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":9,\"completion_tokens\":4}}\n\n",
        "data: [DONE]\n\n");
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/v1/chat/completions"))
        .respond_with(wiremock::ResponseTemplate::new(200)
            .insert_header("content-type","text/event-stream")
            .set_body_raw(body, "text/event-stream"))
        .mount(&server).await;
    let client = LlamaServerClient::new(server.uri());
    let out = client.chat(sample_request(), |_p|{}, &CancellationToken::new()).await.unwrap();
    let (name, args) = out.tool_call.unwrap();
    assert_eq!(name, "Read");
    assert_eq!(args["file_path"], "/x");
    assert_eq!(out.reasoning, "hmm");
    assert_eq!(out.usage, Some((9,4)));
}

#[tokio::test]
async fn chat_aborts_on_cancel() {
    // cancel before/while streaming -> returns InferenceError::Cancelled, no hang
    let token = CancellationToken::new(); token.cancel();
    let client = LlamaServerClient::new("http://127.0.0.1:1".into()); // unreachable
    let r = client.chat(sample_request(), |_p|{}, &token).await;
    assert!(matches!(r, Err(InferenceError::Cancelled)));
}
```

- [ ] **Step 2: Run to verify failure.** Expected: FAIL.

- [ ] **Step 3: Implement.** POST with serde `ChatRequest`; `tokio::select!` the `reqwest` `bytes_stream()` against `cancel.cancelled()`; split on `\n\n`, feed lines to `parse_sse_line`; call `on_piece` with content AND reasoning text (so the existing `agent-generation-piece` emit keeps streaming); accumulate tool-call fragments; stop on `Done`. Add `InferenceError::Cancelled`. Add `tokio-util` `rt` feature to Cargo.toml if `CancellationToken` needs it.

- [ ] **Step 4: Run to verify pass.** Expected: PASS.

- [ ] **Step 5: Commit.** `git commit -m "feat(inference): streaming OpenAI chat client with cancellation"`

---

### Task 3.1: Server supervisor — spawn + health gate

**Files:**

- Create: `src-tauri/src/inference/server.rs`
- Modify: `src-tauri/src/inference/mod.rs` (`pub mod server;`)
- Modify: `src-tauri/src/commands/conversations.rs` (replace `InferenceState` engine holder — see 3.3)

**Interfaces:**

- Produces:
  - `struct ServerHandle { base_url: String, child: CommandChild, port: u16, pid: u32 }`
  - `async fn spawn(app: &AppHandle, model_path: &Path) -> Result<ServerHandle, String>` (picks a free port, spawns the `llama-server` sidecar with the Global-Constraint flags, polls `/health` until 200 or timeout, emits `server-status`)
  - `fn free_port() -> u16`

- [ ] **Step 1: Write failing test for free_port + flag assembly (pure)**

```rust
#[test]
fn builds_launch_args_with_loopback_and_explicit_ctx() {
    let args = launch_args(18080, std::path::Path::new("/m.gguf"));
    assert!(args.contains(&"--host".to_string()));
    let host_i = args.iter().position(|a| a=="--host").unwrap();
    assert_eq!(args[host_i+1], "127.0.0.1");
    assert!(args.iter().any(|a| a=="--jinja"));
    assert!(args.iter().any(|a| a=="--reasoning-format"));
    assert!(args.iter().any(|a| a=="-np"));
    let ctx_i = args.iter().position(|a| a=="--ctx-size").unwrap();
    assert_ne!(args[ctx_i+1], "0");
    assert!(!args.iter().any(|a| a=="0.0.0.0"));
}
```

- [ ] **Step 2: Run to verify failure.** Expected: FAIL.

- [ ] **Step 3: Implement** `launch_args`, `free_port` (bind `TcpListener` to `127.0.0.1:0`, read the port, drop), and `spawn` (via `app.shell().sidecar("llama-server")?.args(launch_args(...)).spawn()`), health-poll loop with a ~60s timeout emitting `server-status` (`starting`→`ready`/`error`).

- [ ] **Step 4: Run to verify pass** (`launch_args` unit test). Expected: PASS. (Full spawn covered by the Task 8.1 integration test.)

- [ ] **Step 5: Commit.** `git commit -m "feat(inference): llama-server supervisor spawn + health gate"`

---

### Task 3.2: Orphan reaping (PID/port file)

**Files:**

- Modify: `src-tauri/src/inference/server.rs`

**Interfaces:**

- Produces: `fn reap_orphan(app: &AppHandle)` (reads `<app_data>/llama-server.pid`, SIGKILLs a live PID owning that port, removes the file); `ServerHandle::persist_pidfile()` and `ServerHandle::remove_pidfile()`.

- [ ] **Step 1: Write failing test — pidfile round-trip + stale-pid handling**

```rust
#[test]
fn reap_removes_stale_pidfile_and_ignores_dead_pid() {
    let dir = tempfile::tempdir().unwrap();
    let pidfile = dir.path().join("llama-server.pid");
    std::fs::write(&pidfile, "999999:18080").unwrap(); // pid unlikely to exist
    reap_orphan_at(&pidfile); // no panic; file removed
    assert!(!pidfile.exists());
}
```

- [ ] **Step 2: Run to verify failure.** Expected: FAIL.

- [ ] **Step 3: Implement** `reap_orphan_at`: parse `pid:port`, `kill(pid, 0)` to test liveness, and if alive AND still bound to that port, `kill(pid, SIGKILL)`; always remove the file. `persist_pidfile` writes `pid:port` on successful spawn. Wire `reap_orphan` at startup (before first spawn) and `RunEvent::ExitRequested` → `child.kill()` + `remove_pidfile()` in `lib.rs`.

- [ ] **Step 4: Run to verify pass.** Expected: PASS.

- [ ] **Step 5: Commit.** `git commit -m "feat(inference): reap orphaned llama-server via pidfile (panic=abort safe)"`

---

### Task 3.3: Lifecycle wiring — InferenceState, download→spawn, model-switch→restart

**Files:**

- Modify: `src-tauri/src/commands/conversations.rs` (`InferenceState` now holds `Option<ServerHandle>` + the vocab-only tokenizer from Task 5)
- Modify: `src-tauri/src/commands/models.rs` (`start_model_install` success path spawns/restarts the server; `set_active_model` restarts)
- Modify: `src-tauri/src/lib.rs` (startup reap; exit kill)

**Interfaces:**

- Consumes: 3.1 `spawn`, 3.2 reap.
- Produces: the invariant "server is running against the active model whenever one is installed."

- [ ] **Step 1: Write failing test** (a thin state-machine unit around a `ServerState` enum: `NoModel → Starting → Ready`, and `set_active_model` transitions `Ready(old) → Starting(new)`). Assert transitions; mock spawn with a closure.

- [ ] **Step 2: Run to verify failure.** Expected: FAIL.

- [ ] **Step 3: Implement** the state transitions; replace `*inference.0.lock() = None` (models.rs:258) with "kill current handle + respawn on new model." In `start_model_install`'s success block (models.rs:120-133) add a spawn trigger.

- [ ] **Step 4: Run to verify pass.** Expected: PASS.

- [ ] **Step 5: Commit.** `git commit -m "feat(inference): server lifecycle wired to install + model switch"`

---

### Task 4.1: Cut RealBackend/SubagentBackend generate over to the HTTP client

**Files:**

- Modify: `src-tauri/src/commands/agent.rs` (`RealBackend::generate` 629-685, `SubagentBackend::generate` 777-804)
- Modify: `src-tauri/src/agent/mod.rs` (`run_loop` 275-385, `parse_response` 76-99)

**Interfaces:**

- Consumes: `LlamaServerClient` (Task 2), `InferenceState` server handle (Task 3).
- Produces: `run_loop` reads structured `tool_calls`; Require-turn empty → retry; `Finish` only via FinishTask.

- [ ] **Step 1: Write failing test** — a `FakeClient` (implements a small `ChatClient` trait the backends depend on) returns a scripted `ChatOutcome` tool call; assert `run_loop` dispatches it and that an empty-`tool_calls` Require outcome triggers a retry, not `Done`. (Extract a `ChatClient` trait so the loop is testable without a live server; `LlamaServerClient` and `FakeClient` both impl it.)

```rust
#[tokio::test]
async fn empty_required_toolcalls_retries_not_done() {
    let client = FakeClient::script(vec![
        ChatOutcome::empty_required(),          // must NOT end the task
        ChatOutcome::tool("FinishTask", json!({"answer":"done"})),
    ]);
    let out = run_loop(&ctx, msgs, &mut backend_with(client)).await;
    assert_eq!(out.unwrap(), "done");
    assert_eq!(client.calls(), 2); // it retried
}
```

- [ ] **Step 2: Run to verify failure.** Expected: FAIL.

- [ ] **Step 3: Implement.** Replace the generate bodies with `client.chat(build_request(messages, tools_for(mode), tool_choice_for(mode), sampling), on_piece, cancel)`. In `run_loop`, branch on `ChatOutcome`: `tool_call` present → `LoopStep::ToolCall`; `finish_reason=="length"` with empty content → the existing "keep thinking brief" retry (re-home from `agent/mod.rs:318`); empty tool_calls under Require → retry with a correction message; text-only under Allow → `Done`. Keep the futile-streak + turn-cap guards.

- [ ] **Step 4: Run to verify pass.** Expected: PASS.

- [ ] **Step 5: Commit.** `git commit -m "feat(agent): drive generation through llama-server; structured tool_calls"`

---

### Task 4.2: Real cancellation + reasoning/piece streaming

**Files:**

- Modify: `src-tauri/src/commands/agent.rs` (thread `CancellationToken` from `send_agent_message`; the `on_piece` closure emits `agent-generation-piece` for content AND reasoning)
- Modify: `src-tauri/src/commands/conversations.rs` (`ActiveGenerations` → provide a cancel token per conversation)

**Interfaces:**

- Consumes: 2.3 `chat(cancel)`, existing `ActiveGenerations`.
- Produces: cancelling a generation aborts the SSE stream; `agent-generation-piece` still streams live.

- [ ] **Step 1: Write failing test** — a cancel token flipped mid-stream causes `chat` to return `Cancelled` and `run_loop` to stop; assert no further calls.

- [ ] **Step 2: Run to verify failure.** Expected: FAIL.

- [ ] **Step 3: Implement.** Store a `CancellationToken` in `ActiveGenerations` keyed by conversation; replace every `|| false` (agent.rs:679, 800; context/mod.rs:406) — the summarize path passes a fresh token. `on_piece` emits the existing `AgentGenerationPiece` for content and reasoning deltas (agent.rs:671-678 shape unchanged).

- [ ] **Step 4: Run to verify pass.** Expected: PASS.

- [ ] **Step 5: Commit.** `git commit -m "feat(agent): real generation cancellation + live reasoning stream"`

---

### Task 5.1: Vocab-only tokenizer for count_tokens

**Files:**

- Modify: `src-tauri/src/inference/mod.rs` (strip `InferenceEngine` down to a vocab-only tokenizer; keep `count_tokens`, `context_window`)
- Modify: `src-tauri/Cargo.toml` (keep `llama-cpp-2` but drop the `metal` feature — vocab-only needs no GPU)

**Interfaces:**

- Consumes: the GGUF path.
- Produces: `struct Tokenizer { model: LlamaModel }` with `fn count_tokens(&self, &str) -> usize` (exact) and a `context_window()` sourced from the server `/props` `n_ctx` (fallback to `CONTEXT_WINDOW_TOKENS`).

- [ ] **Step 1: Write failing test** — load the model vocab-only, count a known string, assert `> 0` and stable/deterministic; assert loading uses `vocab_only` (no context allocated).

- [ ] **Step 2: Run to verify failure.** Expected: FAIL.

- [ ] **Step 3: Implement.** `LlamaModelParams` with `.with_vocab_only(true)` (verify the exact `llama-cpp-2` API; if not exposed, use the lowest-cost load). `count_tokens` = `str_to_token(...).len()`. Keep the existing call sites (`agent.rs:1692/1859`, `context/mod.rs:218/566`, `context/payload.rs`) pointed at this `Tokenizer` instead of the deleted engine.

- [ ] **Step 4: Run to verify pass.** Expected: PASS.

- [ ] **Step 5: Commit.** `git commit -m "refactor(inference): retain llama-cpp-2 vocab-only for exact count_tokens"`

---

### Task 6.1: Registry convergence to the single model

**Files:**

- Modify: `src-tauri/src/model_registry/registry.json` (single model, all 4 tiers, using Task-0 `MODEL`)

**Interfaces:**

- Consumes: Task 0 verdict.
- Produces: `best_candidate_for_tier` returns the one model on every tier.

- [ ] **Step 1: Write/adjust failing test** — a registry test asserting every tier resolves to the single `MODEL` id with the verified sha256.

- [ ] **Step 2: Run to verify failure.** Expected: FAIL.

- [ ] **Step 3: Implement.** Replace all `minicpm5-1b`/`qwen3-4b-thinking` entries with the single Task-0 model, priority 1, all tiers; verified URL/sha256/size from Global Constraints. Bump `updated_at`.

- [ ] **Step 4: Run to verify pass.** Expected: PASS.

- [ ] **Step 5: Commit.** `git commit -m "feat(models): converge registry on the single sidecar model"`

---

### Task 6.2: Remove the Settings model picker

**Files:**

- Modify: `src-tauri/src/commands/models.rs` (delete `list_available_models` + `AvailableModel`; delete `set_active_model` iff Task 3.3 no longer needs it — else keep and repurpose as restart trigger)
- Modify: `src-tauri/src/commands/mod.rs` (drop the two `collect_commands!` lines)
- Modify: `src/views/settings/Settings.tsx` (remove the Model `Card` + models state/effects/handlers + `AvailableModel` import)
- Modify: `src/lib/ipc.ts` (drop `listAvailableModels`, `setActiveModel` iff removed, `AvailableModel`)
- Modify: `src/lib/bindings.ts` (regenerated, not hand-edited)
- Modify: `src/views/settings/Settings.test.tsx`, `src/App.test.tsx` (drop model-picker mocks/tests)

**Interfaces:**

- Consumes: nothing new.
- Produces: a Settings screen with no model section; onboarding download/auto-activate unchanged.

- [ ] **Step 1: Delete the frontend Model card + state** (Settings.tsx:63-136, 216-271) and its tests; remove `AvailableModel` from ipc.ts.

- [ ] **Step 2: Delete the backend commands** (`list_available_models`, `AvailableModel`; `set_active_model` per Task 3.3) and the `collect_commands!` registrations.

- [ ] **Step 3: Regenerate bindings.** Run: `cargo test --lib export_typescript_bindings -- --ignored`. Expected: `bindings.ts` updated, no `listAvailableModels`.

- [ ] **Step 4: Run frontend + rust tests.** Run: `npm test` and `cargo test --lib`. Expected: PASS (after removing the stale picker tests).

- [ ] **Step 5: Format + commit.** Run: `npm run format` (oxfmt) then commit.

```bash
git commit -m "feat(settings): remove model picker (single-model convergence)"
```

---

### Task 7.1: Delete the hand-rolled inference stack

**Files:**

- Delete: `src-tauri/src/inference/dialect.rs`
- Modify: `src-tauri/src/inference/mod.rs` (delete grammar/PromptSession/strip_think/render machinery per spec §Phase 7)
- Modify: `src-tauri/src/agent/mod.rs` (unwind `ToolDialect` from `AgentBackend`, `parse_response`)
- Modify: `src-tauri/src/agent/plan.rs` (drop `call_format_instructions`, `<tools>` block, dead two-mode machine)
- Modify: `src-tauri/src/commands/agent.rs` (drop `dialect` params from `plan_system_message`/`conversation_system_message`)
- Modify: `src-tauri/Cargo.toml` (remove `gbnf`, `hf-chat-template`)

**Interfaces:**

- Consumes: nothing (pure deletion after Tasks 2-4 replace the behavior).
- Produces: a smaller inference surface; `cargo build` clean; `cargo clippy` no dead-code warnings.

- [ ] **Step 1: Delete dialect.rs and remove `pub mod dialect;`.** Run `cargo build`; follow the compiler errors as the worklist.

- [ ] **Step 2: Unwind `ToolDialect`** from `AgentBackend::dialect` (agent/mod.rs:259), `parse_response` (drop the dialect arg + legacy fallback branches now that the server returns structured calls), the `single_mode_system_prompt`/`plan_system_message` signatures, and the OnceLock caches keyed on dialect.

- [ ] **Step 3: Strip `mod.rs`** of `hermes_tool_call_grammar`, `tool_call_schema`, `NAME_KV_*`, `tool_call_grammar_sampler`, `PromptSession`/`new_session`/`common_prefix_len`/`prefill_chunks_from`/`BATCH_CAPACITY`/`generation_seed`/sampler chain, the `<think>` injection, `strip_think_blocks`, `render_chat_prompt`, and `json_schema_to_grammar`/`encoding_rs` imports. Remove `gbnf`/`hf-chat-template` from Cargo.toml.

- [ ] **Step 4: Delete the dormant two-mode machine** in plan.rs (`UNION_TOOL_LINES`, `build_plan_system_prompt`, `plan_system_prompt`, PlanState transitions) and drop the `<tools>` block + `call_format_instructions()` from `build_single_mode_system_prompt`.

- [ ] **Step 5: Build + clippy + test, then commit.** Run: `cargo build && cargo clippy --all-targets && cargo test --lib`. Expected: clean. `git commit -m "refactor(inference): delete hand-rolled grammar/dialect/KV stack"`

---

### Task 8.1: Real-server integration smoke + test harness re-point

**Files:**

- Modify: `src-tauri/tests/agent_tasks.rs`, `src-tauri/tests/real_model_smoke.rs` (drop `InferenceEngine::load`/`new_session`/`render_chat_prompt`; drive the HTTP path — wiremock for unit-ish, a spawned real server behind an env gate for smoke)

**Interfaces:**

- Consumes: Tasks 2-4.
- Produces: green tests; a real-server smoke that reads a file end-to-end.

- [ ] **Step 1: Re-point the test backends** onto the `ChatClient` trait (FakeClient for scripted; a real `LlamaServerClient` behind `DOCE_BENCH_MODEL`/a spawned server for smoke).

- [ ] **Step 2: Write a real-server smoke** (`#[ignore]`): spawn the sidecar on the model, run a one-tool-call agent task, assert a `Read` happened and the final answer references the content. Gate on the built binary + model being present.

- [ ] **Step 3: Run** `cargo test` (unit) and `cargo test -- --ignored real_model` (smoke, local). Expected: PASS.

- [ ] **Step 4: Commit.** `git commit -m "test: re-point agent harness onto the llama-server HTTP path"`

---

### Task 8.2: Benchmark gate

**Files:**

- Modify: the benchmark harness (per project memory: benchmark-gate prompt/inference changes; locate the existing ladder script)

**Interfaces:**

- Consumes: the full cutover.
- Produces: a tool-call-quality score vs the pre-cutover baseline.

- [ ] **Step 1: Run the agent-task benchmark** through the server on the Task-0 model; capture tool-call success rate + task completion.

- [ ] **Step 2: Compare to the pre-cutover baseline** (recorded before Task 4). Assert improvement (the whole cutover's acceptance bar).

- [ ] **Step 3: Record results in the ledger + a short results note; commit.** `git commit -m "bench: llama-server cutover tool-call quality vs baseline"`

---

## Self-Review

- **Spec coverage:** Phases 0-8 in the spec each map to Tasks 0, 1.1-1.2, 2.1-2.3, 3.1-3.3, 4.1-4.2, 5.1, 6.1-6.2, 7.1, 8.1-8.2. The tool-call invariant → Tasks 2.1 (tool_choice) + 4.1 (empty-retry). The critic blockers → each has a task (tokenizer 5.1, orphan 3.2, cancellation 4.2, args-tolerance 2.2, loopback/ctx 3.1, macOS-26 1.1).
- **Type consistency:** `ChatOutcome`, `ChatChunk`, `ToolCallAccum`, `LlamaServerClient`, `ServerHandle`, `Tokenizer`, `ChatClient` trait names are used consistently across tasks.
- **Placeholders:** none; each task carries test code + the concrete change site (file:line from the inventory).
- **Model dependency:** Task 0 resolves `MODEL`; Task 6.1 is the only task that hard-codes it, and it reads from the Global Constraints value chosen at Task 0.
