//! Agent tool-use loop orchestrator (User Story 3, FR-009/FR-013/FR-015/
//! FR-016), wired to real inference + real tools via
//! `commands::agent::send_agent_message`. The loop's control flow (turn
//! counting, tool dispatch, subagent-nesting rejection) is real and
//! tested; a known simplification is called out where it lives
//! (`commands/agent.rs`'s doc comment): turns run synchronously to
//! completion rather than streaming tokens live.
//!
//! Since the llama-server cutover, generation goes through
//! `inference::http::LlamaServerClient::chat`, which returns a STRUCTURED
//! `TurnOutcome` (a resolved tool call, not text to parse), so the loop
//! reads `outcome.tool_call` directly rather than scraping `<tool_call>`
//! tags out of a generated string. Tools are advertised to the server
//! structurally (`inference::http::tools_array`) and constrained via
//! `tool_choice`, not by a hand-rolled grammar.

pub mod dispatch;
pub mod plan;
pub mod rich_content;
pub mod subagent;
pub mod tools;

// NOTE: the flat ReAct system prompt that used to live here as
// `SYSTEM_PROMPT` was moved to tests/agent_tasks.rs
// (`FLAT_BASELINE_SYSTEM_PROMPT`) on 2026-07-12: no production code
// referenced it -- every shipped conversation runs the plan machine
// (`plan::single_mode_system_prompt` via `commands::agent::plan_system_message`)
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
    /// The turn was cancelled (`chat` returned `InferenceError::Cancelled`
    /// because its `CancellationToken` fired). Distinct from `error`: a
    /// cancelled turn is an INTENTIONAL user-requested stop, not a fault —
    /// run_loop halts the loop with `AgentError::Cancelled` (no retry,
    /// nothing persisted as an answer), rather than surfacing an "Error:"
    /// banner or a garbage final string.
    pub cancelled: bool,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum AgentError {
    #[error("agent loop exceeded its {0}-turn cap without producing a final answer")]
    TurnCapExceeded(u32),
    /// The turn was stopped by a `CancellationToken` (the `stop_generation`
    /// command). An intentional halt, NOT a failure: `send_agent_message`
    /// finalizes it quietly (no persisted answer, no error banner) and the
    /// `Task`-tool subagent path folds it into a benign stopped tool result.
    #[error("agent loop cancelled")]
    Cancelled,
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

/// The caller-specific behavior `run_loop`'s control flow depends on,
/// bundled into one trait rather than four separate closure parameters —
/// production implements this once per call site (a `RealBackend` for the
/// top-level loop, a `SubagentBackend` for the `Task`-tool's delegated
/// loop, both in `commands::agent`), tests implement one small
/// `FakeBackend`. Kept as a trait (not hardcoded against the real
/// server/tool dispatch) specifically so `run_loop`'s own
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
/// What executing one tool call produced — `run_loop`'s per-turn
/// tool-dispatch result. `Result` feeds the text back as an ordinary tool
/// result and the loop continues; `Finish` ends the whole `run_loop` call
/// with the given final answer, reached via an ordinary structured tool
/// call (the plan engine's `FinishTask`) rather than a free-text reply —
/// the server enforces exactly one tool call per turn via `tool_choice`.
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
    /// sampler (`ToolCallMode::Require`, mapped to the server's
    /// `tool_choice:"required"`). The production plan loop — top-level
    /// (`RealBackend`) and subagent (`SubagentBackend`) — does, so a turn
    /// that returned NO tool call is a retriable correction, never a
    /// finished task: run_loop feeds a correction message and retries
    /// rather than ending the task, because under Require the only
    /// legitimate finish is a `FinishTask` tool call (dispatched to
    /// `ToolExecution::Finish`). When `false` (Allow/Forbid — the scripted
    /// unit-test backends whose tests end on a plain-text final answer), a
    /// no-tool-call turn is an ordinary final answer. Defaults to `true`:
    /// the production path is Require, and defaulting a new backend to
    /// "correct-and-retry, don't end the task as garbage" is the safe
    /// failure mode.
    fn requires_tool_call(&self) -> bool {
        true
    }
}

/// Runs the tool-use loop to completion: repeatedly generates a response,
/// executes any tool call it contains, and feeds the result back in, until
/// the model produces a plain-text final answer or the turn cap is hit.
///
/// `initial_messages` is a real role-tagged conversation (typically a
/// `system` message from `commands::agent::plan_system_message` plus a
/// `user` message with the task) rather than one flat string — `backend.generate` is expected to
/// render this through the model's own chat template (the llama-server
/// sidecar applies it to the OpenAI `messages`) before tokenizing,
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

        // Graceful cancellation: the turn was stopped mid-flight (the
        // `stop_generation` command fired this turn's `CancellationToken`).
        // This is an INTENTIONAL halt, not a fault — stop the loop AT ONCE
        // with a distinct error the caller finalizes quietly: no retry, no
        // further `generate` call, nothing persisted as an answer. Checked
        // right after the transport-error terminator and before the
        // tool_call match, so a cancel never masquerades as a no-tool-call
        // retry or an ordinary final answer.
        if outcome.cancelled {
            return Err(AgentError::Cancelled);
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
                //     garbage" failure this invariant prevents.
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

    /// Builds a `TurnOutcome` for a single tool call - the structured
    /// equivalent of the Hermes `<tool_call>` strings these run_loop tests
    /// used to round-trip through the (now-deleted) `parse_response`; the
    /// loop reads `outcome.tool_call` directly since the cutover.
    fn tool_outcome(name: &str, arguments: serde_json::Value) -> TurnOutcome {
        TurnOutcome {
            tool_call: Some((name.to_string(), arguments)),
            text: String::new(),
            reasoning: String::new(),
            finish_reason: "tool_calls".to_string(),
            usage: None,
            error: None,
            cancelled: false,
        }
    }

    /// Builds a `TurnOutcome` for a plain-text final answer (no tool call).
    fn text_outcome(text: &str) -> TurnOutcome {
        TurnOutcome {
            tool_call: None,
            text: text.to_string(),
            reasoning: String::new(),
            finish_reason: "stop".to_string(),
            usage: None,
            error: None,
            cancelled: false,
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
        responses: Vec<TurnOutcome>,
        call_index: usize,
        on_execute: Box<dyn FnMut(String, ToolCall) -> ToolExecution>,
    }

    impl FakeBackend {
        fn new(
            responses: Vec<TurnOutcome>,
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
            response
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
                tool_outcome("Grep", serde_json::json!({"pattern": "x"}))
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

    #[tokio::test]
    async fn loop_runs_tools_until_a_final_answer() {
        let context = AgentContext::top_level();
        let mut backend = FakeBackend::new(
            vec![
                tool_outcome("Read", serde_json::json!({"file_path": "/f.txt"})),
                text_outcome("The file says hello."),
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
        // "ended the task as garbage" failure this invariant prevents).
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
                    cancelled: false,
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
                    cancelled: false,
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
                    cancelled: false,
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
                    cancelled: false,
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
                    cancelled: false,
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
                    cancelled: false,
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
    async fn a_cancelled_turn_halts_the_loop_at_once_without_a_further_generate() {
        // Graceful cancellation (Task 4.2a): a turn whose `chat` was stopped
        // by its `CancellationToken` comes back `cancelled: true`. run_loop
        // must halt IMMEDIATELY with `AgentError::Cancelled` -- no retry, no
        // second `generate`, no tool execution -- so the caller can finalize
        // the stop quietly rather than surfacing an error or a garbage answer.
        let context = AgentContext::top_level();
        let mut backend = ScriptedBackend {
            outcomes: vec![
                TurnOutcome {
                    tool_call: None,
                    text: String::new(),
                    reasoning: String::new(),
                    finish_reason: String::new(),
                    usage: None,
                    error: None,
                    cancelled: true,
                },
                // Must never be reached: a cancel stops the loop at once.
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
                    cancelled: false,
                },
            ],
            call_index: 0,
            generate_calls: 0,
            last_user_text: None,
            on_execute: Box::new(|_tool_call_id, _call| {
                panic!("a cancelled turn must halt before any tool executes")
            }),
        };

        let result = run_loop(&context, vec![ChatMessage::user("start")], &mut backend).await;

        assert_eq!(result, Err(AgentError::Cancelled));
        assert_eq!(
            backend.generate_calls, 1,
            "a cancelled turn must halt at once -- exactly one generate call, no retry"
        );
    }

    #[tokio::test]
    async fn a_finish_execution_ends_the_loop_with_that_answer() {
        // The plan engine's FinishTask: "done" is an ordinary
        // grammar-constrained tool call, so a state that requires tool
        // calls can still end the task without free-text replies.
        let context = AgentContext::top_level();
        let mut backend = FakeBackend::new(
            vec![tool_outcome(
                "FinishTask",
                serde_json::json!({"answer": "all verified done"}),
            )],
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
            vec![tool_outcome("Read", serde_json::json!({}))],
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
                tool_outcome("Task", serde_json::json!({"prompt": "delegate further"})),
                text_outcome("I'll handle it myself."),
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
                tool_outcome("Task", serde_json::json!({"prompt": "go do research"})),
                text_outcome("Done, subagent found the answer."),
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
