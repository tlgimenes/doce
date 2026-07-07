use crate::agent::rich_content::{expand_segments, RichMessageContent};
use crate::agent::tools::ask_user::{PendingQuestions, QuestionOption};
use crate::agent::{dispatch, run_loop, subagent, AgentContext, ToolCall, SYSTEM_PROMPT};
use crate::commands::conversations::{ActiveGenerations, InferenceState};
use crate::commands::models::now_ms;
use crate::inference::{ChatMessage, InferenceEngine};
use crate::storage::conversations::load_history;
use crate::storage::DbCell;
use rusqlite::Connection;
use serde::Serialize;
use std::path::Path;
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
async fn persist_tool_call(
    app: Option<&AppHandle>,
    conn: &tokio_rusqlite::Connection,
    conversation_id: &str,
    tool_call_id: &str,
    tool_name: &str,
    arguments: serde_json::Value,
) {
    let conversation_id = conversation_id.to_string();
    let tool_call_id = tool_call_id.to_string();
    let tool_name = tool_name.to_string();
    let now = now_ms();
    let call_content = serde_json::json!({ "arguments": arguments }).to_string();
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
/// `tool_call_id`.
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
) {
    persist_tool_call(
        app,
        conn,
        conversation_id,
        tool_call_id,
        tool_name,
        arguments,
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
    conn: &'a tokio_rusqlite::Connection,
    conversation_id: &'a str,
    app: &'a AppHandle,
    settings: &'a crate::context::ContextSettings,
    threshold: u32,
    cwd: Option<&'a Path>,
    pending: &'a PendingQuestions,
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

    async fn generate(&mut self, messages: Vec<ChatMessage>) -> String {
        match self.engine.render_chat_prompt(&messages) {
            Ok(rendered) => self
                .engine
                .generate(&rendered, 256, true, |_piece| {}, || false)
                .unwrap_or_else(|e| format!("Error: inference failed: {e}")),
            Err(e) => format!("Error: failed to render chat prompt: {e}"),
        }
    }

    async fn execute_tool(&mut self, tool_call_id: String, call: ToolCall) -> String {
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
        .await
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

    async fn generate(&mut self, messages: Vec<ChatMessage>) -> String {
        match self.engine.render_chat_prompt(&messages) {
            Ok(rendered) => self
                .engine
                .generate(&rendered, 256, true, |_piece| {}, || false)
                .unwrap_or_else(|e| format!("Error: inference failed: {e}")),
            Err(e) => format!("Error: failed to render chat prompt: {e}"),
        }
    }

    async fn execute_tool(&mut self, tool_call_id: String, call: ToolCall) -> String {
        // 004-tool-call-widgets: the subagent's own tool activity persists
        // under its own conversation row -- never the parent's --
        // preserving 001's existing FR-015/SC-008 isolation guarantee
        // (only its final answer, inserted by the caller, ever reaches the
        // parent's transcript). No live-refresh event (`app: None`) -- it
        // isn't rendered by any current view, so there's no consumer to
        // notify.
        let outcome = dispatch::execute(&call, self.cwd);
        persist_tool_call_and_result(
            None,
            self.conn,
            self.subagent_id,
            &tool_call_id,
            &call.name,
            call.arguments.clone(),
            &outcome.model_text,
            outcome.detail.clone(),
        )
        .await;
        outcome.model_text
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
        let outcome = dispatch::execute(&call, cwd);

        // 010-context-window-management/US3 (FR-011/FR-012): an oversized
        // result gets truncated to a preview + a `Read`-able pointer before
        // it ever enters the model's context -- the persisted `detail`
        // still carries the full outcome for the transcript UI (widgets
        // decide for themselves whether to show a "view full output"
        // affordance), only `model_text` (what the model actually sees) is
        // substituted.
        let settings = crate::context::ContextSettings::load(conn)
            .await
            .unwrap_or_else(|_| crate::context::ContextSettings::from_raw(&Default::default()));
        let (model_text, offloaded_to) = match app.path().app_data_dir() {
            Ok(app_data_dir) => crate::context::offload::offload_if_oversized(
                &app_data_dir,
                parent_conversation_id,
                &tool_call_id,
                &outcome.model_text,
                settings.tool_output_offload_chars,
            )
            .unwrap_or_else(|_| (outcome.model_text.clone(), None)),
            Err(_) => (outcome.model_text.clone(), None),
        };

        let mut detail = outcome.detail.clone();
        detail["offloadedTo"] = serde_json::json!(offloaded_to);

        persist_tool_call_and_result(
            Some(app),
            conn,
            parent_conversation_id,
            &tool_call_id,
            &call.name,
            call.arguments.clone(),
            &model_text,
            detail,
        )
        .await;
        emit_context_usage_update(app, conn, engine, parent_conversation_id, cwd).await;
        return model_text;
    }

    let Some(prompt) = call.arguments.get("prompt").and_then(|v| v.as_str()) else {
        return "Error: Task requires a prompt argument".to_string();
    };
    let prompt = prompt.to_string();

    let parent_id = parent_conversation_id.to_string();
    let prompt_for_spawn = prompt.clone();
    let subagent_id = match conn
        .call(move |conn: &mut Connection| {
            subagent::spawn_subagent(conn, &parent_id, &prompt_for_spawn)
        })
        .await
    {
        Ok(id) => id,
        Err(e) => return format!("Error: failed to spawn subagent: {e}"),
    };

    // 007-workspace-cwd-resolution/FR-006: inherit the parent's cwd rather
    // than starting the subagent unscoped.
    let sub_context = AgentContext::subagent().with_cwd(cwd.map(|p| p.to_path_buf()));
    // FR-015: a fresh, isolated context — just the system prompt plus the
    // delegated task, no parent conversation history.
    let sub_messages = vec![
        ChatMessage::system(SYSTEM_PROMPT),
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
    };
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
    let _ = conn
        .call(move |conn: &mut Connection| -> rusqlite::Result<()> {
            let seq: i64 = conn.query_row(
                "SELECT COALESCE(MAX(sequence), -1) + 1 FROM messages WHERE conversation_id = ?1",
                [&subagent_id_for_db],
                |row| row.get(0),
            )?;
            conn.execute(
                "INSERT INTO messages (id, conversation_id, role, content_type, content, created_at, sequence, token_count) VALUES (?1, ?2, 'assistant', 'text', ?3, ?4, ?5, ?6)",
                rusqlite::params![Uuid::now_v7().to_string(), subagent_id_for_db, sub_final_for_db, now, seq, sub_token_count],
            )?;
            Ok(())
        })
        .await;

    // 004-tool-call-widgets/FR-010: the parent conversation only ever sees
    // a running/complete status for the delegation itself — never the
    // subagent's own tool calls above, which stayed on `subagent_id`.
    // Always "complete" here since this function only returns once the
    // whole nested loop has finished (research.md § 2 — no live
    // mid-delegation status this pass).
    persist_tool_call_and_result(
        Some(app),
        conn,
        parent_conversation_id,
        &tool_call_id,
        "Task",
        serde_json::json!({ "prompt": prompt }),
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
    if let Ok(usage) = crate::context::compute_usage(
        conn,
        engine,
        conversation_id,
        &skills_dir,
        &system_message(cwd),
    )
    .await
    {
        let _ = app.emit("context-usage-update", usage);
    }
}

/// 006-chat-empty-state (research.md § 1): tells the model what directory
/// it's working in when one is known, so it can construct sensible paths
/// itself. Deliberately just this — it does not make `Bash` run with `cwd`
/// as its process working directory, or make `Read`/`Write`/`Edit`/`Glob`/
/// `Grep` resolve relative paths against it; that fuller fix is its own,
/// separate, larger change (see `plan.md`'s Complexity Tracking).
fn system_message(cwd: Option<&std::path::Path>) -> String {
    match cwd {
        Some(path) => format!(
            "{SYSTEM_PROMPT}\n\nYou are currently working in the directory: {}",
            path.display()
        ),
        None => SYSTEM_PROMPT.to_string(),
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
    let system_prompt = system_message(cwd.as_deref());
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

    let mut backend = RealBackend {
        engine,
        conn: &conn,
        conversation_id: &conversation_id,
        app: &app,
        settings: &settings,
        threshold,
        cwd: cwd.as_deref(),
        pending: &pending_questions,
    };
    let result = run_loop(&context, initial_messages, &mut backend).await;
    drop(guard);

    let final_text = match result {
        Ok(text) => text,
        Err(e) => format!("Error: {e}"),
    };

    let now = now_ms();
    let final_seq: i64 = conn
        .call({
            let conversation_id = conversation_id.clone();
            let final_text = final_text.clone();
            move |conn: &mut Connection| -> rusqlite::Result<i64> {
                let seq: i64 = conn.query_row(
                    "SELECT COALESCE(MAX(sequence), -1) + 1 FROM messages WHERE conversation_id = ?1",
                    [&conversation_id],
                    |row| row.get(0),
                )?;
                conn.execute(
                    "INSERT INTO messages (id, conversation_id, role, content_type, content, created_at, sequence) VALUES (?1, ?2, 'assistant', 'text', ?3, ?4, ?5)",
                    rusqlite::params![Uuid::now_v7().to_string(), conversation_id, final_text, now, seq],
                )?;
                conn.execute("UPDATE conversations SET updated_at = ?1 WHERE id = ?2", rusqlite::params![now, conversation_id])?;
                Ok(seq)
            }
        })
        .await
        .map_err(|e| e.to_string())?;

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
    fn system_message_appends_the_cwd_line_when_known() {
        let msg = system_message(Some(std::path::Path::new("/Users/tester/code/doce")));
        assert!(msg.starts_with(SYSTEM_PROMPT));
        assert!(msg.contains("You are currently working in the directory: /Users/tester/code/doce"));
    }

    #[test]
    fn system_message_is_unchanged_when_no_cwd_is_known() {
        assert_eq!(system_message(None), SYSTEM_PROMPT);
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
        )
        .await;

        // What execute_top_level_tool persists on the PARENT once the
        // delegation itself completes (FR-010).
        persist_tool_call_and_result(
            None,
            &conn,
            "parent",
            "call2",
            "Task",
            serde_json::json!({"prompt": "go read the file"}),
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
}
