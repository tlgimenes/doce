//! 010-context-window-management: a real-model smoke suite, `#[ignore]`d by
//! default (it needs an actual installed GGUF + the built `llama-server`
//! sidecar on this machine, unlike every other test in this codebase, which
//! are all model-free per `context/mod.rs`'s own doc comment on why
//! `compute_usage`/`maybe_compact` aren't unit-tested). Run explicitly via:
//!   cargo test --test real_model_smoke -- --ignored --nocapture
//!
//! The llama-server cutover moved generation off the in-process
//! `InferenceEngine` onto the HTTP client (`inference::http`), so the
//! generation smokes here now spawn a REAL `llama-server` (via
//! `common::TestServer`) and POST at it through `LlamaServerClient::chat` --
//! the exact path production's `RealBackend`/`SubagentBackend` use --
//! rather than driving the in-process engine. The pure-tokenizer /
//! chat-template smokes still exercise the engine directly (token counting
//! and `render_chat_prompt` stay in-process until Task 5.1 strips the engine
//! to a vocab-only tokenizer).
//!
//! This is the closest thing to a live manual QA pass this environment can
//! do without a way to drive/inspect the native Tauri window directly.

mod common;

use doce_lib::agent::{dispatch, run_loop, AgentBackend, AgentContext, ToolCall, ToolExecution};
use doce_lib::context::{self, ContextSettings};
use doce_lib::inference::http::{
    to_openai_messages, tool_choice_for, tools_array, ChatRequest, LlamaServerClient,
};
use doce_lib::inference::{ChatMessage, InferenceEngine, ToolCallMode};
use doce_lib::storage::conversations::HistoryMessage;
use std::path::PathBuf;

/// The exact production prompt for the model under test — the same helper
/// the app itself seeds turns with (prompt drift between app and smoke test
/// is how the 2026-07-12 doom loop shipped green).
fn system_prompt(engine: &InferenceEngine) -> String {
    doce_lib::commands::agent::plan_system_message(None, true, None, engine.dialect())
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

#[test]
#[ignore]
fn count_tokens_and_context_window_report_sane_values_against_the_real_model() {
    let path = installed_model_path();
    assert!(
        path.exists(),
        "expected the real installed model at {path:?}"
    );

    // Pure tokenizer/context-window introspection — no generation, so this
    // stays on the in-process engine (Task 5.1 keeps exactly this much of it).
    let engine = InferenceEngine::load(&path, 4).expect("model should load");
    assert_eq!(
        engine.context_window(),
        doce_lib::inference::CONTEXT_WINDOW_TOKENS
    );

    let count = engine
        .count_tokens("Hello, how are you today?")
        .expect("tokenization should succeed");
    // A short English sentence should tokenize to a small handful of
    // tokens, nowhere near the budget -- a loose sanity bound, not an
    // exact-match assertion (exact counts are tokenizer-version-dependent).
    assert!(count > 0 && count < 30, "unexpected token count: {count}");
}

#[tokio::test]
#[ignore]
async fn a_real_short_completion_streams_from_the_server() {
    let model = installed_model_path();
    let Some(server) = common::TestServer::spawn(&model).await else {
        return; // sidecar binary or model GGUF absent -- skip (see TestServer)
    };

    let messages = vec![
        ChatMessage::system("You are a terse assistant. Answer in exactly one word."),
        ChatMessage::user("What is the capital of France? Reply with just the city name."),
    ];
    // No tools -> `Forbid` maps to (tools: None, tool_choice: None): a plain
    // free-text completion, the cutover equivalent of the old
    // `engine.generate(.., ToolCallMode::Forbid, None, ..)`.
    let req = ChatRequest::build(
        "doce",
        to_openai_messages(&messages),
        None,
        tool_choice_for(ToolCallMode::Forbid).map(|s| s.to_string()),
    );
    let cancel = tokio_util::sync::CancellationToken::new();
    let mut streamed = String::new();
    let outcome = LlamaServerClient::new(server.base_url.clone())
        .chat(req, |piece| streamed.push_str(piece), &cancel)
        .await
        .expect("generation should succeed");

    println!(
        "real model output: text={:?} reasoning={:?}",
        outcome.text, outcome.reasoning
    );
    assert!(
        !outcome.text.trim().is_empty(),
        "expected a non-empty completion"
    );
    // `on_piece` receives BOTH content and reasoning deltas, so the streamed
    // text is a superset of `outcome.text` -- at minimum SOMETHING streamed
    // as the completion was produced.
    assert!(
        !streamed.is_empty(),
        "expected the on_piece callback to receive streamed deltas"
    );
}

#[tokio::test]
#[ignore]
async fn grammar_constrained_tool_call_produces_a_well_formed_tool_call_against_the_server() {
    // The actual point of this test: providing `tools` makes the server parse
    // the model's tool call into a STRUCTURED `ChatOutcome::tool_call` (tags
    // plus schema-valid JSON), which is exactly what replaced the old
    // free-text `<tool_call>` scraping `agent::parse_response` used to do.
    // `tool_choice:"required"` over a single-tool set forces exactly one
    // well-formed Bash call.
    let model = installed_model_path();
    let Some(server) = common::TestServer::spawn(&model).await else {
        return;
    };
    // Loaded only to seed the EXACT production system prompt for this model
    // (its dialect comes from the loaded GGUF, same as the app).
    let engine = InferenceEngine::load(&model, 4).expect("model should load");

    let messages = vec![
        ChatMessage::system(system_prompt(&engine)),
        ChatMessage::user(
            "Use the Bash tool right now to run the command `pwd`. Call the tool, don't just describe it.",
        ),
    ];
    let req = ChatRequest::build(
        "doce",
        to_openai_messages(&messages),
        Some(tools_array(&["Bash"])),
        tool_choice_for(ToolCallMode::Require).map(|s| s.to_string()),
    );
    let cancel = tokio_util::sync::CancellationToken::new();
    let outcome = LlamaServerClient::new(server.base_url.clone())
        .chat(req, |_piece| {}, &cancel)
        .await
        .expect("generation should succeed");
    println!("real model tool-call output: {:?}", outcome.tool_call);

    let (name, args) = outcome
        .tool_call
        .expect("Require mode must yield a structured tool call, not plain text");
    assert_eq!(name, "Bash");
    assert!(
        args.get("command").and_then(|v| v.as_str()).is_some(),
        "expected a string `command` argument, got: {args:?}"
    );
}

#[test]
#[ignore]
fn tool_result_renders_wrapped_in_qwens_own_tool_response_tags() {
    // Verifies ChatMessage::tool_result's actual rendering against the
    // real model's chat template. First tried making the role itself "tool"
    // (on the theory that Qwen's chat template would apply its own
    // role=="tool" -> <tool_response> branch), but that didn't fire in
    // practice -- llama.cpp's template engine rendered an unrecognized
    // role as a bare, never-trained-on `<|im_start|>tool` block instead.
    // So the role stays "user" (reliably handled) and the *text* is
    // wrapped in the literal <tool_response> tags Qwen expects, with no
    // extra "Tool result for X:" framing. This exercises the in-process
    // `render_chat_prompt` (which stays until Task 5.1), NOT generation.
    let path = installed_model_path();
    let engine = InferenceEngine::load(&path, 4).expect("model should load");

    let messages = vec![
        ChatMessage::system(system_prompt(&engine)),
        ChatMessage::user("Run `pwd` using the Bash tool."),
        ChatMessage::tool_use("call-1", "Bash", serde_json::json!({"command": "pwd"})),
        ChatMessage::tool_result("call-1", "Bash", "/tmp/example"),
    ];
    let rendered = engine
        .render_chat_prompt(&messages)
        .expect("render should succeed");
    println!(
        "rendered prompt tail: {:?}",
        &rendered[rendered.len().saturating_sub(300)..]
    );

    assert!(
        rendered.contains("<|im_start|>user\n<tool_response>/tmp/example</tool_response>"),
        "expected the tool result wrapped in <tool_response> tags inside a `user` turn, \
         with no extra framing text, got: {rendered:?}"
    );
}

#[tokio::test]
#[ignore]
async fn apply_lightweight_clearing_then_summarize_against_the_server() {
    let model = installed_model_path();
    let Some(server) = common::TestServer::spawn(&model).await else {
        return;
    };

    // A synthetic history with more tool messages than TOOL_KEEP_N, plus
    // enough real turns that summarize_and_persist has non-protected
    // content to work with.
    let mut history: Vec<HistoryMessage> = Vec::new();
    for i in 0..12 {
        history.push(HistoryMessage {
            chat: ChatMessage::tool_result(
                format!("call-{i}"),
                "Bash",
                format!("output number {i}"),
            ),
            content_type: "tool_result".to_string(),
            sequence: i,
            plan: false,
            payload_ref: None,
        });
    }
    for i in 12..20 {
        history.push(HistoryMessage {
            chat: if i % 2 == 0 {
                ChatMessage::user(format!("User turn {i}"))
            } else {
                ChatMessage::assistant(format!("Assistant reply {i}"))
            },
            content_type: "text".to_string(),
            sequence: i,
            plan: false,
            payload_ref: None,
        });
    }

    let cleared = context::apply_lightweight_clearing(&mut history, 4, None);
    assert!(cleared > 0, "expected some tool messages to be cleared");

    // Real summarization call against the real SERVER -- the summarize path
    // (`context/mod.rs`) is the LAST in-process `engine.generate` caller and
    // flips to this same client in Task 5.1; this smoke proves the
    // prompt/generate path works end-to-end over HTTP. `Forbid` (no tools):
    // a summary must never be able to emit a tool call.
    let protected_recent = 4;
    let to_summarize = &history[..history.len() - protected_recent];
    let mut messages = vec![ChatMessage::system(
        "Summarize the conversation so far concisely, preserving key facts, decisions, and unresolved tasks. Respond with only the summary text, nothing else.",
    )];
    messages.extend(to_summarize.iter().map(|m| m.chat.clone()));
    let req = ChatRequest::build(
        "doce",
        to_openai_messages(&messages),
        None,
        tool_choice_for(ToolCallMode::Forbid).map(|s| s.to_string()),
    );
    let cancel = tokio_util::sync::CancellationToken::new();
    let summary = LlamaServerClient::new(server.base_url.clone())
        .chat(req, |_piece| {}, &cancel)
        .await
        .expect("summarization generate should succeed")
        .text;

    println!("real model summary: {summary:?}");
    assert!(!summary.trim().is_empty(), "expected a non-empty summary");

    // Sanity-check the settings defaults load correctly too (pure logic,
    // but exercised here alongside the real-model assertions for a single
    // combined smoke-test run).
    let settings = ContextSettings::from_raw(&Default::default());
    assert_eq!(
        settings.warn_threshold_pct,
        ContextSettings::DEFAULT_WARN_THRESHOLD_PCT
    );
}

/// A minimal `AgentBackend` for the one-tool-call real-server smoke below:
/// the flat (plan-less) loop with just the `Read` tool on the table,
/// generating through `LlamaServerClient::chat` (the cutover path).
/// `requires_tool_call() == false`, so once the model has read the file it
/// ends the loop with a plain-text final answer. Records whether a `Read`
/// call ever happened — the assertion the smoke turns on.
struct ReadSmokeBackend {
    base_url: String,
    cwd: PathBuf,
    saw_read: bool,
}

impl AgentBackend for ReadSmokeBackend {
    // The smoke's history is a handful of short messages, so it can never
    // approach the budget: measure 0 against a MAX threshold means compact
    // never runs.
    fn measure(&mut self, _messages: &[ChatMessage]) -> u32 {
        0
    }

    fn threshold(&self) -> u32 {
        u32::MAX
    }

    fn compact(&mut self, messages: &[ChatMessage]) -> Vec<ChatMessage> {
        messages.to_vec()
    }

    fn requires_tool_call(&self) -> bool {
        false
    }

    async fn generate(&mut self, messages: Vec<ChatMessage>) -> doce_lib::agent::TurnOutcome {
        let req = ChatRequest::build(
            "doce",
            to_openai_messages(&messages),
            Some(tools_array(&["Read"])),
            tool_choice_for(ToolCallMode::Allow).map(|s| s.to_string()),
        );
        let cancel = tokio_util::sync::CancellationToken::new();
        let result = LlamaServerClient::new(self.base_url.clone())
            .chat(req, |_piece| {}, &cancel)
            .await;
        common::chat_outcome_to_turn_outcome(result)
    }

    async fn execute_tool(&mut self, _tool_call_id: String, call: ToolCall) -> ToolExecution {
        if call.name == "Read" {
            self.saw_read = true;
        }
        let outcome = dispatch::execute(&call, Some(&self.cwd));
        ToolExecution::Result(outcome.model_text)
    }
}

#[tokio::test]
#[ignore]
async fn real_server_one_tool_call_reads_a_file_and_answers() {
    // End-to-end proof of the cutover path: a real server + the real agent
    // loop, one tool call, one file. The model must Read a file we plant in
    // a tempdir and echo its contents back -- exercising generate ->
    // structured tool_call -> dispatch -> tool_result -> final answer.
    let model = installed_model_path();
    let Some(server) = common::TestServer::spawn(&model).await else {
        return;
    };

    let dir = tempfile::tempdir().unwrap();
    let secret = "the launch code is orange-marmalade-42";
    std::fs::write(dir.path().join("notes.txt"), format!("{secret}\n")).unwrap();

    let mut backend = ReadSmokeBackend {
        base_url: server.base_url.clone(),
        cwd: dir.path().to_path_buf(),
        saw_read: false,
    };
    let context = AgentContext {
        is_subagent: false,
        max_turns: 12,
        cwd: Some(dir.path().to_path_buf()),
    };
    let messages = vec![
        ChatMessage::system(
            "You are a coding agent with a Read tool. Use it to read files from disk, \
             then answer the user in plain text.",
        ),
        ChatMessage::user(
            "Read the file notes.txt in the current directory and tell me exactly what it says.",
        ),
    ];

    let answer = run_loop(&context, messages, &mut backend)
        .await
        .expect("the smoke task must produce a final answer");
    println!("real-server smoke answer: {answer:?}");

    assert!(backend.saw_read, "expected the model to call the Read tool");
    assert!(
        answer.contains("orange-marmalade-42"),
        "expected the final answer to reference the file's content, got: {answer:?}"
    );
}
