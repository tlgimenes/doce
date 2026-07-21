use crate::commands::agent::{conversation_cwd, conversation_system_message, memories_section};
use crate::commands::conversations::CompactingConversations;
use crate::context::{self, ContextUsage};
use crate::storage::DbCell;
use tauri::{AppHandle, Manager, State};

/// Marks `conversation_id` as running a standalone `/compact` for its lifetime,
/// clearing it on every exit path (RAII, like `ActiveGenerationGuard`). While it
/// is set, `steer_generation` returns `Rejected` rather than `NoActiveTurn`, so
/// the frontend keeps the message queued instead of dispatching it as a doomed
/// new turn against the busy server.
struct CompactingGuard<'a> {
    compacting: &'a CompactingConversations,
    conversation_id: String,
}

impl Drop for CompactingGuard<'_> {
    fn drop(&mut self) {
        self.compacting
            .0
            .lock()
            .unwrap()
            .remove(&self.conversation_id);
    }
}

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
    // FR-2: the SAME bundled `State` `send_agent_message`/`compact_conversation`
    // read/write (`context::CompactionState`'s own doc comment) -- a
    // conversation's last observed `prompt_tokens` must be the one instance
    // every entry point shares, or this reopen snapshot could disagree with
    // the live indicator.
    compaction_state: State<'_, context::CompactionState>,
    conversation_id: String,
) -> Result<ContextUsage, String> {
    let conn = db_cell.get(&app).await?.clone();
    let skills_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("skills");

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
    let memories = memories_section(&conn, &conversation_id).await;
    let system_prompt = conversation_system_message(
        cwd.as_deref(),
        transcript_dir.as_deref(),
        &conversation_id,
        memories.as_deref(),
    );

    // FR-2: `.cloned()` to drop the lock before `compute_usage` runs.
    let observed = compaction_state
        .observed_usage
        .0
        .lock()
        .unwrap()
        .get(&conversation_id)
        .cloned();
    context::compute_usage(
        &conn,
        &conversation_id,
        &skills_dir,
        &system_prompt,
        observed.as_ref(),
    )
    .await
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
    server_state: State<'_, crate::inference::server::ServerState>,
    // FR-2: same shared bundle as `get_context_usage`/`send_agent_message` --
    // see `context::CompactionState`'s own doc comment.
    compaction_state: State<'_, context::CompactionState>,
    compacting: State<'_, CompactingConversations>,
    conversation_id: String,
) -> Result<ContextUsage, String> {
    // Mark this conversation as compacting for the duration of the call so a
    // concurrent `steer_generation` returns `Rejected` (keep queued), not
    // `NoActiveTurn` (dispatch a doomed turn). RAII-cleared on every exit.
    compacting.0.lock().unwrap().insert(conversation_id.clone());
    let _compacting_guard = CompactingGuard {
        compacting: &compacting,
        conversation_id: conversation_id.clone(),
    };

    let conn = db_cell.get(&app).await?.clone();
    crate::commands::models::ensure_usable_model_path(&app, &conn, &server_state).await?;

    // Manual compaction calls the same model server as a normal turn, so it
    // participates in the same handoff gate and cannot race a server restart.
    // Keep this guard alive for the full compaction operation.
    let _generation_lease = server_state.generation_lease().await;
    let model_path = crate::commands::models::active_model_path(&conn).await?;
    let skills_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("skills");

    // Manual compaction now resolves the same active global model as a normal
    // turn, including a cold start after launch or a recovered fallback.
    let base_url = server_state
        .ensure_running(&app, std::path::Path::new(&model_path))
        .await?;

    let transcript_dir = app
        .path()
        .app_data_dir()
        .ok()
        .map(|d| d.join("transcripts"));
    let cwd = conversation_cwd(&conn, &conversation_id).await?;
    let memories = memories_section(&conn, &conversation_id).await;
    let system_prompt = conversation_system_message(
        cwd.as_deref(),
        transcript_dir.as_deref(),
        &conversation_id,
        memories.as_deref(),
    );
    let cancel = tokio_util::sync::CancellationToken::new();
    let compact = context::maybe_compact(
        &conn,
        transcript_dir,
        &base_url,
        &conversation_id,
        &skills_dir,
        &system_prompt,
        true,
        &compaction_state.failures,
        &compaction_state.observed_usage,
        &cancel,
    );
    tokio::time::timeout(std::time::Duration::from_secs(5 * 60), compact)
        .await
        .map_err(|_| "Compaction took too long and was stopped.".to_string())?
}
