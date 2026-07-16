//! The production-faithful agent-backend harness the benchmark suite drives
//! against a real `llama-server` -- lifted out of `tests/agent_tasks.rs` +
//! `tests/common/mod.rs` (2026-07-16) so any benchmark runner, not just this
//! crate's own `#[ignore]`d integration tests, can drive the SAME backend
//! instead of hand-copying it. A copied backend silently diverges from
//! production and the benchmark stops measuring reality -- see the module
//! docs on `FlatBackend`/`PlanExecBackend` below for exactly what each
//! mirrors.
//!
//! Gated behind the `bench` cargo feature so the shipped app never compiles
//! this (or pulls its `tempfile` dependency): enable with `--features
//! bench`. The task fixtures, scoring, and `#[test]`/`#[ignore]` functions
//! stay in `tests/agent_tasks.rs` and `tests/real_model_smoke.rs`, which
//! consume this module's `pub` items.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::agent::{dispatch, run_loop, AgentBackend, AgentContext};
use crate::context;
use crate::inference::{ChatMessage, MessageContent};

/// The built sidecar binary the Tauri bundle ships (`tauri.conf.json`'s
/// `externalBin`), resolved next to this crate's manifest -- the exact
/// binary `inference::server::spawn` launches in production, minus the
/// AppHandle-bound sidecar wrapper a test can't use.
pub fn sidecar_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries/llama-server-aarch64-apple-darwin")
}

/// A real `llama-server` spawned for one test, killed on `Drop` so the
/// process dies with the test rather than leaking. Health-gated on spawn so
/// no `LlamaServerClient::chat` ever races a server still mmapping its GGUF,
/// exactly like production's [`inference::server::spawn`].
pub struct TestServer {
    child: std::process::Child,
    pub base_url: String,
}

impl TestServer {
    /// Spawns the sidecar on `model_path` and blocks until `GET /health`
    /// answers 200 (up to 60s, matching production's health gate). Returns
    /// `None` -- printing why -- when the sidecar binary or the model GGUF is
    /// absent, so an `#[ignore]` test invoked without them SKIPS cleanly
    /// rather than panicking (the Qwen3.5 GGUF is normally onboarding-
    /// downloaded and may not be present on a given machine).
    pub async fn spawn(model_path: &Path) -> Option<TestServer> {
        let binary = sidecar_binary();
        if !binary.exists() {
            eprintln!(
                "[test-server] skipping: sidecar binary absent at {}",
                binary.display()
            );
            return None;
        }
        if !model_path.exists() {
            eprintln!(
                "[test-server] skipping: model GGUF absent at {}",
                model_path.display()
            );
            return None;
        }

        let port = crate::inference::server::free_port();
        let args = crate::inference::server::launch_args(port, model_path);
        let child = std::process::Command::new(&binary)
            .args(&args)
            // Silence the sidecar's own stdout/stderr; a test that fails is
            // debugged from its assertions, not a firehose of server logs.
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn llama-server sidecar");
        let mut server = TestServer {
            child,
            base_url: format!("http://127.0.0.1:{port}"),
        };

        let http = reqwest::Client::new();
        let health_url = format!("{}/health", server.base_url);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
        loop {
            let healthy = http
                .get(&health_url)
                .timeout(Duration::from_secs(2))
                .send()
                .await
                .is_ok_and(|resp| resp.status().is_success());
            if healthy {
                return Some(server);
            }
            if tokio::time::Instant::now() >= deadline {
                // Half-started and never going to answer -- kill it now
                // rather than leak an orphan holding the port and GPU memory.
                let _ = server.child.kill();
                panic!(
                    "llama-server did not become healthy on {} within 60s",
                    server.base_url
                );
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Maps a `LlamaServerClient::chat` result into the loop's `TurnOutcome`,
/// mirroring production's `commands::agent::chat_result_to_turn_outcome`: a
/// HARD transport/server failure surfaces as `error` (which run_loop checks
/// FIRST and TERMINATES on) rather than an empty no-tool-call outcome a
/// Require-mode loop would retry forever against a dead server. The
/// benchmark backends never cancel, so -- unlike production -- there is no
/// `Cancelled` arm to special-case: any `Err` is a terminal error.
pub fn chat_outcome_to_turn_outcome(
    result: Result<crate::inference::http::ChatOutcome, crate::inference::InferenceError>,
) -> crate::agent::TurnOutcome {
    match result {
        Ok(o) => crate::agent::TurnOutcome {
            tool_call: o.tool_call,
            text: o.text,
            reasoning: o.reasoning,
            finish_reason: o.finish_reason,
            usage: o.usage,
            error: None,
            cancelled: false,
        },
        Err(e) => {
            let msg = format!("Error: inference failed: {e}");
            crate::agent::TurnOutcome {
                tool_call: None,
                text: msg.clone(),
                reasoning: String::new(),
                finish_reason: String::new(),
                usage: None,
                error: Some(msg),
                cancelled: false,
            }
        }
    }
}

/// FNV-1a. Hand-rolled rather than `DefaultHasher`, whose output std
/// explicitly does not guarantee across releases -- a toolchain bump must not
/// silently change this benchmark's prompt bytes.
fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// A fixed 48-bit value standing in for the millisecond timestamp a real
/// `Uuid::now_v7()` puts in its high bits. Real v7 ids minted within one
/// benchmark run share that timestamp's leading digits and differ only in the
/// tail; keeping the same shared-prefix/random-tail profile is what makes
/// `stable_tool_call_id` tokenize like the id it replaces.
const STABLE_ID_PSEUDO_TIMESTAMP_MS: u64 = 0x0198_f3a2_7b31;

/// The `n`-th deterministic stand-in for `run_loop`'s
/// `uuid::Uuid::now_v7().to_string()` -- a pure function of `n`, rendered in
/// exactly the v7 layout (48-bit timestamp | version 7 | 12 random bits |
/// variant | 62 random bits) and therefore exactly 36 chars, so the prompt
/// carries the same number of bytes it always did.
pub fn stable_tool_call_id(n: u64) -> String {
    let rand_a = fnv1a_64(&n.to_le_bytes());
    let rand_b = fnv1a_64(&(!n).to_le_bytes());
    let mut v: u128 = (STABLE_ID_PSEUDO_TIMESTAMP_MS as u128) << 80;
    v |= 0x7_u128 << 76; // version
    v |= ((rand_a >> 52) as u128) << 64; // rand_a (12 bits)
    v |= 0b10_u128 << 62; // variant
    v |= (rand_b as u128) & ((1u128 << 62) - 1); // rand_b (62 bits)
    let b = v.to_be_bytes();
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13],
        b[14], b[15]
    )
}

/// Prints a digest of the EXACT bytes a turn is about to send. Diff two
/// runs' digest streams and the first differing line is the turn where they
/// diverged -- which is the whole diagnosis in one step: if turn 1 already
/// differs, some input is still run-to-run random (the failure this suite
/// spent its life mistaking for sampler noise); if the digests agree up to
/// turn N and the OUTPUTS diverge there, the prompt is reproducible and the
/// remaining variance is llama.cpp's (batching/threading/float
/// non-associativity), which no amount of harness work can remove.
///
/// Deliberately unconditional: it is one short line per turn on a suite that
/// already prints every tool call, and a reproducibility claim nobody can
/// re-check from the log is worth very little.
fn print_prompt_digest(label: &str, turn: u32, wire_json: &str) {
    println!(
        "  [prompt] {label} turn={turn} bytes={} fnv1a={:016x}",
        wire_json.len(),
        fnv1a_64(wire_json.as_bytes())
    );
}

/// Maps `run_loop`'s random per-call tool-call ids onto `stable_tool_call_id`
/// stand-ins, assigned in first-seen order and remembered for the rest of the
/// run so a call and its result keep agreeing.
///
/// `run_loop` mints the id itself (`src/agent/mod.rs`) and this suite does not
/// touch that file, so the substitution happens at the two places a backend
/// owns: `execute_tool` (which names the payload file after the id, putting it
/// in the reference line) and `generate` (which serializes the id onto the
/// wire). `run_loop` pushes the `ToolUse` message and THEN calls
/// `execute_tool`, so by the time any `generate` sees an id it is already
/// mapped; `rewrite` assigns on demand anyway, in message order, so the
/// mapping stays deterministic even for an id no `execute_tool` ever saw.
///
/// The ids are semantically opaque (they only pair a call with its result),
/// so this changes nothing the model can act on -- it just stops 36 random
/// characters per tool call from re-rolling the token stream every run.
#[derive(Default)]
pub struct StableToolCallIds {
    assigned: std::collections::HashMap<String, String>,
}

impl StableToolCallIds {
    fn stabilize(&mut self, real_id: &str) -> String {
        if let Some(stable) = self.assigned.get(real_id) {
            return stable.clone();
        }
        let stable = stable_tool_call_id(self.assigned.len() as u64);
        self.assigned.insert(real_id.to_string(), stable.clone());
        stable
    }

    /// Rewrites every tool-call id in `messages` in place, right before the
    /// list is serialized for the server.
    pub fn rewrite(&mut self, messages: &mut [ChatMessage]) {
        for message in messages {
            match &mut message.content {
                MessageContent::ToolUse { id, .. } => {
                    let real = std::mem::take(id);
                    *id = self.stabilize(&real);
                }
                MessageContent::ToolResult { tool_use_id, .. } => {
                    let real = std::mem::take(tool_use_id);
                    *tool_use_id = self.stabilize(&real);
                }
                MessageContent::Text(_) => {}
            }
        }
    }
}

/// The flat baseline's available tool set, passed to `ChatRequest::build` on
/// every `FlatBackend::generate` (Allow / `tool_choice:"auto"`). These are
/// the dispatchable file+shell tools the `FLAT_BASELINE_SYSTEM_PROMPT`
/// advertises, mapped onto the cutover's schema authority
/// (`inference::http::tools_array`/`tool_def`) -- exactly the set
/// `dispatch::execute` handles. Pre-cutover the flat backend passed `None`
/// tool names to `session.generate` (an unconstrained tool NAME under a
/// structure-only grammar); the client path needs an explicit `tools` array
/// for the server to parse tool calls into `ChatOutcome::tool_call` AT ALL,
/// so this is the faithful post-cutover equivalent of "the flat prompt's
/// tools are on the table."
///
/// This list and the `<tools>` block of `FLAT_BASELINE_SYSTEM_PROMPT` MUST
/// name the same tools. The sidecar runs `--jinja`, so llama-server renders
/// THIS array into the chat template alongside the hand-written block: a
/// name in one and not the other means the model reads two contradictory
/// tool lists in one prompt, and any name `tool_def` has no arm for is
/// silently dropped by `tools_array` -- the grammar then cannot emit it, so
/// the prompt is instructing a call that is structurally impossible.
pub const FLAT_BASELINE_TOOLS: &[&str] =
    &["Read", "Update", "Bash", "Grep", "Glob", "AskUserQuestion"];

/// The flat ReAct prompt for the `run_flat_*` BASELINE harness below --
/// moved here from `src/agent/mod.rs` (2026-07-12) because NO production
/// code uses it: every conversation the app ships runs the plan machine.
/// Flat runs exist only as model-capability baselines to compare planned
/// runs against. Do NOT point a test at this prompt to claim app
/// behavior -- that mismatch is how the "ola" doom loop shipped green
/// (flat tier-0 answered greetings directly while the production prompt
/// confabulated a plan).
///
/// Deliberately STATIC: this is a CONTROL, held fixed so a planned tier's
/// score can be read against a stable flat baseline across prompt-
/// engineering changes. Frozen text is not the same as stale text, though:
/// the `<tools>` block below must keep naming exactly `FLAT_BASELINE_TOOLS`
/// (see that const's doc comment), because those names are what the server
/// is actually handed and what the grammar can actually emit. The block
/// advertised the pre-cutover `Write`/`Edit` pair until 2026-07-15 while
/// the request passed the unified `Update` -- so under `--jinja` the model
/// saw both lists and was told to call two tools that could never be
/// emitted. Corrected to `Update` (`inference::http::tool_def`'s own
/// description + schema, verbatim): the control's INTENT is "the flat
/// prompt's dispatchable tools are on the table", and only `Update` is
/// dispatchable -- pinning the dead names would have frozen a baseline that
/// cannot edit a file, which is what tiers 3/4/5 grade.
pub const FLAT_BASELINE_SYSTEM_PROMPT: &str = r#"You are a coding and system agent with access to tools.

# Tools

You may call one or more functions to assist with the user query.

You are provided with function signatures within <tools></tools> XML tags:
<tools>
{"type": "function", "function": {"name": "Read", "description": "Read a file from disk.", "parameters": {"type": "object", "properties": {"file_path": {"type": "string"}, "offset": {"type": "number"}, "limit": {"type": "number"}}, "required": ["file_path"]}}}
{"type": "function", "function": {"name": "Update", "description": "Create or modify a file. Pass content to create or fully overwrite the file. Pass old_string and new_string (and no content) to replace one exact occurrence in place.", "parameters": {"type": "object", "properties": {"file_path": {"type": "string"}, "content": {"type": "string"}, "old_string": {"type": "string"}, "new_string": {"type": "string"}, "replace_all": {"type": "boolean"}}, "required": ["file_path"]}}}
{"type": "function", "function": {"name": "Bash", "description": "Run a shell command.", "parameters": {"type": "object", "properties": {"command": {"type": "string"}, "timeout": {"type": "number"}}, "required": ["command"]}}}
{"type": "function", "function": {"name": "Glob", "description": "Find files by name pattern using wildcards, e.g. \"bug_*.txt\" or \"*.rs\". The pattern is a single wildcard expression, never a space-separated list of literal filenames -- that matches nothing. Omit path to search the current working directory.", "parameters": {"type": "object", "properties": {"pattern": {"type": "string"}, "path": {"type": "string"}}, "required": ["pattern"]}}}
{"type": "function", "function": {"name": "Grep", "description": "Search file contents with a regular expression. Omit path to search the current working directory.", "parameters": {"type": "object", "properties": {"pattern": {"type": "string"}, "path": {"type": "string"}, "glob": {"type": "string"}}, "required": ["pattern"]}}}
{"type": "function", "function": {"name": "AskUserQuestion", "description": "Pause and ask the user a clarifying question instead of guessing. Only use this when genuinely ambiguous, not for routine confirmations.", "parameters": {"type": "object", "properties": {"header": {"type": "string"}, "question": {"type": "string"}, "options": {"type": "array", "items": {"type": "object", "properties": {"label": {"type": "string"}, "description": {"type": "string"}}, "required": ["label"]}}, "multiSelect": {"type": "boolean"}}, "required": ["header", "question", "options"]}}}
</tools>

For each function call, return a json object with function name and arguments within <tool_call></tool_call> XML tags:
<tool_call>
{"name": <function-name>, "arguments": <args-json-object>}
</tool_call>

Call one function at a time and wait for its result before deciding your next step. Once you have enough information to answer, respond in plain text with your final answer -- never inside <tool_call> tags."#;

/// Calls PRODUCTION's `commands::agent::stage_tool_result_for_persist` --
/// the single function `handle_general_tool_call` and `SubagentBackend::
/// execute_tool` both stage through (they were byte-identical duplicates
/// until production unified them; this suite's own copy of that shape was
/// still describing the two-call-site world it unified away). It is `pub`
/// for exactly this reason: reimplementing the staging shape here meant a
/// change to it -- widening or dropping the `Read` carve-out, reshaping the
/// `-> Read "..."` reference line, moving the threshold source -- would keep
/// feeding the OLD shape into this benchmark's history while production fed
/// the new one, and nothing would go red.
///
/// This wrapper adds nothing but the argument shape the two backends below
/// share: `Some(payload_dir)` (production's `app_data_dir`; the `None`
/// pass-through arm is unreachable from here because both backends always
/// have a payload root), and `.0` -- only `model_text` enters message
/// history, and neither backend persists `detail` anywhere.
///
/// Takes a `count_tokens` closure, matching production's own call site:
/// production also calls `context::annotate_with_token_count(outcome)`
/// right before staging (detail-only, `model_text`-irrelevant token-count
/// metadata), which the real call sites below apply themselves before
/// calling in here -- keeping the estimate a plain closure is what lets
/// `staging_wiring_replaces_oversized_result_with_reference_line` exercise
/// this exact wiring without a loaded model.
pub fn stage_general_tool_result(
    payload_dir: &Path,
    conversation_id: &str,
    tool_call_id: &str,
    call_name: &str,
    outcome: crate::agent::dispatch::ToolOutcome,
    offload_tokens: usize,
    count_tokens: impl Fn(&str) -> usize,
) -> String {
    crate::commands::agent::stage_tool_result_for_persist(
        Some(payload_dir),
        conversation_id,
        tool_call_id,
        call_name,
        &outcome,
        offload_tokens,
        count_tokens,
    )
    .0
}

/// `AgentBackend` for the flat (plan-less) `run_loop` path -- the exact
/// same shape `commands::agent`'s `SubagentBackend` uses (`measure` calls
/// `context::authoritative_prompt_tokens`, `compact` calls
/// `context::fit_turn_to_budget`, and `generate` clamps `max_tokens`
/// through `context::limits::clamp_output_tokens` then records the
/// trailer's usage -- production functions, not test-only
/// reimplementations), plus a turn counter `run_loop` itself has no reason
/// to expose (it only reports turn count on the `TurnCapExceeded` error
/// path, not on success).
pub struct FlatBackend<'a> {
    /// The supervised `llama-server`'s base URL (`http://127.0.0.1:PORT`) --
    /// generation goes through `inference::http::LlamaServerClient::chat`
    /// against this, the same cutover production's `RealBackend` made. The
    /// server owns cross-turn KV-prefix reuse now (`cache_prompt`), so the
    /// per-loop in-process `PromptSession` this backend used to hold is gone.
    pub base_url: String,
    pub cwd: &'a Path,
    pub threshold: u32,
    pub turns: u32,
    /// Every tool call + result this step actually made, in order -- the
    /// raw evidence `agent::plan::check_in` judges a step's completion
    /// against, instead of trusting the step's own final "Done" text (the
    /// exact thing tier 4 showed is not reliable on its own).
    pub trace: Vec<String>,
    /// 2026-07-09 payload-files design: the root `stage_general_tool_result`
    /// stages into (it joins its own `tool-outputs/` under this, exactly
    /// like production's `app_data_dir`) -- a tempdir SIBLING to the task's
    /// fixture dir (`cwd`), never nested inside it: Glob/Grep tool calls
    /// scan `cwd`, and a `tool-outputs/` directory living inside it would
    /// contaminate every fixture-scanning tier.
    pub payload_dir: PathBuf,
    /// Mirrors production's `parent_conversation_id`/`subagent_id` -- just a
    /// namespace for this run's payload subdirectory and this backend's key
    /// into `observed_usage`, not a real conversation.
    pub conversation_id: String,
    /// The unconfigured install's settings (`default_context_settings`) --
    /// production's `SubagentBackend` has none to route through and calls
    /// `authoritative_prompt_tokens` directly, but its staging call site
    /// still sources the offload threshold from `ContextSettings`, so this
    /// suite carries one rather than reaching for the raw default const.
    pub settings: context::ContextSettings,
    /// FR-2: the server's last authoritative `prompt_tokens` per
    /// conversation -- RECORDED at the end of `generate` from the SSE
    /// trailer's `usage`, CONSULTED at the start of `measure`, exactly like
    /// production's backends. Borrowed (not owned) so a `Task`-spawned
    /// subagent shares the parent's map, mirroring production's single
    /// `.manage()`d `LastObservedUsage` shared across `RealBackend` and
    /// every `SubagentBackend` it spawns; the `conversation_id` key keeps
    /// each loop's observation its own.
    pub observed_usage: &'a context::LastObservedUsage,
    /// See `StableToolCallIds` -- this backend's own map (a `Task`-spawned
    /// subagent runs its own `run_loop` and therefore mints its own ids, so
    /// it gets a fresh map rather than sharing the parent's).
    pub stable_ids: StableToolCallIds,
    /// How many times `compact()` actually DROPPED at least one message this
    /// run -- `run_loop` only calls `compact` when `measure() > threshold()`,
    /// and this counts only the calls where `fit_turn_to_budget` returned a
    /// SHORTER list (a real trim of the history middle), not the no-op calls
    /// where everything still fit. A benchmark tier that means to test
    /// cross-compaction retention is only doing its job when this is > 0; see
    /// the tier6 comment for why it is printed alongside the score.
    pub compactions: u32,
}

impl AgentBackend for FlatBackend<'_> {
    /// IDENTICAL to production's `SubagentBackend::measure` (the flat
    /// baseline's production analogue: no `AppHandle`, no event emission):
    /// FR-2 prefers the server's last authoritative `prompt_tokens` as the
    /// base and estimates only the delta since, over the OpenAI shape the
    /// server actually decodes. A chars/4 re-estimate of the WHOLE prompt
    /// (what this used to do) agrees with production on turn 1 only --
    /// `authoritative_prompt_tokens` falls back to the full estimate when
    /// unobserved -- and drifts from turn 2 onward, moving the point where
    /// `fit_turn_to_budget` fires away from where production fires it.
    fn measure(&mut self, messages: &[ChatMessage]) -> u32 {
        // `.cloned()` to drop the lock before the measurement call, as
        // production does.
        let observed = self
            .observed_usage
            .0
            .lock()
            .unwrap()
            .get(&self.conversation_id)
            .cloned();
        let openai_messages = crate::inference::http::to_openai_messages(messages);
        context::authoritative_prompt_tokens(
            observed.as_ref(),
            &openai_messages,
            crate::inference::token_estimate,
        )
    }

    fn threshold(&self) -> u32 {
        self.threshold
    }

    fn compact(&mut self, messages: &[ChatMessage]) -> Vec<ChatMessage> {
        let fitted = context::fit_turn_to_budget(messages).unwrap_or_else(|_| messages.to_vec());
        if fitted.len() < messages.len() {
            self.compactions += 1;
        }
        fitted
    }

    // Flat baseline runs under `ToolCallMode::Allow`, so a no-tool-call turn
    // is an ordinary plain-text final answer, not a Require-mode invariant
    // violation to retry.
    fn requires_tool_call(&self) -> bool {
        false
    }

    async fn generate(&mut self, mut messages: Vec<ChatMessage>) -> crate::agent::TurnOutcome {
        self.turns += 1;
        // Reproducibility, not fidelity: swap `run_loop`'s random per-call
        // uuids for their deterministic same-length stand-ins before anything
        // reads this list, so the bytes on the wire are a pure function of
        // the trajectory. Local to this turn's copy -- `run_loop`'s own list
        // keeps the real ids, and since the stand-in is exactly as long,
        // `measure`/`compact` (which see the real ids) count the same either
        // way. See `StableToolCallIds`.
        self.stable_ids.rewrite(&mut messages);
        // Flat baseline: `ToolCallMode::Allow` (`tool_choice:"auto"`) over the
        // flat tool set, so a no-tool-call turn is an ordinary plain-text
        // final answer (`requires_tool_call() == false`). The client renders
        // `messages` through the model's own chat template server-side, so no
        // in-process `render_chat_prompt` here -- the whole point of the
        // llama-server cutover (Task 8.1 re-points this off `session.generate`
        // + `parse_response` onto the same `LlamaServerClient::chat` path
        // production's `RealBackend`/`SubagentBackend` use).
        //
        // FR-2: the OpenAI-shaped count of the canonical messages, recorded
        // with this turn's observation below. The flat backend pushes no
        // state tail, so this is simply the whole list -- but it is computed
        // the same way production computes it, before any push.
        let at_len = crate::inference::http::to_openai_messages(&messages).len();
        let mut req = crate::inference::http::ChatRequest::build(
            "doce",
            crate::inference::http::to_openai_messages(&messages),
            Some(crate::inference::http::tools_array(FLAT_BASELINE_TOOLS)),
            crate::inference::http::tool_choice_for(crate::inference::ToolCallMode::Allow)
                .map(|s| s.to_string()),
        );
        // Always-max-output (FR-1), byte-for-byte production's
        // `SubagentBackend::generate`: `ChatRequest::build` leaves
        // `max_tokens: None`, which `skip_serializing_if` omits entirely --
        // the server then generates unbounded to its own ctx, a budget
        // production NEVER hands the model. Near the loop threshold the
        // clamp yields ~1,792 output tokens against the ~6,900 an uncapped
        // request got, so an uncapped benchmark cannot see the mid-JSON
        // truncation `AGENT_TURN_MAX_OUTPUT_TOKENS`'s doc records from real
        // runs. Same ceiling, same window, same `prompt_est` shape as
        // production -- no arithmetic of its own.
        let wire_json =
            serde_json::to_string(&crate::inference::http::to_openai_messages(&messages))
                .unwrap_or_default();
        let prompt_est = crate::inference::token_estimate(&wire_json);
        print_prompt_digest("flat", self.turns, &wire_json);
        req.max_tokens = Some(crate::context::limits::clamp_output_tokens(
            crate::context::limits::AGENT_TURN_OUTPUT_CEILING,
            crate::inference::CONTEXT_WINDOW_TOKENS,
            prompt_est,
        ));
        // Benchmarks never cancel; a fresh, never-fired token satisfies the
        // `chat` signature.
        let cancel = tokio_util::sync::CancellationToken::new();
        let result = crate::inference::http::LlamaServerClient::new(self.base_url.clone())
            .chat(req, |_piece| {}, &cancel)
            .await;
        let outcome = chat_outcome_to_turn_outcome(result);
        // FR-2: record the server's authoritative `prompt_tokens` for the
        // next `measure` to prefer over a full chars/4 re-estimate, exactly
        // as production does. `usage` is `None` on an errored turn (no
        // trailer arrived), correctly leaving any prior observation intact.
        if let Some((prompt_tokens, _completion_tokens)) = outcome.usage {
            self.observed_usage.0.lock().unwrap().insert(
                self.conversation_id.clone(),
                context::ObservedUsage {
                    prompt_tokens,
                    at_len,
                },
            );
        }
        outcome
    }

    async fn execute_tool(
        &mut self,
        tool_call_id: String,
        call: crate::agent::ToolCall,
    ) -> crate::agent::ToolExecution {
        // The id names the payload file, whose absolute path rides into the
        // prompt in a `-> Read "..."` reference line -- so it has to be the
        // deterministic stand-in here too, not just on the wire.
        let tool_call_id = self.stable_ids.stabilize(&tool_call_id);
        let outcome = dispatch::execute(&call, Some(self.cwd));
        let outcome = context::annotate_with_token_count(outcome);
        let result = stage_general_tool_result(
            &self.payload_dir,
            &self.conversation_id,
            &tool_call_id,
            &call.name,
            outcome,
            self.settings.tool_output_offload_tokens,
            |text| crate::inference::token_estimate(text) as usize,
        );
        // Printed for interactive runs, and recorded in `self.trace` as
        // the evidence a plan check-in judges completion against -- the
        // thing worth knowing when a run scores 0 despite claiming full
        // success is which step the actual work diverged at, not just
        // that it did.
        let args_preview: String = call.arguments.to_string().chars().take(200).collect();
        let result_preview: String = result.chars().take(200).collect();
        println!(
            "  turn {} tool={} args={args_preview} -> {result_preview:?}",
            self.turns, call.name
        );
        self.trace.push(format!(
            "tool={} args={args_preview} -> {result_preview}",
            call.name
        ));
        crate::agent::ToolExecution::Result(result)
    }
}

/// `AgentBackend` for the single-mode harness: one `run_loop` call, one
/// continuous `messages` history. The todo-list state lives in
/// `agent::plan::PlanState` (embedded below as `plan_state`), shared with
/// production (`commands::agent::RealBackend`) -- this struct keeps only
/// host concerns: dispatching regular tool calls that pass through the
/// todo machine, the canned `AskUserQuestion` answer, the `Task` subagent,
/// and per-turn trace printing. See `agent::plan`'s own doc comment for the
/// history of what this replaced.
pub struct PlanExecBackend<'a> {
    /// The supervised `llama-server`'s base URL -- generation goes through
    /// `LlamaServerClient::chat` against this, identical to production's
    /// `RealBackend`. The server owns cross-turn KV-prefix reuse now
    /// (`cache_prompt`), replacing the per-run in-process `PromptSession`.
    pub base_url: String,
    pub cwd: &'a Path,
    pub threshold: u32,
    pub turns: u32,
    /// Same shape as `FlatBackend::trace`: every tool call this run made
    /// (plan tools included), source of `TaskRun::trace`.
    pub trace: Vec<String>,
    pub plan_state: crate::agent::plan::PlanState,
    /// See `FlatBackend::payload_dir`'s doc comment -- same tempdir-root
    /// shape, shared with any `Task`-spawned `FlatBackend` below (mirroring
    /// production's single shared `app_data_dir` across `RealBackend` and
    /// every `SubagentBackend` it spawns).
    pub payload_dir: PathBuf,
    pub conversation_id: String,
    /// See `FlatBackend::settings` -- production's `RealBackend` loads these
    /// from the DB and routes `measure` through them
    /// (`usage_from_fitted_messages`); this suite has no settings store, so
    /// it uses what an unconfigured install parses to.
    pub settings: context::ContextSettings,
    /// See `FlatBackend::observed_usage` -- the same map, shared with any
    /// `Task`-spawned subagent backend below, exactly as production shares
    /// one across `RealBackend` and every `SubagentBackend`.
    pub observed_usage: &'a context::LastObservedUsage,
    /// See `FlatBackend::stable_ids`.
    pub stable_ids: StableToolCallIds,
    /// See `FlatBackend::compactions` -- how many times `compact()` actually
    /// dropped at least one message this run. tier6 asserts this is > 0, since
    /// its whole reason to exist is testing what survives the middle-of-history
    /// trim `fit_turn_to_budget` performs once the conversation crosses budget.
    pub compactions: u32,
}

impl AgentBackend for PlanExecBackend<'_> {
    /// IDENTICAL to production's `RealBackend::measure`, minus the
    /// `context-usage-update` emit (no `AppHandle` here, and no UI to feed):
    /// FR-2 prefers the server's last authoritative `prompt_tokens` as the
    /// base via `usage_from_fitted_messages` ->
    /// `authoritative_prompt_tokens`, and keeps production's fail-safe --
    /// a measurement failure reports `u32::MAX` so `compact` runs
    /// defensively rather than letting a too-large prompt through.
    fn measure(&mut self, messages: &[ChatMessage]) -> u32 {
        // `.cloned()` to drop the lock before `usage_from_fitted_messages`
        // runs, as production does.
        let observed = self
            .observed_usage
            .0
            .lock()
            .unwrap()
            .get(&self.conversation_id)
            .cloned();
        match context::usage_from_fitted_messages(
            &self.conversation_id,
            messages,
            &self.settings,
            observed.as_ref(),
        ) {
            Ok(usage) => usage.tokens_used,
            Err(_) => u32::MAX,
        }
    }

    fn threshold(&self) -> u32 {
        self.threshold
    }

    fn compact(&mut self, messages: &[ChatMessage]) -> Vec<ChatMessage> {
        let fitted = context::fit_turn_to_budget(messages).unwrap_or_else(|_| messages.to_vec());
        if fitted.len() < messages.len() {
            self.compactions += 1;
        }
        fitted
    }

    async fn generate(&mut self, mut messages: Vec<ChatMessage>) -> crate::agent::TurnOutcome {
        self.turns += 1;
        // Stable-prefix architecture, exactly as production's `RealBackend`:
        // `messages[0]` is the immutable union prompt + cwd line seeded by
        // `run_planned_task` and never touched here, so the server's
        // `cache_prompt` KV prefix survives every plan-state transition. All
        // volatile state (mode banner, current step framing, refusal,
        // recitation checklist) rides in ONE tail message; the current
        // state's tool set is enforced at the sampler (grammar name-enum),
        // not by prompt swaps.
        // Single-mode harness: the tail is the todo recitation, and only
        // exists once todos do — mirroring RealBackend exactly.
        //
        // FR-2: the OpenAI-shaped count of the CANONICAL messages, taken
        // BEFORE the ephemeral tail push (which never reaches run_loop's
        // list nor `measure`) -- a later `authoritative_prompt_tokens`
        // measures its delta as `all_openai_msgs[at_len..]` over the
        // canonical list, so `at_len` must be the pre-tail canonical count.
        // Exactly production's `RealBackend::generate`; see that impl for
        // why the resulting base slightly over-covers in the safe direction.
        // Reproducibility, not fidelity -- see `FlatBackend::generate`'s
        // matching call and `StableToolCallIds`. Before `at_len`, though the
        // stand-in is length-preserving so neither the count nor any token
        // estimate moves.
        self.stable_ids.rewrite(&mut messages);
        let at_len = crate::inference::http::to_openai_messages(&messages).len();
        let tail = self.plan_state.todo_tail();
        if !tail.is_empty() {
            messages.push(ChatMessage::user(tail));
        }

        // Require mode (`tool_choice:"required"`, default
        // `requires_tool_call() == true`) over the FULL single-mode tool set
        // -- IDENTICAL to production's `RealBackend`: a no-tool-call turn is a
        // Require-mode invariant violation run_loop corrects+retries, and
        // FinishTask is the only legitimate finish. Task 8.1 re-points this
        // off `session.generate` + `parse_response` onto the same
        // `LlamaServerClient::chat` path production uses, so the plan machine
        // is driven exactly as the app drives it.
        let mut req = crate::inference::http::ChatRequest::build(
            "doce",
            crate::inference::http::to_openai_messages(&messages),
            Some(crate::inference::http::tools_array(
                self.plan_state.single_mode_tool_names(true),
            )),
            crate::inference::http::tool_choice_for(crate::inference::ToolCallMode::Require)
                .map(|s| s.to_string()),
        );
        // Always-max-output (FR-1), byte-for-byte production's
        // `RealBackend::generate`: the ceiling is the window itself, so the
        // clamp yields `window - prompt_est - margin` -- the max output that
        // structurally fits. `prompt_est` is measured over the exact
        // `messages` this turn sends (tail INCLUDED), in the server-decoded
        // OpenAI shape (FR-4). This is the shape tier 4 lives at: near the
        // loop threshold production hands the model ~1,792 output tokens,
        // where an unset `max_tokens` (build's default, omitted from the
        // wire by `skip_serializing_if`) let the server generate ~6,900 --
        // nearly 4x the real budget, which is why a prompt growth that
        // starves production's output toward the `MIN_OUTPUT_TOKENS` floor
        // could not turn this gate red.
        let wire_json =
            serde_json::to_string(&crate::inference::http::to_openai_messages(&messages))
                .unwrap_or_default();
        let prompt_est = crate::inference::token_estimate(&wire_json);
        print_prompt_digest("planned", self.turns, &wire_json);
        req.max_tokens = Some(crate::context::limits::clamp_output_tokens(
            crate::context::limits::AGENT_TURN_OUTPUT_CEILING,
            crate::inference::CONTEXT_WINDOW_TOKENS,
            prompt_est,
        ));
        let cancel = tokio_util::sync::CancellationToken::new();
        let result = crate::inference::http::LlamaServerClient::new(self.base_url.clone())
            .chat(req, |_piece| {}, &cancel)
            .await;
        let outcome = chat_outcome_to_turn_outcome(result);
        // FR-2: record the server's authoritative `prompt_tokens` for the
        // next `measure`, exactly as production's `RealBackend::generate`
        // does -- `at_len` is the pre-tail canonical count taken above.
        if let Some((prompt_tokens, _completion_tokens)) = outcome.usage {
            self.observed_usage.0.lock().unwrap().insert(
                self.conversation_id.clone(),
                context::ObservedUsage {
                    prompt_tokens,
                    at_len,
                },
            );
        }
        outcome
    }

    async fn execute_tool(
        &mut self,
        tool_call_id: String,
        call: crate::agent::ToolCall,
    ) -> crate::agent::ToolExecution {
        // See `FlatBackend::execute_tool` -- the id names the payload file
        // and namespaces a `Task` subagent's conversation, both of which
        // reach the prompt.
        let tool_call_id = self.stable_ids.stabilize(&tool_call_id);
        let plan_finish: Option<String>;
        let result = if let Some(outcome) = self.plan_state.handle_todo_tool(&call) {
            match outcome {
                crate::agent::plan::PlanToolReply::Reply(text) => {
                    plan_finish = None;
                    text
                }
                crate::agent::plan::PlanToolReply::Finish(answer) => {
                    plan_finish = Some(answer.clone());
                    answer
                }
                crate::agent::plan::PlanToolReply::ProposeComplete { kind, answer } => {
                    // Task 2 STUB: always approve. Task 4 replaces this with
                    // request_verdict(...) against an observer LLM.
                    let approved = true;
                    let missing = "";
                    let (reply, finish) = self
                        .plan_state
                        .apply_completion_verdict(kind, answer, approved, missing);
                    plan_finish = finish;
                    reply
                }
            }
        } else if call.name == "AskUserQuestion" {
            plan_finish = None;
            "Error: no interactive user is available in this test run -- proceed using your own best judgment".to_string()
        } else if call.name == "Task" {
            plan_finish = None;
            // Mirrors commands::agent's real Task handling: an isolated
            // subagent, FR-016 one-level nesting enforced by run_loop
            // itself via is_subagent -- kept out of the shared
            // conversation entirely, only its final answer becomes this
            // tool_result.
            let prompt = call
                .arguments
                .get("prompt")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let sub_context = AgentContext {
                is_subagent: true,
                max_turns: 20,
                cwd: Some(self.cwd.to_path_buf()),
            };
            let sub_messages =
                // KNOWN HARNESS DIVERGENCE: production subagents run the
                // plan machine (`plan_system_message(cwd, false, ..)`,
                // commands/agent.rs) -- this baseline harness spawns a
                // FLAT subagent instead. Fine for capability baselines;
                // do not read Task-delegation results here as app
                // behavior.
                vec![
                    ChatMessage::system(FLAT_BASELINE_SYSTEM_PROMPT),
                    ChatMessage::user(prompt),
                ];
            let mut sub_backend = FlatBackend {
                // Same server as the parent -- production's `SubagentBackend`
                // likewise inherits its `RealBackend`'s `base_url`.
                base_url: self.base_url.clone(),
                cwd: self.cwd,
                threshold: self.threshold,
                turns: 0,
                trace: Vec::new(),
                // Same shared payload root as the parent (mirrors
                // production's single shared `app_data_dir`), namespaced
                // under this `Task` call's own id (mirrors production's
                // fresh `subagent_id` per spawn) so the subagent's payload
                // files land in their own subdirectory.
                payload_dir: self.payload_dir.clone(),
                conversation_id: format!("task-{tool_call_id}"),
                settings: self.settings.clone(),
                // Same shared usage map as the parent, keyed by this
                // subagent's own conversation id -- exactly how production
                // hands its single `LastObservedUsage` down from
                // `RealBackend` to each `SubagentBackend` it spawns.
                observed_usage: self.observed_usage,
                // NOT shared with the parent: this subagent's `run_loop`
                // mints its own ids, and its own map keeps them stable.
                stable_ids: StableToolCallIds::default(),
                compactions: 0,
            };
            let sub_result = run_loop(&sub_context, sub_messages, &mut sub_backend).await;
            self.turns += sub_backend.turns;
            match sub_result {
                Ok(text) => text,
                Err(e) => format!("Error: subagent did not finish ({e})"),
            }
        } else {
            plan_finish = None;
            let outcome = dispatch::execute(&call, Some(self.cwd));
            let outcome = context::annotate_with_token_count(outcome);
            let result = stage_general_tool_result(
                &self.payload_dir,
                &self.conversation_id,
                &tool_call_id,
                &call.name,
                outcome,
                self.settings.tool_output_offload_tokens,
                |text| crate::inference::token_estimate(text) as usize,
            );
            // Evidence log (observer-verified completion): mirrors
            // production's `RealBackend::execute_tool` exactly, through the
            // same shared classifier -- see `mutation_log_entry`'s doc
            // comment for why `Update`/`Bash` are the only tools logged.
            if let Some((target, ok)) =
                crate::commands::agent::mutation_log_entry(&call.name, &call.arguments, &result)
            {
                self.plan_state.record_mutation(&call.name, target, ok);
            }
            result
        };

        let args_preview: String = call.arguments.to_string().chars().take(200).collect();
        let result_preview: String = result.chars().take(300).collect();
        println!(
            "  turn {} tool={} args={args_preview} -> {result_preview:?}",
            self.turns, call.name
        );
        self.trace.push(format!(
            "tool={} args={args_preview} -> {result_preview}",
            call.name
        ));
        match plan_finish {
            Some(answer) => crate::agent::ToolExecution::Finish(answer),
            None => crate::agent::ToolExecution::Result(result),
        }
    }
}
