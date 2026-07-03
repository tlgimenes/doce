use crate::commands::conversations::{
    ActiveGenerations, AssistantMessageComplete, AssistantMessageError, AssistantToken,
};
use crate::commands::models::now_ms;
use crate::inference::InferenceEngine;
use crate::scheduler::{GenerationQueueUpdate, GenerationRequest, Scheduler};
use crate::storage::DbCell;
use rusqlite::Connection;
use tauri::{AppHandle, Emitter, Manager};

/// The single worker every `send_message` call now funnels through
/// (US5/tasks.md T039): pulls one request at a time off the `Scheduler`,
/// runs it to completion (or cancellation), persists the result, and emits
/// the same `assistant-token`/`assistant-message-complete` events the
/// frontend already listens for — none of that changed, only who's
/// actually running the generation.
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            let request = app.state::<Scheduler>().pop_next();
            let Some(request) = request else {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                continue;
            };
            run_one(&app, request).await;
        }
    });
}

async fn run_one(app: &AppHandle, request: GenerationRequest) {
    let _ = app.emit(
        "generation-queue-update",
        GenerationQueueUpdate {
            request_id: request.request_id.clone(),
            conversation_id: request.conversation_id.clone(),
            state: "generating".to_string(),
            position: None,
        },
    );

    let result = run_generation(app, &request).await;
    if let Err(e) = &result {
        let _ = app.emit(
            "assistant-message-error",
            AssistantMessageError {
                conversation_id: request.conversation_id.clone(),
                message_id: request.assistant_message_id.clone(),
                error: e.clone(),
            },
        );
    }
    let _ = request.result_tx.send(result);

    if let Some(active) = app.try_state::<ActiveGenerations>() {
        active.0.lock().unwrap().remove(&request.conversation_id);
    }
    app.state::<Scheduler>().clear_current();
}

async fn run_generation(app: &AppHandle, request: &GenerationRequest) -> Result<String, String> {
    let conn = app.state::<DbCell>().get(app).await?.clone();

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

    let inference_arc = app
        .state::<crate::commands::conversations::InferenceState>()
        .0
        .clone();
    {
        let mut guard = inference_arc.lock().await;
        if guard.is_none() {
            let path = std::path::PathBuf::from(&model_path);
            let engine = tokio::task::spawn_blocking(move || InferenceEngine::load(&path, 4))
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_string())?;
            *guard = Some(engine);
        }
    }

    let app_emit = app.clone();
    let conv_id_emit = request.conversation_id.clone();
    let msg_id_emit = request.assistant_message_id.clone();
    let cancel = request.cancel.clone();

    let guard = inference_arc.lock().await;
    let engine = guard.as_ref().expect("engine loaded above");
    let result = engine.generate(
        &request.prompt,
        64,
        |piece| {
            let _ = app_emit.emit(
                "assistant-token",
                AssistantToken {
                    conversation_id: conv_id_emit.clone(),
                    message_id: msg_id_emit.clone(),
                    token: piece.to_string(),
                },
            );
        },
        || cancel.is_cancelled(),
    );
    drop(guard);

    let full_text = result.map_err(|e| e.to_string())?;
    let now = now_ms();
    let duration_ms = now - request.assistant_created_at;

    conn.call({
        let conversation_id = request.conversation_id.clone();
        let assistant_message_id = request.assistant_message_id.clone();
        let assistant_created_at = request.assistant_created_at;
        let full_text = full_text.clone();
        move |conn: &mut Connection| -> rusqlite::Result<()> {
            let seq: i64 = conn.query_row(
                "SELECT COALESCE(MAX(sequence), -1) + 1 FROM messages WHERE conversation_id = ?1",
                [&conversation_id],
                |row| row.get(0),
            )?;
            conn.execute(
                "INSERT INTO messages (id, conversation_id, role, content_type, content, created_at, sequence, duration_ms) VALUES (?1, ?2, 'assistant', 'text', ?3, ?4, ?5, ?6)",
                rusqlite::params![assistant_message_id, conversation_id, full_text, assistant_created_at, seq, duration_ms],
            )?;
            conn.execute(
                "UPDATE conversations SET updated_at = ?1 WHERE id = ?2",
                rusqlite::params![now, conversation_id],
            )?;
            Ok(())
        }
    })
    .await
    .map_err(|e| e.to_string())?;

    let _ = app.emit(
        "assistant-message-complete",
        AssistantMessageComplete {
            conversation_id: request.conversation_id.clone(),
            message_id: request.assistant_message_id.clone(),
            duration_ms,
        },
    );

    Ok(full_text)
}
