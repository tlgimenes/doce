//! Agent task-completion tests -- the hard pass/fail regression suite for
//! the agent harness running against the real installed GGUF model. This
//! file started life as a print-and-compare benchmark for the 2026-07
//! context-management redesign; that comparison mission is over, and every
//! tier now asserts. A red tier is the definition of done for whatever
//! defect keeps it red, not a result to eyeball -- as of 2026-07-11 the
//! known tier-4 offenders are the reference-line doom loop and the
//! plan-nudge contradiction diagnosed in the last gate run (2/20).
//! `#[ignore]`d like `real_model_smoke.rs` (needs the real installed GGUF).
//! Run via (the `bench` feature is required -- see `doce_lib::bench`):
//!   cargo test --features bench --test agent_tasks -- --ignored --nocapture --test-threads=1
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

use doce_lib::agent::{run_loop, AgentContext, AgentError};
use doce_lib::bench::{
    stable_tool_call_id, stage_general_tool_result, FlatBackend, PlanExecBackend,
    StableToolCallIds, TestServer, FLAT_BASELINE_SYSTEM_PROMPT,
};
use doce_lib::context;
use doce_lib::inference::{ChatMessage, MessageContent};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tempfile::tempdir;

// --- Reproducibility: keeping the PROMPT BYTES fixed across runs ---
//
// `DOCE_GEN_SEED` pins the SAMPLER, not the prompt. Identical code at the
// same seed still swung 0/20 <-> 10/20 <-> 20/20 on tier 4, because two
// run-to-run-random values were reaching the wire and changing the token
// sequence the model conditioned on:
//
//   1. `tempfile::tempdir()` names its directory randomly (`.tmp6NgFCe`,
//      `.tmp7Bl64C`, ...). That absolute path is in `messages[0]` ("You are
//      currently working in the directory: ..." + the transcript pointer)
//      and in every Read/Grep/Glob result and payload reference line.
//      `ScratchDir` below replaces it with a name derived from the test.
//   2. `run_loop` stamps each tool call with `uuid::Uuid::now_v7()`
//      (timestamp + 74 random bits). It is serialized onto the wire TWICE
//      per call -- `tool_calls[0].id` on the assistant message and
//      `tool_call_id` on the tool message (`inference::http::
//      to_openai_message`) -- and it also names the payload file whose path
//      rides in a `-> Read "..."` reference line. `StableToolCallIds` below
//      maps it to a deterministic stand-in.
//
// Both stand-ins are deliberately the SAME LENGTH and SHAPE as what they
// replace (a 10-char `.tmpXXXXXX` sibling under `env::temp_dir()`; a 36-char
// v7-shaped UUID), so the benchmark's token counts -- and therefore its
// DIFFICULTY, where the whole point of tier 4 is context pressure -- are
// unchanged. This removes noise; it does not make the task easier.

/// A deterministic stand-in for `tempfile::tempdir()`: same parent
/// (`env::temp_dir()`) and same `.tmp` + 6 chars leaf shape, but the six
/// chars are a fixed per-test TAG instead of six random ones, so the
/// absolute path that ends up in the prompt is byte-identical run to run.
///
/// WIPED AND RECREATED on construction, never reused in place: every tier
/// asserts on file CONTENT (tier 4 grades all 20 files, tier 5 demands the
/// rest of the file be byte-identical), so a leftover from a previous run
/// inheriting into this one would silently corrupt the score -- a fixed path
/// is only safe BECAUSE of this wipe. Also removed on `Drop`, matching
/// `TempDir`'s own cleanup so nothing else about a run's footprint changes.
///
/// Tags must be unique per directory (see the call sites); they are what
/// keeps tiers from colliding. Uniqueness -- not `--test-threads=1` -- is
/// what makes that safe, so this does not depend on the real-model suite's
/// single-threading. Two CONCURRENT `cargo test` invocations of the same
/// tier on one machine would collide; don't do that.
struct ScratchDir {
    path: PathBuf,
}

impl ScratchDir {
    fn new(tag: &str) -> ScratchDir {
        assert_eq!(
            tag.len(),
            6,
            "scratch tags must be exactly 6 chars so the path length matches tempfile's own \
             `.tmp` + 6 -- a shorter or longer path would change the benchmark's token counts"
        );
        ScratchDir::wiped(std::env::temp_dir().join(format!(".tmp{tag}")))
    }

    /// The wipe every scratch dir gets, and the single reason a FIXED path is
    /// safe here at all. A failure to remove is fatal, not ignored: silently
    /// running a content-graded tier against last run's files is exactly the
    /// corruption this whole change exists to prevent.
    fn wiped(path: PathBuf) -> ScratchDir {
        // Unconditional -- an `if exists()` guard is a wipe that can be skipped.
        if let Err(e) = std::fs::remove_dir_all(&path) {
            assert!(
                e.kind() == std::io::ErrorKind::NotFound,
                "scratch dir {} must be wiped before a run, but could not be removed: {e}",
                path.display()
            );
        }
        std::fs::create_dir_all(&path).unwrap_or_else(|e| {
            panic!("scratch dir {} should create: {e}", path.display());
        });
        ScratchDir { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// The payload/transcript root for a run rooted at `cwd`: the same
/// `.tmpTAG` -> `.payTAG` rename under `env::temp_dir()`, so it is a
/// deterministic SIBLING of `cwd` (never nested inside it -- see
/// `FlatBackend::payload_dir`'s doc comment), unique per tier because `cwd`
/// is, and the same 10-char leaf length `tempdir()` produced.
fn payload_scratch(cwd: &Path) -> ScratchDir {
    let leaf = cwd
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_string();
    let tag = leaf.strip_prefix(".tmp").unwrap_or_else(|| {
        panic!("a benchmark cwd must be a `ScratchDir` (`.tmpTAG`), got {leaf:?}")
    });
    // `.pay` + the same 6-char tag: same length as `.tmp` + 6.
    ScratchDir::wiped(std::env::temp_dir().join(format!(".pay{tag}")))
}

/// Non-ignored guard on the id stand-in's two load-bearing properties: it is
/// a pure function of its index (reproducibility), and it is exactly as long
/// as the `Uuid::now_v7().to_string()` it replaces (unchanged difficulty).
#[test]
fn stable_tool_call_ids_are_deterministic_and_uuid_shaped() {
    assert_eq!(stable_tool_call_id(0), stable_tool_call_id(0));
    assert_ne!(stable_tool_call_id(0), stable_tool_call_id(1));
    let id = stable_tool_call_id(7);
    assert_eq!(id.len(), uuid::Uuid::now_v7().to_string().len());
    assert_eq!(
        id.split('-').map(str::len).collect::<Vec<_>>(),
        vec![8, 4, 4, 4, 12]
    );
    assert!(id.starts_with("0198f3a2-7b31-7"), "v7-shaped: {id}");

    let mut ids = StableToolCallIds::default();
    let mut messages = vec![
        ChatMessage::tool_use("real-a", "Read", serde_json::json!({})),
        ChatMessage::tool_result("real-a", "Read", "ok"),
    ];
    ids.rewrite(&mut messages);
    let (MessageContent::ToolUse { id, .. }, MessageContent::ToolResult { tool_use_id, .. }) =
        (&messages[0].content, &messages[1].content)
    else {
        panic!("shapes preserved");
    };
    assert_eq!(id, tool_use_id, "a call and its result must stay paired");
    assert_eq!(id, &stable_tool_call_id(0));
}

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
    /// How many times `compact()` actually trimmed the history middle during
    /// this user turn (a per-turn delta of the backend's cumulative counter,
    /// same accounting as `turns_taken`). tier6 reads this to prove the
    /// conversation genuinely crossed the trim budget; other tiers ignore it.
    compactions: u32,
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
    // 2026-07-09 payload-files design: a fresh scratch dir SIBLING to `cwd`
    // (never nested inside it -- see `FlatBackend::payload_dir`'s doc
    // comment), kept alive for this whole conversation by staying a local
    // here. Deterministically named off `cwd` (see `payload_scratch`): its
    // path reaches the prompt through every payload reference line, so a
    // random one re-rolled the prompt bytes every run.
    let payload_root = payload_scratch(cwd);
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
        stable_ids: StableToolCallIds::default(),
        compactions: 0,
    };

    let mut history = vec![ChatMessage::system(FLAT_BASELINE_SYSTEM_PROMPT)];
    let mut runs = Vec::new();
    for turn in user_turns {
        history.push(ChatMessage::user(*turn));
        let turns_before = backend.turns;
        let trace_before = backend.trace.len();
        let compactions_before = backend.compactions;
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
            compactions: backend.compactions - compactions_before,
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
    // 2026-07-09 payload-files design: a fresh scratch dir SIBLING to `cwd`
    // (never nested inside it -- see `FlatBackend::payload_dir`'s doc
    // comment), kept alive for this whole conversation by staying a local
    // here. Also hosts the transcript file, outside the workspace like
    // production's app-data dir. Deterministically named off `cwd` (see
    // `payload_scratch`): the transcript path is quoted verbatim into
    // `messages[0]`, so a random root put a fresh random string into the
    // stable prefix of every prompt this suite has ever sent.
    let payload_root = payload_scratch(cwd);
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
        stable_ids: StableToolCallIds::default(),
        compactions: 0,
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
        let compactions_before = backend.compactions;
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
            compactions: backend.compactions - compactions_before,
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
    let Some(server) = TestServer::spawn(&model).await else {
        return;
    };
    let dir = ScratchDir::new("T0RECL");

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
    let Some(server) = TestServer::spawn(&model).await else {
        return;
    };
    let dir = ScratchDir::new("T0GREE");

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
    let Some(server) = TestServer::spawn(&model).await else {
        return;
    };
    let dir = ScratchDir::new("T0VAGU");

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
    let Some(server) = TestServer::spawn(&model).await else {
        return;
    };
    let dir = ScratchDir::new("T1READ");
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
    let Some(server) = TestServer::spawn(&model).await else {
        return;
    };
    let dir = ScratchDir::new("T1PLAN");
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
    let Some(server) = TestServer::spawn(&model).await else {
        return;
    };
    let dir = ScratchDir::new("T2TODO");
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
    let Some(server) = TestServer::spawn(&model).await else {
        return;
    };
    let dir = ScratchDir::new("T3REFA");
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
    let Some(server) = TestServer::spawn(&model).await else {
        return;
    };
    let dir = ScratchDir::new("T4FLAT");
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
    let Some(server) = TestServer::spawn(&model).await else {
        return;
    };
    let dir = ScratchDir::new("T4PLAN");
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
    let Some(server) = TestServer::spawn(&model).await else {
        return;
    };
    let dir = ScratchDir::new("T5SURG");
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

// --- Tier 6: cross-context constraint retention (the ceiling-breaker) ---
//
// tier4 sits at a 20/20 ceiling: it detects regressions but has no headroom
// to guide prompt-engineering or context-management work, and -- like every
// other tier -- fits comfortably in the window. tier6 is the first tier
// whose task DELIBERATELY grows past the trim budget: it plants ONE global
// formatting rule ("prefix every written word with `RS-`") in the opening
// task prompt, then walks the agent through N files it must Read one at a
// time. Each Read result is carved out of payload-offloading
// (`stage_tool_result_for_persist`'s Read arm) and so accumulates inline,
// bounded only by Read's own ~8 KB cap -- so a handful of files is enough to
// push the conversation past `fit_turn_to_budget`'s budget, at which point
// `compact()` drops the MIDDLE of the history (it pins only `messages[0]`,
// the system prompt, and keeps the most-recent-that-fit). The original task
// prompt -- and with it the `RS-` rule -- is `messages[1]`, NOT pinned, so
// it scrolls out. Late files therefore test whether the early rule survived
// via the ONLY carriers left: the plan machine's regenerated todo-tail (if
// the model encoded the rule into a Todo item) or nothing at all.
//
// The scored failure is constraint-forgetting and NOTHING else: no
// arithmetic, no search, no cross-file reasoning, and -- deliberately -- no
// "delete the padding" sub-task. The only place any file's word appears is
// its `write here: <word>` line, so `tier6_score` reduces "did the rule
// survive for this file?" to a whole-file test for `RS-<word>` vs a bare
// `<word>` -- INTENTIONALLY tolerant of exactly how the model rewrote the
// line, so the score moves only with retention, never with task-following.
//
// 2026-07-16 EMPIRICAL NOTES (seed=42, qwen3.5-4b-q4_k_m), two confounders
// found and removed by real runs before the scorer measured retention
// cleanly:
//   1. An "replace the file's ENTIRE contents with `RS-<word>`" task while
//      padding the file scored 0/14 because the model rewrote only line 1
//      with `Update`'s old/new-string mode and left the padding -- the 0 was
//      "didn't delete padding", not "forgot the rule". (It wrote `RS-<word>`
//      on all 14 target lines.)
//   2. An exact-line matcher then still failed those same files because the
//      model kept the framing, producing `write here: RS-bravo` rather than
//      `RS-bravo`. Again the rule WAS honored; the exact match was measuring
//      task-following.
// Both times the model retained the rule on all 14 files across ~25
// compactions, because it spontaneously encoded "word -> RS-word" into its
// per-file Todo items, which the plan machine recites in the regenerated
// todo-tail EVERY turn -- so the rule rode the pinned-adjacent tail and
// never depended on the (long-since-trimmed) task prompt. That is the "plan
// machine already carries it" outcome the brief anticipated; see the
// pass-bar comment on the test for what that means for headroom.

/// A fixed, deterministic list of distinct target words -- one per file, in
/// order (`data_00` -> `alpha`, `data_01` -> `bravo`, ...). No randomness:
/// the same seed reproduces the same fixture byte-for-byte. Length caps
/// `TIER6_FILE_COUNT`.
const TIER6_WORDS: &[&str] = &[
    "alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel", "india", "juliet",
    "kilo", "lima", "mike", "november", "oscar", "papa", "quebec", "romeo", "sierra", "tango",
    "uniform", "victor", "whiskey", "xray", "yankee", "zulu",
];

/// How many files the agent must process. The pressure knob PAIR with
/// `TIER6_FILLER_LINES`: enough files that the conversation crosses the trim
/// budget WELL before the last few, so late files are genuinely written
/// after the early `RS-` rule could have been trimmed out of the
/// non-pinned window. Empirically tuned -- see the test's `[metrics]`/
/// compaction output. Must be <= `TIER6_WORDS.len()`.
const TIER6_FILE_COUNT: usize = 14;

/// Deterministic filler lines appended after each file's `write here:` line,
/// purely to enlarge that file's (inline, non-offloaded) Read result and so
/// speed the conversation toward the trim budget. Sized to keep each Read
/// output comfortably under `fs::READ_MAX_BYTES` (~8 KB) so no continue-from
/// marker is emitted and the whole file is seen in one Read. The target word
/// stays on line 1, so filler never obscures which word to write.
const TIER6_FILLER_LINES: usize = 60;

/// Writes `data_00.txt` .. `data_(n-1).txt`. Line 1 of each is
/// `write here: <word>`; the rest is fixed filler. Entirely deterministic.
fn tier6_fixture(dir: &Path, n: usize) {
    assert!(
        n <= TIER6_WORDS.len(),
        "tier6 needs a distinct word per file; {n} > {}",
        TIER6_WORDS.len()
    );
    for (i, word) in TIER6_WORDS.iter().take(n).enumerate() {
        let mut content = format!("write here: {word}\n");
        for line in 0..TIER6_FILLER_LINES {
            content.push_str(&format!(
                "# note {line:03}: deterministic padding, not part of what you write\n"
            ));
        }
        std::fs::write(dir.join(format!("data_{i:02}.txt")), content).unwrap();
    }
}

/// Per-file grading, mirroring `tier4_score`'s shape (returns
/// `(correct, total, failures)` with a diagnosable failure string per file).
///
/// Scores PURELY for rule retention, nothing else. The only place any
/// file's target word appears is its `write here: <word>` line, so the
/// question "did the `RS-` rule survive for this file?" reduces to a
/// whole-file substring test -- INTENTIONALLY tolerant of exactly HOW the
/// model rewrote the line, because that is task-following, not retention.
/// A run that scored 0/14 on an exact-line matcher while writing
/// `RS-<word>` on every target line (seen 2026-07-16: the model kept the
/// `write here:` prefix, producing `write here: RS-bravo`) was measuring the
/// confounder, not the constraint. Categories:
///   - CORRECT       : the file contains `RS-<word>` -- rule honored
///                     (whether as `RS-<word>` alone or `write here: RS-<word>`)
///   - `bare`        : no `RS-<word>`, but the file was edited to drop the
///                     `write here:` framing and contains `<word>` -- the
///                     rule was FORGOTTEN (the interesting late-file failure)
///   - `unprocessed` : no `RS-<word>`; the original `write here: <word>`
///                     line is still intact -- the file was never touched
///   - `empty`       : missing or blank
///   - `other`       : the word is gone entirely (wrong word / clobbered)
fn tier6_score(dir: &Path, n: usize) -> (usize, usize, Vec<String>) {
    let mut correct = 0;
    let mut failures = Vec::new();
    for (i, word) in TIER6_WORDS.iter().take(n).enumerate() {
        let path = dir.join(format!("data_{i:02}.txt"));
        let raw = std::fs::read_to_string(&path).unwrap_or_default();
        if raw.contains(&format!("RS-{word}")) {
            correct += 1;
        } else if raw.contains(&format!("write here: {word}")) {
            failures.push(format!(
                "data_{i:02}: unprocessed (still `write here: {word}`)"
            ));
        } else if raw.contains(word) {
            failures.push(format!(
                "data_{i:02}: bare (`{word}` present, RS- rule forgotten)"
            ));
        } else if raw.trim().is_empty() {
            failures.push(format!("data_{i:02}: empty (missing or blank)"));
        } else {
            failures.push(format!("data_{i:02}: other (`{word}` gone entirely)"));
        }
    }
    (correct, n, failures)
}

/// Pure-logic guard on `tier6_score` (no model): a run scoring a partial
/// must be diagnosable from the failure strings alone -- correct vs bare
/// (rule forgotten) vs empty vs other must each be recognized and
/// categorized. Runs under `cargo test --features bench --test agent_tasks`
/// without `--ignored`, so it does not need the real model.
#[test]
fn tier6_score_categorizes_each_failure_mode() {
    let dir = tempdir().unwrap();
    // Padding lines are irrelevant; scoring is a whole-file retention test:
    //   data_00 alpha   -> correct (`RS-alpha`, padding ignored)
    //   data_01 bravo   -> correct (`write here: RS-bravo`: rule honored,
    //                      `write here:` framing kept -- the real 2026-07-16
    //                      behavior the exact-line matcher wrongly failed)
    //   data_02 charlie -> bare (edited to `charlie`, RS- rule forgotten)
    //   data_03 delta   -> unprocessed (line still `write here: delta`)
    //   data_04 echo    -> empty (blank)
    //   data_05 foxtrot -> other (word clobbered entirely)
    std::fs::write(
        dir.path().join("data_00.txt"),
        "RS-alpha\n# note 000: padding\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("data_01.txt"),
        "write here: RS-bravo\n# note 000: padding\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("data_02.txt"), "charlie\n# pad\n").unwrap();
    std::fs::write(dir.path().join("data_03.txt"), "write here: delta\n# pad\n").unwrap();
    std::fs::write(dir.path().join("data_04.txt"), "   \n").unwrap();
    std::fs::write(dir.path().join("data_05.txt"), "clobbered\n").unwrap();

    let (correct, total, failures) = tier6_score(dir.path(), 6);
    assert_eq!(correct, 2, "data_00 and data_01 both honor the RS- rule");
    assert_eq!(total, 6);
    assert_eq!(failures.len(), 4);
    assert!(failures[0].contains("data_02") && failures[0].contains("bare"));
    assert!(failures[1].contains("data_03") && failures[1].contains("unprocessed"));
    assert!(failures[2].contains("data_04") && failures[2].contains("empty"));
    assert!(failures[3].contains("data_05") && failures[3].contains("other"));

    // A missing file is empty, not a panic.
    let (correct2, _, failures2) = tier6_score(dir.path(), 7);
    assert_eq!(correct2, 2);
    assert!(failures2
        .iter()
        .any(|f| f.contains("data_06") && f.contains("empty")));
}

/// The ceiling-breaker. Plants the `RS-` rule ONCE in the opening prompt,
/// then walks the model through `TIER6_FILE_COUNT` files whose accumulated
/// (inline, non-offloaded) Read results push the conversation past the trim
/// budget, so `compact()` drops the middle -- including the rule-bearing
/// task prompt -- and late files test retention. Runs on the PRODUCTION plan
/// path (`run_planned_task`), the same backend tier4_planned drives.
///
/// PASS BAR -- see the assertion at the bottom for the honest observed
/// baseline on `main` and the date it was measured. The bar is set to what
/// the model ACTUALLY achieves today, not N/N: a regression drops below it
/// and turns the tier red; an improvement (better context management or
/// prompt engineering carrying the rule further) rises above it, which is
/// the headroom tier4's 20/20 ceiling cannot provide.
#[tokio::test]
#[ignore]
async fn tier6_planned_retains_an_early_global_rule_across_context_pressure() {
    let model = installed_model_path();
    // Spawn the real server FIRST -- it gates on the sidecar binary + model
    // GGUF being present and SKIPS (returns None) with a printed reason when
    // either is absent, so a bare `--ignored` run without the onboarding
    // model no-ops instead of panicking.
    let Some(server) = TestServer::spawn(&model).await else {
        return;
    };
    let dir = ScratchDir::new("T6RETN");
    let n = TIER6_FILE_COUNT;
    tier6_fixture(dir.path(), n);

    let task = format!(
        "This directory contains {n} files named data_00.txt through data_{:02}.txt. \
         Process them IN ORDER, one at a time. Each file has a line `write here: <word>` \
         naming a single word; the remaining lines are padding you should leave \
         untouched. For each file, rewrite ONLY that `write here:` line so it becomes \
         the word by itself -- i.e. replace `write here: <word>` with just `<word>`. \
         GLOBAL RULE that applies to EVERY file: the word you write MUST be prefixed \
         with `RS-`. So the line `write here: alpha` must become exactly `RS-alpha`. \
         Do all {n} files before you finish.",
        n - 1
    );

    // Generous, shared across the whole task (one budget, not per-file):
    // a plan + N x (Read + Update + bookkeeping) + occasional re-checks,
    // matching tier4_planned's cap shape.
    let run = run_planned_task(&server.base_url, &task, dir.path(), 150).await;
    report("tier6_planned", &run);

    let (correct, total, failures) = tier6_score(dir.path(), n);
    for f in &failures {
        println!("  [tier6_planned] {f}");
    }
    println!(
        "[metrics] score={correct}/{total} compactions={} turns={} elapsed_s={:.1} seed={}",
        run.compactions,
        run.turns_taken,
        run.elapsed.as_secs_f32(),
        std::env::var("DOCE_GEN_SEED").unwrap_or_else(|_| "entropy".into())
    );

    // Load-bearing: the tier only measures cross-compaction retention if the
    // conversation ACTUALLY crossed the trim budget. If this is 0, the tier
    // tested nothing -- raise TIER6_FILE_COUNT / TIER6_FILLER_LINES.
    assert!(
        run.compactions > 0,
        "[tier6] compact() must fire for this tier to test anything; it fired 0 times -- \
         raise TIER6_FILE_COUNT/TIER6_FILLER_LINES to build more context pressure"
    );

    // OBSERVED BASELINE on `main` (qwen3.5-4b-q4_k_m), 2026-07-16, seed=42:
    // score=13/14, compactions=12, turns~55. The single miss is data_13 (the
    // LAST file): the model retained the `RS-` rule on all 13 files it touched
    // -- the plan machine's regenerated todo-tail carries it across all 12
    // compactions -- then declared the task done and never processed data_13.
    // So the headroom this tier exposes is NOT rule-forgetting (the plan
    // machine defeats that) but end-of-list completion tracking under context
    // pressure: a strictly harder target than tier4's 20/20 ceiling.
    //
    // DEEPER DIAGNOSIS (2026-07-16, full-trajectory capture): the dominant
    // cause is TODO-LIST DRIFT, not a weak finish bounce. After 12 compactions
    // the model's own Todo list mislabels which file is the gap -- at the
    // premature FinishTask it named `data_12` (already done, `RS-mike`) as
    // undone while `data_13` was the real gap. The `FinishTask` bounce was
    // hardened (it now NAMES undone items and no longer offers "remove them if
    // they no longer apply", an escape the model had exploited to mark undone
    // work done -- see agent::plan) -- a genuine quality fix, inert for tier4
    // (which never bounces at the gate seeds), but it does NOT move this score:
    // a correctly-worded bounce pointing at a drifted todo can't recover the
    // right file. Pushing 13->14 needs the todo bookkeeping itself to stay
    // grounded across compaction (a deeper, partly small-model-bound problem).
    //
    // Bar set to the observed floor (13) so the tier is GREEN now and a
    // REGRESSION -- the rule surviving fewer files, or more files dropped --
    // turns it red, while an improvement to 14/14 has room to show. See the
    // report for the full per-file breakdown and the determinism proof.
    const TIER6_PASS_BAR: usize = 13;
    assert!(
        correct >= TIER6_PASS_BAR,
        "[tier6] score {correct}/{total} regressed below the observed baseline \
         {TIER6_PASS_BAR}; failures: {failures:?}"
    );
}
