use crate::commands::conversations::{InferenceState, CHAT_SYSTEM_PROMPT};
use crate::context::{self, ContextUsage};
use crate::storage::DbCell;
use tauri::{AppHandle, Manager, State};

/// 010-context-window-management/US1 (FR-014): computes and returns the
/// conversation's current context usage on demand — called by the frontend
/// once when a conversation is opened/switched to, independent of any live
/// event, so the indicator is correct immediately after a reopen rather
/// than waiting for the next turn.
#[tauri::command]
#[specta::specta]
pub async fn get_context_usage(
    app: AppHandle,
    db_cell: State<'_, DbCell>,
    inference_state: State<'_, InferenceState>,
    conversation_id: String,
) -> Result<ContextUsage, String> {
    let conn = db_cell.get(&app).await?.clone();
    let skills_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("skills");

    let guard = inference_state.0.lock().await;
    let engine = guard.as_ref().ok_or("No model loaded")?;

    // Plain-chat system prompt is used as the default here: this command is
    // called generically (e.g. right after opening any conversation, before
    // the caller necessarily knows whether it'll be used for chat or agent
    // mode this session) -- both prompts are short enough that the token
    // count this produces is a close, honest estimate either way, and the
    // live `context-usage-update` events emitted from the real send_message/
    // send_agent_message paths (which do know their exact mode) supersede
    // this snapshot the moment a turn actually runs.
    context::compute_usage(
        &conn,
        engine,
        &conversation_id,
        &skills_dir,
        CHAT_SYSTEM_PROMPT,
    )
    .await
}

/// 010-context-window-management/US2 (FR-009): forces the same tiered
/// compaction pipeline `send_message`/`send_agent_message` run pre-flight,
/// immediately, regardless of whether the compaction threshold has actually
/// been crossed. A no-op (returns the current, unchanged usage) if there's
/// nothing eligible to clear or summarize -- never fabricates a notice.
#[tauri::command]
#[specta::specta]
pub async fn compact_conversation(
    app: AppHandle,
    db_cell: State<'_, DbCell>,
    inference_state: State<'_, InferenceState>,
    conversation_id: String,
) -> Result<ContextUsage, String> {
    let conn = db_cell.get(&app).await?.clone();
    let skills_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("skills");

    let guard = inference_state.0.lock().await;
    let engine = guard.as_ref().ok_or("No model loaded")?;

    let transcript_dir = app
        .path()
        .app_data_dir()
        .ok()
        .map(|d| d.join("transcripts"));
    context::maybe_compact(
        &conn,
        transcript_dir,
        engine,
        &conversation_id,
        &skills_dir,
        CHAT_SYSTEM_PROMPT,
        true,
    )
    .await
}
