use crate::agent::{dispatch, run_loop, subagent, AgentContext, ToolCall, SYSTEM_PROMPT};
use crate::commands::conversations::InferenceState;
use crate::commands::models::now_ms;
use crate::inference::{ChatMessage, InferenceEngine};
use crate::storage::conversations::load_history;
use crate::storage::DbCell;
use rusqlite::Connection;
use tauri::{AppHandle, State};
use uuid::Uuid;

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
async fn execute_top_level_tool(
    call: ToolCall,
    conn: &tokio_rusqlite::Connection,
    engine: &InferenceEngine,
    parent_conversation_id: &str,
    cwd: Option<&std::path::Path>,
) -> String {
    if call.name != "Task" {
        return dispatch::execute(&call, cwd);
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
        ChatMessage::user(prompt),
    ];
    let sub_cwd = sub_context.cwd.clone();
    let sub_result = run_loop(
        &sub_context,
        sub_messages,
        |messages| async move {
            match engine.render_chat_prompt(&messages) {
                Ok(rendered) => engine
                    .generate(&rendered, 256, |_piece| {}, || false)
                    .unwrap_or_else(|e| format!("Error: inference failed: {e}")),
                Err(e) => format!("Error: failed to render chat prompt: {e}"),
            }
        },
        |c| {
            let sub_cwd = sub_cwd.clone();
            async move { dispatch::execute(&c, sub_cwd.as_deref()) }
        },
    )
    .await;

    let sub_final = match sub_result {
        Ok(text) => text,
        Err(e) => format!("Error: {e}"),
    };

    let now = now_ms();
    let sub_final_for_db = sub_final.clone();
    let _ = conn
        .call(move |conn: &mut Connection| -> rusqlite::Result<()> {
            let seq: i64 = conn.query_row(
                "SELECT COALESCE(MAX(sequence), -1) + 1 FROM messages WHERE conversation_id = ?1",
                [&subagent_id],
                |row| row.get(0),
            )?;
            conn.execute(
                "INSERT INTO messages (id, conversation_id, role, content_type, content, created_at, sequence) VALUES (?1, ?2, 'assistant', 'text', ?3, ?4, ?5)",
                rusqlite::params![Uuid::now_v7().to_string(), subagent_id, sub_final_for_db, now, seq],
            )?;
            Ok(())
        })
        .await;

    sub_final
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
pub async fn send_agent_message(
    app: AppHandle,
    db_cell: State<'_, DbCell>,
    inference: State<'_, InferenceState>,
    conversation_id: String,
    content: String,
) -> Result<String, String> {
    let conn = db_cell.get(&app).await?.clone();
    let now = now_ms();

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

    conn.call({
        let conversation_id = conversation_id.clone();
        let content = content.clone();
        move |conn: &mut Connection| -> rusqlite::Result<()> {
            conn.execute(
                "INSERT INTO messages (id, conversation_id, role, content_type, content, created_at, sequence) VALUES (?1, ?2, 'user', 'text', ?3, ?4, ?5)",
                rusqlite::params![Uuid::now_v7().to_string(), conversation_id, content, now, next_seq],
            )?;
            Ok(())
        }
    })
    .await
    .map_err(|e| e.to_string())?;

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

    // Full history (including the user message just inserted above) so the
    // model sees prior turns in this workspace conversation rather than
    // generating each reply from a blank slate.
    let history = conn
        .call({
            let conversation_id = conversation_id.clone();
            move |conn: &mut Connection| load_history(conn, &conversation_id)
        })
        .await
        .map_err(|e| e.to_string())?;
    let mut initial_messages = vec![ChatMessage::system(SYSTEM_PROMPT)];
    initial_messages.extend(history);

    let result = run_loop(
        &context,
        initial_messages,
        |messages| async move {
            match engine.render_chat_prompt(&messages) {
                Ok(rendered) => engine
                    .generate(&rendered, 256, |_piece| {}, || false)
                    .unwrap_or_else(|e| format!("Error: inference failed: {e}")),
                Err(e) => format!("Error: failed to render chat prompt: {e}"),
            }
        },
        |call| execute_top_level_tool(call, &conn, engine, &conversation_id, cwd.as_deref()),
    )
    .await;
    drop(guard);

    let final_text = match result {
        Ok(text) => text,
        Err(e) => format!("Error: {e}"),
    };

    let now = now_ms();
    conn.call({
        let conversation_id = conversation_id.clone();
        let final_text = final_text.clone();
        move |conn: &mut Connection| -> rusqlite::Result<()> {
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
            Ok(())
        }
    })
    .await
    .map_err(|e| e.to_string())?;

    Ok(final_text)
}
