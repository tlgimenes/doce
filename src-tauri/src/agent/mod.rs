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

// NOTE: the flat ReAct system prompt that used to live here as
// `SYSTEM_PROMPT` was moved to tests/agent_tasks.rs
// (`FLAT_BASELINE_SYSTEM_PROMPT`) on 2026-07-12: no production code
// referenced it -- every shipped conversation runs the plan machine
// (`plan::plan_system_prompt` via `commands::agent::plan_system_message`)
// -- yet its presence in src let the tier-0 tests read as covering the
// app while actually exercising a dead path (how the "ola" doom loop
// shipped green).

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

/// One turn's structured generation result — what `AgentBackend::generate`
/// now returns instead of a raw `String` the loop had to re-parse. Since
/// the cutover, generation goes through `inference::http::LlamaServerClient::
/// chat`, which hands back a *structured* `ChatOutcome` (a resolved tool
/// call, not text to grammar-parse); the fields here mirror it one-to-one
/// (see `ChatOutcome`), plus `error` for the transport-failure case that the
/// in-process engine used to fold into its returned string.
#[derive(Debug, Clone, PartialEq)]
pub struct TurnOutcome {
    /// The single tool call the model made this turn, already resolved to
    /// `(name, arguments)` — `None` when the model produced no call (a
    /// plain-text answer, or, under Require, an invariant-violating turn the
    /// loop corrects and retries). Requests set `parallel_tool_calls:false`,
    /// so this is at most one call.
    pub tool_call: Option<(String, serde_json::Value)>,
    /// The assistant's final text — used only when this turn is NOT a tool
    /// call (an Allow/Forbid final answer, or the assistant text carried
    /// into a Require correction turn).
    pub text: String,
    /// Streamed `<think>`-equivalent reasoning (already emitted live via the
    /// `on_piece` ticker during generation) — carried for completeness; the
    /// loop does not read it directly.
    pub reasoning: String,
    /// Why generation stopped: `"stop"`, `"tool_calls"`, `"length"`, … — the
    /// loop keys the Require-mode correction on `"length"` (re-issue briefly)
    /// vs. everything else (require exactly one tool call).
    pub finish_reason: String,
    /// `(prompt_tokens, completion_tokens)` for later token accounting — not
    /// wired into the loop yet.
    pub usage: Option<(u32, u32)>,
    /// A HARD transport/server failure (`chat` returned `Err`). When `Some`,
    /// run_loop TERMINATES the turn surfacing this text as the final answer
    /// — it must NEVER be retried (a no-tool-call *success* under Require is
    /// what retries; a dead server would otherwise loop forever). Mirrors the
    /// pre-cutover behavior where `"Error: inference failed: {e}"` became the
    /// final string.
    pub error: Option<String>,
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
pub fn parse_response(text: &str, dialect: crate::inference::ToolDialect) -> LoopStep {
    // The dialect owns the primary extraction (tool-dialects design):
    // Hermes keeps the historical first-tag-pair-anywhere behavior;
    // MiniCPM parses its `<function>` XML.
    if let Some((name, arguments)) = dialect.parse_first(text) {
        return LoopStep::ToolCall(ToolCall { name, arguments });
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
    async fn generate(&mut self, messages: Vec<ChatMessage>) -> TurnOutcome;
    async fn execute_tool(&mut self, tool_call_id: String, call: ToolCall) -> ToolExecution;

    /// Whether this backend generates with tool calls REQUIRED at the
    /// sampler (`ToolCallMode::Require`). The production plan loop — top-level
    /// (`RealBackend`) and subagent (`SubagentBackend`) — does, so a turn
    /// that returned NO tool call is an INVARIANT VIOLATION: run_loop feeds a
    /// correction and retries rather than ending the task, because under
    /// Require the only legitimate finish is a `FinishTask` tool call
    /// (dispatched to `ToolExecution::Finish`). When `false` (Allow/Forbid —
    /// the flat benchmark harness, and the scripted unit-test backends whose
    /// tests end on a plain-text final answer), a no-tool-call turn is an
    /// ordinary `LoopStep::Done`. Defaults to `true`: the production path is
    /// Require, and defaulting a new backend to "correct-and-retry, don't end
    /// the task as garbage" is the safe failure mode (the exact regression
    /// the old grammar prevented).
    fn requires_tool_call(&self) -> bool {
        true
    }

    /// The active model's tool-call dialect — retained for the pre-cutover
    /// benchmark backends that still adapt `engine.generate`'s String via
    /// `parse_response` (flipped to the client in Task 8.1). run_loop no
    /// longer reads it (generation is structured now), but leaving the
    /// default here keeps those backends' overrides valid. Defaults to Hermes
    /// (the historical assumption).
    fn dialect(&self) -> crate::inference::ToolDialect {
        crate::inference::ToolDialect::HermesJson
    }
}

/// Runs the tool-use loop to completion: repeatedly generates a response,
/// executes any tool call it contains, and feeds the result back in, until
/// the model produces a plain-text final answer or the turn cap is hit.
///
/// `initial_messages` is a real role-tagged conversation (typically a
/// `system` message from `commands::agent::plan_system_message` plus a
/// `user` message with the task) rather than one flat string — `backend.generate` is expected to
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

    // Futile-repetition breaker: the last (call, raw result) exchange and
    // how many consecutive turns it has repeated EXACTLY. A small model
    // that has stopped acting re-issues the identical call verbatim
    // (observed 2026-07-12: tier4 re-ran the same Grep 50 turns straight,
    // one file from done, burning the whole turn budget) -- when the same
    // call keeps producing the same result, the result itself must say
    // that repeating it is pointless. Compared on the RAW result, before
    // the note is appended, so the streak survives its own annotation.
    let mut last_exchange: Option<(ToolCall, String)> = None;
    let mut futile_streak: u32 = 0;

    for _turn in 0..context.max_turns {
        if backend.measure(&messages) > backend.threshold() {
            messages = backend.compact(&messages);
        }
        let outcome = backend.generate(messages.clone()).await;

        // A HARD transport/server failure (`chat` returned `Err`) TERMINATES
        // the turn, surfacing the error text as the final answer — exactly
        // the pre-cutover behavior where `"Error: inference failed: {e}"`
        // became the returned string. This is checked FIRST and never
        // retried: because a no-tool-call Require turn now *retries* (below),
        // a dead server must not become an infinite retry loop.
        if let Some(error) = outcome.error {
            return Ok(error);
        }

        let call = match outcome.tool_call {
            Some((name, arguments)) => ToolCall { name, arguments },
            None => {
                // No tool call this turn. Under Require (the production plan
                // loop) that is an INVARIANT VIOLATION — the model was
                // required to call a tool and didn't — never a finished task:
                // the only legitimate finish is a `FinishTask` tool call
                // (handled in the `ToolExecution::Finish` arm below). Feed a
                // correction and RETRY (consuming a turn, bounded by the same
                // turn cap + futile-streak guards), re-homing the old
                // truncated-generation recovery:
                //   - `finish_reason == "length"`: the generation spent its
                //     whole token budget (typically inside <think>) before
                //     emitting a call — ask it to keep thinking brief and end
                //     with one call. Observed for real: a 20-step CreatePlan
                //     cut off mid-JSON, and (MiniCPM5-1B) a zero-length reply.
                //   - otherwise (e.g. "stop" but no call): require exactly one
                //     tool call. Letting an empty/among-text required turn
                //     masquerade as Done is the exact "ended the task as
                //     garbage" failure the old grammar prevented.
                if backend.requires_tool_call() {
                    let correction = if outcome.finish_reason == "length" {
                        "Your response was cut off inside your reasoning, before any tool call was produced. Keep your thinking brief this time and end with exactly one tool call."
                    } else {
                        "You must respond with exactly one tool call."
                    };
                    messages.push(ChatMessage::assistant(outcome.text));
                    messages.push(ChatMessage::user(correction));
                    continue;
                }
                // Allow/Forbid: a plain-text reply is the final answer.
                return Ok(outcome.text);
            }
        };

        let tool_call_id = uuid::Uuid::now_v7().to_string();
        let tool_name = call.name.clone();
        let arguments = call.arguments.clone();
        messages.push(ChatMessage::tool_use(
            tool_call_id.clone(),
            tool_name.clone(),
            arguments.clone(),
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
            ToolExecution::Result(mut result) => {
                let call_now = ToolCall {
                    name: tool_name.clone(),
                    arguments,
                };
                futile_streak = match &last_exchange {
                    Some((prev_call, prev_result))
                        if *prev_call == call_now && *prev_result == result =>
                    {
                        futile_streak + 1
                    }
                    _ => 1,
                };
                last_exchange = Some((call_now, result.clone()));
                if futile_streak >= 3 {
                    result.push_str(&format!(
                        "\n\nNote: this exact call has now returned this exact result {futile_streak} times in a row. Repeating it changes nothing -- act on this result, or take a different action."
                    ));
                }
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

    /// Adapts a canned model STRING into the structured `TurnOutcome` the
    /// backend contract now returns, by running the same `parse_response`
    /// the pre-cutover loop used — so string-scripted test backends
    /// (`FakeBackend`, `RepeatBackend`) keep expressing their intent as
    /// Hermes `<tool_call>` text without a semantic rewrite. Mirrors the
    /// benchmark backends' adaptation in `tests/agent_tasks.rs`.
    fn outcome_from_string(s: &str) -> TurnOutcome {
        match parse_response(s, crate::inference::ToolDialect::HermesJson) {
            LoopStep::ToolCall(tc) => TurnOutcome {
                tool_call: Some((tc.name, tc.arguments)),
                text: String::new(),
                reasoning: String::new(),
                finish_reason: "tool_calls".to_string(),
                usage: None,
                error: None,
            },
            LoopStep::Done(text) => TurnOutcome {
                tool_call: None,
                text,
                reasoning: String::new(),
                finish_reason: "stop".to_string(),
                usage: None,
                error: None,
            },
        }
    }

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

        // These tool-dispatch/nesting flow tests end on a plain-text final
        // answer (Allow semantics), so a no-tool-call turn is `Done`, not a
        // Require-mode invariant violation. The Require-mode invariant is
        // covered by `ScriptedBackend`'s tests below.
        fn requires_tool_call(&self) -> bool {
            false
        }

        async fn generate(&mut self, _messages: Vec<ChatMessage>) -> TurnOutcome {
            let response = self.responses[self.call_index.min(self.responses.len() - 1)].clone();
            self.call_index += 1;
            outcome_from_string(&response)
        }

        async fn execute_tool(&mut self, tool_call_id: String, call: ToolCall) -> ToolExecution {
            (self.on_execute)(tool_call_id, call)
        }
    }

    /// Test-only Require-mode `AgentBackend`: returns a QUEUE of pre-built
    /// `TurnOutcome`s (repeating the last once exhausted), tracks how many
    /// times `generate` was called, and records the last user-role message
    /// text it saw — so a test can assert the loop RETRIED (>1 generate call)
    /// and which correction it fed back. `requires_tool_call` defaults to
    /// `true`, exercising the production plan-loop semantics.
    struct ScriptedBackend {
        outcomes: Vec<TurnOutcome>,
        call_index: usize,
        generate_calls: u32,
        last_user_text: Option<String>,
        on_execute: Box<dyn FnMut(String, ToolCall) -> ToolExecution>,
    }

    impl AgentBackend for ScriptedBackend {
        fn measure(&mut self, _messages: &[ChatMessage]) -> u32 {
            0
        }

        fn threshold(&self) -> u32 {
            u32::MAX
        }

        fn compact(&mut self, _messages: &[ChatMessage]) -> Vec<ChatMessage> {
            panic!("compact should never run in this test")
        }

        async fn generate(&mut self, messages: Vec<ChatMessage>) -> TurnOutcome {
            self.generate_calls += 1;
            self.last_user_text = messages.iter().rev().find_map(|m| match &m.content {
                crate::inference::MessageContent::Text(t) if m.role == "user" => Some(t.clone()),
                _ => None,
            });
            let outcome = self.outcomes[self.call_index.min(self.outcomes.len() - 1)].clone();
            self.call_index += 1;
            outcome
        }

        async fn execute_tool(&mut self, tool_call_id: String, call: ToolCall) -> ToolExecution {
            (self.on_execute)(tool_call_id, call)
        }
    }

    /// The futile-repetition breaker: the same call returning the same
    /// result 3+ turns in a row gets the result annotated so the model is
    /// told, in the freshest context, that repeating is pointless
    /// (observed 2026-07-12: tier4 re-ran one identical Grep for ~50
    /// turns, one file from done, to the turn cap).
    #[tokio::test]
    async fn run_loop_annotates_futile_identical_call_repetition() {
        struct RepeatBackend {
            last_result_seen: Option<String>,
        }
        impl AgentBackend for RepeatBackend {
            fn measure(&mut self, _messages: &[ChatMessage]) -> u32 {
                0
            }
            fn threshold(&self) -> u32 {
                u32::MAX
            }
            fn compact(&mut self, _messages: &[ChatMessage]) -> Vec<ChatMessage> {
                panic!("compact should never run in this test")
            }
            async fn generate(&mut self, messages: Vec<ChatMessage>) -> TurnOutcome {
                if let Some(content) = messages.iter().rev().find_map(|m| match &m.content {
                    crate::inference::MessageContent::ToolResult { content, .. } => {
                        Some(content.clone())
                    }
                    _ => None,
                }) {
                    self.last_result_seen = Some(content);
                }
                outcome_from_string(
                    "<tool_call>\n{\"name\": \"Grep\", \"arguments\": {\"pattern\": \"x\"}}\n</tool_call>",
                )
            }
            async fn execute_tool(
                &mut self,
                _tool_call_id: String,
                _call: ToolCall,
            ) -> ToolExecution {
                ToolExecution::Result("same result".to_string())
            }
        }

        let mut backend = RepeatBackend {
            last_result_seen: None,
        };
        let context = AgentContext {
            is_subagent: false,
            max_turns: 5,
            cwd: None,
        };
        let result = run_loop(&context, vec![ChatMessage::user("go")], &mut backend).await;
        assert!(matches!(result, Err(AgentError::TurnCapExceeded(5))));

        let last = backend.last_result_seen.expect("tool results were seen");
        assert!(
            last.contains("times in a row")
                && last.contains("act on this result")
                && last.starts_with("same result"),
            "the 3rd+ identical exchange must carry the futility note appended to the raw result, got: {last:?}"
        );
    }

    #[test]
    fn parses_a_native_format_tool_call() {
        // Qwen3's trained (Hermes-style) format: JSON inside
        // <tool_call></tool_call> XML tags.
        let text =
            "<tool_call>\n{\"name\": \"Read\", \"arguments\": {\"file_path\": \"/tmp/f.txt\"}}\n</tool_call>";
        let step = parse_response(text, crate::inference::ToolDialect::HermesJson);
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
            parse_response(text, crate::inference::ToolDialect::HermesJson),
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
        assert_eq!(
            parse_response(text, crate::inference::ToolDialect::HermesJson),
            LoopStep::Done(text.to_string())
        );
    }

    #[test]
    fn plain_text_is_a_final_answer() {
        assert_eq!(
            parse_response(
                "The answer is 4.",
                crate::inference::ToolDialect::HermesJson
            ),
            LoopStep::Done("The answer is 4.".to_string())
        );
    }

    #[test]
    fn malformed_json_falls_back_to_plain_text() {
        let text = "<tool_call>\n{\"name\": \"Read\"}\n</tool_call>"; // missing "arguments"
        assert_eq!(
            parse_response(text, crate::inference::ToolDialect::HermesJson),
            LoopStep::Done(text.to_string())
        );
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
            parse_response(text, crate::inference::ToolDialect::HermesJson),
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
            parse_response(text, crate::inference::ToolDialect::HermesJson),
            LoopStep::ToolCall(ToolCall {
                name: "Read".to_string(),
                arguments: serde_json::json!({"file_path": "/a.txt"}),
            })
        );
    }

    #[test]
    fn a_closing_brace_inside_a_string_argument_does_not_end_the_call_early() {
        let text = "<tool_call>\n{\"name\": \"Write\", \"arguments\": {\"file_path\": \"/a.txt\", \"content\": \"func f() { return 1; }\"}}\n</tool_call>";
        let step = parse_response(text, crate::inference::ToolDialect::HermesJson);
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
        assert_eq!(
            parse_response(text, crate::inference::ToolDialect::HermesJson),
            LoopStep::Done(text.to_string())
        );
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
    async fn a_required_turn_with_no_tool_call_retries_instead_of_becoming_the_final_answer() {
        // THE HEART of the cutover invariant: under Require (the production
        // plan loop), a successful generate that produced NO tool call is an
        // invariant violation -- the model was required to call a tool and
        // didn't. It must NEVER masquerade as a finished task (the exact
        // "ended the task as garbage" failure the old grammar prevented).
        // The loop feeds a correction and RETRIES; the only legitimate finish
        // is a later FinishTask tool call.
        let context = AgentContext::top_level();
        let mut backend = ScriptedBackend {
            outcomes: vec![
                // A "stop"-finish turn with free text but no tool call.
                TurnOutcome {
                    tool_call: None,
                    text: "here is my final answer".to_string(),
                    reasoning: String::new(),
                    finish_reason: "stop".to_string(),
                    usage: None,
                    error: None,
                },
                // A later turn finally calls FinishTask -- the ONLY way a
                // Require-mode loop ends with a final answer.
                TurnOutcome {
                    tool_call: Some((
                        "FinishTask".to_string(),
                        serde_json::json!({"answer": "verified done"}),
                    )),
                    text: String::new(),
                    reasoning: String::new(),
                    finish_reason: "tool_calls".to_string(),
                    usage: None,
                    error: None,
                },
            ],
            call_index: 0,
            generate_calls: 0,
            last_user_text: None,
            on_execute: Box::new(|_tool_call_id, call| {
                assert_eq!(call.name, "FinishTask");
                ToolExecution::Finish("verified done".to_string())
            }),
        };

        let result = run_loop(&context, vec![ChatMessage::user("start")], &mut backend)
            .await
            .unwrap();

        assert_eq!(result, "verified done");
        assert!(
            backend.generate_calls > 1,
            "the empty required turn must RETRY, not finish -- expected >1 generate call, got {}",
            backend.generate_calls
        );
        assert_eq!(
            backend.last_user_text.as_deref(),
            Some("You must respond with exactly one tool call."),
            "a non-length no-tool-call turn must get the generic 'exactly one tool call' correction"
        );
    }

    #[tokio::test]
    async fn a_length_finish_with_no_tool_call_triggers_the_brief_recovery_retry() {
        // The re-homed truncated-generation recovery: under Require, a turn
        // that stopped for `finish_reason == "length"` (spent its whole token
        // budget, typically inside <think>) before emitting a call gets the
        // "keep your thinking brief" correction and retries -- NOT the generic
        // one, and never a finished task. Observed for real: a 20-step
        // CreatePlan cut off mid-JSON, and a MiniCPM zero-length reply.
        let context = AgentContext::top_level();
        let mut backend = ScriptedBackend {
            outcomes: vec![
                TurnOutcome {
                    tool_call: None,
                    text: String::new(),
                    reasoning: String::new(),
                    finish_reason: "length".to_string(),
                    usage: None,
                    error: None,
                },
                TurnOutcome {
                    tool_call: Some((
                        "FinishTask".to_string(),
                        serde_json::json!({"answer": "recovered"}),
                    )),
                    text: String::new(),
                    reasoning: String::new(),
                    finish_reason: "tool_calls".to_string(),
                    usage: None,
                    error: None,
                },
            ],
            call_index: 0,
            generate_calls: 0,
            last_user_text: None,
            on_execute: Box::new(|_tool_call_id, _call| {
                ToolExecution::Finish("recovered".to_string())
            }),
        };

        let result = run_loop(&context, vec![ChatMessage::user("start")], &mut backend)
            .await
            .unwrap();

        assert_eq!(result, "recovered");
        assert!(
            backend.generate_calls > 1,
            "a length-truncated required turn must RETRY, not finish"
        );
        assert!(
            backend
                .last_user_text
                .as_deref()
                .unwrap_or_default()
                .contains("Keep your thinking brief"),
            "a length-finish no-tool-call turn must get the brief-recovery correction, got: {:?}",
            backend.last_user_text
        );
    }

    #[tokio::test]
    async fn a_transport_error_terminates_the_turn_and_never_retries() {
        // CORRECTNESS: because a no-tool-call Require turn now retries, a HARD
        // transport/server error must NOT become an infinite retry -- it
        // terminates the turn surfacing its text as the final answer, exactly
        // the pre-cutover behavior where "Error: inference failed: {e}" became
        // the returned string. The FinishTask outcome that follows must never
        // be reached.
        let context = AgentContext::top_level();
        let mut backend = ScriptedBackend {
            outcomes: vec![
                TurnOutcome {
                    tool_call: None,
                    text: String::new(),
                    reasoning: String::new(),
                    finish_reason: String::new(),
                    usage: None,
                    error: Some("Error: inference failed: connection refused".to_string()),
                },
                TurnOutcome {
                    tool_call: Some((
                        "FinishTask".to_string(),
                        serde_json::json!({"answer": "should never run"}),
                    )),
                    text: String::new(),
                    reasoning: String::new(),
                    finish_reason: "tool_calls".to_string(),
                    usage: None,
                    error: None,
                },
            ],
            call_index: 0,
            generate_calls: 0,
            last_user_text: None,
            on_execute: Box::new(|_tool_call_id, _call| {
                panic!("a transport error must terminate before any tool executes")
            }),
        };

        let result = run_loop(&context, vec![ChatMessage::user("start")], &mut backend)
            .await
            .unwrap();

        assert_eq!(result, "Error: inference failed: connection refused");
        assert_eq!(
            backend.generate_calls, 1,
            "a transport error must NOT retry -- exactly one generate call"
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
