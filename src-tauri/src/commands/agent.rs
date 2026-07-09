use crate::agent::rich_content::{expand_segments, RichMessageContent};
use crate::agent::tools::ask_user::{PendingQuestions, QuestionOption};
use crate::agent::{dispatch, run_loop, subagent, AgentContext, ToolCall, ToolExecution};
use crate::commands::conversations::{ActiveGenerations, InferenceState};
use crate::commands::models::now_ms;
use crate::inference::{ChatMessage, InferenceEngine, PromptSession};
use crate::storage::conversations::load_history;
use crate::storage::DbCell;
use rusqlite::Connection;
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

/// 004-tool-call-widgets (`001`'s originally-specified `ask-user-question`
/// event, implemented here — contracts/tool-widgets.md): fired the moment
/// the loop hits an `AskUserQuestion` call, so the frontend can show the
/// prompt while `send_agent_message`'s own promise is still pending — the
/// one live event this feature adds (research.md § 3).
#[derive(Debug, Clone, Serialize, specta::Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct AskUserQuestionEvent {
    pub conversation_id: String,
    pub question_id: String,
    pub header: String,
    pub question: String,
    pub options: Vec<QuestionOption>,
    pub multi_select: bool,
}

/// Streaming (loop-level, not token-level) UI updates: fired every time a
/// new row lands in `messages` for `conversation_id` during an agent turn
/// (a tool_call, its paired tool_result, or the final answer) — the
/// frontend's own signal to re-fetch `list_messages` and re-render, rather
/// than waiting for `send_agent_message`'s single promise to resolve at the
/// very end of the whole (up to 200-turn) loop. Deliberately just a
/// conversation id, not the message itself: the frontend already owns a
/// `list_messages` call for the initial load, and re-running that same
/// query is simpler and more robust here than keeping a second, ad hoc
/// message shape in sync with it.
#[derive(Debug, Clone, Serialize, specta::Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct AgentMessagePersisted {
    pub conversation_id: String,
}

/// Live plan state per conversation — the plan-tracker twin of
/// `ActiveGenerations`: in-memory, per-process, cleared by RAII at turn
/// end. `get_active_plan` reads it for mount/reload recovery; the
/// `plan-update` event streams changes while the turn runs.
#[derive(Default)]
pub struct ActivePlans(pub Mutex<HashMap<String, PlanSnapshot>>);

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PlanStepSnapshot {
    pub description: String,
    pub done: bool,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PlanSnapshot {
    pub goal: String,
    pub steps: Vec<PlanStepSnapshot>,
    /// `None` while Planning (between steps / during plan revision).
    pub current_step_index: Option<u32>,
}

/// Fired on every plan mutation during an agent turn, and once with
/// `plan: None` when the turn ends — the tracker's fade-out signal.
#[derive(Debug, Clone, Serialize, specta::Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct PlanUpdate {
    pub conversation_id: String,
    pub plan: Option<PlanSnapshot>,
}

fn plan_snapshot(state: &crate::agent::plan::PlanState) -> PlanSnapshot {
    PlanSnapshot {
        goal: state.plan.goal.clone(),
        steps: state
            .plan
            .steps
            .iter()
            .map(|s| PlanStepSnapshot {
                description: s.description.clone(),
                done: s.done,
            })
            .collect(),
        current_step_index: match state.state {
            crate::agent::plan::LoopState::Executing { step_index } => Some(step_index as u32),
            crate::agent::plan::LoopState::Planning => None,
        },
    }
}

/// Updates the live map and emits `plan-update` — called after every
/// handled plan tool. A state with no plan yet (trivial turns never call
/// CreatePlan) registers nothing, so the tracker never appears for them.
/// `app: Option<&AppHandle>` so unit tests exercise the map half without a
/// live Tauri app, mirroring `persist_tool_call`'s pattern.
fn publish_plan_update(
    app: Option<&AppHandle>,
    active_plans: &ActivePlans,
    conversation_id: &str,
    state: &crate::agent::plan::PlanState,
) {
    if !state.has_plan() {
        return;
    }
    let snapshot = plan_snapshot(state);
    active_plans
        .0
        .lock()
        .unwrap()
        .insert(conversation_id.to_string(), snapshot.clone());
    if let Some(app) = app {
        let _ = app.emit(
            "plan-update",
            PlanUpdate {
                conversation_id: conversation_id.to_string(),
                plan: Some(snapshot),
            },
        );
    }
}

/// Clears this conversation's live plan on every turn exit path and, if a
/// plan was actually registered, emits the `plan: None` fade-out event —
/// the plan-tracker twin of `ActiveGenerationGuard`.
struct ActivePlanGuard<'a> {
    active_plans: &'a ActivePlans,
    /// `None` in unit tests (no live Tauri app to emit through) — Drop
    /// still clears the map either way, only the emit half is skipped.
    app: Option<AppHandle>,
    conversation_id: String,
}

impl Drop for ActivePlanGuard<'_> {
    fn drop(&mut self) {
        let had_plan = self
            .active_plans
            .0
            .lock()
            .unwrap()
            .remove(&self.conversation_id)
            .is_some();
        if had_plan {
            if let Some(app) = &self.app {
                let _ = app.emit(
                    "plan-update",
                    PlanUpdate {
                        conversation_id: self.conversation_id.clone(),
                        plan: None,
                    },
                );
            }
        }
    }
}

/// Mount/reload recovery for the plan tracker — the same reload-proof
/// pattern as `conversations::is_generation_active`.
#[tauri::command]
#[specta::specta]
pub fn get_active_plan(
    active_plans: State<'_, ActivePlans>,
    conversation_id: String,
) -> Option<PlanSnapshot> {
    active_plans.0.lock().unwrap().get(&conversation_id).cloned()
}

/// Removes `conversation_id` from `ActiveGenerations` when dropped —
/// guarantees cleanup on every exit path (`?` early-returns included, not
/// just the happy path) without a manual `remove` call before each one.
struct ActiveGenerationGuard<'a> {
    active_generations: &'a ActiveGenerations,
    conversation_id: String,
}

impl Drop for ActiveGenerationGuard<'_> {
    fn drop(&mut self) {
        self.active_generations
            .0
            .lock()
            .unwrap()
            .remove(&self.conversation_id);
    }
}

/// 004-tool-call-widgets: persists a tool invocation's `tool_call` row
/// (role `assistant`, matching this project's existing convention for the
/// model's own action — see `commands/conversations.rs`'s `compute_status`
/// test fixtures) — the arguments alone, at the next available sequence
/// number. Split from `persist_tool_result` (rather than always inserting
/// both together) specifically so `AskUserQuestion` can leave this row as
/// the *latest* message for as long as it's genuinely pending — that's
/// what `compute_status`'s existing "latest message is a pending
/// AskUserQuestion tool_call" check relies on to report `requires_action`.
///
/// `plan` mirrors the same `"plan": true` marker `persist_plan_tool` has
/// always stamped onto this call's paired `tool_result` row (via its own
/// `detail`) — a review finding on the row-shape asymmetry: this row used
/// to persist bare `{"arguments": ...}` even for a plan-machine tool,
/// silently miscounting it as an ordinary/regular tool row by
/// `context::apply_lightweight_clearing`'s plan/regular partitioning,
/// which could push genuine tool history out of `TOOL_KEEP_N` prematurely.
/// `false` for every non-plan caller — the persisted shape for those is
/// byte-for-byte unchanged.
async fn persist_tool_call(
    app: Option<&AppHandle>,
    conn: &tokio_rusqlite::Connection,
    conversation_id: &str,
    tool_call_id: &str,
    tool_name: &str,
    arguments: serde_json::Value,
    plan: bool,
) {
    let conversation_id = conversation_id.to_string();
    let tool_call_id = tool_call_id.to_string();
    let tool_name = tool_name.to_string();
    let now = now_ms();
    let call_content = if plan {
        serde_json::json!({ "arguments": arguments, "plan": true }).to_string()
    } else {
        serde_json::json!({ "arguments": arguments }).to_string()
    };
    let _ = conn
        .call({
            let conversation_id = conversation_id.clone();
            move |conn: &mut Connection| -> rusqlite::Result<()> {
                let seq: i64 = conn.query_row(
                    "SELECT COALESCE(MAX(sequence), -1) + 1 FROM messages WHERE conversation_id = ?1",
                    [&conversation_id],
                    |row| row.get(0),
                )?;
                conn.execute(
                    "INSERT INTO messages (id, conversation_id, role, content_type, content, tool_name, created_at, sequence, tool_call_id) VALUES (?1, ?2, 'assistant', 'tool_call', ?3, ?4, ?5, ?6, ?7)",
                    rusqlite::params![Uuid::now_v7().to_string(), conversation_id, call_content, tool_name, now, seq, tool_call_id],
                )?;
                Ok(())
            }
        })
        .await;
    if let Some(app) = app {
        let _ = app.emit(
            "agent-message-persisted",
            AgentMessagePersisted { conversation_id },
        );
    }
}

/// The `tool_result` counterpart to `persist_tool_call` (role `tool`, the
/// schema's dedicated role for exactly this, previously unused) — `detail`
/// is a tool-shaped, self-sufficient payload a widget renders from without
/// needing its paired `tool_call` row (data-model.md's row-shape table).
/// `tool_call_id` links this row back to its `tool_call` row directly
/// (migration 0006) instead of relying on sequence-adjacency. `model_text`
/// is the plain, model-facing text for this result (post-offload-
/// truncation if applicable) — what reconstructing this row's in-memory
/// `ChatMessage::tool_result` on a later reload should actually use,
/// distinct from `detail`'s richer, widget-rendering-oriented shape.
///
/// Idempotent per `tool_call_id`: if a result row for this call already
/// exists (e.g. startup healing in another process paired it with an
/// interrupted-error result while this turn was still running), the second
/// insert is skipped — one ToolUse must never reconstruct with two
/// ToolResults in `load_history`.
async fn persist_tool_result(
    app: Option<&AppHandle>,
    conn: &tokio_rusqlite::Connection,
    conversation_id: &str,
    tool_call_id: &str,
    tool_name: &str,
    model_text: &str,
    detail: serde_json::Value,
) {
    let conversation_id = conversation_id.to_string();
    let tool_call_id = tool_call_id.to_string();
    let tool_name = tool_name.to_string();
    let model_text = model_text.to_string();
    let now = now_ms();
    let content = detail.to_string();
    let _ = conn
        .call({
            let conversation_id = conversation_id.clone();
            move |conn: &mut Connection| -> rusqlite::Result<()> {
                let already_paired: bool = conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM messages WHERE conversation_id = ?1 AND tool_call_id = ?2 AND content_type = 'tool_result')",
                    rusqlite::params![&conversation_id, &tool_call_id],
                    |row| row.get(0),
                )?;
                if already_paired {
                    return Ok(());
                }
                let seq: i64 = conn.query_row(
                    "SELECT COALESCE(MAX(sequence), -1) + 1 FROM messages WHERE conversation_id = ?1",
                    [&conversation_id],
                    |row| row.get(0),
                )?;
                conn.execute(
                    "INSERT INTO messages (id, conversation_id, role, content_type, content, tool_name, created_at, sequence, tool_call_id, model_text) VALUES (?1, ?2, 'tool', 'tool_result', ?3, ?4, ?5, ?6, ?7, ?8)",
                    rusqlite::params![Uuid::now_v7().to_string(), conversation_id, content, tool_name, now, seq, tool_call_id, model_text],
                )?;
                Ok(())
            }
        })
        .await;
    if let Some(app) = app {
        let _ = app.emit(
            "agent-message-persisted",
            AgentMessagePersisted { conversation_id },
        );
    }
}

/// Convenience wrapper for the six tools whose call and result are always
/// known together (everything but `AskUserQuestion`) — both land at
/// adjacent sequence numbers, one right after the other, sharing the same
/// `tool_call_id`. `plan` forwards straight to `persist_tool_call` (see its
/// own doc comment) — the paired `tool_result`'s `plan`/other markers stay
/// wherever they already live inside `detail`, unaffected by this param.
#[allow(clippy::too_many_arguments)]
async fn persist_tool_call_and_result(
    app: Option<&AppHandle>,
    conn: &tokio_rusqlite::Connection,
    conversation_id: &str,
    tool_call_id: &str,
    tool_name: &str,
    arguments: serde_json::Value,
    model_text: &str,
    detail: serde_json::Value,
    plan: bool,
) {
    persist_tool_call(
        app,
        conn,
        conversation_id,
        tool_call_id,
        tool_name,
        arguments,
        plan,
    )
    .await;
    persist_tool_result(
        app,
        conn,
        conversation_id,
        tool_call_id,
        tool_name,
        model_text,
        detail,
    )
    .await;
}

/// Persists a plan-machine tool interaction (one of the five plan tools,
/// or a state-gated rejection of a regular tool) as an ordinary
/// call/result pair — the model's reconstructed history needs them — with
/// a `"plan": true` marker in BOTH rows (the result's `detail` and, as of
/// this fix, the call's own content too — see `persist_tool_call`'s doc
/// comment for why the call row needs it independently), which is the
/// frontend's signal to keep the row out of the transcript (spec: plan
/// activity is tracker-only) and `context::apply_lightweight_clearing`'s
/// signal to clear it under `PLAN_KEEP_N` rather than `TOOL_KEEP_N`.
async fn persist_plan_tool(
    app: Option<&AppHandle>,
    conn: &tokio_rusqlite::Connection,
    conversation_id: &str,
    tool_call_id: &str,
    call: &ToolCall,
    result: &str,
) {
    persist_tool_call_and_result(
        app,
        conn,
        conversation_id,
        tool_call_id,
        &call.name,
        call.arguments.clone(),
        result,
        serde_json::json!({
            "toolName": call.name,
            "arguments": call.arguments,
            "plan": true,
            "outcome": {"ok": !result.starts_with("Error"), "text": result},
        }),
        true,
    )
    .await;
}

fn parse_question_options(call: &ToolCall) -> Vec<QuestionOption> {
    call.arguments
        .get("options")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|o| {
                    Some(QuestionOption {
                        label: o.get("label")?.as_str()?.to_string(),
                        description: o
                            .get("description")
                            .and_then(|d| d.as_str())
                            .unwrap_or("")
                            .to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// US3/FR-008 (`001`'s FR-010): the one tool that pauses the loop for a
/// real answer. Persists the `tool_call` row immediately — so it's the
/// *latest* message for as long as the pause lasts, which is what
/// `compute_status`'s existing "latest message is a pending
/// AskUserQuestion tool_call" check relies on for `requires_action` — then
/// registers with `pending`, hands the event to `emit_question` (an
/// injected closure rather than a direct `app.emit()` call, so this whole
/// function is testable without a live Tauri app — matching
/// `PendingQuestions`' own deliberately Tauri-agnostic design), and awaits
/// the answer before persisting the `tool_result`. `tool_call_id` (assigned
/// by `agent::run_loop`, not generated here) doubles as the question's own
/// `questionId` — there was never a real reason these were two separate
/// concepts; unifying them is what the structured-tool-call redesign is
/// about.
async fn handle_ask_user_question(
    app: Option<&AppHandle>,
    conn: &tokio_rusqlite::Connection,
    pending: &PendingQuestions,
    conversation_id: &str,
    tool_call_id: &str,
    call: &ToolCall,
    emit_question: impl FnOnce(AskUserQuestionEvent),
) -> String {
    let header = call
        .arguments
        .get("header")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let question = call
        .arguments
        .get("question")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let multi_select = call
        .arguments
        .get("multiSelect")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let options = parse_question_options(call);

    let rx = pending.register(tool_call_id.to_string());

    // The id is folded into the persisted tool_call's own arguments (not
    // just handed to the live `ask-user-question` event) so the frontend
    // can recover and re-render the pending prompt purely from
    // `list_messages` -- e.g. after switching away from this conversation
    // and back, or reopening the app -- while this call is still
    // genuinely blocked below on `rx.await`. Without this, the only way to
    // ever answer (and unblock the loop, which is holding the engine lock
    // for as long as this awaits) is to have had a UI mounted at the
    // exact moment the event fired.
    let mut arguments_with_id = call.arguments.clone();
    if let serde_json::Value::Object(ref mut map) = arguments_with_id {
        map.insert("questionId".to_string(), serde_json::json!(tool_call_id));
    }
    persist_tool_call(
        app,
        conn,
        conversation_id,
        tool_call_id,
        "AskUserQuestion",
        arguments_with_id,
        false,
    )
    .await;

    emit_question(AskUserQuestionEvent {
        conversation_id: conversation_id.to_string(),
        question_id: tool_call_id.to_string(),
        header: header.clone(),
        question: question.clone(),
        options: options.clone(),
        multi_select,
    });

    let answer = rx.await.unwrap_or_default();
    let model_text = format!("User answered: {}", answer.join(", "));

    persist_tool_result(
        app,
        conn,
        conversation_id,
        tool_call_id,
        "AskUserQuestion",
        &model_text,
        serde_json::json!({
            "toolName": "AskUserQuestion",
            "questionId": tool_call_id,
            "header": header,
            "question": question,
            "options": options,
            "multiSelect": multi_select,
            "answer": answer,
        }),
    )
    .await;

    model_text
}

/// `AgentBackend` (see that trait's own doc comment for why this is a
/// struct+impl rather than four closures) for the top-level agent loop
/// (`send_agent_message`): wraps the real `InferenceEngine`, DB connection,
/// and event emission that loop actually runs against.
struct RealBackend<'a> {
    engine: &'a InferenceEngine,
    /// One persistent inference context for the whole turn: each turn's
    /// prompt extends the previous one's, so its KV cache reuses the shared
    /// prefix and re-decodes only the newest tool exchange, rather than
    /// re-prefilling the entire growing history every turn. Owned (not a
    /// borrow) so its `LlamaContext` is dropped with the backend, before the
    /// engine's mutex guard is released in `send_agent_message`.
    session: PromptSession<'a>,
    conn: &'a tokio_rusqlite::Connection,
    conversation_id: &'a str,
    app: &'a AppHandle,
    settings: &'a crate::context::ContextSettings,
    threshold: u32,
    cwd: Option<&'a Path>,
    pending: &'a PendingQuestions,
    plan_state: crate::agent::plan::PlanState,
    active_plans: &'a ActivePlans,
}

impl crate::agent::AgentBackend for RealBackend<'_> {
    fn measure(&mut self, messages: &[ChatMessage]) -> u32 {
        // Reuses `settings` (already loaded by the caller for the
        // hard-limit check) rather than a DB round-trip every turn --
        // still emits `context-usage-update` on every turn (not just when
        // `compact` actually runs) to keep the UI's live indicator
        // responsive, not just notified of compaction events.
        match crate::context::usage_from_fitted_messages(
            self.engine,
            self.conversation_id,
            messages,
            self.settings,
        ) {
            Ok(usage) => {
                let tokens_used = usage.tokens_used;
                let _ = self.app.emit("context-usage-update", usage);
                tokens_used
            }
            // Fail-safe: treat a measurement failure as over-threshold so
            // `compact` runs defensively, rather than silently
            // under-measuring and letting a too-large prompt through.
            Err(_) => u32::MAX,
        }
    }

    fn threshold(&self) -> u32 {
        self.threshold
    }

    fn compact(&mut self, messages: &[ChatMessage]) -> Vec<ChatMessage> {
        crate::context::fit_turn_to_budget(self.engine, messages).unwrap_or_else(|_| messages.to_vec())
    }

    async fn generate(&mut self, mut messages: Vec<ChatMessage>) -> String {
        // Stable-prefix prompt architecture: `messages[0]` is the immutable
        // union prompt (+ turn-stable cwd line) seeded by
        // `send_agent_message` and NEVER touched here, so the session's KV
        // prefix survives every Planning<->Executing and step->step
        // transition -- only the tail below (plus the newest tool exchange)
        // re-decodes each turn. Everything volatile (mode banner, current
        // step framing, refusal context, recitation checklist) rides in ONE
        // tail message, appended to this call's own clone of `messages`
        // (run_loop clones before every `generate`), never written back to
        // run_loop's canonical list.
        messages.push(ChatMessage::user(self.plan_state.state_tail()));
        // The plan loop REQUIRES a tool call at the sampler level in BOTH
        // states: a plain-text reply anywhere would end the entire task,
        // and the model was observed degrading into exactly that
        // (`StepDone(...)` as prose mid-step; a bare "ResumeExecution"
        // text after twenty repetitive AddStep calls). "Done" is itself a
        // tool call now (FinishTask), so requiring tool calls never traps
        // the loop. The union prompt advertises BOTH states' tools, so the
        // current state's set is enforced here at the sampler instead: a
        // tool outside it is unsamplable (grammar name-enum gating).
        match self.engine.render_chat_prompt(&messages) {
            Ok(rendered) => self
                .session
                .generate(
                    self.engine,
                    &rendered,
                    crate::context::limits::AGENT_TURN_MAX_OUTPUT_TOKENS as i32,
                    crate::inference::ToolCallMode::Require,
                    Some(self.plan_state.allowed_tool_names(true)),
                    |_piece| {},
                    || false,
                )
                .unwrap_or_else(|e| format!("Error: inference failed: {e}")),
            Err(e) => format!("Error: failed to render chat prompt: {e}"),
        }
    }

    async fn execute_tool(&mut self, tool_call_id: String, call: ToolCall) -> ToolExecution {
        // Plan machine first: the plan tools (and state-gated rejections)
        // never reach dispatch. Their rows persist like any tool's —
        // marked "plan": true so the transcript skips them — and every
        // handled call refreshes the live tracker surface. FinishTask ends
        // the whole loop with the model's verified final answer.
        if let Some(outcome) = self.plan_state.handle_plan_tool(&call) {
            let (result_text, execution) = match outcome {
                crate::agent::plan::PlanToolReply::Reply(text) => {
                    let execution = ToolExecution::Result(text.clone());
                    (text, execution)
                }
                crate::agent::plan::PlanToolReply::Finish(answer) => {
                    let execution = ToolExecution::Finish(answer.clone());
                    (answer, execution)
                }
            };
            persist_plan_tool(
                Some(self.app),
                self.conn,
                self.conversation_id,
                &tool_call_id,
                &call,
                &result_text,
            )
            .await;
            publish_plan_update(
                Some(self.app),
                self.active_plans,
                self.conversation_id,
                &self.plan_state,
            );
            return execution;
        }
        ToolExecution::Result(
            execute_top_level_tool(
                tool_call_id,
                call,
                self.conn,
                self.engine,
                self.conversation_id,
                self.cwd,
                self.app,
                self.pending,
            )
            .await,
        )
    }
}

/// `AgentBackend` for the `Task`-tool's delegated subagent loop
/// (`execute_top_level_tool` below): same fit-to-budget guarantee as
/// `RealBackend`, minus event emission -- FR-015 isolation means the
/// subagent's own transcript isn't rendered by any current view, so
/// there's no live indicator to notify.
struct SubagentBackend<'a> {
    engine: &'a InferenceEngine,
    conn: &'a tokio_rusqlite::Connection,
    subagent_id: &'a str,
    cwd: Option<&'a Path>,
    threshold: u32,
    plan_state: crate::agent::plan::PlanState,
}

impl crate::agent::AgentBackend for SubagentBackend<'_> {
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
        crate::context::fit_turn_to_budget(self.engine, messages).unwrap_or_else(|_| messages.to_vec())
    }

    async fn generate(&mut self, mut messages: Vec<ChatMessage>) -> String {
        // Same stable-prefix architecture as `RealBackend::generate` (see
        // that impl's doc comment for the full rationale): `messages[0]` is
        // the immutable subagent union prompt (`allow_task = false` -- no
        // `Task` tool, FR-016) seeded by `execute_top_level_tool`, never
        // touched here; all volatile state rides the single tail message,
        // and the current state's tool set is enforced at the sampler.
        messages.push(ChatMessage::user(self.plan_state.state_tail()));
        match self.engine.render_chat_prompt(&messages) {
            Ok(rendered) => self
                .engine
                .generate(
                    &rendered,
                    crate::context::limits::AGENT_TURN_MAX_OUTPUT_TOKENS as i32,
                    crate::inference::ToolCallMode::Require,
                    Some(self.plan_state.allowed_tool_names(false)),
                    |_piece| {},
                    || false,
                )
                .unwrap_or_else(|e| format!("Error: inference failed: {e}")),
            Err(e) => format!("Error: failed to render chat prompt: {e}"),
        }
    }

    async fn execute_tool(&mut self, tool_call_id: String, call: ToolCall) -> ToolExecution {
        // Plan machine first, same as `RealBackend::execute_tool` -- the
        // plan tools (and state-gated rejections) never reach dispatch.
        // Persisted under the subagent's own conversation with the
        // `"plan": true` marker; no ActivePlans/events -- subagents have no
        // tracker.
        if let Some(outcome) = self.plan_state.handle_plan_tool(&call) {
            let (result_text, execution) = match outcome {
                crate::agent::plan::PlanToolReply::Reply(text) => {
                    let execution = ToolExecution::Result(text.clone());
                    (text, execution)
                }
                crate::agent::plan::PlanToolReply::Finish(answer) => {
                    let execution = ToolExecution::Finish(answer.clone());
                    (answer, execution)
                }
            };
            persist_plan_tool(
                None,
                self.conn,
                self.subagent_id,
                &tool_call_id,
                &call,
                &result_text,
            )
            .await;
            return execution;
        }

        // 004-tool-call-widgets: the subagent's own tool activity persists
        // under its own conversation row -- never the parent's --
        // preserving 001's existing FR-015/SC-008 isolation guarantee
        // (only its final answer, inserted by the caller, ever reaches the
        // parent's transcript). No live-refresh event (`app: None`) -- it
        // isn't rendered by any current view, so there's no consumer to
        // notify.
        let outcome =
            dispatch::execute_async(call.clone(), self.cwd.map(|p| p.to_path_buf())).await;
        let outcome = crate::context::annotate_with_token_count(self.engine, outcome);
        persist_tool_call_and_result(
            None,
            self.conn,
            self.subagent_id,
            &tool_call_id,
            &call.name,
            call.arguments.clone(),
            &outcome.model_text,
            outcome.detail.clone(),
            false,
        )
        .await;
        ToolExecution::Result(outcome.model_text)
    }
}

/// Handles a single tool call for the top-level agent loop: `Task` spawns
/// a real, isolated subagent (FR-015) and runs its own nested loop to
/// completion against the same loaded model, returning its final answer
/// as the tool result; everything else dispatches to the built-in tools
/// directly. The nested loop uses `AgentContext::subagent()`, so a `Task`
/// call *from* the subagent is rejected by `run_loop` itself before ever
/// reaching this function (FR-016's one-level nesting cap).
///
/// `cwd` (007-workspace-cwd-resolution) is passed straight through to
/// `dispatch::execute` for the top-level call, and onto the subagent's own
/// `AgentContext` below — a subagent resolves relative paths against the
/// same working directory as its parent, not a fresh, unscoped one (FR-006).
#[allow(clippy::too_many_arguments)]
async fn execute_top_level_tool(
    tool_call_id: String,
    call: ToolCall,
    conn: &tokio_rusqlite::Connection,
    engine: &InferenceEngine,
    parent_conversation_id: &str,
    cwd: Option<&std::path::Path>,
    app: &AppHandle,
    pending: &PendingQuestions,
) -> String {
    if call.name == "AskUserQuestion" {
        return handle_ask_user_question(
            Some(app),
            conn,
            pending,
            parent_conversation_id,
            &tool_call_id,
            &call,
            |event| {
                let _ = app.emit("ask-user-question", event);
            },
        )
        .await;
    }

    if call.name != "Task" {
        let model_text = handle_general_tool_call(
            Some(app),
            conn,
            engine,
            parent_conversation_id,
            cwd,
            &tool_call_id,
            &call,
        )
        .await;
        emit_context_usage_update(app, conn, engine, parent_conversation_id, cwd).await;
        return model_text;
    }

    let Some(prompt) = call.arguments.get("prompt").and_then(|v| v.as_str()) else {
        return "Error: Task requires a prompt argument".to_string();
    };
    let prompt = prompt.to_string();

    persist_tool_call(
        Some(app),
        conn,
        parent_conversation_id,
        &tool_call_id,
        "Task",
        serde_json::json!({ "prompt": prompt }),
        false,
    )
    .await;

    let parent_id = parent_conversation_id.to_string();
    let prompt_for_spawn = prompt.clone();
    let subagent_id = match conn
        .call(move |conn: &mut Connection| {
            subagent::spawn_subagent(conn, &parent_id, &prompt_for_spawn)
        })
        .await
    {
        Ok(id) => id,
        Err(e) => {
            // A spawn failure still needs its tool_result pairing (the
            // tool_call above is already durably persisted by this point)
            // -- otherwise this row is left permanently unpaired, which
            // widgets that match tool_call/tool_result by tool_call_id
            // would render as stuck "pending" forever. No real
            // subagentConversationId exists in this failure case (nothing
            // was ever spawned), so it's "" here, same "complete" state as
            // the success path below since the delegation attempt is over
            // either way from the parent's perspective.
            let error_text = format!("Error: failed to spawn subagent: {e}");
            persist_tool_result(
                Some(app),
                conn,
                parent_conversation_id,
                &tool_call_id,
                "Task",
                &error_text,
                serde_json::json!({
                    "toolName": "Task",
                    "prompt": prompt,
                    "subagentConversationId": "",
                    "state": "complete",
                }),
            )
            .await;
            return error_text;
        }
    };

    // 007-workspace-cwd-resolution/FR-006: inherit the parent's cwd rather
    // than starting the subagent unscoped.
    let sub_context = AgentContext::subagent().with_cwd(cwd.map(|p| p.to_path_buf()));
    // Subagents now run the same two-state plan engine as the top-level
    // loop (rather than the flat SYSTEM_PROMPT ReAct loop) — the fresh
    // state is owned by the backend literal below; the seed prompt is the
    // state-free subagent union prompt (`allow_task = false`).
    let plan_state = crate::agent::plan::PlanState::default();
    let sub_system_prompt = plan_system_message(sub_context.cwd.as_deref(), false);
    // FR-015: a fresh, isolated context — just the system prompt plus the
    // delegated task, no parent conversation history.
    let sub_messages = vec![
        ChatMessage::system(sub_system_prompt),
        ChatMessage::user(prompt.clone()),
    ];
    // Same fit-to-budget guarantee as the top-level loop, now automatic for
    // every `run_loop` caller (this subagent path had no such protection
    // before this became the loop's own per-turn decision rather than
    // something each caller's `generate` closure had to remember to do).
    let sub_threshold = engine
        .context_window()
        .saturating_sub(crate::context::limits::AGENT_TURN_MAX_OUTPUT_TOKENS);
    let mut sub_backend = SubagentBackend {
        engine,
        conn,
        subagent_id: &subagent_id,
        cwd: sub_context.cwd.as_deref(),
        threshold: sub_threshold,
        plan_state,
    };
    let sub_started_at = now_ms();
    let sub_result = run_loop(&sub_context, sub_messages, &mut sub_backend).await;

    let sub_final = match sub_result {
        Ok(text) => text,
        Err(e) => format!("Error: {e}"),
    };

    let now = now_ms();
    let sub_final_for_db = sub_final.clone();
    let subagent_id_for_db = subagent_id.clone();
    // 010-context-window-management (UI refactor): output tokens for the
    // subagent's own final answer -- `engine` is already in scope here
    // (this function's own parameter), so no follow-up-update dance needed.
    let sub_token_count = engine
        .count_tokens(&sub_final_for_db)
        .ok()
        .map(|n| n as i64);
    let _ = persist_assistant_text_reply(
        conn,
        &subagent_id_for_db,
        &sub_final_for_db,
        sub_started_at,
        now,
        sub_token_count,
    )
    .await;

    // 004-tool-call-widgets/FR-010: the parent conversation only ever sees
    // a running/complete status for the delegation itself — never the
    // subagent's own tool calls above, which stayed on `subagent_id`.
    // Always "complete" here since this function only returns once the
    // whole nested loop has finished (research.md § 2 — no live
    // mid-delegation status this pass).
    persist_tool_result(
        Some(app),
        conn,
        parent_conversation_id,
        &tool_call_id,
        "Task",
        &sub_final,
        serde_json::json!({
            "toolName": "Task",
            "prompt": prompt,
            "subagentConversationId": subagent_id,
            "state": "complete",
        }),
    )
    .await;

    sub_final
}

/// Handles a single non-`Task`, non-`AskUserQuestion` tool call for the
/// top-level loop. Persists the `tool_call` row *before* executing —
/// mirrors `handle_ask_user_question`'s existing early-persist pattern —
/// so a slow tool (e.g. a long-running `Bash` command) is visible as "in
/// flight" the moment it starts, not only once it's already finished.
/// `app: Option<&AppHandle>` (not mandatory, unlike the enclosing
/// `execute_top_level_tool`) specifically so this is unit-testable without
/// a live Tauri app.
async fn handle_general_tool_call(
    app: Option<&AppHandle>,
    conn: &tokio_rusqlite::Connection,
    engine: &InferenceEngine,
    parent_conversation_id: &str,
    cwd: Option<&std::path::Path>,
    tool_call_id: &str,
    call: &ToolCall,
) -> String {
    persist_tool_call(
        app,
        conn,
        parent_conversation_id,
        tool_call_id,
        &call.name,
        call.arguments.clone(),
        false,
    )
    .await;

    let outcome = dispatch::execute_async(call.clone(), cwd.map(|p| p.to_path_buf())).await;
    let outcome = crate::context::annotate_with_token_count(engine, outcome);

    // 010-context-window-management/US3 (FR-011/FR-012): an oversized
    // result gets truncated to a preview + a `Read`-able pointer before it
    // ever enters the model's context -- the persisted `detail` still
    // carries the full outcome for the transcript UI, only `model_text`
    // (what the model actually sees) is substituted.
    let settings = crate::context::ContextSettings::load(conn)
        .await
        .unwrap_or_else(|_| crate::context::ContextSettings::from_raw(&Default::default()));
    let (model_text, offloaded_to) = match app.and_then(|a| a.path().app_data_dir().ok()) {
        Some(app_data_dir) => crate::context::offload::offload_if_oversized(
            &app_data_dir,
            parent_conversation_id,
            tool_call_id,
            &outcome.model_text,
            settings.tool_output_offload_chars,
        )
        .unwrap_or_else(|_| (outcome.model_text.clone(), None)),
        None => (outcome.model_text.clone(), None),
    };

    let mut detail = outcome.detail.clone();
    detail["offloadedTo"] = serde_json::json!(offloaded_to);

    persist_tool_result(
        app,
        conn,
        parent_conversation_id,
        tool_call_id,
        &call.name,
        &model_text,
        detail,
    )
    .await;

    model_text
}

/// 010-context-window-management/US1: recomputes and emits this
/// conversation's context usage — called after each turn's persistence step
/// in the agent loop (a tool_call/tool_result pair, or the final answer) so
/// the indicator stays live through a whole agent-mode run, not just at the
/// start. Best-effort: a failure here (e.g. no model loaded, which can't
/// actually happen mid-loop, but `compute_usage` still returns a `Result`)
/// is swallowed rather than aborting the loop over a UI-only concern.
async fn emit_context_usage_update(
    app: &AppHandle,
    conn: &tokio_rusqlite::Connection,
    engine: &InferenceEngine,
    conversation_id: &str,
    cwd: Option<&std::path::Path>,
) {
    let Ok(skills_dir) = app.path().app_data_dir().map(|d| d.join("skills")) else {
        return;
    };
    // Measure usage against the plan engine's actual seed prompt (matches the top-level loop's
    // initial system prompt), not the flat SYSTEM_PROMPT which understated usage by ~300 tokens.
    let system_prompt = plan_system_message(cwd, true);
    if let Ok(usage) = crate::context::compute_usage(
        conn,
        engine,
        conversation_id,
        &skills_dir,
        &system_prompt,
    )
    .await
    {
        let _ = app.emit("context-usage-update", usage);
    }
}

async fn persist_assistant_text_reply(
    conn: &tokio_rusqlite::Connection,
    conversation_id: &str,
    content: &str,
    created_at: i64,
    persisted_at: i64,
    token_count: Option<i64>,
) -> Result<i64, String> {
    let conversation_id = conversation_id.to_string();
    let content = content.to_string();
    let duration_ms = (persisted_at - created_at).max(0);

    conn.call(move |conn: &mut Connection| -> rusqlite::Result<i64> {
        let seq: i64 = conn.query_row(
            "SELECT COALESCE(MAX(sequence), -1) + 1 FROM messages WHERE conversation_id = ?1",
            [&conversation_id],
            |row| row.get(0),
        )?;
        conn.execute(
            "INSERT INTO messages (id, conversation_id, role, content_type, content, created_at, sequence, duration_ms, token_count) VALUES (?1, ?2, 'assistant', 'text', ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                Uuid::now_v7().to_string(),
                conversation_id,
                content,
                created_at,
                seq,
                duration_ms,
                token_count,
            ],
        )?;
        conn.execute(
            "UPDATE conversations SET updated_at = ?1 WHERE id = ?2",
            rusqlite::params![persisted_at, conversation_id],
        )?;
        Ok(seq)
    })
    .await
    .map_err(|e| e.to_string())
}

/// The plan engine's immutable union prompt plus the cwd line that tells
/// the model where it's working — what seeds `initial_messages[0]` (and
/// the pre-loop compaction budget / usage measurement). Deliberately
/// state-free: both inputs are turn-stable (the union prompt is a cached
/// static; a conversation's workspace can't change mid-turn), so the
/// message this returns is byte-identical every time a given host renders
/// it — the invariant `PromptSession`'s KV-prefix reuse depends on.
/// `allow_task` picks the host flavor: `false` for the subagent path
/// (FR-016's one-level nesting cap means `run_loop` rejects any `Task`
/// call from a subagent, so its prompt must not advertise the tool at
/// all), `true` everywhere top-level.
fn plan_system_message(cwd: Option<&std::path::Path>, allow_task: bool) -> String {
    let base = crate::agent::plan::plan_system_prompt(allow_task);
    match cwd {
        Some(path) => format!(
            "{base}\n\nYou are currently working in the directory: {}",
            path.display()
        ),
        None => base.to_string(),
    }
}

/// 009-rich-chat-input/US2 (contracts/rich-chat-input.md): persists this
/// turn's user message row and derives the text the model actually sees
/// for it.
///
/// `rich_content = None` takes the exact path `send_agent_message` has
/// always taken: persists `content_type='text'`/`content=content`
/// verbatim, no parsing, no `expand_segments` call, and the returned model
/// text is `content` itself, unchanged (byte-for-byte identical to before
/// this feature existed).
///
/// `rich_content = Some(json)` parses `json` as `RichMessageContent`
/// first — `Err` here means nothing is persisted at all, matching how
/// every other pre-inference failure in this function already returns
/// `Err(String)` before doing anything. On success, persists
/// `content_type='rich_text'` with `content=json` verbatim — never the
/// flat `content` param, which in this case is only a UI-side
/// fallback/plain-text mirror, not persisted twice — then resolves the
/// model text via `expand_segments(&segments, skills_dir, expand_skills:
/// true)`, propagating its `Err` (e.g. an unresolvable `skill` segment,
/// FR-014) after the row has already been persisted.
///
/// Split out of `send_agent_message` (which needs a live `AppHandle` and a
/// loaded inference engine end-to-end, neither mockable in a unit test)
/// purely so this feature's actual new logic — the `None`/`Some` branch,
/// the exact persisted shape, and the expansion — is unit-testable against
/// a real, temporary DB connection and skills directory, the same way
/// `persist_tool_call`/`persist_tool_result` above already are.
async fn persist_user_turn(
    conn: &tokio_rusqlite::Connection,
    skills_dir: &Path,
    conversation_id: &str,
    next_seq: i64,
    now: i64,
    content: &str,
    rich_content: Option<&str>,
) -> Result<String, String> {
    let rich: Option<RichMessageContent> = rich_content
        .map(serde_json::from_str::<RichMessageContent>)
        .transpose()
        .map_err(|e| format!("invalid rich_content: {e}"))?;

    match &rich {
        Some(_) => {
            let json = rich_content
                .expect("rich_content is Some whenever `rich` parsed above is Some")
                .to_string();
            let conversation_id = conversation_id.to_string();
            conn.call(move |conn: &mut Connection| -> rusqlite::Result<()> {
                conn.execute(
                    "INSERT INTO messages (id, conversation_id, role, content_type, content, created_at, sequence) VALUES (?1, ?2, 'user', 'rich_text', ?3, ?4, ?5)",
                    rusqlite::params![Uuid::now_v7().to_string(), conversation_id, json, now, next_seq],
                )?;
                Ok(())
            })
            .await
            .map_err(|e| e.to_string())?;
        }
        None => {
            let conversation_id = conversation_id.to_string();
            let content = content.to_string();
            conn.call(move |conn: &mut Connection| -> rusqlite::Result<()> {
                conn.execute(
                    "INSERT INTO messages (id, conversation_id, role, content_type, content, created_at, sequence) VALUES (?1, ?2, 'user', 'text', ?3, ?4, ?5)",
                    rusqlite::params![Uuid::now_v7().to_string(), conversation_id, content, now, next_seq],
                )?;
                Ok(())
            })
            .await
            .map_err(|e| e.to_string())?;
        }
    }

    match &rich {
        Some(r) => expand_segments(&r.segments, skills_dir, true),
        None => Ok(content.to_string()),
    }
}

/// FR-008/FR-009: runs the agent tool-use loop to completion for one user
/// message in a workspace-scoped conversation, using the real built-in
/// tools (`agent::dispatch`) and the same loaded model `send_message`
/// uses. Two known, deliberate simplifications versus the full spec (both
/// called out in `agent/mod.rs` too): this bypasses the scheduler's queue
/// entirely rather than submitting turns through it (agent-mode work isn't
/// yet scheduled alongside chat requests — a real gap if a chat message
/// and an agent turn are in flight at once), and it runs synchronously to
/// completion rather than streaming intermediate tool calls/reasoning to
/// the UI live (FR-017's `agent-activity` events aren't wired up) — the
/// frontend sees a single "thinking…" state and then the final answer,
/// not a live trace of each tool call.
#[tauri::command]
#[specta::specta]
// 009-rich-chat-input/US2's `rich_content` param tips this over clippy's
// default 7-argument threshold; every parameter here is either a
// framework-injected `State`/`AppHandle` or a real, distinct piece of the
// IPC contract (contracts/rich-chat-input.md) -- there's no natural
// sub-struct to group them into without inventing an artificial one.
#[allow(clippy::too_many_arguments)]
pub async fn send_agent_message(
    app: AppHandle,
    db_cell: State<'_, DbCell>,
    inference: State<'_, InferenceState>,
    active_generations: State<'_, ActiveGenerations>,
    active_plans: State<'_, ActivePlans>,
    pending_questions: State<'_, PendingQuestions>,
    conversation_id: String,
    content: String,
    rich_content: Option<String>,
) -> Result<String, String> {
    let conn = db_cell.get(&app).await?.clone();
    let now = now_ms();

    // 009-rich-chat-input/US2: resolved once, up front, the same way
    // `commands::skills::list_skills` resolves its skills directory
    // (`app.path().app_data_dir()?.join("skills")`) -- reused below both by
    // `persist_user_turn`'s `expand_segments` call for this turn and by
    // `load_history`'s expansion of any earlier `rich_text` turns.
    let skills_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("skills");

    let next_seq = conn
        .call({
            let conversation_id = conversation_id.clone();
            move |conn: &mut Connection| -> rusqlite::Result<i64> {
                conn.query_row(
                    "SELECT COALESCE(MAX(sequence), -1) + 1 FROM messages WHERE conversation_id = ?1",
                    [&conversation_id],
                    |row| row.get::<_, i64>(0),
                )
            }
        })
        .await
        .map_err(|e| e.to_string())?;

    let model_text_for_turn = persist_user_turn(
        &conn,
        &skills_dir,
        &conversation_id,
        next_seq,
        now,
        &content,
        rich_content.as_deref(),
    )
    .await?;
    // Streaming (UI refactor): the real, DB-confirmed user turn is ready to
    // show immediately, before the (potentially long) agent loop even
    // starts -- lets the frontend replace its own optimistic bubble with
    // the persisted one right away instead of waiting for the whole turn.
    let _ = app.emit(
        "agent-message-persisted",
        AgentMessagePersisted {
            conversation_id: conversation_id.clone(),
        },
    );

    // 004-tool-call-widgets: registers this conversation as in-progress for
    // the whole turn (matching the chat path's existing ActiveGenerations
    // use) — without this, `compute_status` would see whatever
    // intermediate tool_call/tool_result row this turn's dispatch calls
    // just persisted as the "latest message" while polled mid-turn, and
    // its `role != "assistant"` fallback would misreport a still-running
    // turn as "failed" the moment a `tool_result` (role `tool`) row lands.
    // An RAII guard (not a manual remove-before-every-`?`) covers every
    // early-return between here and the end, including ones this function
    // already had before this feature touched it.
    active_generations
        .0
        .lock()
        .unwrap()
        .insert(conversation_id.clone());
    let _active_guard = ActiveGenerationGuard {
        active_generations: &active_generations,
        conversation_id: conversation_id.clone(),
    };
    let _plan_guard = ActivePlanGuard {
        active_plans: &active_plans,
        app: Some(app.clone()),
        conversation_id: conversation_id.clone(),
    };

    let model_path: Option<String> = conn
        .call(|conn: &mut Connection| -> rusqlite::Result<String> {
            conn.query_row(
                "SELECT local_path FROM models WHERE is_active = 1",
                [],
                |row| row.get(0),
            )
        })
        .await
        .ok();
    let model_path = model_path.ok_or_else(|| "no active model installed".to_string())?;

    {
        let mut guard = inference.0.lock().await;
        if guard.is_none() {
            let path = std::path::PathBuf::from(&model_path);
            let engine = tokio::task::spawn_blocking(move || InferenceEngine::load(&path, 4))
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_string())?;
            *guard = Some(engine);
        }
    }

    // 007-workspace-cwd-resolution: resolved once per turn, not per tool
    // call — a conversation's workspace can't change mid-turn. `None` for
    // a conversation with no workspace_id (the LEFT JOIN's `w.path` column
    // is simply NULL in that row), which every downstream cwd-aware
    // function treats as "behave exactly as before this feature existed."
    let workspace_path: Option<String> = conn
        .call({
            let conversation_id = conversation_id.clone();
            move |conn: &mut Connection| -> rusqlite::Result<Option<String>> {
                conn.query_row(
                    "SELECT w.path FROM conversations c LEFT JOIN workspaces w ON w.id = c.workspace_id WHERE c.id = ?1",
                    [&conversation_id],
                    |row| row.get(0),
                )
            }
        })
        .await
        .map_err(|e| e.to_string())?;
    let cwd = workspace_path.map(std::path::PathBuf::from);

    let context = AgentContext::top_level().with_cwd(cwd.clone());
    let guard = inference.0.lock().await;
    let engine = guard.as_ref().expect("engine loaded above");

    // 010-context-window-management (UI refactor): same follow-up-update
    // pattern as commands::conversations::send_message -- the user turn was
    // already persisted above (by `persist_user_turn`, before the engine
    // was necessarily loaded), keyed back here by conversation_id+sequence
    // since `persist_user_turn` never returns its generated row id.
    if let Ok(token_count) = engine.count_tokens(&model_text_for_turn) {
        let conversation_id_for_update = conversation_id.clone();
        let _ = conn
            .call(move |conn: &mut Connection| -> rusqlite::Result<()> {
                conn.execute(
                    "UPDATE messages SET token_count = ?1 WHERE conversation_id = ?2 AND sequence = ?3",
                    rusqlite::params![token_count as i64, conversation_id_for_update, next_seq],
                )?;
                Ok(())
            })
            .await;
    }

    // 010-context-window-management/US2 (FR-005/FR-006/FR-007): compacts
    // before the loop's first turn -- see `emit_context_usage_update`/the
    // per-turn `maybe_compact` calls inside the loop for why this alone
    // isn't sufficient for agent mode (tool results can push a *later* turn
    // over budget even when the first turn was fine).
    let plan_state = crate::agent::plan::PlanState::default();
    let system_prompt = plan_system_message(cwd.as_deref(), true);
    let usage = crate::context::maybe_compact(
        &conn,
        engine,
        &conversation_id,
        &skills_dir,
        &system_prompt,
        false,
    )
    .await?;
    let settings = crate::context::ContextSettings::load(&conn).await?;
    // Emitted *before* the hard-limit check (not after) -- otherwise the
    // one reading that actually caused a rejection is the one the user
    // never sees, which is backwards for a feature about transparency.
    let _ = app.emit("context-usage-update", usage.clone());
    if (usage.tokens_used as f64) >= settings.hard_limit_pct * usage.token_budget as f64 {
        return Err("This message is too large for the model's context window, even after compacting the conversation. Try a shorter message or start a new conversation.".to_string());
    }

    // Full history (including the user message just inserted above, and
    // reflecting whatever `maybe_compact` just did) so the model sees prior
    // turns in this workspace conversation rather than generating each
    // reply from a blank slate. 009-rich-chat-input: `load_history` needs
    // `skills_dir` (resolved once, above) to expand any `rich_text` rows in
    // that history.
    let history = conn
        .call({
            let conversation_id = conversation_id.clone();
            let skills_dir = skills_dir.clone();
            move |conn: &mut Connection| load_history(conn, &conversation_id, &skills_dir)
        })
        .await
        .map_err(|e| e.to_string())?;
    let mut initial_messages = vec![ChatMessage::system(system_prompt.clone())];
    initial_messages.extend(history);

    // 009-rich-chat-input/US2: `history`'s final element is always the row
    // just persisted above (its `sequence` is the highest in the
    // conversation). When it was a rich-text turn, override it with
    // `persist_user_turn`'s already-computed `expand_segments` output so
    // the model sees the fully-expanded text (pasted content inline,
    // skills resolved and injected) rather than the raw JSON `load_history`
    // would otherwise pass through verbatim for this turn. `rich_content`
    // being `None` leaves this whole step un-entered -- byte-for-byte
    // today's behavior.
    if rich_content.is_some() {
        if let Some(last) = initial_messages.last_mut() {
            *last = ChatMessage::user(model_text_for_turn);
        }
    }

    // The loop's own per-turn decision (`run_loop`'s `measure`/`threshold`/
    // `compact`): every turn checks whether the in-flight messages already
    // fit this same budget before ever calling `fit_turn_to_budget` --
    // skips the fit entirely on turns that don't need it, rather than
    // unconditionally re-measuring every message every turn.
    let threshold = engine
        .context_window()
        .saturating_sub(crate::context::limits::AGENT_TURN_MAX_OUTPUT_TOKENS);

    // One session for the whole turn (KV-prefix reuse across turns). Built
    // here, inside the engine-guard scope, so its `LlamaContext`'s borrow of
    // the model stays valid for as long as the backend runs.
    let session = engine.new_session().map_err(|e| e.to_string())?;
    let mut backend = RealBackend {
        engine,
        session,
        conn: &conn,
        conversation_id: &conversation_id,
        app: &app,
        settings: &settings,
        threshold,
        cwd: cwd.as_deref(),
        pending: &pending_questions,
        plan_state,
        active_plans: &active_plans,
    };
    let result = run_loop(&context, initial_messages, &mut backend).await;
    // Drop the backend (and with it the session's `LlamaContext`, which
    // borrows the model) BEFORE releasing the engine guard — the context's
    // Drop must run while the engine it borrows is still locked in scope.
    drop(backend);
    drop(guard);

    let final_text = match result {
        Ok(text) => text,
        Err(e) => format!("Error: {e}"),
    };

    let final_persisted_at = now_ms();
    let final_seq = persist_assistant_text_reply(
        &conn,
        &conversation_id,
        &final_text,
        now,
        final_persisted_at,
        None,
    )
    .await?;

    // Streaming (UI refactor): the final answer is the last item Loop 1
    // ever appends -- signal it the same way every tool_call/tool_result
    // did throughout the turn, so the frontend's live view converges on
    // the real persisted text rather than relying solely on this
    // function's own return value.
    let _ = app.emit(
        "agent-message-persisted",
        AgentMessagePersisted {
            conversation_id: conversation_id.clone(),
        },
    );

    // 010-context-window-management/US1: re-acquires the engine (the
    // earlier `guard` was dropped before this final persistence) so the
    // indicator reflects usage including the assistant's own final answer,
    // not just the state as of the last tool call. Also fills in this final
    // answer's own output token_count (UI refactor), same follow-up-update
    // pattern used elsewhere in this file.
    {
        let guard = inference.0.lock().await;
        if let Some(engine) = guard.as_ref() {
            if let Ok(token_count) = engine.count_tokens(&final_text) {
                let conversation_id_for_update = conversation_id.clone();
                let _ = conn
                    .call(move |conn: &mut Connection| -> rusqlite::Result<()> {
                        conn.execute(
                            "UPDATE messages SET token_count = ?1 WHERE conversation_id = ?2 AND sequence = ?3",
                            rusqlite::params![token_count as i64, conversation_id_for_update, final_seq],
                        )?;
                        Ok(())
                    })
                    .await;
            }
            emit_context_usage_update(&app, &conn, engine, &conversation_id, cwd.as_deref()).await;
        }
    }

    Ok(final_text)
}

/// US3/`001` FR-010: resolves a pending `AskUserQuestion` tool call
/// (`contracts/tool-widgets.md`). Errors, rather than silently no-op-ing,
/// if `question_id` is unknown — already answered, or never registered
/// (FR-009's "no second answer" guarantee, enforced by
/// `PendingQuestions::answer`'s existing one-shot-consume semantics).
#[tauri::command]
#[specta::specta]
pub async fn answer_user_question(
    pending_questions: State<'_, PendingQuestions>,
    question_id: String,
    answer: Vec<String>,
) -> Result<(), String> {
    if pending_questions.answer(&question_id, answer) {
        Ok(())
    } else {
        Err(format!(
            "no pending question with id {question_id} (already answered, or never registered)"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_system_message_appends_the_cwd_line_when_known() {
        let msg = plan_system_message(Some(std::path::Path::new("/Users/tester/code/doce")), true);
        assert!(msg.contains("You are currently working in the directory: /Users/tester/code/doce"));
        // Verify the prompt body is the immutable union prompt.
        let base = crate::agent::plan::plan_system_prompt(true);
        assert!(msg.starts_with(base));
    }

    #[test]
    fn plan_system_message_is_unchanged_when_no_cwd_is_known() {
        let msg = plan_system_message(None, true);
        assert_eq!(msg, crate::agent::plan::plan_system_prompt(true));
    }

    /// The KV-prefix invariant: what seeds `messages[0]` must be
    /// byte-identical on every render for a given host flavor — there is
    /// no state input left to vary it (`plan_system_message` is
    /// deliberately state-free), so consecutive turns and plan-state
    /// transitions can never swap the prompt out from under the session.
    #[test]
    fn plan_system_message_is_byte_stable_across_renders() {
        let cwd = std::path::Path::new("/Users/tester/code/doce");
        assert_eq!(
            plan_system_message(Some(cwd), true),
            plan_system_message(Some(cwd), true)
        );
        assert_eq!(
            plan_system_message(Some(cwd), false),
            plan_system_message(Some(cwd), false)
        );
        // The subagent flavor differs (no Task tool) but is stable too.
        assert_ne!(
            plan_system_message(Some(cwd), true),
            plan_system_message(Some(cwd), false)
        );
    }

    // --- 004-tool-call-widgets: US3 (AskUserQuestion pause/resume) ---

    async fn seed_conversation(conn: &tokio_rusqlite::Connection, id: &str) {
        let id = id.to_string();
        conn.call(move |conn: &mut Connection| {
            conn.execute(
                "INSERT INTO conversations (id, title, created_at, updated_at) VALUES (?1, 't', 0, 0)",
                [&id],
            )
        })
        .await
        .unwrap();
    }

    async fn latest_message(
        conn: &tokio_rusqlite::Connection,
        conversation_id: &str,
    ) -> (String, String, Option<String>, String) {
        let conversation_id = conversation_id.to_string();
        conn.call(move |conn: &mut Connection| {
            conn.query_row(
                "SELECT role, content_type, tool_name, content FROM messages WHERE conversation_id = ?1 ORDER BY sequence DESC LIMIT 1",
                [&conversation_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
        })
        .await
        .unwrap()
    }

    // --- 009-rich-chat-input: US2 (send_agent_message's rich_content wiring,
    // exercised via persist_user_turn -- send_agent_message itself needs a
    // live AppHandle and a loaded inference engine, neither available in a
    // unit test) ---

    #[tokio::test]
    async fn persist_user_turn_with_no_rich_content_persists_plain_text_and_returns_it_unchanged() {
        let conn = crate::storage::test_async_connection().await;
        seed_conversation(&conn, "c1").await;
        let skills_dir = tempfile::tempdir().unwrap();

        let model_text =
            persist_user_turn(&conn, skills_dir.path(), "c1", 0, 0, "plain hello", None)
                .await
                .unwrap();

        assert_eq!(model_text, "plain hello");
        let (role, content_type, _, content) = latest_message(&conn, "c1").await;
        assert_eq!(role, "user");
        assert_eq!(content_type, "text");
        assert_eq!(content, "plain hello");
    }

    #[tokio::test]
    async fn persist_user_turn_with_rich_content_persists_the_raw_json_and_returns_expanded_text() {
        let conn = crate::storage::test_async_connection().await;
        seed_conversation(&conn, "c1").await;
        let skills_dir = tempfile::tempdir().unwrap();
        let rich_json = serde_json::json!({
            "segments": [
                {"type": "text", "text": "before "},
                {"type": "pastedText", "id": "p1", "text": "pasted body", "lineCount": 1},
                {"type": "text", "text": " after"},
            ]
        })
        .to_string();

        let model_text = persist_user_turn(
            &conn,
            skills_dir.path(),
            "c1",
            0,
            0,
            "plain hello", // deliberately ignored -- never persisted or returned
            Some(&rich_json),
        )
        .await
        .unwrap();

        assert_eq!(model_text, "before pasted body after");
        let (role, content_type, _, content) = latest_message(&conn, "c1").await;
        assert_eq!(role, "user");
        assert_eq!(content_type, "rich_text");
        // The raw JSON payload, verbatim -- never the flat `content` param.
        assert_eq!(content, rich_json);
    }

    #[tokio::test]
    async fn persist_user_turn_with_malformed_rich_content_json_errors_and_persists_nothing() {
        let conn = crate::storage::test_async_connection().await;
        seed_conversation(&conn, "c1").await;
        let skills_dir = tempfile::tempdir().unwrap();

        let result = persist_user_turn(
            &conn,
            skills_dir.path(),
            "c1",
            0,
            0,
            "plain hello",
            Some("not valid json"),
        )
        .await;

        assert!(result.is_err());
        let count: i64 = conn
            .call(|conn: &mut Connection| {
                conn.query_row(
                    "SELECT COUNT(*) FROM messages WHERE conversation_id = 'c1'",
                    [],
                    |row| row.get(0),
                )
            })
            .await
            .unwrap();
        assert_eq!(count, 0, "a malformed payload must not persist anything");
    }

    #[tokio::test]
    async fn persist_user_turn_with_an_unresolvable_skill_errors_after_persisting_the_row() {
        // Matches the contract's ordering: parse -> persist -> resolve
        // skills_dir -> expand_segments -> propagate Err. The row is
        // already durably saved by the time expansion fails.
        let conn = crate::storage::test_async_connection().await;
        seed_conversation(&conn, "c1").await;
        let skills_dir = tempfile::tempdir().unwrap(); // no skill written into it
        let rich_json = serde_json::json!({
            "segments": [
                {"type": "skill", "id": "s1", "name": "missing-skill"},
            ]
        })
        .to_string();

        let result = persist_user_turn(
            &conn,
            skills_dir.path(),
            "c1",
            0,
            0,
            "plain hello",
            Some(&rich_json),
        )
        .await;

        assert!(result.is_err());
        let (role, content_type, _, content) = latest_message(&conn, "c1").await;
        assert_eq!(role, "user");
        assert_eq!(content_type, "rich_text");
        assert_eq!(content, rich_json);
    }

    #[tokio::test]
    async fn persist_assistant_text_reply_records_elapsed_duration() {
        let conn = crate::storage::test_async_connection().await;
        seed_conversation(&conn, "c1").await;

        let seq = persist_assistant_text_reply(&conn, "c1", "final answer", 1_000, 3_750, None)
            .await
            .unwrap();

        assert_eq!(seq, 0);
        let row: (String, String, i64, Option<i64>, String) = conn
            .call(|conn: &mut Connection| {
                conn.query_row(
                    "SELECT role, content_type, created_at, duration_ms, content FROM messages WHERE conversation_id = 'c1'",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
                )
            })
            .await
            .unwrap();
        assert_eq!(row.0, "assistant");
        assert_eq!(row.1, "text");
        assert_eq!(row.2, 1_000);
        assert_eq!(row.3, Some(2_750));
        assert_eq!(row.4, "final answer");

        let updated_at: i64 = conn
            .call(|conn: &mut Connection| {
                conn.query_row(
                    "SELECT updated_at FROM conversations WHERE id = 'c1'",
                    [],
                    |row| row.get(0),
                )
            })
            .await
            .unwrap();
        assert_eq!(updated_at, 3_750);
    }

    #[tokio::test]
    async fn persist_tool_result_is_idempotent_per_tool_call_id() {
        // Defense in depth against the multi-instance hazard: if another
        // process (or startup healing) already paired this tool_call with
        // a result, a second result for the same call must not land — one
        // ToolUse must never reconstruct with two ToolResults in history.
        let conn = crate::storage::test_async_connection().await;
        seed_conversation(&conn, "c1").await;

        persist_tool_result(
            None,
            &conn,
            "c1",
            "tc1",
            "Bash",
            "first result",
            serde_json::json!({"toolName": "Bash"}),
        )
        .await;
        persist_tool_result(
            None,
            &conn,
            "c1",
            "tc1",
            "Bash",
            "second result",
            serde_json::json!({"toolName": "Bash"}),
        )
        .await;

        let count: i64 = conn
            .call(|conn: &mut Connection| {
                conn.query_row(
                    "SELECT COUNT(*) FROM messages WHERE conversation_id = 'c1' AND content_type = 'tool_result' AND tool_call_id = 'tc1'",
                    [],
                    |row| row.get(0),
                )
            })
            .await
            .unwrap();
        assert_eq!(count, 1, "a tool_call_id must never gain a second result");
    }

    #[tokio::test]
    async fn ask_user_question_blocks_until_answered_then_persists_and_returns_the_answer() {
        let conn = crate::storage::test_async_connection().await;
        seed_conversation(&conn, "c1").await;
        let pending = std::sync::Arc::new(PendingQuestions::default());
        let call = ToolCall {
            name: "AskUserQuestion".to_string(),
            arguments: serde_json::json!({
                "header": "Pick one",
                "question": "Which approach?",
                "options": [{"label": "A", "description": "first"}, {"label": "B", "description": "second"}],
                "multiSelect": false,
            }),
        };
        let emitted: std::sync::Arc<std::sync::Mutex<Option<AskUserQuestionEvent>>> =
            std::sync::Arc::new(std::sync::Mutex::new(None));

        let pending_bg = pending.clone();
        let conn_bg = conn.clone();
        let emitted_bg = emitted.clone();
        let handle = tokio::spawn(async move {
            handle_ask_user_question(None, &conn_bg, &pending_bg, "c1", "q1", &call, |event| {
                *emitted_bg.lock().unwrap() = Some(event);
            })
            .await
        });

        // Let the spawned task run up to (and block on) the `.await` inside
        // `rx.await` — it must not resolve on its own without an answer.
        // Poll the actual condition (the event callback having fired)
        // rather than a fixed yield count: a fixed count was observed
        // flaky in CI (a single failure out of many runs) — on a slower or
        // more loaded scheduler, a fixed number of yields isn't guaranteed
        // to be enough for the background task to reach its blocking
        // point, even though it always does eventually.
        for _ in 0..1000 {
            if emitted.lock().unwrap().is_some() {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert!(emitted.lock().unwrap().is_some(), "event was never emitted");
        assert!(!handle.is_finished(), "must block until answered");

        // The pending tool_call is the latest message while genuinely
        // paused — this is what compute_status's requires_action check
        // relies on.
        let (role, content_type, tool_name, _) = latest_message(&conn, "c1").await;
        assert_eq!(role, "assistant");
        assert_eq!(content_type, "tool_call");
        assert_eq!(tool_name.as_deref(), Some("AskUserQuestion"));

        let event = emitted.lock().unwrap().clone().expect("event was emitted");
        assert_eq!(event.conversation_id, "c1");
        assert_eq!(event.header, "Pick one");
        assert_eq!(event.options.len(), 2);
        let question_id = event.question_id.clone();

        assert!(pending.answer(&question_id, vec!["A".to_string()]));
        let result = handle.await.unwrap();
        assert_eq!(result, "User answered: A");

        let (role, content_type, tool_name, content) = latest_message(&conn, "c1").await;
        assert_eq!(role, "tool");
        assert_eq!(content_type, "tool_result");
        assert_eq!(tool_name.as_deref(), Some("AskUserQuestion"));
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["answer"], serde_json::json!(["A"]));
    }

    #[tokio::test]
    async fn answer_user_question_on_unknown_or_already_answered_id_errors_not_silently() {
        let pending = PendingQuestions::default();
        let _rx = pending.register("q1".to_string());
        assert!(pending.answer("q1", vec!["A".to_string()]));

        // Already answered — the second attempt must not succeed silently.
        assert!(!pending.answer("q1", vec!["B".to_string()]));
        // Never registered at all.
        assert!(!pending.answer("never-registered", vec![]));
    }

    // --- 004-tool-call-widgets: US4 (Task delegation persistence/isolation) ---

    async fn all_messages(
        conn: &tokio_rusqlite::Connection,
        conversation_id: &str,
    ) -> Vec<(String, Option<String>)> {
        let conversation_id = conversation_id.to_string();
        conn.call(move |conn: &mut Connection| -> rusqlite::Result<Vec<(String, Option<String>)>> {
            let mut stmt = conn.prepare(
                "SELECT content_type, tool_name FROM messages WHERE conversation_id = ?1 ORDER BY sequence",
            )?;
            let rows = stmt
                .query_map([&conversation_id], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn task_delegation_persists_a_complete_status_on_the_parent_and_keeps_subagent_activity_isolated(
    ) {
        // `execute_top_level_tool`'s Task branch needs a real loaded model
        // for the nested run_loop itself (not mockable in a unit test), so
        // this exercises exactly what that branch does at the persistence
        // layer — the actual claim T023 cares about — rather than the full
        // spawn+generate flow, which is covered separately by
        // `agent::subagent`'s own tests and the real e2e subagent spec.
        let conn = crate::storage::test_async_connection().await;
        seed_conversation(&conn, "parent").await;
        seed_conversation(&conn, "sub").await;

        // What the subagent's own nested dispatch does for its own tool
        // calls (mirrors execute_top_level_tool's `|tool_call_id, c| { ...
        // }` closure).
        persist_tool_call_and_result(
            None,
            &conn,
            "sub",
            "call1",
            "Read",
            serde_json::json!({"file_path": "/tmp/notes.txt"}),
            "hi",
            serde_json::json!({"toolName": "Read", "filePath": "/tmp/notes.txt", "outcome": {"ok": true, "content": "hi", "truncated": false}}),
            false,
        )
        .await;

        // What execute_top_level_tool now persists on the PARENT: the
        // tool_call row immediately (before spawn_subagent/run_loop), the
        // tool_result row once the delegation completes (FR-010).
        persist_tool_call(
            None,
            &conn,
            "parent",
            "call2",
            "Task",
            serde_json::json!({"prompt": "go read the file"}),
            false,
        )
        .await;
        persist_tool_result(
            None,
            &conn,
            "parent",
            "call2",
            "Task",
            "the file says hi",
            serde_json::json!({
                "toolName": "Task", "prompt": "go read the file",
                "subagentConversationId": "sub", "state": "complete",
            }),
        )
        .await;

        let parent_messages = all_messages(&conn, "parent").await;
        assert_eq!(
            parent_messages,
            vec![
                ("tool_call".to_string(), Some("Task".to_string())),
                ("tool_result".to_string(), Some("Task".to_string())),
            ]
        );
        let (_, _, tool_name, content) = latest_message(&conn, "parent").await;
        assert_eq!(tool_name.as_deref(), Some("Task"));
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["state"], "complete");
        assert_eq!(parsed["subagentConversationId"], "sub");

        // FR-015/SC-008: the subagent's own Read call is on ITS row only —
        // never on the parent's.
        assert!(parent_messages
            .iter()
            .all(|(_, tool_name)| tool_name.as_deref() != Some("Read")));
        let sub_messages = all_messages(&conn, "sub").await;
        assert_eq!(
            sub_messages,
            vec![
                ("tool_call".to_string(), Some("Read".to_string())),
                ("tool_result".to_string(), Some("Read".to_string())),
            ]
        );
    }

    #[tokio::test]
    async fn task_delegation_persists_a_tool_result_when_spawn_subagent_fails_instead_of_leaving_the_tool_call_orphaned(
    ) {
        // Regression test for a review finding on the change above: between
        // the early `persist_tool_call` and the final `persist_tool_result`,
        // `execute_top_level_tool`'s `Task` branch used to `return` straight
        // out of the `spawn_subagent` error arm, skipping `persist_tool_result`
        // entirely -- leaving a `tool_call` row with no paired `tool_result`
        // (widgets pair the two by `tool_call_id`, so an unpaired row renders
        // as stuck "pending" forever). This exercises exactly what that arm
        // now does at the persistence layer -- same reasoning as
        // `task_delegation_persists_a_complete_status_on_the_parent_and_keeps_subagent_activity_isolated`
        // above for why this doesn't call `execute_top_level_tool` itself
        // (it needs a real, live `AppHandle` -- not `Option<&AppHandle>`
        // like `handle_general_tool_call` -- which isn't constructible in a
        // unit test) -- except the failure here is the *real*
        // `subagent::spawn_subagent`, not a simulated one, so the trigger is
        // genuinely deterministic rather than assumed.
        let conn = crate::storage::test_async_connection().await;
        // "parent" itself genuinely exists (`messages.conversation_id` has a
        // `REFERENCES conversations(id)` FK, enforced under
        // `test_async_connection`'s `PRAGMA foreign_keys = ON` -- an INSERT
        // against a nonexistent conversation id would just silently no-op,
        // since `persist_tool_call`/`persist_tool_result` swallow their
        // `conn.call` error). `spawn_subagent` below is deliberately called
        // against a *different*, nonexistent id purely to get the same
        // deterministic `SubagentError::ParentNotFound` it would raise for a
        // missing parent -- see `agent/subagent.rs`'s own
        // `spawning_from_a_nonexistent_parent_is_a_clear_error` test for that
        // same trigger. (`execute_top_level_tool` always passes one and the
        // same id to both; splitting them here only isolates the two DB
        // effects so this test doesn't need a real, live parent-deletion
        // race to exercise the Err arm.)
        seed_conversation(&conn, "parent").await;

        // What execute_top_level_tool's Task branch persists immediately,
        // before spawn_subagent ever runs.
        persist_tool_call(
            None,
            &conn,
            "parent",
            "call1",
            "Task",
            serde_json::json!({"prompt": "go read the file"}),
            false,
        )
        .await;

        // The real spawn_subagent call, against a parent id absent from the
        // conversations table -- deterministically Err(ParentNotFound).
        let spawn_result = conn
            .call(|conn: &mut Connection| {
                subagent::spawn_subagent(conn, "does-not-exist", "go read the file")
            })
            .await;
        let e = spawn_result.expect_err("spawning from a nonexistent parent must fail");

        // What the fixed Err arm now does: persist a paired tool_result
        // (state "complete", empty subagentConversationId -- nothing was
        // ever spawned) instead of returning without pairing the row.
        let error_text = format!("Error: failed to spawn subagent: {e}");
        persist_tool_result(
            None,
            &conn,
            "parent",
            "call1",
            "Task",
            &error_text,
            serde_json::json!({
                "toolName": "Task",
                "prompt": "go read the file",
                "subagentConversationId": "",
                "state": "complete",
            }),
        )
        .await;

        let parent_messages = all_messages(&conn, "parent").await;
        assert_eq!(
            parent_messages,
            vec![
                ("tool_call".to_string(), Some("Task".to_string())),
                ("tool_result".to_string(), Some("Task".to_string())),
            ],
            "the tool_call must not be left orphaned -- a tool_result has to follow it"
        );

        let (role, content_type, tool_name, content) = latest_message(&conn, "parent").await;
        assert_eq!(role, "tool");
        assert_eq!(content_type, "tool_result");
        assert_eq!(tool_name.as_deref(), Some("Task"));
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["state"], "complete");
        assert_eq!(parsed["subagentConversationId"], "");
        assert_eq!(parsed["prompt"], "go read the file");

        // `model_text` (what the model actually sees for this tool result)
        // must carry the error, not a generic/empty placeholder -- the same
        // text `execute_top_level_tool`'s fixed Err arm returns to its
        // caller.
        assert!(error_text.contains("failed to spawn subagent"));
    }

    // --- Task 2: token-count annotation ---

    fn test_model_path() -> std::path::PathBuf {
        let home = std::env::var("HOME").expect("HOME must be set");
        std::path::PathBuf::from(home).join(
            "Library/Application Support/app.doce.desktop/models/qwen3-4b-instruct-2507-q4_k_m.gguf",
        )
    }

    #[tokio::test]
    #[ignore]
    async fn subagent_backend_tool_result_carries_a_real_token_count_for_read() {
        let conn = crate::storage::test_async_connection().await;
        seed_conversation(&conn, "sub").await;
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("notes.txt"), "hello world").unwrap();

        let engine = crate::inference::InferenceEngine::load(&test_model_path(), 4)
            .expect("model should load");
        let mut backend = SubagentBackend {
            engine: &engine,
            conn: &conn,
            subagent_id: "sub",
            cwd: Some(dir.path()),
            threshold: 1024,
            plan_state: crate::agent::plan::PlanState::default(),
        };
        use crate::agent::AgentBackend;
        let call = crate::agent::ToolCall {
            name: "Read".to_string(),
            arguments: serde_json::json!({"file_path": "notes.txt"}),
        };
        backend.execute_tool("call1".to_string(), call).await;

        let (_, _, _, content) = latest_message(&conn, "sub").await;
        let detail: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert!(detail["tokenCount"].as_u64().is_some());
    }

    // --- Task 3: early tool_call persist for the general top-level path ---

    #[tokio::test]
    #[ignore]
    async fn handle_general_tool_call_persists_the_tool_call_row_before_the_tool_result_row() {
        let conn = crate::storage::test_async_connection().await;
        seed_conversation(&conn, "c1").await;
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("notes.txt"), "hello world").unwrap();

        let engine = crate::inference::InferenceEngine::load(&test_model_path(), 4)
            .expect("model should load");
        let call = ToolCall {
            name: "Read".to_string(),
            arguments: serde_json::json!({"file_path": "notes.txt"}),
        };

        let model_text = handle_general_tool_call(
            None,
            &conn,
            &engine,
            "c1",
            Some(dir.path()),
            "call1",
            &call,
        )
        .await;

        assert!(model_text.contains("hello world"));

        // `all_messages` (already defined in this test module, near
        // `task_delegation_persists_...`) returns `Vec<(content_type,
        // tool_name)>`, ordered by sequence — enough to confirm the
        // tool_call row landed before the tool_result row.
        let rows = all_messages(&conn, "c1").await;
        assert_eq!(rows.len(), 2, "expected exactly a tool_call row and a tool_result row");
        assert_eq!(rows[0].0, "tool_call");
        assert_eq!(rows[1].0, "tool_result");

        // `latest_message` (already defined in this test module) returns
        // (role, content_type, tool_name, content) for the newest row —
        // which, after the two inserts above, is the tool_result row.
        let (_, content_type, _, content) = latest_message(&conn, "c1").await;
        assert_eq!(content_type, "tool_result");
        let result_detail: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert!(
            result_detail["tokenCount"].as_u64().is_some(),
            "Read is one of the four annotated tools"
        );
    }

    #[test]
    fn plan_snapshot_reflects_state_and_current_step() {
        use crate::agent::plan::{LoopState, Plan, PlanState, PlanStep};
        let mut state = PlanState::default();
        state.plan = Plan {
            goal: "g".to_string(),
            steps: vec![
                PlanStep { description: "a".to_string(), done: true },
                PlanStep { description: "b".to_string(), done: false },
            ],
        };
        state.state = LoopState::Executing { step_index: 1 };

        let snapshot = plan_snapshot(&state);
        assert_eq!(snapshot.goal, "g");
        assert_eq!(snapshot.steps.len(), 2);
        assert!(snapshot.steps[0].done);
        assert_eq!(snapshot.current_step_index, Some(1));

        state.state = LoopState::Planning;
        assert_eq!(plan_snapshot(&state).current_step_index, None);
    }

    #[test]
    fn publish_plan_update_only_registers_a_plan_that_exists_and_guard_drop_clears_it() {
        use crate::agent::plan::PlanState;
        let active_plans = ActivePlans::default();
        let mut state = PlanState::default();

        // No plan yet (empty steps): publishing must not register an entry.
        publish_plan_update(None, &active_plans, "c1", &state);
        assert!(active_plans.0.lock().unwrap().get("c1").is_none());

        state.handle_plan_tool(&crate::agent::ToolCall {
            name: "CreatePlan".to_string(),
            arguments: serde_json::json!({"goal": "g", "steps": ["a"]}),
        });
        publish_plan_update(None, &active_plans, "c1", &state);
        assert_eq!(
            active_plans.0.lock().unwrap().get("c1").unwrap().goal,
            "g"
        );

        // Guard clear is exercised without an AppHandle via the map
        // directly (the emit half needs a live app; the map half is the
        // reload-recovery source of truth get_active_plan reads).
        active_plans.0.lock().unwrap().remove("c1");
        assert!(active_plans.0.lock().unwrap().get("c1").is_none());
    }

    #[test]
    fn active_plan_guard_drop_clears_a_registered_plan_without_an_app_handle() {
        use crate::agent::plan::PlanState;
        let active_plans = ActivePlans::default();
        let mut state = PlanState::default();
        state.handle_plan_tool(&crate::agent::ToolCall {
            name: "CreatePlan".to_string(),
            arguments: serde_json::json!({"goal": "g", "steps": ["a"]}),
        });
        publish_plan_update(None, &active_plans, "c1", &state);
        assert!(
            active_plans.0.lock().unwrap().get("c1").is_some(),
            "precondition: a plan must actually be registered before the guard drops"
        );

        {
            let _guard = ActivePlanGuard {
                active_plans: &active_plans,
                app: None,
                conversation_id: "c1".to_string(),
            };
        }

        assert!(
            active_plans.0.lock().unwrap().get("c1").is_none(),
            "ActivePlanGuard's Drop must remove the entry even with no AppHandle to emit through"
        );
    }

    #[tokio::test]
    async fn persist_plan_tool_marks_both_rows_shape_with_plan_true() {
        let conn = crate::storage::test_async_connection().await;
        seed_conversation(&conn, "c1").await;

        persist_plan_tool(
            None,
            &conn,
            "c1",
            "tc1",
            &crate::agent::ToolCall {
                name: "CreatePlan".to_string(),
                arguments: serde_json::json!({"goal": "g", "steps": ["a"]}),
            },
            "Plan created with 1 steps. Call ResumeExecution to begin.",
        )
        .await;

        let (role, content_type, tool_name, content) = latest_message(&conn, "c1").await;
        assert_eq!(role, "tool");
        assert_eq!(content_type, "tool_result");
        assert_eq!(tool_name.as_deref(), Some("CreatePlan"));
        let detail: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(detail["plan"], true, "the transcript-skip marker");
        assert_eq!(detail["outcome"]["ok"], true);
    }

    /// Fetches the sole `tool_call` row's `content` for `conversation_id` --
    /// unlike `latest_message` (which returns whatever row sorts last, the
    /// paired `tool_result` for a `persist_plan_tool`/
    /// `persist_tool_call_and_result` call), this targets the CALL row
    /// specifically.
    async fn tool_call_content(conn: &tokio_rusqlite::Connection, conversation_id: &str) -> String {
        let conversation_id = conversation_id.to_string();
        conn.call(move |conn: &mut Connection| {
            conn.query_row(
                "SELECT content FROM messages WHERE conversation_id = ?1 AND content_type = 'tool_call'",
                [&conversation_id],
                |row| row.get(0),
            )
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn persist_plan_tool_marks_the_call_row_with_plan_true_too() {
        // Regression for a review finding on baab3f3: the CALL row used to
        // persist bare `{"arguments": ...}` with no "plan" marker at all --
        // only the RESULT row (asserted in
        // persist_plan_tool_marks_both_rows_shape_with_plan_true above)
        // ever carried one. That asymmetry let
        // context::apply_lightweight_clearing's plan/regular partitioning
        // silently miscount a plan interaction's call row as an ordinary
        // tool row, which could push genuine tool history out of
        // TOOL_KEEP_N prematurely (reproduced at the pure-function level by
        // context::mod's own
        // plan_call_rows_are_plan_partitioned_and_never_displace_regular_tool_history
        // test).
        let conn = crate::storage::test_async_connection().await;
        seed_conversation(&conn, "c1").await;

        persist_plan_tool(
            None,
            &conn,
            "c1",
            "tc1",
            &crate::agent::ToolCall {
                name: "CreatePlan".to_string(),
                arguments: serde_json::json!({"goal": "g", "steps": ["a"]}),
            },
            "Plan created with 1 steps. Call ResumeExecution to begin.",
        )
        .await;

        let call_content = tool_call_content(&conn, "c1").await;
        let call_detail: serde_json::Value = serde_json::from_str(&call_content).unwrap();
        assert_eq!(
            call_detail["plan"], true,
            "the call row must carry the same plan marker as its paired result row"
        );
        assert_eq!(call_detail["arguments"]["goal"], "g");
    }

    #[tokio::test]
    async fn persist_tool_call_for_a_regular_tool_never_gains_a_plan_marker() {
        // The other half of the same fix: the plan marker must be opt-in,
        // not leak onto ordinary tool calls that never touch the plan
        // machine -- locks the persisted shape for every non-plan caller
        // (handle_general_tool_call, handle_ask_user_question, the Task
        // branch) as byte-for-byte unchanged from before this fix.
        let conn = crate::storage::test_async_connection().await;
        seed_conversation(&conn, "c1").await;

        persist_tool_call(
            None,
            &conn,
            "c1",
            "tc1",
            "Bash",
            serde_json::json!({"command": "ls"}),
            false,
        )
        .await;

        let call_content = tool_call_content(&conn, "c1").await;
        assert_eq!(
            call_content,
            serde_json::json!({"arguments": {"command": "ls"}}).to_string(),
            "a non-plan tool_call row must not gain a plan key at all"
        );
    }

    #[tokio::test]
    async fn subagent_plan_rows_persist_under_the_subagent_conversation_with_the_plan_marker() {
        let conn = crate::storage::test_async_connection().await;
        seed_conversation(&conn, "sub-1").await;

        persist_plan_tool(
            None,
            &conn,
            "sub-1",
            "tc1",
            &crate::agent::ToolCall {
                name: "CreatePlan".to_string(),
                arguments: serde_json::json!({"goal": "g", "steps": ["a"]}),
            },
            "Plan created with 1 steps.",
        )
        .await;
        let (_, content_type, _, content) = latest_message(&conn, "sub-1").await;
        assert_eq!(content_type, "tool_result");
        let detail: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(detail["plan"], true);
    }
}
