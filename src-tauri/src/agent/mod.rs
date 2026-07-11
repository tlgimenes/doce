//! Agent tool-use loop orchestrator (User Story 3, FR-009/FR-013/FR-015/
//! FR-016), wired to real inference + real tools via
//! `commands::agent::send_agent_message`. The loop's control flow (turn
//! counting, tool dispatch, subagent-nesting rejection, response parsing)
//! is real and tested; a known simplification is called out where it
//! lives (`commands/agent.rs`'s doc comment): turns run synchronously to
//! completion rather than streaming tokens live.
//!
//! Tool calls are prompted for (`SYSTEM_PROMPT` below) *and*
//! grammar-constrained at generation time
//! (`InferenceEngine::generate`'s `allow_tool_calls`, a lazy GBNF grammar
//! that only activates once the model starts emitting `<tool_call>`) —
//! the model's JSON is guaranteed syntactically valid once that happens,
//! so `parse_response` below trusts it rather than defending against
//! malformed output. A model that never starts down that path at all just
//! answers in plain text, same as before.
//!
//! The calling convention is Qwen's own trained (Hermes-style) format —
//! tool signatures declared inside `<tools></tools>` in the system
//! message, calls emitted as JSON inside `<tool_call></tool_call>` tags —
//! rather than a bespoke convention the model has to learn in-context.
//! Small models converge dramatically better inside their training
//! distribution than outside it; the previous bespoke
//! `{"tool_call": ...}` JSON shape is still accepted by `parse_response`
//! as a fallback.

pub mod dispatch;
pub mod plan;
pub mod rich_content;
pub mod subagent;
pub mod tools;

/// Describes the built-in tool set in the model's own trained format —
/// this is the `system`-role message of every flat agent loop run (see
/// `run_loop`'s `initial_messages`), not raw text concatenated onto the
/// user's task. The `<tools>` block of JSON function signatures and the
/// `<tool_call>` emission instruction reproduce Qwen3's chat-template
/// tool wording as closely as possible: a small model reproduces formats
/// it was trained on far more reliably than formats taught in-context.
/// Empirical guidance earned against the real model (the Glob
/// wildcard-vs-filename-list mistake, "clarify only when genuinely
/// ambiguous") lives in each function's `description` field.
pub const SYSTEM_PROMPT: &str = r#"You are a coding and system agent with access to tools.

# Tools

You may call one or more functions to assist with the user query.

You are provided with function signatures within <tools></tools> XML tags:
<tools>
{"type": "function", "function": {"name": "Read", "description": "Read a file from disk.", "parameters": {"type": "object", "properties": {"file_path": {"type": "string"}, "offset": {"type": "number"}, "limit": {"type": "number"}}, "required": ["file_path"]}}}
{"type": "function", "function": {"name": "Write", "description": "Create or overwrite a file.", "parameters": {"type": "object", "properties": {"file_path": {"type": "string"}, "content": {"type": "string"}}, "required": ["file_path", "content"]}}}
{"type": "function", "function": {"name": "Edit", "description": "Targeted in-place edit: replace old_string with new_string inside the file.", "parameters": {"type": "object", "properties": {"file_path": {"type": "string"}, "old_string": {"type": "string"}, "new_string": {"type": "string"}, "replace_all": {"type": "boolean"}}, "required": ["file_path", "old_string", "new_string"]}}}
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

/// The convention the model is prompted to follow — Qwen's own trained
/// format: `{"name": ..., "arguments": ...}` JSON inside
/// `<tool_call></tool_call>` tags. Takes the *first* tag pair anywhere in
/// the text rather than requiring the whole response to be exactly one
/// call — found necessary against a real model in practice: it sometimes
/// runs on past the first call and appends a second one, or wraps the
/// call in a little prose, and a strict whole-string parse silently
/// degraded those into "the raw text became the final answer" instead of
/// actually calling the tool. The pre-native bespoke shape
/// (`{"tool_call": {...}}` as a bare JSON object) is still accepted as a
/// fallback. Anything matching neither is the final answer verbatim — a
/// model that doesn't follow the convention degrades to "always answers
/// directly", not a crash.
pub fn parse_response(text: &str) -> LoopStep {
    if let Some(inner) = first_tool_call_tag(text) {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(inner.trim()) {
            if let (Some(name), Some(arguments)) = (
                value.get("name").and_then(|n| n.as_str()),
                value.get("arguments"),
            ) {
                return LoopStep::ToolCall(ToolCall {
                    name: name.to_string(),
                    arguments: arguments.clone(),
                });
            }
        }
    }
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

/// The content of the first complete `<tool_call>...</tool_call>` pair,
/// if any. An unclosed opening tag (e.g. generation cut off by the
/// max-token cap) yields `None` — the caller's fallback/final-answer
/// handling deals with it.
fn first_tool_call_tag(text: &str) -> Option<&str> {
    let start = text.find("<tool_call>")? + "<tool_call>".len();
    let end = text[start..].find("</tool_call>")? + start;
    Some(&text[start..end])
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
/// What executing one tool call did to the loop. `Result` feeds the text
/// back as an ordinary tool result and the loop continues; `Finish` ends
/// the whole `run_loop` call with the given final answer. `Finish` exists
/// so a state-driven backend can put "the task is done" behind an
/// ordinary grammar-constrained tool call (the plan engine's `FinishTask`)
/// instead of relying on a free-text reply — free text is exactly what a
/// small model degrades into emitting for tool NAMES after long
/// repetitive stretches (observed for real: after twenty AddStep calls in
/// a row, the model answered the bare text "ResumeExecution", which ended
/// the task as a garbage final answer).
#[derive(Debug, Clone, PartialEq)]
pub enum ToolExecution {
    Result(String),
    Finish(String),
}

#[allow(async_fn_in_trait)]
pub trait AgentBackend {
    fn measure(&mut self, messages: &[ChatMessage]) -> u32;
    fn threshold(&self) -> u32;
    fn compact(&mut self, messages: &[ChatMessage]) -> Vec<ChatMessage>;
    async fn generate(&mut self, messages: Vec<ChatMessage>) -> String;
    async fn execute_tool(&mut self, tool_call_id: String, call: ToolCall) -> ToolExecution;
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
            LoopStep::Done(text) => {
                // A response that STARTED a tool call but never closed it
                // is a generation cut off by the max-token cap, not a real
                // final answer — the grammar guarantees any started call is
                // well-formed through its closing tag, so truncation is the
                // one way a garbage call escapes it. Feed a correction back
                // (consuming a turn) instead of silently ending the whole
                // task with the truncated text as the "answer" — observed
                // for real: a 20-step CreatePlan cut off mid-JSON became
                // the turn's final answer and the task ended at 0/20.
                if text.contains("<tool_call>") && !text.contains("</tool_call>") {
                    messages.push(ChatMessage::assistant(text));
                    messages.push(ChatMessage::user(
                        "Your tool call was cut off before the closing </tool_call> tag. Re-issue the complete call -- if it was long, make it more concise.",
                    ));
                    continue;
                }
                return Ok(text);
            }
            LoopStep::ToolCall(call) => {
                let tool_call_id = uuid::Uuid::now_v7().to_string();
                let tool_name = call.name.clone();
                let arguments = call.arguments.clone();
                messages.push(ChatMessage::tool_use(
                    tool_call_id.clone(),
                    tool_name.clone(),
                    arguments,
                ));
                let execution = if call.name == "Task" && context.is_subagent {
                    // FR-016: one-level nesting — a subagent cannot itself
                    // spawn a further subagent. Fed back as an ordinary
                    // tool-error result rather than aborting the loop, so
                    // the model can recover (e.g. do the work itself).
                    ToolExecution::Result(
                        "Error: subagents cannot spawn further subagents (one-level nesting limit)"
                            .to_string(),
                    )
                } else {
                    backend.execute_tool(tool_call_id.clone(), call).await
                };
                match execution {
                    ToolExecution::Finish(answer) => return Ok(answer),
                    ToolExecution::Result(result) => {
                        messages.push(ChatMessage::tool_result(tool_call_id, tool_name, result));
                    }
                }
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
        on_execute: Box<dyn FnMut(String, ToolCall) -> ToolExecution>,
    }

    impl FakeBackend {
fn new(
            responses: Vec<String>,
            on_execute: impl FnMut(String, ToolCall) -> ToolExecution + 'static,
        ) -> Self {
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

        async fn execute_tool(&mut self, tool_call_id: String, call: ToolCall) -> ToolExecution {
            (self.on_execute)(tool_call_id, call)
        }
    }

    #[test]
    fn parses_a_native_format_tool_call() {
        // Qwen3's trained (Hermes-style) format: JSON inside
        // <tool_call></tool_call> XML tags.
        let text =
            "<tool_call>\n{\"name\": \"Read\", \"arguments\": {\"file_path\": \"/tmp/f.txt\"}}\n</tool_call>";
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
    fn parses_the_legacy_json_format_as_a_fallback() {
        // The pre-native bespoke convention -- kept parseable so an
        // off-distribution reply degrades into a working call instead of
        // a garbage final answer.
        let text = r#"{"tool_call": {"name": "Read", "arguments": {"file_path": "/tmp/f.txt"}}}"#;
        assert_eq!(
            parse_response(text),
            LoopStep::ToolCall(ToolCall {
                name: "Read".to_string(),
                arguments: serde_json::json!({"file_path": "/tmp/f.txt"}),
            })
        );
    }

    #[test]
    fn an_unclosed_tool_call_tag_is_a_final_answer_not_a_crash() {
        // e.g. generation cut off by the max-token cap mid-call.
        let text = "<tool_call>\n{\"name\": \"Read\", \"arguments\": {\"file_p";
        assert_eq!(parse_response(text), LoopStep::Done(text.to_string()));
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
        let text = "<tool_call>\n{\"name\": \"Read\"}\n</tool_call>"; // missing "arguments"
        assert_eq!(parse_response(text), LoopStep::Done(text.to_string()));
    }

    #[test]
    fn extracts_the_first_tool_call_when_the_model_appends_a_second_one() {
        // Real behavior observed against the actual installed model: it ran
        // on past the first tool call and appended a second, unrelated one
        // on the same line, which a strict whole-string JSON parse
        // rejected entirely (falling back to "the raw JSON is the final
        // answer" — the tool never actually got called).
        let text = "<tool_call>\n{\"name\": \"Read\", \"arguments\": {\"file_path\": \"/a.txt\"}}\n</tool_call>\n<tool_call>\n{\"name\": \"Grep\", \"arguments\": {\"pattern\": \"x\"}}\n</tool_call>";
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
        let text = "Sure, let me check that file.\n<tool_call>\n{\"name\": \"Read\", \"arguments\": {\"file_path\": \"/a.txt\"}}\n</tool_call>\nLet me know if that's not what you wanted.";
        assert_eq!(
            parse_response(text),
            LoopStep::ToolCall(ToolCall {
                name: "Read".to_string(),
                arguments: serde_json::json!({"file_path": "/a.txt"}),
            })
        );
    }

    #[test]
    fn a_closing_brace_inside_a_string_argument_does_not_end_the_call_early() {
        let text = "<tool_call>\n{\"name\": \"Write\", \"arguments\": {\"file_path\": \"/a.txt\", \"content\": \"func f() { return 1; }\"}}\n</tool_call>";
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
                "<tool_call>\n{\"name\": \"Read\", \"arguments\": {\"file_path\": \"/f.txt\"}}\n</tool_call>".to_string(),
                "The file says hello.".to_string(),
            ],
            |_tool_call_id, call| {
                assert_eq!(call.name, "Read");
                ToolExecution::Result("hello".to_string())
            },
        );

        let result = run_loop(&context, vec![ChatMessage::user("start")], &mut backend)
            .await
            .unwrap();

        assert_eq!(result, "The file says hello.");
    }

    #[tokio::test]
    async fn a_truncated_tool_call_gets_a_correction_turn_instead_of_becoming_the_final_answer() {
        // The grammar guarantees a STARTED tool call is well-formed to its
        // closing tag -- the one hole is the max-token cap cutting
        // generation off mid-call. Observed for real: a 20-step CreatePlan
        // truncated mid-JSON silently became the whole turn's "final
        // answer" (benchmark scored 0/20 at turn 2). The loop must feed a
        // correction back instead.
        let context = AgentContext::top_level();
        let executed = std::sync::Arc::new(AtomicU32::new(0));
        let executed_clone = executed.clone();
        let mut backend = FakeBackend::new(
            vec![
                "<tool_call>\n{\"name\": \"Read\", \"arguments\": {\"file_p".to_string(),
                "<tool_call>\n{\"name\": \"Read\", \"arguments\": {\"file_path\": \"/f.txt\"}}\n</tool_call>"
                    .to_string(),
                "Recovered and done.".to_string(),
            ],
            move |_tool_call_id, call| {
                assert_eq!(call.name, "Read");
                executed_clone.fetch_add(1, Ordering::SeqCst);
                ToolExecution::Result("hello".to_string())
            },
        );

        let result = run_loop(&context, vec![ChatMessage::user("start")], &mut backend)
            .await
            .unwrap();

        assert_eq!(result, "Recovered and done.");
        assert_eq!(
            executed.load(Ordering::SeqCst),
            1,
            "the re-issued call must actually execute"
        );
    }

    #[tokio::test]
    async fn a_finish_execution_ends_the_loop_with_that_answer() {
        // The plan engine's FinishTask: "done" is an ordinary
        // grammar-constrained tool call, so a state that requires tool
        // calls can still end the task without free-text replies.
        let context = AgentContext::top_level();
        let mut backend = FakeBackend::new(
            vec![
                "<tool_call>\n{\"name\": \"FinishTask\", \"arguments\": {\"answer\": \"all verified done\"}}\n</tool_call>"
                    .to_string(),
            ],
            |_tool_call_id, call| {
                assert_eq!(call.name, "FinishTask");
                ToolExecution::Finish("all verified done".to_string())
            },
        );

        let result = run_loop(&context, vec![ChatMessage::user("start")], &mut backend)
            .await
            .unwrap();
        assert_eq!(result, "all verified done");
    }

    #[tokio::test]
    async fn exceeding_the_turn_cap_is_an_error_not_an_infinite_loop() {
        let context = AgentContext {
            is_subagent: false,
            max_turns: 3,
            cwd: None,
        };

        let mut backend = FakeBackend::new(
            vec!["<tool_call>\n{\"name\": \"Read\", \"arguments\": {}}\n</tool_call>".to_string()],
            |_tool_call_id, _call| ToolExecution::Result("ok".to_string()),
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
                "<tool_call>\n{\"name\": \"Task\", \"arguments\": {\"prompt\": \"delegate further\"}}\n</tool_call>".to_string(),
                "I'll handle it myself.".to_string(),
            ],
            move |_tool_call_id, _call| {
                // A real subagent-nesting rejection never reaches here —
                // `run_loop` intercepts `Task` calls under `is_subagent`
                // before calling `execute_tool` at all.
                executed_tools_clone.fetch_add(1, Ordering::SeqCst);
                ToolExecution::Result("should not run".to_string())
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
                "<tool_call>\n{\"name\": \"Task\", \"arguments\": {\"prompt\": \"go do research\"}}\n</tool_call>".to_string(),
                "Done, subagent found the answer.".to_string(),
            ],
            |_tool_call_id, call| {
                assert_eq!(call.name, "Task");
                ToolExecution::Result("subagent result: 42".to_string())
            },
        );

        let result = run_loop(&context, vec![ChatMessage::user("start")], &mut backend)
            .await
            .unwrap();

        assert_eq!(result, "Done, subagent found the answer.");
    }
}
