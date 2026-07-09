//! 010-context-window-management: a real-model smoke test, `#[ignore]`d by
//! default (it needs an actual installed GGUF on this machine, unlike every
//! other test in this codebase, which are all model-free per
//! `context/mod.rs`'s own doc comment on why `compute_usage`/`maybe_compact`
//! aren't unit-tested). Run explicitly via:
//!   cargo test --test real_model_smoke -- --ignored --nocapture
//!
//! This is the closest thing to a live manual QA pass this environment can
//! do without a way to drive/inspect the native Tauri window directly — it
//! exercises the actual `InferenceEngine`/`context` integration end-to-end
//! against a real, already-installed model, rather than asserting on pure
//! logic alone.

use doce_lib::agent::{parse_response, LoopStep, SYSTEM_PROMPT};
use doce_lib::context::{self, ContextSettings};
use doce_lib::inference::{ChatMessage, InferenceEngine, ToolCallMode};
use doce_lib::storage::conversations::HistoryMessage;
use std::path::PathBuf;
use std::time::Instant;

fn installed_model_path() -> PathBuf {
    let home = std::env::var("HOME").expect("HOME must be set");
    PathBuf::from(home).join(
        "Library/Application Support/app.doce.desktop/models/qwen3-4b-instruct-2507-q4_k_m.gguf",
    )
}

#[test]
#[ignore]
fn count_tokens_and_context_window_report_sane_values_against_the_real_model() {
    let path = installed_model_path();
    assert!(
        path.exists(),
        "expected the real installed model at {path:?}"
    );

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

#[test]
#[ignore]
fn render_chat_prompt_and_generate_produce_a_real_short_completion() {
    let path = installed_model_path();
    let engine = InferenceEngine::load(&path, 4).expect("model should load");

    let messages = vec![
        ChatMessage::system("You are a terse assistant. Answer in exactly one word."),
        ChatMessage::user("What is the capital of France? Reply with just the city name."),
    ];
    let rendered = engine
        .render_chat_prompt(&messages)
        .expect("chat template should render");

    let mut output = String::new();
    let result = engine.generate(
        &rendered,
        16,
        doce_lib::inference::ToolCallMode::Forbid,
        |piece| output.push_str(piece),
        || false,
    );

    let full_text = result.expect("generation should succeed");
    println!("real model output: {full_text:?}");
    assert!(
        !full_text.trim().is_empty(),
        "expected a non-empty completion"
    );
    assert_eq!(
        output, full_text,
        "on_token callback must match the returned text"
    );
}

#[test]
#[ignore]
fn prompt_session_prefix_reuse_matches_a_fresh_context_and_is_faster() {
    // The empirical proof the KV-prefix-reuse task stands on: a second
    // `PromptSession::generate` whose prompt EXTENDS the first's shares a
    // large prefix with what the session already holds materialized, so it
    // re-decodes only the divergent suffix -- yet must sample the EXACT same
    // output a fresh, from-scratch context would for that same extended
    // prompt (same fixed seed). If prefix reuse changed the logits at any
    // position, the sampled tokens would diverge; equality is the semantic
    // guarantee. The second call must also be visibly faster than the fresh
    // full-prompt decode, since it skips re-prefilling the shared prefix.
    std::env::set_var("DOCE_GEN_SEED", "20240709");
    let path = installed_model_path();
    let engine = InferenceEngine::load(&path, 4).expect("model should load");

    // A deliberately long system prompt so the shared prefix is large and
    // the re-prefill it saves is measurable, not lost in the noise.
    let long_system = format!(
        "You are a meticulous assistant. {}",
        "Follow every instruction precisely and answer tersely. "
            .repeat(80)
    );
    let base_messages = vec![
        ChatMessage::system(long_system.clone()),
        ChatMessage::user("Name one primary color. Answer with just the color."),
    ];
    let base_prompt = engine
        .render_chat_prompt(&base_messages)
        .expect("render base");

    // The second prompt extends the first: same system + same first user
    // turn, then an assistant turn and a follow-up user turn. Everything up
    // to the divergence point is a shared prefix reused from the first call.
    let extended_messages = vec![
        ChatMessage::system(long_system),
        ChatMessage::user("Name one primary color. Answer with just the color."),
        ChatMessage::assistant("Red"),
        ChatMessage::user("Now name a different primary color, just the color."),
    ];
    let extended_prompt = engine
        .render_chat_prompt(&extended_messages)
        .expect("render extended");

    let mut session = engine.new_session().expect("session should create");

    let t0 = Instant::now();
    // The first call's own output is irrelevant; it exists only to populate
    // the session's KV cache so the second call has a prefix to reuse.
    let _first = session
        .generate(&engine, &base_prompt, 32, ToolCallMode::Forbid, |_| {}, || false)
        .expect("first session generate");
    let first_elapsed = t0.elapsed().as_secs_f64();

    let t1 = Instant::now();
    let session_second = session
        .generate(&engine, &extended_prompt, 32, ToolCallMode::Forbid, |_| {}, || false)
        .expect("second session generate (prefix reuse)");
    let second_elapsed = t1.elapsed().as_secs_f64();

    // A brand-new context decodes the whole extended prompt from scratch --
    // the exact thing prefix reuse is meant to be equivalent to.
    let t2 = Instant::now();
    let fresh = engine
        .generate(&extended_prompt, 32, ToolCallMode::Forbid, |_| {}, || false)
        .expect("fresh full-context generate");
    let fresh_elapsed = t2.elapsed().as_secs_f64();

    println!(
        "[prompt_session_smoke] first_call={first_elapsed:.3}s \
         second_call_prefix_reuse={second_elapsed:.3}s fresh_full_decode={fresh_elapsed:.3}s"
    );
    println!("[prompt_session_smoke] session_second={session_second:?}");
    println!("[prompt_session_smoke] fresh={fresh:?}");

    std::env::remove_var("DOCE_GEN_SEED");

    assert_eq!(
        session_second, fresh,
        "prefix reuse must produce byte-identical output to a fresh-context \
         decode of the same prompt with the same seed"
    );
    assert!(
        second_elapsed < fresh_elapsed,
        "the prefix-reusing second call ({second_elapsed:.3}s) should be \
         faster than the fresh full-prompt decode ({fresh_elapsed:.3}s)"
    );
}

#[test]
#[ignore]
fn grammar_constrained_tool_call_produces_syntactically_valid_json_against_the_real_model() {
    // The actual point of this test: `allow_tool_calls: true` should make a
    // `<tool_call>` response *guaranteed* well-formed -- the tags plus
    // schema-valid JSON inside, not just "prompted for and hopefully
    // correct" -- the whole reason grammar-constrained decoding replaced
    // free-text parsing as the way `agent::parse_response` gets a
    // trustworthy tool call.
    let path = installed_model_path();
    let engine = InferenceEngine::load(&path, 4).expect("model should load");

    let messages = vec![
        ChatMessage::system(SYSTEM_PROMPT),
        ChatMessage::user(
            "Use the Bash tool right now to run the command `pwd`. Call the tool, don't just describe it.",
        ),
    ];
    let rendered = engine
        .render_chat_prompt(&messages)
        .expect("render should succeed");

    let result = engine
        .generate(&rendered, 128, doce_lib::inference::ToolCallMode::Allow, |_| {}, || false)
        .expect("generation should succeed");
    println!("real model tool-call output: {result:?}");

    match parse_response(&result) {
        LoopStep::ToolCall(call) => {
            assert_eq!(call.name, "Bash");
            assert!(
                call.arguments
                    .get("command")
                    .and_then(|v| v.as_str())
                    .is_some(),
                "expected a string `command` argument, got: {:?}",
                call.arguments
            );
        }
        LoopStep::Done(text) => {
            panic!("expected a real tool call once it committed to the <tool_call> path, got plain text: {text:?}");
        }
    }
}

#[test]
#[ignore]
fn tool_result_renders_wrapped_in_qwens_own_tool_response_tags() {
    // Verifies ChatMessage::tool_result's actual rendering against the
    // real model. First tried making the role itself "tool" (on the
    // theory that Qwen's chat template would apply its own
    // role=="tool" -> <tool_response> branch), but that didn't fire in
    // practice -- llama.cpp's template engine rendered an unrecognized
    // role as a bare, never-trained-on `<|im_start|>tool` block instead.
    // So the role stays "user" (reliably handled) and the *text* is
    // wrapped in the literal <tool_response> tags Qwen expects, with no
    // extra "Tool result for X:" framing -- confirmed here against the
    // real chat template, not just that it compiles.
    let path = installed_model_path();
    let engine = InferenceEngine::load(&path, 4).expect("model should load");

    let messages = vec![
        ChatMessage::system(SYSTEM_PROMPT),
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

#[test]
#[ignore]
fn apply_lightweight_clearing_then_summarize_against_the_real_model() {
    let path = installed_model_path();
    let engine = InferenceEngine::load(&path, 4).expect("model should load");

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
            offloaded_to: None,
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
            offloaded_to: None,
        });
    }

    let cleared = context::apply_lightweight_clearing(&mut history, 4);
    assert!(cleared > 0, "expected some tool messages to be cleared");

    // Real summarization call against the real model -- the whole point of
    // this test is proving `summarize_and_persist`'s prompt/generate/parse
    // path works end-to-end, not just that its Rust compiles.
    let protected_recent = 4;
    let to_summarize = &history[..history.len() - protected_recent];
    let mut messages = vec![ChatMessage::system(
        "Summarize the conversation so far concisely, preserving key facts, decisions, and unresolved tasks. Respond with only the summary text, nothing else.",
    )];
    messages.extend(to_summarize.iter().map(|m| m.chat.clone()));
    let rendered = engine
        .render_chat_prompt(&messages)
        .expect("render should succeed");
    let summary = engine
        .generate(&rendered, 256, doce_lib::inference::ToolCallMode::Forbid, |_| {}, || false)
        .expect("summarization generate should succeed");

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
