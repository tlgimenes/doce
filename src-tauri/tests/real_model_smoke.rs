//! 010-context-window-management: a real-model smoke suite, `#[ignore]`d by
//! default (it needs an actual installed GGUF + the built `llama-server`
//! sidecar on this machine, unlike every other test in this codebase, which
//! are all model-free per `context/mod.rs`'s own doc comment on why
//! `compute_usage`/`maybe_compact` aren't unit-tested). Run explicitly via:
//!   cargo test --test real_model_smoke -- --ignored --nocapture
//!
//! The llama-server cutover moved generation onto the HTTP client
//! (`inference::http`), so the generation smokes here spawn a REAL
//! `llama-server` (via `doce_lib::bench::TestServer`) and POST at it through
//! `LlamaServerClient::chat` -- the exact path production's
//! `RealBackend`/`SubagentBackend` use. The in-process engine is gone
//! entirely: token counting is a pure chars/4 estimate
//! (`inference::token_estimate`, no model needed), and the tool dialect is
//! pinned to `HermesJson` (doce ships one Hermes model).
//!
//! This is the closest thing to a live manual QA pass this environment can
//! do without a way to drive/inspect the native Tauri window directly.

use doce_lib::agent::{dispatch, run_loop, AgentBackend, AgentContext, ToolCall, ToolExecution};
use doce_lib::context;
use doce_lib::inference::http::{
    to_openai_messages, tool_choice_for, tools_array, ChatRequest, LlamaServerClient,
};
use doce_lib::inference::{ChatMessage, ToolCallMode};
use doce_lib::storage::conversations::HistoryMessage;
use std::path::PathBuf;

/// The exact production prompt for the model under test — the same helper
/// the app itself seeds turns with (prompt drift between app and smoke test
/// is how the 2026-07-12 doom loop shipped green).
fn system_prompt() -> String {
    doce_lib::commands::agent::plan_system_message(None, true, None, None)
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
fn the_token_estimate_is_sane() {
    // The chars/4 estimate for a short English sentence is a small handful of
    // tokens, nowhere near the budget -- a loose sanity bound (no model
    // needed for the estimate itself; the in-process engine is gone).
    let count = doce_lib::inference::token_estimate("Hello, how are you today?");
    assert!(
        count > 0 && count < 30,
        "unexpected token estimate: {count}"
    );
}

#[tokio::test]
#[ignore]
async fn a_real_short_completion_streams_from_the_server() {
    let model = installed_model_path();
    let Some(server) = doce_lib::bench::TestServer::spawn(&model).await else {
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
    let Some(server) = doce_lib::bench::TestServer::spawn(&model).await else {
        return;
    };

    let messages = vec![
        ChatMessage::system(system_prompt()),
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

// (Removed `tool_result_renders_wrapped_in_qwens_own_tool_response_tags`:
// it exercised the in-process chat-prompt rendering, which is deleted now
// that the server renders the chat template from OpenAI `messages`. The
// equivalent rendering is covered by `inference::http::to_openai_messages`'s
// own unit tests plus the real-server smokes below.)

// (Removed `apply_lightweight_clearing_then_summarize_against_the_server`:
// it hand-rolled a summarization request instead of calling
// `summarize_and_persist`, and every byte of that copy had drifted from
// production. It sent a one-sentence system prompt retired at SP3 (`grep`
// found that string nowhere but in the test itself) rather than
// `limits::SUMMARIZATION_PROMPT`'s structured `<state_snapshot>` prompt; it
// summarized `&history[..len-4]` rather than `messages_to_summarize`'s span
// (which also drops the first genuine user message, keep-first); and it left
// `max_tokens` unset where production caps at `SUMMARY_MAX_TOKENS`. It never
// called `evaluate_summary` either, so it could not observe the screen that
// decides whether production persists anything at all. It asserted a
// non-empty string came back from a request production does not send -- which
// is how it passed green through the entire life of the trailing-assistant
// prefill bug it was supposedly demonstrating.
//
// It is deleted rather than rewritten because
// `the_real_model_summarizes_a_span_that_ends_with_an_assistant_message`
// below already drives the real `summarize_and_persist` over a realistic span
// that ends on an assistant message, and already asserts the echo guard plus
// the `<state_snapshot>`/`GOAL:` contract. A rewrite would have been that
// test again with a worse fixture: 12 rows of synthetic "output number {i}"
// filler, whose span is small enough that a structured snapshot could plausibly
// trip `evaluate_summary`'s RejectInflated guard -- a flaky duplicate costing a
// second real-model round-trip for no marginal signal.
//
// Its two non-summarization assertions needed no model and are already covered,
// model-free: `apply_lightweight_clearing` by `context`'s six inline clearing
// tests (which assert the exact placeholder text, and use the production
// `TOOL_KEEP_N` rather than this test's hardcoded 4), and
// `ContextSettings::from_raw`'s defaults by `context`'s own `from_raw` tests.)

/// Does a line look like the model ignored "no bullets, no numbering"? Only
/// the `- ` prefix is defensively stripped by the parser, so anything else
/// leaking through is a real contract violation. A fact that merely STARTS
/// with a number ("3 retries are allowed") is not a list item -- the marker
/// has to be a digit run followed by `.` or `)`.
fn looks_like_a_list_item(line: &str) -> bool {
    for marker in ["- ", "* ", "+ ", "• ", "– "] {
        if line.starts_with(marker) {
            return true;
        }
    }
    let digits: String = line.chars().take_while(|c| c.is_ascii_digit()).collect();
    !digits.is_empty() && line[digits.len()..].starts_with(['.', ')'])
}

/// Seeds the one workspace + conversation row `extract_and_persist_memories`
/// resolves through (`workspace_id_for_conversation` reads `conversations`,
/// and the FK needs the workspace to exist), mirroring `context::tests`' own
/// seeding against the same fully-migrated in-memory schema.
async fn seed_workspace_and_conversation(conn: &tokio_rusqlite::Connection) {
    conn.call(|conn: &mut rusqlite::Connection| {
        conn.execute(
            "INSERT INTO workspaces (id, path, display_name, created_at, last_opened_at) \
             VALUES ('w1', 'w1', 'Memory smoke workspace', 0, 0)",
            [],
        )?;
        conn.execute(
            "INSERT INTO conversations (id, workspace_id, spawned_by_conversation_id, title, \
             created_at, updated_at) VALUES ('c1', 'w1', NULL, 'Memory smoke', 0, 0)",
            [],
        )
    })
    .await
    .expect("seed workspace + conversation");
}

fn span_message(chat: ChatMessage, content_type: &str, sequence: i64) -> HistoryMessage {
    HistoryMessage {
        chat,
        content_type: content_type.to_string(),
        sequence,
        plan: false,
        payload_ref: None,
        tool_name: None,
    }
}

/// THE POINT OF THIS SUITE, applied to SP4's memory extraction: does the REAL
/// model actually OBEY `MEMORY_EXTRACTION_PROMPT`'s "one fact per line, no
/// commentary, no headers" contract?
///
/// Every other test of this pass stubs the llama-server and feeds the parser a
/// canned, already-well-formed fact list -- which asserts that IF the model
/// behaves, we parse it. That is an assumption about the model, and it is
/// exactly the assumption `is_plausible_fact`'s guards were written blind
/// against (the trailing-colon preamble rule exists because a 4B was OBSERVED
/// emitting "Here is the updated set of memories:"). Only a real model can
/// settle it, so this drives the REAL `extract_and_persist_memories` against a
/// REAL `llama-server` over a realistic span.
///
/// The assertions are the CONTRACT, never the wording: the model is
/// stochastic, so "did it remember the oxfmt preference" is not a test, but
/// "is everything it persisted fact-shaped" is. The `--nocapture` print of
/// every persisted fact is as much the deliverable as the asserts -- it is the
/// only way a human sees what the model actually produced, and whether the
/// facts are any GOOD (as opposed to merely well-formed) is a judgement this
/// test deliberately leaves to that reader.
#[tokio::test]
#[ignore]
async fn the_real_model_obeys_the_memory_extraction_contract() {
    let model = installed_model_path();
    let Some(server) = doce_lib::bench::TestServer::spawn(&model).await else {
        return; // sidecar binary or model GGUF absent -- skip (see TestServer)
    };

    let conn = doce_lib::storage::test_async_connection().await;
    seed_workspace_and_conversation(&conn).await;

    // A realistic span about to be condensed: two genuinely durable facts (a
    // stated user preference, a project constraint) buried in transient
    // chatter -- a test run's pass count, a tool result, and what the agent is
    // doing "right now" -- all of which the prompt explicitly says never to
    // remember. Both kinds are present because a span of pure facts would let
    // a model that just echoes everything pass.
    let span: Vec<HistoryMessage> = vec![
        span_message(
            ChatMessage::user(
                "Before we start: always run `cargo fmt` before you commit. Every time, \
                 without asking me first.",
            ),
            "text",
            0,
        ),
        span_message(
            ChatMessage::assistant("Understood -- I'll run `cargo fmt` before every commit."),
            "text",
            1,
        ),
        span_message(
            ChatMessage::user(
                "One hard constraint for this project: the Rust backend must never take a \
                 dependency on `tracing`. Use `eprintln!` for logging instead.",
            ),
            "text",
            2,
        ),
        span_message(
            ChatMessage::assistant(
                "Got it. I'll keep logging on `eprintln!` and won't add `tracing`.",
            ),
            "text",
            3,
        ),
        span_message(
            ChatMessage::user("Can you run the test suite now? I want to see where we are."),
            "text",
            4,
        ),
        span_message(
            ChatMessage::assistant("Running the suite now -- one moment."),
            "text",
            5,
        ),
        HistoryMessage {
            chat: ChatMessage::tool_result(
                "call-0".to_string(),
                "Bash",
                "test result: ok. 381 passed; 0 failed; 0 ignored; finished in 2.31s".to_string(),
            ),
            content_type: "tool_result".to_string(),
            sequence: 6,
            plan: false,
            payload_ref: None,
            tool_name: Some("Bash".to_string()),
        },
        span_message(
            ChatMessage::assistant("All 381 tests pass. I'm on the memory-extraction task next."),
            "text",
            7,
        ),
    ];
    let span_refs: Vec<&HistoryMessage> = span.iter().collect();

    context::extract_and_persist_memories(&conn, &server.base_url, "c1", &span_refs, 1_000)
        .await
        .expect("extraction must never fail the turn");

    let memories = doce_lib::storage::memories::load_memories(&conn, Some("w1"))
        .await
        .expect("load the persisted set");

    // THE DELIVERABLE: what the model actually emitted, for a human to read.
    println!(
        "\n=== real model: persisted memories ({}) ===",
        memories.len()
    );
    for (i, m) in memories.iter().enumerate() {
        println!("  [{i}] {:?}", m.content);
    }
    println!("=== end persisted memories ===\n");

    // An extraction that persisted nothing means the model refused the task,
    // emitted a preamble-dominated response, or blew the output cap -- all of
    // which the guards correctly reject, and all of which are contract
    // failures worth failing on rather than shrugging at.
    assert!(
        !memories.is_empty(),
        "the real model persisted NOTHING from a span containing a stated user preference and \
         an explicit project constraint -- it did not obey MEMORY_EXTRACTION_PROMPT"
    );

    for m in &memories {
        let fact = &m.content;
        // The real-model contract check: the guard written blind against this
        // model, finally pointed at it.
        assert!(
            context::is_plausible_fact(fact),
            "persisted fact is not fact-shaped: {fact:?}"
        );
        // Redundant with `is_plausible_fact`'s rule 1 by construction, and
        // asserted separately on purpose: "no preamble leaked through" is the
        // property, and it must keep being tested even if that rule is ever
        // loosened.
        assert!(
            !fact.ends_with(':'),
            "a preamble/header leaked through as a durable fact: {fact:?}"
        );
        assert!(
            !looks_like_a_list_item(fact),
            "the model emitted a list marker despite 'no bullets, no numbering': {fact:?}"
        );
    }

    // THE ASSERTION THAT ACTUALLY BITES, and the reason this test exists.
    //
    // Every check above passes on a well-formed SENTENCE, and the transcript
    // is full of well-formed sentences -- so a model that simply echoes a
    // message back at us satisfies all of them while extracting nothing. That
    // is not hypothetical: it is what this model does today (see the report at
    // .superpowers/sdd/sp4-real-model-test-report.md). `to_summarize` ends
    // with an assistant message, `extract_and_persist_memories` appends
    // nothing after it, and llama-server's chat template treats a trailing
    // assistant message as a PREFILL to continue -- so the model closes it out
    // immediately and the "extraction" comes back as that message, verbatim,
    // deterministically, which then persists as a durable memory forever.
    //
    // Safe for THIS span (nothing in it is a legitimate durable memory
    // verbatim -- every message is conversational), and robust to
    // stochasticity: it asserts a relationship to the input, never a wording.
    let span_texts: Vec<&str> = span
        .iter()
        .filter_map(|m| match &m.chat.content {
            doce_lib::inference::MessageContent::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect();
    for m in &memories {
        assert!(
            !span_texts.contains(&m.content.as_str()),
            "the model ECHOED a transcript message verbatim instead of extracting a durable \
             fact from it: {:?}\nThis is the trailing-assistant-message prefill bug: the \
             extraction never actually ran.",
            m.content
        );
    }
}

/// THE SAME QUESTION AS THE MEMORY TEST ABOVE, pointed at tier-2 compaction --
/// the project's core context-management feature, and until 2026-07-15 the
/// single largest untested surface in it (`context/mod.rs`'s own doc comment
/// admits `compute_usage`/`maybe_compact` are not unit-tested because they need
/// a live server, so `summarize_and_persist` had only ever been run against
/// HTTP stubs that fed it a canned, already-well-formed summary).
///
/// It was broken. `summarize_and_persist` built `[system(SUMMARIZATION_PROMPT)]
/// + span` and appended nothing, and `messages_to_summarize` returns an
/// arbitrary slice of history that routinely ENDS WITH AN ASSISTANT MESSAGE --
/// which llama-server's chat template treats as a prefill to CONTINUE. The
/// model closed out that sentence instead of answering the system prompt, and
/// because the echo is non-empty, un-truncated, and smaller than the span it
/// replaces, `evaluate_summary` ACCEPTED it. Compaction did not fail loudly; it
/// silently replaced the conversation's state with a continuation of its own
/// last sentence. This test pins the fix (`SUMMARIZATION_FINAL_TURN` +
/// `disable_thinking`) against the real model.
///
/// The span here deliberately ends on an assistant message, because that is the
/// production hazard. As with the memory smoke, the assertions are the CONTRACT
/// and never the wording -- the model is stochastic -- and the `--nocapture`
/// print of the real summary is as much the deliverable as the asserts.
#[tokio::test]
#[ignore]
async fn the_real_model_summarizes_a_span_that_ends_with_an_assistant_message() {
    let model = installed_model_path();
    let Some(server) = doce_lib::bench::TestServer::spawn(&model).await else {
        return; // sidecar binary or model GGUF absent -- skip (see TestServer)
    };

    let conn = doce_lib::storage::test_async_connection().await;
    seed_workspace_and_conversation(&conn).await;

    // A realistic debugging conversation: a task statement, tool work, a stated
    // user preference, and a resolution -- the kind of span a real compaction
    // eats. The last message of the SUMMARIZABLE part (everything but the four
    // protected recent) is an assistant turn, on purpose.
    let history: Vec<HistoryMessage> = vec![
        span_message(
            ChatMessage::user("The login page throws a 500 on submit. Find the bug and fix it."),
            "text",
            0,
        ),
        span_message(
            ChatMessage::assistant("I'll start by finding the login handler."),
            "text",
            1,
        ),
        HistoryMessage {
            chat: ChatMessage::tool_result(
                "call-0".to_string(),
                "Read",
                "pub async fn login(form: LoginForm) -> Result<Session> {\n    let user = \
                 db.find_user(&form.email).await.unwrap();\n    verify(&form.password, \
                 &user.hash)?;\n    Ok(Session::new(user.id))\n}"
                    .to_string(),
            ),
            content_type: "tool_result".to_string(),
            sequence: 2,
            plan: false,
            payload_ref: None,
            tool_name: Some("Read".to_string()),
        },
        span_message(
            ChatMessage::assistant(
                "The bug is the `.unwrap()` on `db.find_user` in src/api/auth.rs -- an \
                 unknown email panics the handler, which axum turns into a 500. It should \
                 be a 401 instead.",
            ),
            "text",
            3,
        ),
        span_message(
            ChatMessage::user(
                "Right. Fix it, and never use unwrap() in request handlers in this \
                 codebase -- always propagate with ?.",
            ),
            "text",
            4,
        ),
        span_message(
            ChatMessage::assistant("Understood -- no unwrap() in handlers. Applying the fix now."),
            "text",
            5,
        ),
        HistoryMessage {
            chat: ChatMessage::tool_result(
                "call-1".to_string(),
                "Edit",
                "Edited src/api/auth.rs: replaced unwrap() with ok_or(AuthError::Unknown)?"
                    .to_string(),
            ),
            content_type: "tool_result".to_string(),
            sequence: 6,
            plan: false,
            payload_ref: None,
            tool_name: Some("Edit".to_string()),
        },
        HistoryMessage {
            chat: ChatMessage::tool_result(
                "call-2".to_string(),
                "Bash",
                "test result: ok. 128 passed; 0 failed; finished in 3.02s".to_string(),
            ),
            content_type: "tool_result".to_string(),
            sequence: 7,
            plan: false,
            payload_ref: None,
            tool_name: Some("Bash".to_string()),
        },
        // *** The summarizable span's LAST message: an assistant turn -- the
        // exact shape that produced the prefill echo. ***
        span_message(
            ChatMessage::assistant(
                "All 128 tests pass. The login handler now returns 401 for unknown emails.",
            ),
            "text",
            8,
        ),
        // --- the four protected-recent messages below are never summarized ---
        span_message(
            ChatMessage::user("Great. Now add a rate limiter."),
            "text",
            9,
        ),
        span_message(
            ChatMessage::assistant("Adding a rate limiter to the login route."),
            "text",
            10,
        ),
        span_message(ChatMessage::user("Use a token bucket."), "text", 11),
        span_message(
            ChatMessage::assistant("Token bucket it is -- starting now."),
            "text",
            12,
        ),
    ];
    let protected_recent = 4;

    // Guard the FIXTURE itself: if a future edit to `history` stops the span
    // ending on an assistant message, this test would still pass while no
    // longer testing the bug it exists for.
    let last_summarized = &history[history.len() - protected_recent - 1];
    assert_eq!(
        last_summarized.chat.role, "assistant",
        "fixture is wrong: this test only tests the prefill bug if the summarized span \
         ENDS on an assistant message"
    );
    let echoed_text = match &last_summarized.chat.content {
        doce_lib::inference::MessageContent::Text(t) => t.clone(),
        other => panic!("fixture's last summarized message must be text, got {other:?}"),
    };

    let result = context::summarize_and_persist(
        &conn,
        None,
        &server.base_url,
        "c1",
        &history,
        protected_recent,
    )
    .await
    .expect("summarization must not error against a healthy server");

    let summary = match result {
        context::SummaryResult::Persisted(s) => s,
        context::SummaryResult::NothingToSummarize => {
            panic!("the span was non-empty -- summarization must not no-op here")
        }
        context::SummaryResult::Rejected(decision) => panic!(
            "the real model's summary was REJECTED by evaluate_summary ({decision:?}). \
             Tier-2 compaction cannot condense a real span."
        ),
    };

    // THE DELIVERABLE: the real summary, for a human to judge.
    println!("\n=== real model: tier-2 summary ===\n{summary}\n=== end summary ===\n");

    // THE ASSERTION THAT BITES: the summary must not be a continuation of the
    // span's last assistant message. A relationship to the input, never a
    // wording -- robust to stochasticity, and the exact bug this test exists
    // for. `starts_with` rather than `contains`: prefill continuation echoes the
    // message and then keeps going, whereas a legitimate snapshot may well
    // quote a fact from it in passing.
    assert!(
        !summary.starts_with(echoed_text.trim()),
        "the model CONTINUED the span's trailing assistant message instead of summarizing: \
         {summary:?}\nThis is the trailing-assistant prefill bug -- the summarization never ran."
    );

    // The prompt's own contract: output ONLY the <state_snapshot> block. Asserted
    // because the echo bug is invisible to a mere non-emptiness check -- an
    // echoed sentence is perfectly well-formed prose.
    assert!(
        summary.contains("<state_snapshot>") && summary.contains("</state_snapshot>"),
        "the summary does not carry the <state_snapshot> block SUMMARIZATION_PROMPT \
         demands: {summary:?}"
    );
    assert!(
        summary.contains("GOAL:"),
        "the snapshot is missing the GOAL section the prompt's structure requires: {summary:?}"
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
        doce_lib::bench::chat_outcome_to_turn_outcome(result)
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
    let Some(server) = doce_lib::bench::TestServer::spawn(&model).await else {
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

// ---------------------------------------------------------------------------
// `maybe_compact`: the TOP-LEVEL context-management entry point.
//
// `context/mod.rs`'s own doc comment still says `compute_usage`/`maybe_compact`
// "need a live DB connection (and, for `maybe_compact`, a running
// llama-server)" and that "their correctness is exercised by `quickstart.md`'s
// manual validation pass" -- i.e. by a human clicking through the app. That gap
// is not hypothetical: it hid the trailing-assistant prefill echo for the whole
// life of tier 2 (see `SUMMARIZATION_FINAL_TURN`'s doc comment), a bug whose
// entire character was that it LOOKED like success. The tests below close it by
// driving the REAL `maybe_compact` -- the exact call `send_agent_message` makes
// every turn (`commands::agent`) -- against a REAL llama-server and a REAL
// migrated DB, and asserting the state production actually expects afterwards.
//
// Nothing here rebuilds a shape production builds: the fixtures are DB rows
// (seeded through `storage::messages::insert`, production's single insert path)
// and `settings` rows (read back by production's own `ContextSettings::load`
// through production's own key constants). Every number asserted on comes out
// of `maybe_compact`/`compute_usage` themselves.
// ---------------------------------------------------------------------------

/// Seeds the two `context.*` threshold rows `ContextSettings::load` reads.
///
/// Compaction triggers at a PERCENTAGE of the 16384-token window, so a fixture
/// that crosses the 0.75 default has to carry ~11.9k tokens of history -- a
/// wall of filler whose span is too big to summarize quickly and too synthetic
/// to tell us anything (exactly the fixture the deleted
/// `apply_lightweight_clearing_then_summarize_against_the_server` had). Moving
/// the threshold instead is not a test-only hack: `context.compactThresholdPct`
/// is a real, user-settable production setting, read here through production's
/// own `ContextSettings::KEY_*` constants, and it leaves every byte of the
/// pipeline `maybe_compact` runs identical. The conversation is then genuinely
/// over its own configured threshold, and stays legible.
///
/// `warn_pct` must be passed too, and low: `from_raw` clamps `compact` UP to
/// `warn` (its warn <= compact <= hardLimit invariant), so seeding `compact`
/// alone would silently leave the default 0.5 warn threshold in force.
async fn seed_compaction_thresholds(
    conn: &tokio_rusqlite::Connection,
    warn_pct: f64,
    compact_pct: f64,
) {
    conn.call(move |conn: &mut rusqlite::Connection| {
        conn.execute(
            "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, 0)",
            rusqlite::params![
                doce_lib::context::ContextSettings::KEY_WARN_THRESHOLD_PCT,
                warn_pct.to_string()
            ],
        )?;
        conn.execute(
            "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, 0)",
            rusqlite::params![
                doce_lib::context::ContextSettings::KEY_COMPACT_THRESHOLD_PCT,
                compact_pct.to_string()
            ],
        )
    })
    .await
    .expect("seed context threshold settings");
}

/// Inserts one `text` row through `storage::messages::insert` -- production's
/// single insert path (sequence allocation included), not hand-rolled SQL.
async fn seed_text(conn: &tokio_rusqlite::Connection, role: &'static str, text: String) {
    conn.call(move |conn: &mut rusqlite::Connection| {
        doce_lib::storage::messages::insert(
            conn,
            None,
            &doce_lib::storage::messages::NewMessage {
                conversation_id: "c1",
                role,
                content_type: "text",
                content: &text,
                tool_name: None,
                tool_call_id: None,
                model_text: None,
                created_at: 0,
                duration_ms: None,
                token_count: None,
            },
        )
        .map(|_| ())
    })
    .await
    .expect("seed text row");
}

/// Inserts one `tool_result` row in the exact shape `persist_tool_result`
/// writes: role `tool`, `detail`-shaped JSON `content` (what
/// `parse_tool_row_flags` reads `plan`/`payloadRef` back off at load), and
/// `model_text` carrying the model-facing text -- which is the part
/// `apply_lightweight_clearing` actually replaces and therefore the part whose
/// size tier 1 actually frees.
async fn seed_tool_result(
    conn: &tokio_rusqlite::Connection,
    tool_call_id: &'static str,
    tool_name: &'static str,
    model_text: String,
    payload_ref: Option<&'static str>,
) {
    conn.call(move |conn: &mut rusqlite::Connection| {
        let detail = match payload_ref {
            Some(p) => serde_json::json!({ "output": model_text, "payloadRef": p }).to_string(),
            None => serde_json::json!({ "output": model_text }).to_string(),
        };
        doce_lib::storage::messages::insert(
            conn,
            None,
            &doce_lib::storage::messages::NewMessage {
                conversation_id: "c1",
                role: "tool",
                content_type: "tool_result",
                content: &detail,
                tool_name: Some(tool_name),
                tool_call_id: Some(tool_call_id),
                model_text: Some(&model_text),
                created_at: 0,
                duration_ms: None,
                token_count: None,
            },
        )
        .map(|_| ())
    })
    .await
    .expect("seed tool_result row");
}

/// Every persisted `context_notice` row's parsed JSON, oldest first -- the
/// ONLY durable trace tier 1 and tier 2 leave, and therefore what "did
/// compaction actually do?" has to be answered from. Read straight off the
/// `messages` table rather than through `load_history_annotated`, which
/// deliberately hides notice rows behind its splice.
async fn context_notices(conn: &tokio_rusqlite::Connection) -> Vec<serde_json::Value> {
    conn.call(
        |conn: &mut rusqlite::Connection| -> rusqlite::Result<Vec<String>> {
            let mut stmt = conn.prepare(
                "SELECT content FROM messages WHERE conversation_id = 'c1' AND \
             content_type = 'context_notice' ORDER BY sequence ASC",
            )?;
            let rows = stmt
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        },
    )
    .await
    .expect("read context_notice rows")
    .iter()
    .map(|s| serde_json::from_str(s).expect("a context_notice row must be valid JSON"))
    .collect()
}

fn notices_of_kind<'a>(notices: &'a [serde_json::Value], kind: &str) -> Vec<&'a serde_json::Value> {
    notices
        .iter()
        .filter(|n| n.get("kind").and_then(|k| k.as_str()) == Some(kind))
        .collect()
}

/// Loads the conversation exactly as production reloads it after a compaction
/// -- `load_history_annotated`, the same function `maybe_compact` and the agent
/// seed both go through, splice and all.
async fn reload_history(
    conn: &tokio_rusqlite::Connection,
    skills_dir: &std::path::Path,
) -> Vec<HistoryMessage> {
    let skills_dir = skills_dir.to_path_buf();
    conn.call(move |conn: &mut rusqlite::Connection| {
        doce_lib::storage::conversations::load_history_annotated(conn, "c1", &skills_dir)
    })
    .await
    .expect("reload history")
}

/// Prints a loaded history's SHAPE (never its full text) -- the deliverable a
/// human reads to see what the splice actually did to the conversation.
fn print_history_shape(label: &str, history: &[HistoryMessage]) {
    println!("=== {label} ({} messages) ===", history.len());
    for m in history {
        let text = match &m.chat.content {
            doce_lib::inference::MessageContent::Text(t) => t.clone(),
            other => format!("{other:?}"),
        };
        let flat = text.replace('\n', " ");
        let head: String = flat.chars().take(72).collect();
        let ellipsis = if flat.chars().count() > 72 { "..." } else { "" };
        println!(
            "  seq={:<3} {:<9} {:<14} {head}{ellipsis}",
            m.sequence, m.chat.role, m.content_type
        );
    }
    println!("=== end {label} ===");
}

/// The task statement of both fixtures below. Kept as a const because
/// `messages_to_summarize`'s keep-first rule turns on it: it is the first
/// genuine user message, so it is the one message the span must never contain.
const TASK_STATEMENT: &str =
    "The checkout service started returning 500s on every POST /orders about an hour after \
     yesterday's deploy. Find the root cause and fix it. Don't restart anything in prod until \
     we know what it is.";

/// The `PROTECTED_RECENT_MESSAGES` (10) most recent turns of the tier-2
/// fixture. A const rather than an inline literal because the assertions need
/// them by value: these are exactly the messages `messages_to_summarize`
/// REFUSES to summarize, so "did they survive the compaction verbatim?" is the
/// invariant the protected window exists to provide, and it can only be checked
/// against the real strings.
const PROTECTED_RECENT_TURNS: [(&str, &str); 10] = [
    ("user", "Good. Fix the prefix in the deploy config too."),
    (
        "assistant",
        "Correcting the pricing prefix in deploy/prod.yaml now.",
    ),
    ("user", "And add a test for the empty-cell case."),
    (
        "assistant",
        "Adding a test that asserts 503 rather than a panic on an empty cell.",
    ),
    (
        "user",
        "Does the same dropped-handle pattern exist anywhere else?",
    ),
    (
        "assistant",
        "Checking main() for other spawned startup tasks whose handles are dropped.",
    ),
    ("user", "Report back before you change any of them."),
    (
        "assistant",
        "Will do -- I'll list them first and not touch anything yet.",
    ),
    (
        "user",
        "Also confirm the 503 is what the client retries on.",
    ),
    (
        "assistant",
        "Checking the client's retry policy against 503 now.",
    ),
];

/// A realistic, text-heavy debugging conversation, seeded through production's
/// insert path. Deliberately carries exactly TWO `tool_result` rows -- i.e.
/// exactly `TOOL_KEEP_N` -- so tier 1 clears NOTHING and this fixture isolates
/// tier 2. Deliberately ends its SUMMARIZABLE span on an ASSISTANT message
/// (the message at `len - PROTECTED_RECENT_MESSAGES - 1`), because that is the
/// production hazard that produced the prefill echo.
///
/// Returns the text of that trailing assistant message: the echo the summary is
/// asserted against.
async fn seed_tier_two_fixture(conn: &tokio_rusqlite::Connection) -> String {
    // --- the summarizable span (indices 0..=7; index 0 is dropped keep-first) ---
    seed_text(conn, "user", TASK_STATEMENT.to_string()).await;
    seed_text(
        conn,
        "assistant",
        "Starting with the deploy diff and the handler itself, since the failure began after a \
         deploy rather than under load. If it were saturation I'd expect timeouts and a ramp, not \
         a clean 500 on every single POST from the first minute onward. Reading the orders \
         handler now."
            .to_string(),
    )
    .await;
    seed_tool_result(
        conn,
        "call-0",
        "Read",
        "pub async fn create_order(State(app): State<AppState>, Json(body): Json<OrderBody>) \
         -> Result<Json<Order>, ApiError> {\n    let pricing = app.pricing.get().expect(\"pricing \
         table loaded at boot\");\n    let total = pricing.total_for(&body.items)?;\n    let order \
         = app.db.insert_order(&body.customer_id, total).await?;\n    app.events.publish(\
         OrderCreated { id: order.id }).await?;\n    Ok(Json(order))\n}\n\n// pricing is a \
         OnceCell<PricingTable> populated by load_pricing() during startup\n// see \
         src/startup.rs:118"
            .to_string(),
        None,
    )
    .await;
    seed_text(
        conn,
        "assistant",
        "Found the shape of it. `create_order` calls `app.pricing.get().expect(...)` on a \
         `OnceCell<PricingTable>`. If that cell was never populated the expect panics, axum \
         catches the panic in its default layer and turns it into a bare 500 with no body -- \
         which matches exactly what we're seeing: every POST fails, identically, with nothing in \
         the error log because a panic never reaches our own error-mapping code. So the question \
         is not the handler, it's why the cell is empty."
            .to_string(),
    )
    .await;
    seed_text(
        conn,
        "user",
        "That fits. One rule for the fix, and I want it applied everywhere, not just here: no \
         expect() or unwrap() in a request handler in this codebase, ever. If a value can be \
         missing at request time it gets propagated as a typed error with ? and mapped to a real \
         status code. Panicking in a handler is never an acceptable failure mode for us."
            .to_string(),
    )
    .await;
    seed_text(
        conn,
        "assistant",
        "Understood -- no expect()/unwrap() in handlers, typed error plus ? instead. Tracing the \
         startup path: load_pricing() is spawned as a background task in main() and its JoinHandle \
         is dropped, so if it fails the process keeps serving with an empty cell and nothing ever \
         notices. Yesterday's deploy changed the pricing table's S3 prefix, so the fetch 404s and \
         the task dies silently. That is the root cause: the deploy broke the fetch, and the \
         dropped handle turned a fatal startup error into a silent one."
            .to_string(),
    )
    .await;
    seed_tool_result(
        conn,
        "call-1",
        "Edit",
        "Edited src/api/orders.rs: replaced `app.pricing.get().expect(\"pricing table loaded at \
         boot\")` with `app.pricing.get().ok_or(ApiError::Unavailable)?`, which maps to 503.\n\
         Edited src/startup.rs: load_pricing() now runs to completion before the listener binds, \
         and a failure aborts startup instead of being dropped."
            .to_string(),
        None,
    )
    .await;
    // *** The span's LAST message: an assistant turn -- the prefill hazard. ***
    let trailing_assistant = "Both changes are in. The handler no longer panics on an empty \
         pricing cell -- it returns 503 Unavailable through the normal error path -- and startup \
         now fails loudly if the pricing fetch fails instead of serving with an empty table. The \
         S3 prefix in the deploy config still needs correcting; that's the actual trigger."
        .to_string();
    seed_text(conn, "assistant", trailing_assistant.clone()).await;

    // --- the PROTECTED_RECENT_MESSAGES (10) most recent: never summarized ---
    for (role, text) in PROTECTED_RECENT_TURNS {
        seed_text(conn, role, text.to_string()).await;
    }

    trailing_assistant
}

/// THE HAPPY PATH, end to end: a conversation genuinely over its compaction
/// threshold, handed to the REAL `maybe_compact` against a REAL llama-server,
/// asserted on the state production expects afterwards.
///
/// This covers, in one real round-trip through the real pipeline:
///   * tier 2 actually runs and PERSISTS a real summary;
///   * THE CORRUPTION CLASS -- the persisted summary is a summary, not an echo/
///     continuation of the span's trailing assistant message (the bug fixed in
///     a770cae, which `evaluate_summary` accepted while reporting success);
///   * the DB ends up in the shape `load_history_annotated` splices correctly:
///     the summary spliced in at the front, the protected-recent turns after it
///     in order, and the summarized span GONE;
///   * usage genuinely DROPS, and `state` is `"justCompacted"`;
///   * the Accept arm's chain into `extract_and_persist_memories` runs.
#[tokio::test]
#[ignore]
async fn the_real_maybe_compact_condenses_an_over_threshold_conversation() {
    let model = installed_model_path();
    let Some(server) = doce_lib::bench::TestServer::spawn(&model).await else {
        return; // sidecar binary or model GGUF absent -- skip (see TestServer)
    };

    let conn = doce_lib::storage::test_async_connection().await;
    seed_workspace_and_conversation(&conn).await;
    seed_compaction_thresholds(&conn, 0.04, 0.08).await;
    let echoed_text = seed_tier_two_fixture(&conn).await;

    let skills_dir = tempfile::tempdir().unwrap();
    let system_prompt = system_prompt();
    let failures = doce_lib::context::CompactionFailures::default();
    let observed_usage = doce_lib::context::LastObservedUsage::default();

    // Guard the FIXTURE with production's own accounting, not a hand-rolled
    // estimate: if this conversation is not actually over its threshold, the
    // test below would pass while never entering the pipeline at all.
    let before =
        doce_lib::context::compute_usage(&conn, "c1", skills_dir.path(), &system_prompt, None)
            .await
            .expect("compute_usage must succeed against a migrated DB");
    println!(
        "\n=== before: tokens_used={} budget={} state={:?} ===",
        before.tokens_used, before.token_budget, before.state
    );
    assert!(
        (before.tokens_used as f64) >= 0.08 * before.token_budget as f64,
        "fixture is wrong: the conversation is NOT over its configured 8% compaction \
         threshold ({} of {}), so maybe_compact would return without compacting and every \
         assertion below would be vacuous",
        before.tokens_used,
        before.token_budget
    );

    let before_history = reload_history(&conn, skills_dir.path()).await;
    print_history_shape("before: history", &before_history);

    // THE REAL PRODUCTION CALL -- byte-for-byte the one `send_agent_message`
    // makes every turn, `force = false` and all.
    let usage = doce_lib::context::maybe_compact(
        &conn,
        None,
        &server.base_url,
        "c1",
        skills_dir.path(),
        &system_prompt,
        false,
        &failures,
        &observed_usage,
    )
    .await
    .expect("maybe_compact must not error against a healthy server");

    let notices = context_notices(&conn).await;
    let summarized = notices_of_kind(&notices, "summarized");
    let summary = summarized
        .first()
        .and_then(|n| n.get("summary"))
        .and_then(|s| s.as_str())
        .unwrap_or("<NONE PERSISTED>")
        .to_string();

    // THE DELIVERABLE: the real artifacts, for a human to judge.
    println!(
        "\n=== real model: the summary maybe_compact PERSISTED ===\n{summary}\n=== end summary ==="
    );
    println!(
        "\n=== after: tokens_used={} budget={} state={:?} ===",
        usage.tokens_used, usage.token_budget, usage.state
    );
    println!(
        "=== persisted context_notice kinds: {:?} ===",
        notices
            .iter()
            .map(|n| n.get("kind").and_then(|k| k.as_str()).unwrap_or("?"))
            .collect::<Vec<_>>()
    );

    // --- tier 2 ran and persisted exactly one summary ---
    assert_eq!(
        summarized.len(),
        1,
        "tier 2 must persist exactly one `summarized` context_notice; got {} (notices: {notices:?})",
        summarized.len()
    );
    assert!(
        notices_of_kind(&notices, "cleared").is_empty(),
        "this fixture carries exactly TOOL_KEEP_N tool rows, so tier 1 must clear nothing and \
         persist no `cleared` notice -- it did: {notices:?}"
    );

    // --- THE CORRUPTION CLASS: a summary, not a prefill echo ---
    // A relationship to the input, never a wording -- robust to a stochastic
    // model, and the exact defect that shipped green for tier 2's whole life.
    assert!(
        !summary.starts_with(echoed_text.trim()),
        "maybe_compact PERSISTED a continuation of the span's trailing assistant message \
         instead of a summary: {summary:?}\nThis is the trailing-assistant prefill bug -- \
         compaction reported success and corrupted the conversation."
    );
    assert!(
        summary.contains("<state_snapshot>") && summary.contains("</state_snapshot>"),
        "the persisted summary does not carry the <state_snapshot> block SUMMARIZATION_PROMPT \
         demands: {summary:?}"
    );
    assert!(
        summary.contains("GOAL:"),
        "the persisted snapshot is missing the GOAL section the prompt's structure requires: \
         {summary:?}"
    );

    // --- usage genuinely dropped, and the state says so ---
    assert!(
        usage.tokens_used < before.tokens_used,
        "compaction reported success but usage did not actually drop: {} -> {}",
        before.tokens_used,
        usage.tokens_used
    );
    assert_eq!(
        usage.state, "justCompacted",
        "a compaction that changed something must report state:\"justCompacted\""
    );

    // --- the Accept arm's chain into extraction ran ---
    let memories = doce_lib::storage::memories::load_memories(&conn, Some("w1"))
        .await
        .expect("load the persisted set");
    println!(
        "\n=== real model: memories extracted by the compaction chain ({}) ===",
        memories.len()
    );
    for (i, m) in memories.iter().enumerate() {
        println!("  [{i}] {:?}", m.content);
    }
    println!("=== end memories ===\n");
    assert!(
        !memories.is_empty(),
        "the Accept arm awaits extract_and_persist_memories over a span carrying an explicit, \
         stated coding rule -- the chain persisted NOTHING"
    );
    for m in &memories {
        assert!(
            doce_lib::context::is_plausible_fact(&m.content),
            "the compaction chain persisted a non-fact-shaped memory: {:?}",
            m.content
        );
    }

    // The failure breaker must be clear after a Persisted summary.
    assert!(
        failures.0.lock().unwrap().get("c1").is_none(),
        "a Persisted summary must reset the conversation's consecutive-failure count"
    );

    // --- THE INVARIANT THE SPAN SELECTION EXISTS TO GUARANTEE ---
    //
    // `messages_to_summarize` deliberately EXCLUDES two things from the span it
    // hands the model: the most recent `PROTECTED_RECENT_MESSAGES` (10) turns,
    // and the first genuine user message -- keep-first, whose own doc comment
    // says summarization "must never be the thing that makes the model forget
    // what it was asked to do". Excluded from the span means NOT DESCRIBED BY
    // THE SUMMARY. So the only way either can survive a compaction is VERBATIM,
    // in the reloaded history: if the splice drops them, they are gone with
    // nothing standing in for them, and both protections are worth exactly zero.
    //
    // This is checked through `load_history_annotated` -- the same function the
    // agent seed (`load_history`) and `maybe_compact` itself both reload
    // through -- so it is literally what the model sees on the next turn.
    let after_history = reload_history(&conn, skills_dir.path()).await;
    print_history_shape("after: history as production reloads it", &after_history);

    let text_of = |m: &HistoryMessage| match &m.chat.content {
        doce_lib::inference::MessageContent::Text(t) => t.clone(),
        other => format!("{other:?}"),
    };
    let after_texts: Vec<String> = after_history.iter().map(&text_of).collect();

    // The summary itself is spliced in, verbatim, as a synthesized system row.
    let spliced = after_history
        .iter()
        .find(|m| m.content_type == "context_notice")
        .unwrap_or_else(|| {
            panic!("the persisted summary must be spliced into the reloaded history")
        });
    assert_eq!(
        spliced.chat.role, "system",
        "the spliced summary is synthesized as a system message"
    );
    assert_eq!(
        text_of(spliced),
        summary,
        "the spliced message must carry the summary that was persisted, verbatim"
    );

    // Keep-first: the task statement was never summarized, so it must survive.
    assert!(
        after_texts.iter().any(|t| t == TASK_STATEMENT),
        "the first genuine user message -- the TASK STATEMENT -- is gone from the reloaded \
         history, and `messages_to_summarize` excluded it from the span, so the summary does \
         not describe it either. Keep-first is defeated: the model has no record of what it \
         was asked to do."
    );

    // The protected window: all ten most recent turns survive, verbatim, in order.
    let protected_positions: Vec<Option<usize>> = PROTECTED_RECENT_TURNS
        .iter()
        .map(|(_, text)| after_texts.iter().position(|t| t == text))
        .collect();
    let missing: Vec<&str> = PROTECTED_RECENT_TURNS
        .iter()
        .zip(&protected_positions)
        .filter(|(_, pos)| pos.is_none())
        .map(|((_, text), _)| *text)
        .collect();
    assert!(
        missing.is_empty(),
        "{} of the {} PROTECTED_RECENT_MESSAGES were DESTROYED by the compaction. \
         `messages_to_summarize` excluded them from the span on purpose, so the summary does \
         not describe them -- they are simply gone. maybe_compact reported \
         state:\"justCompacted\" and a usage drop while doing this.\nDestroyed: {missing:#?}",
        missing.len(),
        PROTECTED_RECENT_TURNS.len()
    );
    let found: Vec<usize> = protected_positions.into_iter().flatten().collect();
    assert!(
        found.windows(2).all(|w| w[0] < w[1]),
        "the protected-recent turns survived but in the wrong order: {found:?}"
    );

    // The span itself is gone -- that is what the summary replaces.
    assert!(
        !after_history
            .iter()
            .any(|m| m.content_type == "tool_result"),
        "both tool_results were inside the summarized span -- none may survive the splice"
    );
    assert!(
        !after_texts
            .iter()
            .any(|t| t.starts_with(echoed_text.trim())),
        "the span's trailing assistant message survived the splice -- the summary was supposed \
         to replace it"
    );

    let sequences: Vec<i64> = after_history.iter().map(|m| m.sequence).collect();
    assert!(
        sequences.windows(2).all(|w| w[0] < w[1]),
        "the reloaded history must stay strictly ordered by sequence, got {sequences:?}"
    );
}

/// A conversation whose bulk lives in OLD `tool_result` rows: tier 1 alone
/// frees more than enough, so tier 2 must never be reached. Five tool rows --
/// three huge ones early (which tier 1 clears) and two small ones among the
/// protected recent (which `TOOL_KEEP_N == 2` keeps) -- so the clearing is
/// unambiguous and the kept rows stay cheap.
async fn seed_tier_one_fixture(conn: &tokio_rusqlite::Connection) {
    // ~6k chars each: a plausible `Read` of a real source file, big enough that
    // three of them dominate the conversation the way real tool output does.
    let big_read = |name: &str| -> String {
        let mut out = format!("// {name}\nuse crate::prelude::*;\n\n");
        for i in 0..60 {
            out.push_str(&format!(
                "pub fn handler_{i}(state: &AppState, req: Request) -> Result<Response, Error> \
                 {{\n    let ctx = state.context_for(&req)?;\n    let body = \
                 ctx.decode::<Payload>(req.body())?;\n    let out = ctx.service.apply(body, \
                 {i})?;\n    Ok(Response::json(&out))\n}}\n\n"
            ));
        }
        out
    };

    seed_text(conn, "user", TASK_STATEMENT.to_string()).await;
    seed_text(
        conn,
        "assistant",
        "Reading the handler modules.".to_string(),
    )
    .await;
    seed_tool_result(conn, "call-0", "Read", big_read("src/api/orders.rs"), None).await;
    seed_text(conn, "assistant", "Now the pricing module.".to_string()).await;
    seed_tool_result(
        conn,
        "call-1",
        "Read",
        big_read("src/pricing/table.rs"),
        None,
    )
    .await;
    seed_text(conn, "assistant", "And the startup path.".to_string()).await;
    seed_tool_result(conn, "call-2", "Read", big_read("src/startup.rs"), None).await;
    seed_text(conn, "assistant", "I have what I need.".to_string()).await;

    // --- the 10 protected recent, including the two SMALL tool rows tier 1 keeps ---
    seed_text(conn, "user", "What did you find?".to_string()).await;
    seed_text(
        conn,
        "assistant",
        "A dropped JoinHandle in startup.".to_string(),
    )
    .await;
    seed_text(conn, "user", "Confirm it with the logs.".to_string()).await;
    seed_tool_result(
        conn,
        "call-3",
        "Bash",
        "no matching log lines".to_string(),
        None,
    )
    .await;
    seed_text(
        conn,
        "assistant",
        "Nothing logged -- consistent with a panic.".to_string(),
    )
    .await;
    seed_text(conn, "user", "Check the deploy diff.".to_string()).await;
    seed_tool_result(
        conn,
        "call-4",
        "Bash",
        "prod.yaml: -prefix: v3 +prefix: v4".to_string(),
        None,
    )
    .await;
    seed_text(
        conn,
        "assistant",
        "The prefix changed in the deploy.".to_string(),
    )
    .await;
    seed_text(conn, "user", "That's the trigger then.".to_string()).await;
    seed_text(
        conn,
        "assistant",
        "Agreed -- writing the fix now.".to_string(),
    )
    .await;
}

/// TIER 1 ALONE SUFFICES: over the compaction threshold, but clearing the old
/// tool results frees enough that tier 2 must NOT run.
///
/// This matters because tier 2 is the lossy, generative, expensive step -- the
/// one that spends a llama-server round-trip and replaces real turns with a
/// small model's paraphrase of them. `maybe_compact` re-checks the threshold
/// after tier 1 precisely so that never happens when the cheap tier was enough.
/// Run against a REAL server, so a tier 2 that wrongly fires actually fires and
/// leaves its real trace in the DB, rather than being asserted about in the
/// abstract.
#[tokio::test]
#[ignore]
async fn tier_one_alone_stops_the_pipeline_when_the_clearing_frees_enough() {
    let model = installed_model_path();
    let Some(server) = doce_lib::bench::TestServer::spawn(&model).await else {
        return; // sidecar binary or model GGUF absent -- skip (see TestServer)
    };

    let conn = doce_lib::storage::test_async_connection().await;
    seed_workspace_and_conversation(&conn).await;
    seed_compaction_thresholds(&conn, 0.05, 0.10).await;
    seed_tier_one_fixture(&conn).await;

    let skills_dir = tempfile::tempdir().unwrap();
    let system_prompt = system_prompt();
    let failures = doce_lib::context::CompactionFailures::default();
    let observed_usage = doce_lib::context::LastObservedUsage::default();
    let threshold_tokens = 0.10 * doce_lib::inference::CONTEXT_WINDOW_TOKENS as f64;

    let usage = doce_lib::context::maybe_compact(
        &conn,
        None,
        &server.base_url,
        "c1",
        skills_dir.path(),
        &system_prompt,
        false,
        &failures,
        &observed_usage,
    )
    .await
    .expect("maybe_compact must not error against a healthy server");

    let notices = context_notices(&conn).await;
    println!(
        "\n=== tier-1-only: after tokens_used={} budget={} state={:?} threshold={threshold_tokens} ===",
        usage.tokens_used, usage.token_budget, usage.state
    );
    println!("=== persisted context_notices: {notices:#?} ===");
    print_history_shape(
        "tier-1-only: history as production reloads it",
        &reload_history(&conn, skills_dir.path()).await,
    );

    let cleared = notices_of_kind(&notices, "cleared");
    assert_eq!(
        cleared.len(),
        1,
        "tier 1 must persist exactly one `cleared` notice; got {notices:?}"
    );
    assert_eq!(
        cleared[0].get("clearedCount").and_then(|c| c.as_u64()),
        Some(3),
        "tier 1 must clear exactly the three huge tool results beyond TOOL_KEEP_N: {:?}",
        cleared[0]
    );

    // THE ASSERTION THAT BITES: tier 1 freed plenty, so tier 2 -- the lossy,
    // generative step -- must not have run. Its trace is a `summarized` notice.
    assert!(
        notices_of_kind(&notices, "summarized").is_empty(),
        "tier 1 already brought usage to {} against a {threshold_tokens} threshold, but tier 2 \
         RAN ANYWAY and summarized the conversation -- a lossy, expensive step taken when the \
         cheap one had already succeeded: {notices:?}",
        usage.tokens_used
    );

    // The Accept arm is the only thing that calls extraction, so an empty
    // memory set is independent corroboration that tier 2 never ran.
    let memories = doce_lib::storage::memories::load_memories(&conn, Some("w1"))
        .await
        .expect("load the persisted set");
    assert!(
        memories.is_empty(),
        "extraction only runs from tier 2's Accept arm -- these memories prove it ran: {memories:?}"
    );

    assert!(
        (usage.tokens_used as f64) < threshold_tokens,
        "tier 1 must have brought usage back under the compaction threshold, got {} vs \
         {threshold_tokens}",
        usage.tokens_used
    );
    assert_eq!(
        usage.state, "justCompacted",
        "tier 1 cleared three rows, so the pass changed something and must say so"
    );

    // --- TIER 1'S ACTUAL CONTRACT: the clearing has to reach the MODEL ---
    //
    // Everything above is `maybe_compact`'s own report of itself: a notice
    // saying "3 old tool results cleared to save space", and a usage that fell
    // from ~5.1k to 770 tokens BECAUSE those three rows were replaced with
    // `TOOL_CLEARED_PLACEHOLDER`. Both claims are only true if the history
    // production seeds the NEXT turn with is actually cleared.
    //
    // It is the same reload: `send_agent_message` seeds from `load_history`
    // (`commands::agent`), a thin `.map(|m| m.chat)` over the very
    // `load_history_annotated` this calls. So this is literally the next turn's
    // prompt, and the placeholder is the only durable evidence tier 1 ever ran.
    let reloaded = reload_history(&conn, skills_dir.path()).await;
    let text_of = |m: &HistoryMessage| match &m.chat.content {
        doce_lib::inference::MessageContent::Text(t) => t.clone(),
        other => format!("{other:?}"),
    };
    let uncleared: Vec<(i64, usize)> = reloaded
        .iter()
        .filter(|m| m.content_type == "tool_result")
        .map(|m| (m.sequence, text_of(m).chars().count()))
        .filter(|(_, len)| *len > doce_lib::context::limits::TOOL_CLEARED_PLACEHOLDER.len() * 4)
        .collect();
    assert!(
        uncleared.len() <= 2,
        "maybe_compact reported usage={} (down from ~5.1k) and persisted \"3 old tool results \
         cleared to save space\", but the history production reloads STILL CARRIES {} \
         full-size tool results (seq, chars): {uncleared:?}\nTOOL_KEEP_N is 2, so at most two \
         may survive. `apply_lightweight_clearing` mutated a local copy inside maybe_compact \
         that was then dropped; nothing persisted it and no load path re-applies it, so the \
         model is seeded with every byte tier 1 claimed to free. The clearing, the notice, and \
         the usage drop are all fiction.",
        usage.tokens_used,
        uncleared.len()
    );
}
