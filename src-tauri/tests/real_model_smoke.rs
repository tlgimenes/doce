//! 010-context-window-management: a real-model smoke suite, `#[ignore]`d by
//! default (it needs an actual installed GGUF + the built `llama-server`
//! sidecar on this machine, unlike every other test in this codebase, which
//! are all model-free per `context/mod.rs`'s own doc comment on why
//! `compute_usage`/`maybe_compact` aren't unit-tested). Run explicitly via:
//!   cargo test --test real_model_smoke -- --ignored --nocapture
//!
//! The llama-server cutover moved generation onto the HTTP client
//! (`inference::http`), so the generation smokes here spawn a REAL
//! `llama-server` (via `common::TestServer`) and POST at it through
//! `LlamaServerClient::chat` -- the exact path production's
//! `RealBackend`/`SubagentBackend` use. The in-process engine is gone
//! entirely: token counting is a pure chars/4 estimate
//! (`inference::token_estimate`, no model needed), and the tool dialect is
//! pinned to `HermesJson` (doce ships one Hermes model).
//!
//! This is the closest thing to a live manual QA pass this environment can
//! do without a way to drive/inspect the native Tauri window directly.

mod common;

use doce_lib::agent::{dispatch, run_loop, AgentBackend, AgentContext, ToolCall, ToolExecution};
use doce_lib::context::{self, ContextSettings};
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
            tool_name: Some("Bash".to_string()),
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
            tool_name: None,
        });
    }

    let cleared = context::apply_lightweight_clearing(&mut history, 4, None);
    assert!(cleared > 0, "expected some tool messages to be cleared");

    // Real summarization call against the real SERVER, proving the
    // prompt/generate path works end-to-end over HTTP. `Forbid` (no tools): a
    // summary must never be able to emit a tool call.
    //
    // This smoke HAND-ROLLS its request rather than calling
    // `summarize_and_persist` (which needs a seeded DB;
    // `the_real_model_summarizes_a_span_that_ends_with_an_assistant_message`
    // covers the real function). That hand-rolling is precisely how it spent
    // months passing green while DEMONSTRATING the trailing-assistant prefill
    // bug: its span ends on "Assistant reply 19", the request appended nothing
    // after it, and the model dutifully echoed that message back as the
    // "summary" -- which a bare non-emptiness assert waves through. So the two
    // production-shape fixes are mirrored here deliberately (a final user turn
    // + thinking off), and the echo is now asserted against rather than
    // ignored. A smoke that reimplements a request shape must reimplement the
    // CURRENT one, or it silently pins the bug.
    let protected_recent = 4;
    let to_summarize = &history[..history.len() - protected_recent];
    // Derived, never hardcoded: the span's trailing message is what a prefill
    // continuation would echo, and it must stay in step with the fixture above.
    let last_summarized = match &to_summarize.last().unwrap().chat.content {
        doce_lib::inference::MessageContent::Text(t) => t.clone(),
        other => panic!("expected a text message at the end of the span, got {other:?}"),
    };
    assert_eq!(
        to_summarize.last().unwrap().chat.role,
        "assistant",
        "this smoke only exercises the prefill hazard while its span ends on an assistant \
         message"
    );
    let mut messages = vec![ChatMessage::system(
        "Summarize the conversation so far concisely, preserving key facts, decisions, and unresolved tasks. Respond with only the summary text, nothing else.",
    )];
    messages.extend(to_summarize.iter().map(|m| m.chat.clone()));
    messages.push(ChatMessage::user(
        doce_lib::context::limits::SUMMARIZATION_FINAL_TURN,
    ));
    let mut req = ChatRequest::build(
        "doce",
        to_openai_messages(&messages),
        None,
        tool_choice_for(ToolCallMode::Forbid).map(|s| s.to_string()),
    );
    req.disable_thinking();
    let cancel = tokio_util::sync::CancellationToken::new();
    let summary = LlamaServerClient::new(server.base_url.clone())
        .chat(req, |_piece| {}, &cancel)
        .await
        .expect("summarization generate should succeed")
        .text;

    println!("real model summary: {summary:?}");
    assert!(!summary.trim().is_empty(), "expected a non-empty summary");
    // A summary of a 16-message span is never just a continuation of its final
    // line. Before the fix this came back as exactly "Assistant reply 15".
    assert!(
        !summary.starts_with(last_summarized.trim()),
        "the model continued the span's trailing assistant message ({last_summarized:?}) \
         instead of summarizing: {summary:?}"
    );

    // Sanity-check the settings defaults load correctly too (pure logic,
    // but exercised here alongside the real-model assertions for a single
    // combined smoke-test run).
    let settings = ContextSettings::from_raw(&Default::default());
    assert_eq!(
        settings.warn_threshold_pct,
        ContextSettings::DEFAULT_WARN_THRESHOLD_PCT
    );
}

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
    let Some(server) = common::TestServer::spawn(&model).await else {
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
    let Some(server) = common::TestServer::spawn(&model).await else {
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
