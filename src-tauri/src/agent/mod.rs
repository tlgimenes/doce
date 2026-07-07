//! Agent tool-use loop orchestrator (User Story 3, FR-009/FR-013/FR-015/
//! FR-016), wired to real inference + real tools via
//! `commands::agent::send_agent_message`. The loop's control flow (turn
//! counting, tool dispatch, subagent-nesting rejection, response parsing)
//! is real and tested; a known simplification is called out where it
//! lives (`commands/agent.rs`'s doc comment): agent turns bypass the
//! scheduler queue.
//!
//! Tool calls are prompted for (`SYSTEM_PROMPT` below) *and*
//! grammar-constrained at generation time
//! (`InferenceEngine::generate`'s `allow_tool_calls`, a lazy GBNF grammar
//! that only activates once the model starts emitting `{"tool_call"`) —
//! the model's JSON is guaranteed syntactically valid once that happens,
//! so `parse_response` below trusts it rather than defending against
//! malformed output. A model that never starts down that path at all just
//! answers in plain text, same as before.

pub mod dispatch;
pub mod rich_content;
pub mod subagent;
pub mod tools;

/// Describes the built-in tool set and the JSON calling convention
/// `parse_response` expects — this is the `system`-role message of every
/// agent loop run (see `run_loop`'s `initial_messages`), not raw text
/// concatenated onto the user's task. Small models tend to need the exact
/// shape spelled out (with an example) to reliably produce parseable
/// output — general "you have tools available" phrasing is not enough in
/// practice.
pub const SYSTEM_PROMPT: &str = r#"You are a coding and system agent with access to tools. To use a tool, respond with ONLY a JSON object in this exact shape, nothing else:
{"tool_call": {"name": "ToolName", "arguments": {...}}}

Available tools:
- Read: {"file_path": string, "offset"?: number, "limit"?: number} — read a file
- Write: {"file_path": string, "content": string} — create or overwrite a file
- Edit: {"file_path": string, "old_string": string, "new_string": string, "replace_all"?: boolean} — targeted in-place edit
- Bash: {"command": string, "timeout"?: number} — run a shell command
- Glob: {"pattern": string, "path"?: string} — find files by name pattern
- Grep: {"pattern": string, "path"?: string, "glob"?: string} — search file contents
- AskUserQuestion: {"header": string, "question": string, "options": [{"label": string, "description"?: string}], "multiSelect"?: boolean} — pause and ask the user a clarifying question instead of guessing; only use this when genuinely ambiguous, not for routine confirmations

Example tool call:
{"tool_call": {"name": "Read", "arguments": {"file_path": "/tmp/example.txt"}}}

Once you have enough information to answer, respond in plain text with your final answer — do not wrap the final answer in JSON."#;

use crate::inference::ChatMessage;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolCall {
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LoopStep {
    ToolCall(ToolCall),
    Done(String),
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum AgentError {
    #[error("agent loop exceeded its {0}-turn cap without producing a final answer")]
    TurnCapExceeded(u32),
}

/// The convention the model is prompted to follow: a JSON object with a
/// `tool_call` key, or nothing at all (a normal text answer). Extracts the
/// *first* balanced `{...}` object anywhere in the text rather than
/// requiring the entire response to be exactly one JSON value — found
/// necessary against a real model in practice: it sometimes runs on past
/// the first tool call and appends a second one, or wraps the JSON in a
/// little prose, and requiring an exact whole-string parse silently
/// degraded those into "the raw JSON became the final answer" instead of
/// actually calling the tool. Anything with no valid `tool_call`-shaped
/// object anywhere is treated as the final answer verbatim — a model that
/// doesn't understand the convention degrades to "always answers
/// directly", not a crash.
pub fn parse_response(text: &str) -> LoopStep {
    if let Some(json_str) = first_balanced_json_object(text) {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&json_str) {
            if let Some(call) = value.get("tool_call") {
                if let (Some(name), Some(arguments)) = (
                    call.get("name").and_then(|n| n.as_str()),
                    call.get("arguments"),
                ) {
                    return LoopStep::ToolCall(ToolCall {
                        name: name.to_string(),
                        arguments: arguments.clone(),
                    });
                }
            }
        }
    }
    LoopStep::Done(text.to_string())
}

/// Finds the first `{...}` substring with balanced braces (respecting
/// quoted strings, so a `}` inside a string argument doesn't end the
/// object early), starting from the first `{` in the text.
fn first_balanced_json_object(text: &str) -> Option<String> {
    let start = text.find('{')?;
    let bytes = text.as_bytes();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;

    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(text[start..=i].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

/// Context a single loop run is bounded by. `is_subagent` gates the
/// one-level nesting rule (FR-016): a subagent's own loop runs with this
/// set, so any `Task` tool call it attempts is rejected rather than
/// recursing into a further subagent.
///
/// `cwd` (007-workspace-cwd-resolution): the conversation's workspace
/// path, when it has one — resolved once per `send_agent_message` call
/// and read identically by the top-level loop and by the `Task` tool's
/// nested subagent loop, so a subagent inherits the same working
/// directory as its parent by construction rather than by a second call
/// site separately remembering to pass it along (FR-006).
#[derive(Debug, Clone)]
pub struct AgentContext {
    pub is_subagent: bool,
    pub max_turns: u32,
    pub cwd: Option<std::path::PathBuf>,
}

impl AgentContext {
    /// Top-level agent (opened workspace, not a subagent): unbounded by a
    /// nesting rule of its own, but still turn-capped — an intentionally
    /// generous cap, since only subagents (FR-016) have the tight 30-turn
    /// limit that exists specifically to bound delegated work.
    pub fn top_level() -> Self {
        Self {
            is_subagent: false,
            max_turns: 200,
            cwd: None,
        }
    }

    /// FR-016: subagents are capped at 30 turns and cannot spawn further
    /// subagents.
    pub fn subagent() -> Self {
        Self {
            is_subagent: true,
            max_turns: 30,
            cwd: None,
        }
    }

    /// Builder-style setter so call sites can write
    /// `AgentContext::top_level().with_cwd(path)` rather than
    /// constructing the struct by hand.
    pub fn with_cwd(mut self, cwd: Option<std::path::PathBuf>) -> Self {
        self.cwd = cwd;
        self
    }
}

/// Runs the tool-use loop to completion: repeatedly generates a response,
/// executes any tool call it contains, and feeds the result back in, until
/// the model produces a plain-text final answer or the turn cap is hit.
///
/// `initial_messages` is a real role-tagged conversation (typically a
/// `system` message from `SYSTEM_PROMPT` plus a `user` message with the
/// task) rather than one flat string — `generate` is expected to render
/// this through the model's own chat template (see
/// `inference::InferenceEngine::render_chat_prompt`) before tokenizing,
/// since chat-tuned models are trained on role-tagged turns, not raw
/// concatenated text.
///
/// The caller-specific behavior `run_loop`'s control flow depends on,
/// bundled into one trait rather than four separate closure parameters —
/// production implements this once per call site (a `RealBackend` for the
/// top-level loop, a `SubagentBackend` for the `Task`-tool's delegated
/// loop, both in `commands::agent`), tests implement one small
/// `FakeBackend`. Kept as a trait (not hardcoded against the real
/// `InferenceEngine`/tool dispatch) specifically so `run_loop`'s own
/// control flow — the part with real correctness requirements (turn
/// counting, nesting rejection, now the compact-threshold check) — stays
/// unit-testable in milliseconds, without a loaded model or a filesystem.
///
/// `measure`/`threshold`/`compact` are the loop's own explicit per-turn
/// context-fit decision, not something buried inside whatever `generate`
/// happens to do: `measure(&messages)` is checked first, and `compact`
/// only runs (replacing `messages`) when it exceeds `threshold` — a turn
/// that already fits skips straight to `generate`, unchanged. Because this
/// lives in `run_loop` itself rather than each caller's own `generate`
/// logic, it applies uniformly to *every* implementor — the subagent path
/// had no such protection at all before this became the loop's own
/// decision, since only the top-level caller used to build it into its
/// closure.
///
/// `execute_tool` receives the freshly-assigned `tool_call_id` alongside
/// the call itself — generated by `run_loop` (not the model, which only
/// ever decides `name`/`arguments`), the same convention OpenAI/Anthropic
/// use, so `commands::agent`'s persistence layer can store the same id on
/// both the `tool_call` and `tool_result` rows it writes for this call.
///
/// `#[allow(async_fn_in_trait)]`: `run_loop` only ever takes `B: AgentBackend`
/// as a static generic bound, never `dyn AgentBackend` — nothing here
/// crosses a `tokio::spawn` boundary, so the `Send`-bound limitation this
/// lint warns about (relevant for trait objects / spawned futures) doesn't
/// apply.
#[allow(async_fn_in_trait)]
pub trait AgentBackend {
    fn measure(&mut self, messages: &[ChatMessage]) -> u32;
    fn threshold(&self) -> u32;
    fn compact(&mut self, messages: &[ChatMessage]) -> Vec<ChatMessage>;
    async fn generate(&mut self, messages: Vec<ChatMessage>) -> String;
    async fn execute_tool(&mut self, tool_call_id: String, call: ToolCall) -> String;
}

/// Runs the tool-use loop to completion: repeatedly generates a response,
/// executes any tool call it contains, and feeds the result back in, until
/// the model produces a plain-text final answer or the turn cap is hit.
///
/// `initial_messages` is a real role-tagged conversation (typically a
/// `system` message from `SYSTEM_PROMPT` plus a `user` message with the
/// task) rather than one flat string — `backend.generate` is expected to
/// render this through the model's own chat template (see
/// `inference::InferenceEngine::render_chat_prompt`) before tokenizing,
/// since chat-tuned models are trained on role-tagged turns, not raw
/// concatenated text.
pub async fn run_loop<B: AgentBackend>(
    context: &AgentContext,
    initial_messages: Vec<ChatMessage>,
    backend: &mut B,
) -> Result<String, AgentError> {
    let mut messages = initial_messages;

    for _turn in 0..context.max_turns {
        if backend.measure(&messages) > backend.threshold() {
            messages = backend.compact(&messages);
        }
        let response = backend.generate(messages.clone()).await;
        match parse_response(&response) {
            LoopStep::Done(text) => return Ok(text),
            LoopStep::ToolCall(call) => {
                let tool_call_id = uuid::Uuid::now_v7().to_string();
                let tool_name = call.name.clone();
                let arguments = call.arguments.clone();
                messages.push(ChatMessage::tool_use(
                    tool_call_id.clone(),
                    tool_name.clone(),
                    arguments,
                ));
                let result = if call.name == "Task" && context.is_subagent {
                    // FR-016: one-level nesting — a subagent cannot itself
                    // spawn a further subagent. Fed back as an ordinary
                    // tool-error result rather than aborting the loop, so
                    // the model can recover (e.g. do the work itself).
                    "Error: subagents cannot spawn further subagents (one-level nesting limit)"
                        .to_string()
                } else {
                    backend.execute_tool(tool_call_id.clone(), call).await
                };
                messages.push(ChatMessage::tool_result(tool_call_id, tool_name, result));
            }
        }
    }

    Err(AgentError::TurnCapExceeded(context.max_turns))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// Test-only `AgentBackend`: `responses` is a canned reply sequence
    /// (repeating the last entry once exhausted, so a test that keeps
    /// forcing tool calls doesn't need one entry per turn), `on_execute` is
    /// per-test custom `execute_tool` behavior (assertions, call counting,
    /// etc.). `measure` always reports 0 against a `u32::MAX` threshold —
    /// these tests exercise turn-count/nesting control flow, not
    /// context-fitting — so `compact` panics if it's ever reached, proving
    /// that's true.
    struct FakeBackend {
        responses: Vec<String>,
        call_index: usize,
        on_execute: Box<dyn FnMut(String, ToolCall) -> String>,
    }

    impl FakeBackend {
        fn new(responses: Vec<String>, on_execute: impl FnMut(String, ToolCall) -> String + 'static) -> Self {
            Self {
                responses,
                call_index: 0,
                on_execute: Box::new(on_execute),
            }
        }
    }

    impl AgentBackend for FakeBackend {
        fn measure(&mut self, _messages: &[ChatMessage]) -> u32 {
            0
        }

        fn threshold(&self) -> u32 {
            u32::MAX
        }

        fn compact(&mut self, _messages: &[ChatMessage]) -> Vec<ChatMessage> {
            panic!("compact should never run in this test")
        }

        async fn generate(&mut self, _messages: Vec<ChatMessage>) -> String {
            let response = self.responses[self.call_index.min(self.responses.len() - 1)].clone();
            self.call_index += 1;
            response
        }

        async fn execute_tool(&mut self, tool_call_id: String, call: ToolCall) -> String {
            (self.on_execute)(tool_call_id, call)
        }
    }

    #[test]
    fn parses_a_tool_call() {
        let text = r#"{"tool_call": {"name": "Read", "arguments": {"file_path": "/tmp/f.txt"}}}"#;
        let step = parse_response(text);
        assert_eq!(
            step,
            LoopStep::ToolCall(ToolCall {
                name: "Read".to_string(),
                arguments: serde_json::json!({"file_path": "/tmp/f.txt"}),
            })
        );
    }

    #[test]
    fn plain_text_is_a_final_answer() {
        assert_eq!(
            parse_response("The answer is 4."),
            LoopStep::Done("The answer is 4.".to_string())
        );
    }

    #[test]
    fn malformed_json_falls_back_to_plain_text() {
        let text = r#"{"tool_call": {"name": "Read"}}"#; // missing "arguments"
        assert_eq!(parse_response(text), LoopStep::Done(text.to_string()));
    }

    #[test]
    fn extracts_the_first_tool_call_when_the_model_appends_a_second_one() {
        // Real behavior observed against the actual installed model: it ran
        // on past the first tool call and appended a second, unrelated one
        // on the same line, which a strict whole-string JSON parse
        // rejected entirely (falling back to "the raw JSON is the final
        // answer" — the tool never actually got called).
        let text = r#"{"tool_call": {"name": "Read", "arguments": {"file_path": "/a.txt"}}} {"tool_call": {"name": "Grep", "arguments": {"pattern": "x"}}}"#;
        assert_eq!(
            parse_response(text),
            LoopStep::ToolCall(ToolCall {
                name: "Read".to_string(),
                arguments: serde_json::json!({"file_path": "/a.txt"}),
            })
        );
    }

    #[test]
    fn extracts_a_tool_call_wrapped_in_surrounding_prose() {
        let text = "Sure, let me check that file.\n{\"tool_call\": {\"name\": \"Read\", \"arguments\": {\"file_path\": \"/a.txt\"}}}\nLet me know if that's not what you wanted.";
        assert_eq!(
            parse_response(text),
            LoopStep::ToolCall(ToolCall {
                name: "Read".to_string(),
                arguments: serde_json::json!({"file_path": "/a.txt"}),
            })
        );
    }

    #[test]
    fn a_closing_brace_inside_a_string_argument_does_not_end_the_object_early() {
        let text = r#"{"tool_call": {"name": "Write", "arguments": {"file_path": "/a.txt", "content": "func f() { return 1; }"}}}"#;
        let step = parse_response(text);
        assert_eq!(
            step,
            LoopStep::ToolCall(ToolCall {
                name: "Write".to_string(),
                arguments: serde_json::json!({"file_path": "/a.txt", "content": "func f() { return 1; }"}),
            })
        );
    }

    #[test]
    fn plain_prose_with_no_json_at_all_is_a_final_answer() {
        let text = "The secret ingredient is pancakes.";
        assert_eq!(parse_response(text), LoopStep::Done(text.to_string()));
    }

    #[tokio::test]
    async fn loop_runs_tools_until_a_final_answer() {
        let context = AgentContext::top_level();
        let mut backend = FakeBackend::new(
            vec![
                r#"{"tool_call": {"name": "Read", "arguments": {"file_path": "/f.txt"}}}"#.to_string(),
                "The file says hello.".to_string(),
            ],
            |_tool_call_id, call| {
                assert_eq!(call.name, "Read");
                "hello".to_string()
            },
        );

        let result = run_loop(&context, vec![ChatMessage::user("start")], &mut backend)
            .await
            .unwrap();

        assert_eq!(result, "The file says hello.");
    }

    #[tokio::test]
    async fn exceeding_the_turn_cap_is_an_error_not_an_infinite_loop() {
        let context = AgentContext {
            is_subagent: false,
            max_turns: 3,
            cwd: None,
        };

        let mut backend = FakeBackend::new(
            vec![r#"{"tool_call": {"name": "Read", "arguments": {}}}"#.to_string()],
            |_tool_call_id, _call| "ok".to_string(),
        );

        let result = run_loop(&context, vec![ChatMessage::user("start")], &mut backend).await;

        assert_eq!(result, Err(AgentError::TurnCapExceeded(3)));
    }

    #[tokio::test]
    async fn subagent_cannot_spawn_a_further_subagent() {
        let context = AgentContext::subagent();
        assert!(context.is_subagent);
        assert_eq!(context.max_turns, 30);

        let executed_tools = std::sync::Arc::new(AtomicU32::new(0));
        let executed_tools_clone = executed_tools.clone();

        let mut backend = FakeBackend::new(
            vec![
                r#"{"tool_call": {"name": "Task", "arguments": {"prompt": "delegate further"}}}"#.to_string(),
                "I'll handle it myself.".to_string(),
            ],
            move |_tool_call_id, _call| {
                // A real subagent-nesting rejection never reaches here —
                // `run_loop` intercepts `Task` calls under `is_subagent`
                // before calling `execute_tool` at all.
                executed_tools_clone.fetch_add(1, Ordering::SeqCst);
                "should not run".to_string()
            },
        );

        let result = run_loop(&context, vec![ChatMessage::user("start")], &mut backend)
            .await
            .unwrap();

        assert_eq!(result, "I'll handle it myself.");
        assert_eq!(executed_tools.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn top_level_agent_can_spawn_a_subagent_via_task_tool() {
        let context = AgentContext::top_level();
        assert!(!context.is_subagent);

        let mut backend = FakeBackend::new(
            vec![
                r#"{"tool_call": {"name": "Task", "arguments": {"prompt": "go do research"}}}"#.to_string(),
                "Done, subagent found the answer.".to_string(),
            ],
            |_tool_call_id, call| {
                assert_eq!(call.name, "Task");
                "subagent result: 42".to_string()
            },
        );

        let result = run_loop(&context, vec![ChatMessage::user("start")], &mut backend)
            .await
            .unwrap();

        assert_eq!(result, "Done, subagent found the answer.");
    }
}
