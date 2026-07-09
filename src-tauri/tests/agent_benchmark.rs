//! Agent task-completion benchmark, built to get an objective before/after
//! comparison when the context-management architecture changes (the
//! plan-and-execute + retrieval redesign under discussion) rather than
//! relying on intuition about whether a new design "feels" more convergent.
//! `#[ignore]`d like `real_model_smoke.rs` (needs the real installed GGUF).
//! Run via:
//!   cargo test --test agent_benchmark -- --ignored --nocapture --test-threads=1
//!
//! Calls `doce_lib::agent::run_loop` + `dispatch::execute` directly against
//! the real `InferenceEngine` -- the harness itself, no Tauri/UI involved,
//! matching how these tasks would actually run through `send_agent_message`.
//! Deliberately calls `context::fit_turn_to_budget` (the exact function
//! `commands::agent`'s real generate closure calls) rather than
//! reimplementing its own version of the pre-generate context-fit step --
//! this benchmark exists to test what actually ships, not a parallel
//! implementation that could quietly drift from it.
//!
//! Four tiers of increasing difficulty. Tiers 1-2 are baseline sanity --
//! they must always pass; a failure there means something is fundamentally
//! broken, not that an architecture failed to converge. Tiers 3-4 are
//! deliberately hard under today's pure-ReAct loop and are NOT hard-asserted
//! to pass -- they print a clear result/score instead, so the same test can
//! be re-run after an architecture change and the two runs compared by eye,
//! without editing the test to "unskip" an expected failure.
//!
//! Tier 4 in particular is graded (N of 20 fixed), not pass/fail: it's the
//! one that actually exercises whether the agent loses track of earlier
//! progress as a task runs long across many small, independent units of
//! work -- the exact failure mode motivating the redesign this benchmark
//! exists to validate.
//!
//! `_planned` variants (`tier1_planned_...`, `tier4_planned_...`) run the
//! same task through `run_planned_benchmark_task`'s two-stage loop
//! (`PlanningBackend`) instead of a single flat `run_loop` call, and are
//! directly comparable against their flat counterparts.

use doce_lib::agent::{dispatch, run_loop, AgentBackend, AgentContext, AgentError, SYSTEM_PROMPT};
use doce_lib::context;
use doce_lib::inference::{ChatMessage, InferenceEngine};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tempfile::tempdir;

fn installed_model_path() -> PathBuf {
    let home = std::env::var("HOME").expect("HOME must be set");
    PathBuf::from(home).join(
        "Library/Application Support/app.doce.desktop/models/qwen3-4b-instruct-2507-q4_k_m.gguf",
    )
}

struct BenchmarkRun {
    result: Result<String, AgentError>,
    turns_taken: u32,
    elapsed: Duration,
}

fn report(name: &str, run: &BenchmarkRun) {
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

/// `AgentBackend` for this benchmark -- the exact same shape
/// `commands::agent`'s `SubagentBackend` uses (measure/compact call
/// `context::fit_turn_to_budget`/its `InferenceEngine` counterparts
/// directly, not a benchmark-only reimplementation), plus a turn counter
/// `run_loop` itself has no reason to expose (it only reports turn count
/// on the `TurnCapExceeded` error path, not on success).
struct BenchBackend<'a> {
    engine: &'a InferenceEngine,
    cwd: &'a Path,
    threshold: u32,
    turns: u32,
    /// Every tool call + result this step actually made, in order -- the
    /// raw evidence `agent::plan::check_in` judges a step's completion
    /// against, instead of trusting the step's own final "Done" text (the
    /// exact thing tier 4 showed is not reliable on its own).
    trace: Vec<String>,
}

impl AgentBackend for BenchBackend<'_> {
    fn measure(&mut self, messages: &[ChatMessage]) -> u32 {
        self.engine
            .render_chat_prompt(messages)
            .and_then(|r| self.engine.count_tokens(&r).map(|n| n as u32))
            .unwrap_or(u32::MAX)
    }

    fn threshold(&self) -> u32 {
        self.threshold
    }

    fn compact(&mut self, messages: &[ChatMessage]) -> Vec<ChatMessage> {
        context::fit_turn_to_budget(self.engine, messages).unwrap_or_else(|_| messages.to_vec())
    }

    async fn generate(&mut self, messages: Vec<ChatMessage>) -> String {
        self.turns += 1;
        let rendered = self
            .engine
            .render_chat_prompt(&messages)
            .expect("chat template should render");
        // max_tokens matches commands::agent's real generate() call
        // (limits::AGENT_TURN_MAX_OUTPUT_TOKENS) for the same reason.
        self.engine
            .generate(
                &rendered,
                doce_lib::context::limits::AGENT_TURN_MAX_OUTPUT_TOKENS as i32,
                doce_lib::inference::ToolCallMode::Allow,
                |_| {},
                || false,
            )
            .unwrap_or_else(|e| format!("Error: generation failed: {e}"))
    }

    async fn execute_tool(
        &mut self,
        _tool_call_id: String,
        call: doce_lib::agent::ToolCall,
    ) -> doce_lib::agent::ToolExecution {
        let result = dispatch::execute(&call, Some(self.cwd)).model_text;
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

/// Runs `task` through the real agent harness rooted at `cwd`, capturing
/// turns taken and wall-clock time alongside the loop's own `Result`.
async fn run_benchmark_task(
    engine: &InferenceEngine,
    task: &str,
    cwd: &Path,
    max_turns: u32,
) -> BenchmarkRun {
    let context = AgentContext {
        is_subagent: false,
        max_turns,
        cwd: Some(cwd.to_path_buf()),
    };
    let initial_messages = vec![ChatMessage::system(SYSTEM_PROMPT), ChatMessage::user(task)];
    let start = Instant::now();

    // Same measure/threshold/compact commands::agent's real generate
    // closure uses -- run_loop itself now makes the fit-to-budget decision
    // on every turn, so this benchmark calls exactly what ships rather than
    // reimplementing its own version of the pre-generate step.
    let threshold = engine
        .context_window()
        .saturating_sub(doce_lib::context::limits::AGENT_TURN_MAX_OUTPUT_TOKENS);
    let mut backend = BenchBackend {
        engine,
        cwd,
        threshold,
        turns: 0,
        trace: Vec::new(),
    };

    let result = run_loop(&context, initial_messages, &mut backend).await;

    BenchmarkRun {
        result,
        turns_taken: backend.turns,
        elapsed: start.elapsed(),
    }
}

/// `AgentBackend` for the single two-state loop (`agent::plan::LoopState`):
/// one `run_loop` call, one continuous `messages` history. The state
/// machine and prompt-swap themselves live in `agent::plan::PlanState`
/// (embedded below as `plan_state`), shared with production
/// (`commands::agent::RealBackend`) -- this struct keeps only host
/// concerns: dispatching regular tool calls that pass through the plan
/// machine, the canned `AskUserQuestion` answer, the `Task` subagent, and
/// benchmark tracing. See `agent::plan`'s own doc comment for why the
/// two-state design replaced an earlier two-backend/recursive-`run_loop`
/// design.
struct PlanExecBackend<'a> {
    engine: &'a InferenceEngine,
    cwd: &'a Path,
    threshold: u32,
    turns: u32,
    plan_state: doce_lib::agent::plan::PlanState,
}

impl AgentBackend for PlanExecBackend<'_> {
    fn measure(&mut self, messages: &[ChatMessage]) -> u32 {
        self.engine
            .render_chat_prompt(messages)
            .and_then(|r| self.engine.count_tokens(&r).map(|n| n as u32))
            .unwrap_or(u32::MAX)
    }

    fn threshold(&self) -> u32 {
        self.threshold
    }

    fn compact(&mut self, messages: &[ChatMessage]) -> Vec<ChatMessage> {
        context::fit_turn_to_budget(self.engine, messages).unwrap_or_else(|_| messages.to_vec())
    }

    async fn generate(&mut self, mut messages: Vec<ChatMessage>) -> String {
        self.turns += 1;
        if let Some(first) = messages.first_mut() {
            // Same cwd suffix production's plan_system_message appends --
            // without it the model has no anchor for path arguments and
            // was observed globbing the filesystem root ("path": "/"),
            // concluding the task's files don't exist.
            *first = ChatMessage::system(format!(
                "{}

You are currently working in the directory: {}",
                self.plan_state.system_prompt(true),
                self.cwd.display()
            ));
        }
        // Recite the live plan at the context tail (Manus's recitation
        // trick) to keep the global plan inside the model's recent attention
        // span on long tasks -- the in-memory clone only, never persisted.
        if let Some(recitation) = self.plan_state.recitation_text() {
            messages.push(doce_lib::inference::ChatMessage::user(recitation));
        }

        let rendered = self
            .engine
            .render_chat_prompt(&messages)
            .expect("chat template should render");
        self.engine
            .generate(
                &rendered,
                doce_lib::context::limits::AGENT_TURN_MAX_OUTPUT_TOKENS as i32,
                doce_lib::inference::ToolCallMode::Require,
                |_| {},
                || false,
            )
            .unwrap_or_else(|e| format!("Error: generation failed: {e}"))
    }

    async fn execute_tool(
        &mut self,
        _tool_call_id: String,
        call: doce_lib::agent::ToolCall,
    ) -> doce_lib::agent::ToolExecution {
        let plan_finish: Option<String>;
        let result = if let Some(outcome) = self.plan_state.handle_plan_tool(&call) {
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
            "Error: no interactive user is available in this benchmark run -- proceed using your own best judgment".to_string()
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
                vec![ChatMessage::system(SYSTEM_PROMPT), ChatMessage::user(prompt)];
            let mut sub_backend = BenchBackend {
                engine: self.engine,
                cwd: self.cwd,
                threshold: self.threshold,
                turns: 0,
                trace: Vec::new(),
            };
            let sub_result = run_loop(&sub_context, sub_messages, &mut sub_backend).await;
            self.turns += sub_backend.turns;
            match sub_result {
                Ok(text) => text,
                Err(e) => format!("Error: subagent did not finish ({e})"),
            }
        } else {
            plan_finish = None;
            dispatch::execute(&call, Some(self.cwd)).model_text
        };

        let args_preview: String = call.arguments.to_string().chars().take(200).collect();
        let result_preview: String = result.chars().take(300).collect();
        println!(
            "  [{:?}] turn {} tool={} args={args_preview} -> {result_preview:?}",
            self.plan_state.state, self.turns, call.name
        );
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

/// Runs `task` through the single two-state loop (`PlanExecBackend`) --
/// one `run_loop` call, not two, and one continuous conversation shared by
/// planning and every step's execution.
async fn run_planned_benchmark_task(
    engine: &InferenceEngine,
    task: &str,
    cwd: &Path,
    max_plan_turns: u32,
) -> BenchmarkRun {
    use doce_lib::agent::plan::PLANNING_SYSTEM_PROMPT;

    let context = AgentContext {
        is_subagent: false,
        max_turns: max_plan_turns,
        cwd: Some(cwd.to_path_buf()),
    };
    let initial_messages = vec![
        ChatMessage::system(PLANNING_SYSTEM_PROMPT),
        ChatMessage::user(task),
    ];
    let start = Instant::now();

    let threshold = engine
        .context_window()
        .saturating_sub(doce_lib::context::limits::AGENT_TURN_MAX_OUTPUT_TOKENS);
    let mut backend = PlanExecBackend {
        engine,
        cwd,
        threshold,
        turns: 0,
        plan_state: doce_lib::agent::plan::PlanState::default(),
    };

    let result = run_loop(&context, initial_messages, &mut backend).await;
    report_plan("planned", &backend.plan_state.plan);

    BenchmarkRun {
        result,
        turns_taken: backend.turns,
        elapsed: start.elapsed(),
    }
}

// --- Tier 1: single tool call (baseline sanity) ---

#[tokio::test]
#[ignore]
async fn tier1_single_tool_call_reads_a_known_file() {
    let engine = InferenceEngine::load(&installed_model_path(), 4).expect("model should load");
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("config.txt"), "hello=world\nsecond=line\n").unwrap();

    let run = run_benchmark_task(
        &engine,
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
    let engine = InferenceEngine::load(&installed_model_path(), 4).expect("model should load");
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("config.txt"), "hello=world\nsecond=line\n").unwrap();

    let run = run_planned_benchmark_task(
        &engine,
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
    let engine = InferenceEngine::load(&installed_model_path(), 4).expect("model should load");
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("a.rs"), "// TODO: fix this\nfn a() {}\n").unwrap();
    std::fs::write(dir.path().join("b.rs"), "fn b() {}\n").unwrap();
    std::fs::write(dir.path().join("c.rs"), "// TODO: refactor\nfn c() {}\n").unwrap();
    std::fs::write(dir.path().join("d.rs"), "fn d() {}\n").unwrap();
    std::fs::write(dir.path().join("e.rs"), "// TODO: cleanup\nfn e() {}\n").unwrap();
    std::fs::write(dir.path().join("f.rs"), "fn f() {}\n").unwrap();

    let run = run_benchmark_task(
        &engine,
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
    let engine = InferenceEngine::load(&installed_model_path(), 4).expect("model should load");
    let dir = tempdir().unwrap();
    tier3_fixture(dir.path());

    let run = run_benchmark_task(
        &engine,
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

    println!(
        "[tier3] cargo build succeeded: {} (stderr tail: {})",
        build.status.success(),
        String::from_utf8_lossy(&build.stderr)
            .lines()
            .rev()
            .take(15)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join(" | ")
    );
    // Not hard-asserted (see module doc): today's architecture may
    // genuinely fail this. The printed line is what a before/after
    // comparison actually reads.
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
                if marker_gone { "" } else { "marker still present; " },
                if fixed_line_present { "" } else { "fixed line missing" }
            ));
        }
    }
    (fixed, TIER4_BUG_COUNT, failures)
}

#[tokio::test]
#[ignore]
async fn tier4_long_running_fixes_many_scattered_bugs() {
    let engine = InferenceEngine::load(&installed_model_path(), 4).expect("model should load");
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

    let run = run_benchmark_task(
        &engine,
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
    // Deliberately not hard-asserted -- this is the graded stress test
    // (see module doc). The printed score is the actual benchmark output.
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
    let engine = InferenceEngine::load(&installed_model_path(), 4).expect("model should load");
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
    let run = run_planned_benchmark_task(&engine, &task, dir.path(), 150).await;
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
    // Deliberately not hard-asserted, same reasoning as tier 4 itself.
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
    let engine = InferenceEngine::load(&installed_model_path(), 4).expect("model should load");
    let dir = tempdir().unwrap();
    let original = tier5_fixture(dir.path());

    let run = run_benchmark_task(
        &engine,
        "The file big.txt in this directory has exactly one line containing the \
         word TARGET, somewhere among 3000 lines. Find that line and change it so \
         it reads exactly: TARGET: the answer is correct -- leave every other \
         line in the file completely unchanged.",
        dir.path(),
        AgentContext::top_level().max_turns,
    )
    .await;
    report("tier5", &run);

    match tier5_check(dir.path(), &original) {
        Ok(()) => println!("[tier5] check: PASS -- target fixed, rest of file untouched"),
        Err(e) => println!("[tier5] check: FAIL -- {e}"),
    }
    // Not hard-asserted, same reasoning as tiers 3-4.
}
