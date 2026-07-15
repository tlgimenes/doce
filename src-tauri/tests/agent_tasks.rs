//! Agent task-completion tests -- the hard pass/fail regression suite for
//! the agent harness running against the real installed GGUF model. This
//! file started life as a print-and-compare benchmark for the 2026-07
//! context-management redesign; that comparison mission is over, and every
//! tier now asserts. A red tier is the definition of done for whatever
//! defect keeps it red, not a result to eyeball -- as of 2026-07-11 the
//! known tier-4 offenders are the reference-line doom loop and the
//! plan-nudge contradiction diagnosed in the last gate run (2/20).
//! `#[ignore]`d like `real_model_smoke.rs` (needs the real installed GGUF).
//! Run via:
//!   cargo test --test agent_tasks -- --ignored --nocapture --test-threads=1
//!
//! Calls `doce_lib::agent::run_loop` + `dispatch::execute` directly against
//! a real `llama-server` -- the harness itself, no Tauri/UI involved,
//! matching how these tasks would actually run through `send_agent_message`.
//! Deliberately calls `context::fit_turn_to_budget` (the exact function
//! `commands::agent`'s real generate closure calls) rather than
//! reimplementing its own version of the pre-generate context-fit step --
//! this suite exists to test what actually ships, not a parallel
//! implementation that could quietly drift from it.
//!
//! Tiers 0-5 of increasing difficulty, all hard-asserted:
//!   0:   conversational baselines -- a plain greeting answered directly
//!        with zero tool calls, and a two-turn exchange that must recall
//!        the user's name from the first turn.
//!   1-2: baseline sanity, single/few tool calls -- a failure here means
//!        something is fundamentally broken.
//!   3:   multi-step refactor, graded by whether `cargo build` succeeds on
//!        the result.
//!   4:   20 scattered single-file fixes, graded per file against ground
//!        truth (the agent's own "Done" claim counts for nothing); must
//!        score 20/20. This is the tier that exercises whether the agent
//!        loses track of earlier progress as a task runs long across many
//!        small, independent units of work.
//!   5:   surgical edit inside a ~3000-line file; the target line must be
//!        fixed and every other line left byte-identical.
//!
//! `_planned` variants (`tier1_planned_...`, `tier4_planned_...`) run the
//! same task through `run_planned_task`'s two-state loop
//! (`agent::plan::PlanState`) instead of a single flat `run_loop` call, and
//! are directly comparable against their flat counterparts. Runs are
//! stochastic (`DOCE_GEN_SEED` respected, entropy default) -- the
//! three-seed gate protocol lives around this suite, not inside it.

mod common;

use doce_lib::agent::{dispatch, run_loop, AgentBackend, AgentContext, AgentError};
use doce_lib::context;
use doce_lib::inference::ChatMessage;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tempfile::tempdir;

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
const FLAT_BASELINE_TOOLS: &[&str] = &["Read", "Update", "Bash", "Grep", "Glob", "AskUserQuestion"];

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
const FLAT_BASELINE_SYSTEM_PROMPT: &str = r#"You are a coding and system agent with access to tools.

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

fn installed_model_path() -> PathBuf {
    // DOCE_BENCH_MODEL points a run at any GGUF (the ladder A/Bs models
    // without editing code); the default is the registry's current
    // primary as installed by the app.
    if let Ok(path) = std::env::var("DOCE_BENCH_MODEL") {
        return PathBuf::from(path);
    }
    let home = std::env::var("HOME").expect("HOME must be set");
    PathBuf::from(home)
        .join("Library/Application Support/app.doce.desktop/models/qwen3.5-4b-q4_k_m.gguf")
}

struct TaskRun {
    result: Result<String, AgentError>,
    turns_taken: u32,
    elapsed: Duration,
    /// Every tool call this run made (name + arg/result previews), copied
    /// from the backend's trace -- empty when the model answered directly
    /// without a single tool call, which tier 0 asserts on.
    trace: Vec<String>,
}

fn report(name: &str, run: &TaskRun) {
    match &run.result {
        Ok(answer) => println!(
            "[{name}] turns={} elapsed={:.1}s -> Done: {answer:?}",
            run.turns_taken,
            run.elapsed.as_secs_f64()
        ),
        Err(e) => println!(
            "[{name}] turns={} elapsed={:.1}s -> {e}",
            run.turns_taken,
            run.elapsed.as_secs_f64()
        ),
    }
}

/// The unconfigured install's `ContextSettings` -- what production's
/// `ContextSettings::load` yields against a settings table with no rows,
/// obtained by calling production's own pure parser on an empty map rather
/// than by hand-writing the default field values here. Every benchmark
/// backend carries one, and drives exactly the production functions
/// (`usage_from_fitted_messages`, `stage_tool_result_for_persist`) that take
/// settings, so a change to any default reaches this suite automatically.
fn default_context_settings() -> context::ContextSettings {
    context::ContextSettings::from_raw(&std::collections::HashMap::new())
}

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
fn stage_general_tool_result(
    payload_dir: &Path,
    conversation_id: &str,
    tool_call_id: &str,
    call_name: &str,
    outcome: doce_lib::agent::dispatch::ToolOutcome,
    offload_tokens: usize,
    count_tokens: impl Fn(&str) -> usize,
) -> String {
    doce_lib::commands::agent::stage_tool_result_for_persist(
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

/// Sanity assertion (no real model, not `#[ignore]`d): proves this suite's
/// own wiring above -- the argument shape it hands production's
/// `stage_tool_result_for_persist`, not `stage_tool_result` in isolation,
/// which already has its own unit tests in `context/payload.rs` -- replaces
/// an oversized synthetic result with a status reference line pointing at a
/// payload file that holds the full text.
#[test]
fn staging_wiring_replaces_oversized_result_with_reference_line() {
    let dir = tempdir().unwrap();
    let big = "line of output\n".repeat(2000);
    let outcome = doce_lib::agent::dispatch::ToolOutcome {
        model_text: big.clone(),
        detail: serde_json::json!({"toolName": "Grep", "matches": ["a", "b"], "outcome": {"ok": true}}),
    };

    // 1 char == 1 "token": comfortably trips
    // `DEFAULT_TOOL_OUTPUT_OFFLOAD_TOKENS` (1024) with a ~30,000-char fixture,
    // without needing a real tokenizer/loaded model.
    let result = stage_general_tool_result(
        dir.path(),
        "wiring-conv",
        "call-1",
        "Grep",
        outcome,
        default_context_settings().tool_output_offload_tokens,
        |text| text.chars().count(),
    );

    assert!(
        result.contains("→ Read \""),
        "expected a status reference line, got: {result:?}"
    );
    assert!(
        !result.contains("line of output"),
        "no content should leak into a reference line, got: {result:?}"
    );
    let path = result
        .split("→ Read \"")
        .nth(1)
        .and_then(|s| s.split('"').next())
        .expect("reference line should contain a quoted payload path");
    assert_eq!(
        std::fs::read_to_string(path).expect("payload file should exist"),
        big,
        "payload file should hold the full, untruncated text"
    );
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
struct FlatBackend<'a> {
    /// The supervised `llama-server`'s base URL (`http://127.0.0.1:PORT`) --
    /// generation goes through `inference::http::LlamaServerClient::chat`
    /// against this, the same cutover production's `RealBackend` made. The
    /// server owns cross-turn KV-prefix reuse now (`cache_prompt`), so the
    /// per-loop in-process `PromptSession` this backend used to hold is gone.
    base_url: String,
    cwd: &'a Path,
    threshold: u32,
    turns: u32,
    /// Every tool call + result this step actually made, in order -- the
    /// raw evidence `agent::plan::check_in` judges a step's completion
    /// against, instead of trusting the step's own final "Done" text (the
    /// exact thing tier 4 showed is not reliable on its own).
    trace: Vec<String>,
    /// 2026-07-09 payload-files design: the root `stage_general_tool_result`
    /// stages into (it joins its own `tool-outputs/` under this, exactly
    /// like production's `app_data_dir`) -- a tempdir SIBLING to the task's
    /// fixture dir (`cwd`), never nested inside it: Glob/Grep tool calls
    /// scan `cwd`, and a `tool-outputs/` directory living inside it would
    /// contaminate every fixture-scanning tier.
    payload_dir: PathBuf,
    /// Mirrors production's `parent_conversation_id`/`subagent_id` -- just a
    /// namespace for this run's payload subdirectory and this backend's key
    /// into `observed_usage`, not a real conversation.
    conversation_id: String,
    /// The unconfigured install's settings (`default_context_settings`) --
    /// production's `SubagentBackend` has none to route through and calls
    /// `authoritative_prompt_tokens` directly, but its staging call site
    /// still sources the offload threshold from `ContextSettings`, so this
    /// suite carries one rather than reaching for the raw default const.
    settings: context::ContextSettings,
    /// FR-2: the server's last authoritative `prompt_tokens` per
    /// conversation -- RECORDED at the end of `generate` from the SSE
    /// trailer's `usage`, CONSULTED at the start of `measure`, exactly like
    /// production's backends. Borrowed (not owned) so a `Task`-spawned
    /// subagent shares the parent's map, mirroring production's single
    /// `.manage()`d `LastObservedUsage` shared across `RealBackend` and
    /// every `SubagentBackend` it spawns; the `conversation_id` key keeps
    /// each loop's observation its own.
    observed_usage: &'a context::LastObservedUsage,
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
        let openai_messages = doce_lib::inference::http::to_openai_messages(messages);
        context::authoritative_prompt_tokens(
            observed.as_ref(),
            &openai_messages,
            doce_lib::inference::token_estimate,
        )
    }

    fn threshold(&self) -> u32 {
        self.threshold
    }

    fn compact(&mut self, messages: &[ChatMessage]) -> Vec<ChatMessage> {
        context::fit_turn_to_budget(messages).unwrap_or_else(|_| messages.to_vec())
    }

    // Flat baseline runs under `ToolCallMode::Allow`, so a no-tool-call turn
    // is an ordinary plain-text final answer, not a Require-mode invariant
    // violation to retry.
    fn requires_tool_call(&self) -> bool {
        false
    }

    async fn generate(&mut self, messages: Vec<ChatMessage>) -> doce_lib::agent::TurnOutcome {
        self.turns += 1;
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
        let at_len = doce_lib::inference::http::to_openai_messages(&messages).len();
        let mut req = doce_lib::inference::http::ChatRequest::build(
            "doce",
            doce_lib::inference::http::to_openai_messages(&messages),
            Some(doce_lib::inference::http::tools_array(FLAT_BASELINE_TOOLS)),
            doce_lib::inference::http::tool_choice_for(doce_lib::inference::ToolCallMode::Allow)
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
        let prompt_est = doce_lib::inference::token_estimate(
            &serde_json::to_string(&doce_lib::inference::http::to_openai_messages(&messages))
                .unwrap_or_default(),
        );
        req.max_tokens = Some(doce_lib::context::limits::clamp_output_tokens(
            doce_lib::context::limits::AGENT_TURN_OUTPUT_CEILING,
            doce_lib::inference::CONTEXT_WINDOW_TOKENS,
            prompt_est,
        ));
        // Benchmarks never cancel; a fresh, never-fired token satisfies the
        // `chat` signature.
        let cancel = tokio_util::sync::CancellationToken::new();
        let result = doce_lib::inference::http::LlamaServerClient::new(self.base_url.clone())
            .chat(req, |_piece| {}, &cancel)
            .await;
        let outcome = common::chat_outcome_to_turn_outcome(result);
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
        call: doce_lib::agent::ToolCall,
    ) -> doce_lib::agent::ToolExecution {
        let outcome = dispatch::execute(&call, Some(self.cwd));
        let outcome = context::annotate_with_token_count(outcome);
        let result = stage_general_tool_result(
            &self.payload_dir,
            &self.conversation_id,
            &tool_call_id,
            &call.name,
            outcome,
            self.settings.tool_output_offload_tokens,
            |text| doce_lib::inference::token_estimate(text) as usize,
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
        doce_lib::agent::ToolExecution::Result(result)
    }
}

/// Runs a multi-turn conversation through the flat loop rooted at `cwd`:
/// one `run_loop` call per user turn, with each turn's final answer
/// appended back into the running history as an assistant message before
/// the next user turn -- the same accumulation shape `send_agent_message`
/// gives a real conversation. One backend (and one llama-server, whose
/// `cache_prompt` KV reuse spans the whole run) for the whole conversation.
/// Returns one `TaskRun` per user turn; its turns/elapsed/trace are
/// per-turn, not cumulative.
async fn run_flat_conversation(
    base_url: &str,
    user_turns: &[&str],
    cwd: &Path,
    max_turns: u32,
) -> Vec<TaskRun> {
    let context = AgentContext {
        is_subagent: false,
        max_turns,
        cwd: Some(cwd.to_path_buf()),
    };

    // Same measure/threshold/compact commands::agent's real backends use --
    // run_loop itself now makes the fit-to-budget decision on every turn, so
    // this suite calls exactly what ships rather than reimplementing its own
    // version of the pre-generate step. `measure` was the one of the three
    // that did NOT hold up (it re-estimated the whole prompt at chars/4
    // forever and dropped `ChatOutcome::usage` on the floor, agreeing with
    // production on turn 1 only); it now drives the same FR-2 authoritative
    // path -- see `FlatBackend::measure`.
    let threshold = doce_lib::inference::CONTEXT_WINDOW_TOKENS
        .saturating_sub(doce_lib::context::limits::AGENT_TURN_MAX_OUTPUT_TOKENS);
    // 2026-07-09 payload-files design: a fresh tempdir SIBLING to `cwd`
    // (never nested inside it -- see `FlatBackend::payload_dir`'s doc
    // comment), kept alive for this whole conversation by staying a local
    // here.
    let payload_root = tempdir().expect("payload tempdir should create");
    // FR-2: one map for the whole conversation (and every subagent it
    // spawns), mirroring production's single `.manage()`d `LastObservedUsage`
    // -- kept alive by staying a local here, as the backend only borrows it.
    let observed_usage = context::LastObservedUsage::default();
    let mut backend = FlatBackend {
        base_url: base_url.to_string(),
        cwd,
        threshold,
        turns: 0,
        trace: Vec::new(),
        payload_dir: payload_root.path().to_path_buf(),
        conversation_id: "flat-top".to_string(),
        settings: default_context_settings(),
        observed_usage: &observed_usage,
    };

    let mut history = vec![ChatMessage::system(FLAT_BASELINE_SYSTEM_PROMPT)];
    let mut runs = Vec::new();
    for turn in user_turns {
        history.push(ChatMessage::user(*turn));
        let turns_before = backend.turns;
        let trace_before = backend.trace.len();
        let start = Instant::now();

        let result = run_loop(&context, history.clone(), &mut backend).await;

        if let Ok(answer) = &result {
            history.push(ChatMessage::assistant(answer.clone()));
        }
        runs.push(TaskRun {
            result,
            turns_taken: backend.turns - turns_before,
            elapsed: start.elapsed(),
            trace: backend.trace[trace_before..].to_vec(),
        });
    }
    runs
}

/// Runs a single `task` through the real agent harness rooted at `cwd`,
/// capturing turns taken and wall-clock time alongside the loop's own
/// `Result` -- the one-user-turn case of `run_flat_conversation`.
async fn run_flat_task(base_url: &str, task: &str, cwd: &Path, max_turns: u32) -> TaskRun {
    run_flat_conversation(base_url, &[task], cwd, max_turns)
        .await
        .pop()
        .expect("one run per user turn")
}

/// `AgentBackend` for the single-mode harness: one `run_loop` call, one
/// continuous `messages` history. The todo-list state lives in
/// `agent::plan::PlanState` (embedded below as `plan_state`), shared with
/// production (`commands::agent::RealBackend`) -- this struct keeps only
/// host concerns: dispatching regular tool calls that pass through the
/// todo machine, the canned `AskUserQuestion` answer, the `Task` subagent,
/// and per-turn trace printing. See `agent::plan`'s own doc comment for the
/// history of what this replaced.
struct PlanExecBackend<'a> {
    /// The supervised `llama-server`'s base URL -- generation goes through
    /// `LlamaServerClient::chat` against this, identical to production's
    /// `RealBackend`. The server owns cross-turn KV-prefix reuse now
    /// (`cache_prompt`), replacing the per-run in-process `PromptSession`.
    base_url: String,
    cwd: &'a Path,
    threshold: u32,
    turns: u32,
    /// Same shape as `FlatBackend::trace`: every tool call this run made
    /// (plan tools included), source of `TaskRun::trace`.
    trace: Vec<String>,
    plan_state: doce_lib::agent::plan::PlanState,
    /// See `FlatBackend::payload_dir`'s doc comment -- same tempdir-root
    /// shape, shared with any `Task`-spawned `FlatBackend` below (mirroring
    /// production's single shared `app_data_dir` across `RealBackend` and
    /// every `SubagentBackend` it spawns).
    payload_dir: PathBuf,
    conversation_id: String,
    /// See `FlatBackend::settings` -- production's `RealBackend` loads these
    /// from the DB and routes `measure` through them
    /// (`usage_from_fitted_messages`); this suite has no settings store, so
    /// it uses what an unconfigured install parses to.
    settings: context::ContextSettings,
    /// See `FlatBackend::observed_usage` -- the same map, shared with any
    /// `Task`-spawned subagent backend below, exactly as production shares
    /// one across `RealBackend` and every `SubagentBackend`.
    observed_usage: &'a context::LastObservedUsage,
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
        context::fit_turn_to_budget(messages).unwrap_or_else(|_| messages.to_vec())
    }

    async fn generate(&mut self, mut messages: Vec<ChatMessage>) -> doce_lib::agent::TurnOutcome {
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
        let at_len = doce_lib::inference::http::to_openai_messages(&messages).len();
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
        let mut req = doce_lib::inference::http::ChatRequest::build(
            "doce",
            doce_lib::inference::http::to_openai_messages(&messages),
            Some(doce_lib::inference::http::tools_array(
                self.plan_state.single_mode_tool_names(true),
            )),
            doce_lib::inference::http::tool_choice_for(doce_lib::inference::ToolCallMode::Require)
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
        let prompt_est = doce_lib::inference::token_estimate(
            &serde_json::to_string(&doce_lib::inference::http::to_openai_messages(&messages))
                .unwrap_or_default(),
        );
        req.max_tokens = Some(doce_lib::context::limits::clamp_output_tokens(
            doce_lib::context::limits::AGENT_TURN_OUTPUT_CEILING,
            doce_lib::inference::CONTEXT_WINDOW_TOKENS,
            prompt_est,
        ));
        let cancel = tokio_util::sync::CancellationToken::new();
        let result = doce_lib::inference::http::LlamaServerClient::new(self.base_url.clone())
            .chat(req, |_piece| {}, &cancel)
            .await;
        let outcome = common::chat_outcome_to_turn_outcome(result);
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
        call: doce_lib::agent::ToolCall,
    ) -> doce_lib::agent::ToolExecution {
        let plan_finish: Option<String>;
        let result = if let Some(outcome) = self.plan_state.handle_todo_tool(&call) {
            match outcome {
                doce_lib::agent::plan::PlanToolReply::Reply(text) => {
                    plan_finish = None;
                    text
                }
                doce_lib::agent::plan::PlanToolReply::Finish(answer) => {
                    plan_finish = Some(answer.clone());
                    answer
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
            stage_general_tool_result(
                &self.payload_dir,
                &self.conversation_id,
                &tool_call_id,
                &call.name,
                outcome,
                self.settings.tool_output_offload_tokens,
                |text| doce_lib::inference::token_estimate(text) as usize,
            )
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
            Some(answer) => doce_lib::agent::ToolExecution::Finish(answer),
            None => doce_lib::agent::ToolExecution::Result(result),
        }
    }
}

fn report_plan(name: &str, plan: &doce_lib::agent::plan::Plan) {
    println!("[{name}] final plan (goal: {:?}):", plan.goal);
    for (i, step) in plan.steps.iter().enumerate() {
        println!(
            "  {i}. [{}] {}",
            if step.done { "x" } else { " " },
            step.description
        );
    }
}

/// Runs a multi-turn conversation through the two-state loop -- one
/// `run_loop` call per user turn, seeded with PRODUCTION's exact
/// messages[0] via the same `plan_system_message` constructor
/// `send_agent_message` uses (union prompt + cwd line + transcript
/// pointer -- prompt drift between app and benchmark is how the
/// 2026-07-12 "ola" doom loop shipped despite green tier-0 tests). Each
/// user turn starts with a FRESH `PlanState`, and each turn's final
/// answer is appended back into the running history, both mirroring
/// production (`send_agent_message` builds a new plan state per send;
/// finished turns replay as history). One llama-server for the whole
/// conversation, whose `cache_prompt` KV reuse matches production's.
async fn run_planned_conversation(
    base_url: &str,
    user_turns: &[&str],
    cwd: &Path,
    max_turns_per_user_turn: u32,
) -> Vec<TaskRun> {
    let context = AgentContext {
        is_subagent: false,
        max_turns: max_turns_per_user_turn,
        cwd: Some(cwd.to_path_buf()),
    };
    // Reserves room for the output tokens AND the per-turn state tail
    // `PlanExecBackend::generate` pushes after run_loop's threshold check
    // has already passed (see `limits::STATE_TAIL_RESERVE_TOKENS`),
    // matching production's `RealBackend` threshold exactly.
    let threshold = doce_lib::inference::CONTEXT_WINDOW_TOKENS.saturating_sub(
        doce_lib::context::limits::AGENT_TURN_MAX_OUTPUT_TOKENS
            + doce_lib::context::limits::STATE_TAIL_RESERVE_TOKENS,
    );
    // 2026-07-09 payload-files design: a fresh tempdir SIBLING to `cwd`
    // (never nested inside it -- see `FlatBackend::payload_dir`'s doc
    // comment), kept alive for this whole conversation by staying a local
    // here. Also hosts the transcript file, outside the workspace like
    // production's app-data dir.
    let payload_root = tempdir().expect("payload tempdir should create");
    let transcript_path = payload_root.path().join("transcript.txt");
    std::fs::write(&transcript_path, "").expect("transcript file should create");
    // FR-2: one map for this whole conversation and every `Task` subagent it
    // spawns -- see `FlatBackend::observed_usage`.
    let observed_usage = context::LastObservedUsage::default();
    let mut backend = PlanExecBackend {
        base_url: base_url.to_string(),
        cwd,
        threshold,
        turns: 0,
        trace: Vec::new(),
        plan_state: doce_lib::agent::plan::PlanState::default(),
        payload_dir: payload_root.path().to_path_buf(),
        conversation_id: "planned-top".to_string(),
        settings: default_context_settings(),
        observed_usage: &observed_usage,
    };

    let mut history = vec![ChatMessage::system(
        doce_lib::commands::agent::plan_system_message(
            Some(cwd),
            true,
            Some(&transcript_path.display().to_string()),
            None,
        ),
    )];
    let mut runs = Vec::new();
    for turn in user_turns {
        backend.plan_state = doce_lib::agent::plan::PlanState::default();
        history.push(ChatMessage::user(*turn));
        let turns_before = backend.turns;
        let trace_before = backend.trace.len();
        let start = Instant::now();

        let result = run_loop(&context, history.clone(), &mut backend).await;

        if let Ok(answer) = &result {
            history.push(ChatMessage::assistant(answer.clone()));
        }
        report_plan("planned", &backend.plan_state.plan);
        runs.push(TaskRun {
            result,
            turns_taken: backend.turns - turns_before,
            elapsed: start.elapsed(),
            trace: backend.trace[trace_before..].to_vec(),
        });
    }
    runs
}

/// Runs a single `task` through the two-state loop -- the one-user-turn
/// case of `run_planned_conversation`.
async fn run_planned_task(base_url: &str, task: &str, cwd: &Path, max_plan_turns: u32) -> TaskRun {
    run_planned_conversation(base_url, &[task], cwd, max_plan_turns)
        .await
        .pop()
        .expect("one run per user turn")
}

// --- Tier 0: conversational baselines, on the PRODUCTION path ---
//
// Every conversation the app runs goes through the plan machine
// (`send_agent_message` -> `plan_system_message` -> the two-state loop);
// the flat `run_flat_*` harness below is a model-capability baseline
// only. Tier 0 lived on the flat path until 2026-07-12, which is exactly
// how the "ola" doom loop shipped green: the flat prompt answered
// greetings directly while the production prompt confabulated a plan.
// Conversational baselines therefore MUST run through
// `run_planned_conversation`/`run_planned_task`, never `run_flat_*`.

#[tokio::test]
#[ignore]
async fn tier0_multi_turn_recalls_user_name() {
    let model = installed_model_path();
    // Spawn the real server FIRST -- it gates on the sidecar binary + model
    // GGUF being present and SKIPS (returns None) with a printed reason when
    // either is absent, so a bare `--ignored` run without the onboarding
    // model no-ops instead of panicking. Generation goes to
    // `server.base_url`; token counting is a pure chars/4 estimate, no
    // in-process model load.
    let Some(server) = common::TestServer::spawn(&model).await else {
        return;
    };
    let dir = tempdir().unwrap();

    let runs = run_planned_conversation(
        &server.base_url,
        &["Hi! My name is Heitor.", "What is my name?"],
        dir.path(),
        8,
    )
    .await;
    report("tier0_name_turn1", &runs[0]);
    report("tier0_name_turn2", &runs[1]);

    for run in &runs {
        assert!(
            !run.trace.iter().any(|t| t.contains("tool=CreatePlan")),
            "small talk must not create a plan; got trace: {:?}",
            run.trace
        );
    }
    runs[0]
        .result
        .as_ref()
        .expect("first turn must always succeed");
    let answer = runs[1]
        .result
        .as_ref()
        .expect("second turn must always succeed");
    assert!(
        answer.contains("Heitor"),
        "expected the model to recall the name from the first turn, got: {answer:?}"
    );
}

// --- Tier 0 (plan machine): conversational baselines through the
// PRODUCTION prompt+state machine. The flat tier-0 tests above never
// exercised what ships: on 2026-07-12 a bare "ola" through the plan host
// confabulated a "fix the syntax error in main.py" plan and looped to the
// 200-turn cap. These pin the triage behavior on the real path. ---

#[tokio::test]
#[ignore]
async fn tier0_plan_greeting_answers_directly_without_planning() {
    let model = installed_model_path();
    // Spawn the real server FIRST -- it gates on the sidecar binary + model
    // GGUF being present and SKIPS (returns None) with a printed reason when
    // either is absent, so a bare `--ignored` run without the onboarding
    // model no-ops instead of panicking. Generation goes to
    // `server.base_url`; token counting is a pure chars/4 estimate, no
    // in-process model load.
    let Some(server) = common::TestServer::spawn(&model).await else {
        return;
    };
    let dir = tempdir().unwrap();

    for greeting in ["ola", "Hello!"] {
        let run = run_planned_task(&server.base_url, greeting, dir.path(), 8).await;
        report(&format!("tier0_plan_greeting({greeting})"), &run);

        assert!(
            !run.trace.iter().any(|t| t.contains("tool=CreatePlan")),
            "a greeting must never create a plan; got trace: {:?}",
            run.trace
        );
        let answer = run.result.expect("a greeting must produce a direct answer");
        assert!(
            !answer.trim().is_empty(),
            "expected a non-empty greeting reply"
        );
    }
}

#[tokio::test]
#[ignore]
async fn tier0_plan_vague_request_asks_before_planning() {
    let model = installed_model_path();
    // Spawn the real server FIRST -- it gates on the sidecar binary + model
    // GGUF being present and SKIPS (returns None) with a printed reason when
    // either is absent, so a bare `--ignored` run without the onboarding
    // model no-ops instead of panicking. Generation goes to
    // `server.base_url`; token counting is a pure chars/4 estimate, no
    // in-process model load.
    let Some(server) = common::TestServer::spawn(&model).await else {
        return;
    };
    let dir = tempdir().unwrap();

    let run = run_planned_task(
        &server.base_url,
        "something is broken, please fix it",
        dir.path(),
        24,
    )
    .await;
    report("tier0_plan_vague", &run);

    // Read/Grep/Glob before deciding is fine (assessment); the first
    // COMMITTING move must be a question, not a plan invented around
    // files the user never named.
    let first_decision = run
        .trace
        .iter()
        .find(|t| {
            t.contains("tool=CreatePlan")
                || t.contains("tool=AskUserQuestion")
                || t.contains("tool=FinishTask")
        })
        .cloned();
    assert!(
        first_decision
            .as_deref()
            .is_some_and(|t| t.contains("tool=AskUserQuestion")),
        "a vague request must be clarified before any plan; first decision: {first_decision:?}, full trace: {:?}",
        run.trace
    );
}

// --- Tier 1: single tool call (baseline sanity) ---

#[tokio::test]
#[ignore]
async fn tier1_single_tool_call_reads_a_known_file() {
    let model = installed_model_path();
    // Spawn the real server FIRST -- it gates on the sidecar binary + model
    // GGUF being present and SKIPS (returns None) with a printed reason when
    // either is absent, so a bare `--ignored` run without the onboarding
    // model no-ops instead of panicking. Generation goes to
    // `server.base_url`; token counting is a pure chars/4 estimate, no
    // in-process model load.
    let Some(server) = common::TestServer::spawn(&model).await else {
        return;
    };
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("config.txt"), "hello=world\nsecond=line\n").unwrap();

    let run = run_flat_task(
        &server.base_url,
        "This directory has a file named config.txt. What's on its first line? \
         Answer with just the line's content, nothing else.",
        dir.path(),
        AgentContext::top_level().max_turns,
    )
    .await;
    report("tier1", &run);

    let answer = run.result.expect("tier 1 must always succeed");
    assert!(
        answer.contains("hello=world"),
        "expected the first line's content in the answer, got: {answer:?}"
    );
}

/// Fast smoke test for the single two-state loop itself (CreatePlan/
/// ResumeExecution/StepDone state transitions, per-state prompt
/// construction) on the smallest possible task, before trusting it on
/// tier 4's much longer, slower run.
#[tokio::test]
#[ignore]
async fn tier1_planned_single_tool_call_reads_a_known_file() {
    let model = installed_model_path();
    // Spawn the real server FIRST -- it gates on the sidecar binary + model
    // GGUF being present and SKIPS (returns None) with a printed reason when
    // either is absent, so a bare `--ignored` run without the onboarding
    // model no-ops instead of panicking. Generation goes to
    // `server.base_url`; token counting is a pure chars/4 estimate, no
    // in-process model load.
    let Some(server) = common::TestServer::spawn(&model).await else {
        return;
    };
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("config.txt"), "hello=world\nsecond=line\n").unwrap();

    let run = run_planned_task(
        &server.base_url,
        "This directory has a file named config.txt. What's on its first line? \
         Answer with just the line's content, nothing else.",
        dir.path(),
        15,
    )
    .await;
    report("tier1_planned", &run);

    let answer = run.result.expect("planned tier 1 must always succeed");
    assert!(
        answer.contains("hello=world"),
        "expected the first line's content in the answer, got: {answer:?}"
    );
}

// --- Tier 2: a few tool calls (2-4) ---

#[tokio::test]
#[ignore]
async fn tier2_few_tool_calls_finds_todo_files() {
    let model = installed_model_path();
    // Spawn the real server FIRST -- it gates on the sidecar binary + model
    // GGUF being present and SKIPS (returns None) with a printed reason when
    // either is absent, so a bare `--ignored` run without the onboarding
    // model no-ops instead of panicking. Generation goes to
    // `server.base_url`; token counting is a pure chars/4 estimate, no
    // in-process model load.
    let Some(server) = common::TestServer::spawn(&model).await else {
        return;
    };
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("a.rs"), "// TODO: fix this\nfn a() {}\n").unwrap();
    std::fs::write(dir.path().join("b.rs"), "fn b() {}\n").unwrap();
    std::fs::write(dir.path().join("c.rs"), "// TODO: refactor\nfn c() {}\n").unwrap();
    std::fs::write(dir.path().join("d.rs"), "fn d() {}\n").unwrap();
    std::fs::write(dir.path().join("e.rs"), "// TODO: cleanup\nfn e() {}\n").unwrap();
    std::fs::write(dir.path().join("f.rs"), "fn f() {}\n").unwrap();

    let run = run_flat_task(
        &server.base_url,
        "List every .rs file in this directory that contains the string TODO, \
         and tell me how many there are.",
        dir.path(),
        AgentContext::top_level().max_turns,
    )
    .await;
    report("tier2", &run);

    let answer = run.result.expect("tier 2 must always succeed");
    for expected in ["a.rs", "c.rs", "e.rs"] {
        assert!(
            answer.contains(expected),
            "expected {expected} to be named in the answer, got: {answer:?}"
        );
    }
    for unexpected in ["b.rs", "d.rs", "f.rs"] {
        assert!(
            !answer.contains(unexpected),
            "did not expect {unexpected} (no TODO) to be named, got: {answer:?}"
        );
    }
}

// --- Tier 3: genuinely multi-step, deliberately hard today ---

fn tier3_fixture(dir: &Path) {
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"tier3-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\npath = \"src/lib.rs\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("src/lib.rs"),
        "pub mod widget;\npub mod site_a;\npub mod site_b;\npub mod site_c;\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/widget.rs"),
        "pub struct Widget {\n    pub id: u32,\n    pub name: String,\n}\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/site_a.rs"),
        "use crate::widget::Widget;\n\npub fn make_one() -> Widget {\n    Widget { id: 1, name: \"one\".to_string() }\n}\n\npub fn make_two() -> Widget {\n    Widget { id: 2, name: \"two\".to_string() }\n}\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/site_b.rs"),
        "use crate::widget::Widget;\n\npub fn make_three() -> Widget {\n    Widget { id: 3, name: \"three\".to_string() }\n}\n\npub fn make_four() -> Widget {\n    Widget { id: 4, name: \"four\".to_string() }\n}\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/site_c.rs"),
        "use crate::widget::Widget;\n\npub fn make_five() -> Widget {\n    Widget { id: 5, name: \"five\".to_string() }\n}\n\npub fn make_six() -> Widget {\n    Widget { id: 6, name: \"six\".to_string() }\n}\n",
    )
    .unwrap();
}

#[tokio::test]
#[ignore]
async fn tier3_multi_step_refactor_adds_a_field_and_updates_call_sites() {
    let model = installed_model_path();
    // Spawn the real server FIRST -- it gates on the sidecar binary + model
    // GGUF being present and SKIPS (returns None) with a printed reason when
    // either is absent, so a bare `--ignored` run without the onboarding
    // model no-ops instead of panicking. Generation goes to
    // `server.base_url`; token counting is a pure chars/4 estimate, no
    // in-process model load.
    let Some(server) = common::TestServer::spawn(&model).await else {
        return;
    };
    let dir = tempdir().unwrap();
    tier3_fixture(dir.path());

    let run = run_flat_task(
        &server.base_url,
        "Add a `created_at: String` field to the `Widget` struct defined in \
         src/widget.rs. Then update every place in this crate (the files under \
         src/) that constructs a `Widget` using struct-literal syntax so it also \
         sets `created_at`, for example to `String::new()`. When you are done, \
         the crate must compile with `cargo build`.",
        dir.path(),
        AgentContext::top_level().max_turns,
    )
    .await;
    report("tier3", &run);

    // Ground truth is independent of the agent's own claim: does the crate
    // actually compile. --offline guards against a network stall in a
    // sandboxed environment; there are no external dependencies to fetch.
    let build = std::process::Command::new("cargo")
        .args(["build", "--offline"])
        .current_dir(dir.path())
        .output()
        .expect("failed to invoke cargo build");

    let stderr_tail = String::from_utf8_lossy(&build.stderr)
        .lines()
        .rev()
        .take(15)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join(" | ");
    assert!(
        build.status.success(),
        "[tier3] crate must compile after the refactor; cargo build stderr tail: {stderr_tail}"
    );
}

// --- Tier 4: long-running, many discrete units (the real convergence stress test) ---

const TIER4_BUG_COUNT: usize = 20;

fn tier4_fixture(dir: &Path) {
    for i in 0..TIER4_BUG_COUNT {
        let a = i as i32;
        let b = (i + 1) as i32;
        let content = format!(
            "// BUG: this should compute a + b, but uses the wrong operator\nlet result = {a} - {b};\n"
        );
        std::fs::write(dir.join(format!("bug_{i:02}.txt")), content).unwrap();
    }
}

/// Per-file grading for tier 4: fixed = the `// BUG:` marker is gone AND
/// the corrected line is present. Returns (fixed_count, total, failures)
/// where each failure names the file and which criterion failed — a 0/20
/// where every operator was actually fixed but the comments remained
/// (observed for real) must be diagnosable from the output alone.
fn tier4_score(dir: &Path) -> (usize, usize, Vec<String>) {
    let mut fixed = 0;
    let mut failures = Vec::new();
    for i in 0..TIER4_BUG_COUNT {
        let a = i as i32;
        let b = (i + 1) as i32;
        let path = dir.join(format!("bug_{i:02}.txt"));
        let content = std::fs::read_to_string(&path).unwrap_or_default();
        let marker_gone = !content.contains("// BUG:");
        let fixed_line_present = content.contains(&format!("let result = {a} + {b};"));
        if marker_gone && fixed_line_present {
            fixed += 1;
        } else {
            failures.push(format!(
                "bug_{i:02}: {}{}",
                if marker_gone {
                    ""
                } else {
                    "marker still present; "
                },
                if fixed_line_present {
                    ""
                } else {
                    "fixed line missing"
                }
            ));
        }
    }
    (fixed, TIER4_BUG_COUNT, failures)
}

#[tokio::test]
#[ignore]
async fn tier4_long_running_fixes_many_scattered_bugs() {
    let model = installed_model_path();
    // Spawn the real server FIRST -- it gates on the sidecar binary + model
    // GGUF being present and SKIPS (returns None) with a printed reason when
    // either is absent, so a bare `--ignored` run without the onboarding
    // model no-ops instead of panicking. Generation goes to
    // `server.base_url`; token counting is a pure chars/4 estimate, no
    // in-process model load.
    let Some(server) = common::TestServer::spawn(&model).await else {
        return;
    };
    let dir = tempdir().unwrap();
    tier4_fixture(dir.path());

    let task = format!(
        "This directory contains {TIER4_BUG_COUNT} files named bug_00.txt through \
         bug_{:02}.txt. Each one has a single bug marked by a `// BUG:` comment \
         directly above the buggy line, describing what the line should actually \
         do. Go through every file, fix the bug so the line matches what the \
         comment says, then remove the `// BUG:` comment line entirely. Do this \
         for all {TIER4_BUG_COUNT} files before giving your final answer.",
        TIER4_BUG_COUNT - 1
    );

    let run = run_flat_task(
        &server.base_url,
        &task,
        dir.path(),
        AgentContext::top_level().max_turns,
    )
    .await;
    report("tier4", &run);

    let (fixed, total, failures) = tier4_score(dir.path());
    for f in &failures {
        println!("  [tier4] {f}");
    }
    println!(
        "[metrics] score={fixed}/{total} turns={} elapsed_s={:.1} seed={}",
        run.turns_taken,
        run.elapsed.as_secs_f32(),
        std::env::var("DOCE_GEN_SEED").unwrap_or_else(|_| "entropy".into())
    );
    assert_eq!(
        fixed, total,
        "[tier4] every bug must be fixed; failures: {failures:?}"
    );
}

/// Same task as `tier4_long_running_fixes_many_scattered_bugs`, run through
/// the two-stage plan loop instead of one flat `run_loop` call --
/// directly comparable against that test's 0/20 result (with the model
/// confidently claiming full success despite never once removing the
/// `// BUG:` marker) to see whether independent per-step verification
/// (real plan tools + the ability to re-check a file itself) actually
/// catches what a single flat run does not.
#[tokio::test]
#[ignore]
async fn tier4_planned_long_running_fixes_many_scattered_bugs() {
    let model = installed_model_path();
    // Spawn the real server FIRST -- it gates on the sidecar binary + model
    // GGUF being present and SKIPS (returns None) with a printed reason when
    // either is absent, so a bare `--ignored` run without the onboarding
    // model no-ops instead of panicking. Generation goes to
    // `server.base_url`; token counting is a pure chars/4 estimate, no
    // in-process model load.
    let Some(server) = common::TestServer::spawn(&model).await else {
        return;
    };
    let dir = tempdir().unwrap();
    tier4_fixture(dir.path());

    let task = format!(
        "This directory contains {TIER4_BUG_COUNT} files named bug_00.txt through \
         bug_{:02}.txt. Each one has a single bug marked by a `// BUG:` comment \
         directly above the buggy line, describing what the line should actually \
         do. Go through every file, fix the bug so the line matches what the \
         comment says, then remove the `// BUG:` comment line entirely. Do this \
         for all {TIER4_BUG_COUNT} files before giving your final answer.",
        TIER4_BUG_COUNT - 1
    );

    // Generous and shared across the whole task (one budget, not a
    // separate one per step): CreatePlan + 20 x (a few tool calls per
    // file + StepDone) + occasional independent verification + final
    // review, matching production's own top-level cap (200).
    let run = run_planned_task(&server.base_url, &task, dir.path(), 150).await;
    report("tier4_planned", &run);

    let (fixed, total, failures) = tier4_score(dir.path());
    for f in &failures {
        println!("  [tier4_planned] {f}");
    }
    println!(
        "[metrics] score={fixed}/{total} turns={} elapsed_s={:.1} seed={}",
        run.turns_taken,
        run.elapsed.as_secs_f32(),
        std::env::var("DOCE_GEN_SEED").unwrap_or_else(|_| "entropy".into())
    );
    assert_eq!(
        fixed, total,
        "[tier4_planned] every bug must be fixed; failures: {failures:?}"
    );
}

// --- Tier 5: surgical edit inside one huge file ---
//
// Distinct failure mode from tier 4's "many small files": here there's only
// one file, but it's big enough (~3000 lines, well over what fits in an 8K
// context read in full) that a naive "Read the whole thing, then Write the
// whole thing back" approach either doesn't fit the budget or risks
// silently corrupting/dropping unrelated content on the way back out.
// Passing requires finding the one target line (via Grep or an offset-
// limited Read) and editing it surgically, leaving every other line
// untouched.

const TIER5_LINE_COUNT: usize = 3000;
const TIER5_TARGET_LINE: usize = 1500;

fn tier5_fixture(dir: &Path) -> Vec<String> {
    let mut lines = Vec::with_capacity(TIER5_LINE_COUNT);
    for i in 0..TIER5_LINE_COUNT {
        if i == TIER5_TARGET_LINE {
            lines.push("TARGET: the answer is wrong".to_string());
        } else {
            lines.push(format!("line {i:04}: filler content for padding purposes"));
        }
    }
    std::fs::write(dir.join("big.txt"), lines.join("\n") + "\n").unwrap();
    lines
}

/// Verifies the target line was fixed exactly and every other line is
/// byte-identical to the original -- a partial credit score isn't
/// meaningful here (there's exactly one thing to get right), but silent
/// corruption of unrelated lines is a distinct, separately-worth-knowing
/// failure from just "didn't find the target."
fn tier5_check(dir: &Path, original: &[String]) -> Result<(), String> {
    let content = std::fs::read_to_string(dir.join("big.txt")).map_err(|e| e.to_string())?;
    let lines: Vec<&str> = content.lines().collect();

    if lines.len() != original.len() {
        return Err(format!(
            "line count changed: expected {}, got {}",
            original.len(),
            lines.len()
        ));
    }
    if lines[TIER5_TARGET_LINE] != "TARGET: the answer is correct" {
        return Err(format!(
            "target line not fixed as expected, got: {:?}",
            lines[TIER5_TARGET_LINE]
        ));
    }
    for (i, (got, want)) in lines.iter().zip(original.iter()).enumerate() {
        if i == TIER5_TARGET_LINE {
            continue;
        }
        if got != want {
            return Err(format!(
                "unrelated line {i} was altered: expected {want:?}, got {got:?}"
            ));
        }
    }
    Ok(())
}

#[tokio::test]
#[ignore]
async fn tier5_surgical_edit_in_one_huge_file() {
    let model = installed_model_path();
    // Spawn the real server FIRST -- it gates on the sidecar binary + model
    // GGUF being present and SKIPS (returns None) with a printed reason when
    // either is absent, so a bare `--ignored` run without the onboarding
    // model no-ops instead of panicking. Generation goes to
    // `server.base_url`; token counting is a pure chars/4 estimate, no
    // in-process model load.
    let Some(server) = common::TestServer::spawn(&model).await else {
        return;
    };
    let dir = tempdir().unwrap();
    let original = tier5_fixture(dir.path());

    let run = run_flat_task(
        &server.base_url,
        "The file big.txt in this directory has exactly one line containing the \
         word TARGET, somewhere among 3000 lines. Find that line and change it so \
         it reads exactly: TARGET: the answer is correct -- leave every other \
         line in the file completely unchanged.",
        dir.path(),
        AgentContext::top_level().max_turns,
    )
    .await;
    report("tier5", &run);

    if let Err(e) = tier5_check(dir.path(), &original) {
        panic!("[tier5] target must be fixed with the rest of the file untouched: {e}");
    }
}
