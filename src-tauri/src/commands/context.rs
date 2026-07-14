use crate::commands::agent::{conversation_cwd, conversation_system_message};
use crate::commands::conversations::InferenceState;
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

    // The exact system prompt the next real turn will run with, resolved
    // through the same helpers
    // `send_agent_message` uses — so this on-demand snapshot and the live
    // `context-usage-update` events can never disagree about the prompt.
    let cwd = conversation_cwd(&conn, &conversation_id).await?;
    let transcript_dir = app
        .path()
        .app_data_dir()
        .ok()
        .map(|d| d.join("transcripts"));
    let system_prompt = conversation_system_message(
        cwd.as_deref(),
        transcript_dir.as_deref(),
        &conversation_id,
        engine.dialect(),
    );

    context::compute_usage(&conn, engine, &conversation_id, &skills_dir, &system_prompt).await
}

/// 010-context-window-management/US2 (FR-009): forces the same tiered
/// compaction pipeline `send_agent_message` runs pre-flight, immediately,
/// regardless of whether the compaction threshold has actually been
/// crossed. A no-op (returns the current, unchanged usage) if there's
/// nothing eligible to clear or summarize -- never fabricates a notice.
#[tauri::command]
#[specta::specta]
pub async fn compact_conversation(
    app: AppHandle,
    db_cell: State<'_, DbCell>,
    inference_state: State<'_, InferenceState>,
    server_state: State<'_, crate::inference::server::ServerState>,
    conversation_id: String,
) -> Result<ContextUsage, String> {
    let conn = db_cell.get(&app).await?.clone();
    let skills_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("skills");

    // A manual "Compact now" summarizes through the supervised server, so it
    // requires one to already be running (a turn spawns it; there's no model
    // path in hand here to launch one on demand). Erroring if none is up is
    // honest -- there's nothing to generate the summary against otherwise.
    let base_url = server_state
        .current_base_url()
        .await
        .ok_or("model server not running")?;

    let guard = inference_state.0.lock().await;
    let engine = guard.as_ref().ok_or("No model loaded")?;

    let transcript_dir = app
        .path()
        .app_data_dir()
        .ok()
        .map(|d| d.join("transcripts"));
    let cwd = conversation_cwd(&conn, &conversation_id).await?;
    let system_prompt = conversation_system_message(
        cwd.as_deref(),
        transcript_dir.as_deref(),
        &conversation_id,
        engine.dialect(),
    );
    context::maybe_compact(
        &conn,
        transcript_dir,
        engine,
        &base_url,
        &conversation_id,
        &skills_dir,
        &system_prompt,
        true,
    )
    .await
}
