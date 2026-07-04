use crate::agent::rich_content::{expand_segments, RichMessageContent};
use crate::commands::models::now_ms;
use crate::inference::{ChatMessage, InferenceEngine};
use crate::storage::conversations::{generate_title, load_history};
use crate::storage::DbCell;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::Mutex as AsyncMutex;
use uuid::Uuid;

/// System-role message for plain chat mode (User Story 2) — sets basic
/// identity/behavior so the model isn't generating with an empty system
/// turn. Agent mode uses its own, tool-focused system prompt instead
/// (`agent::SYSTEM_PROMPT`).
const CHAT_SYSTEM_PROMPT: &str =
    "You are Doce, a helpful AI assistant running entirely locally on the user's device.";

pub struct InferenceState(pub Arc<AsyncMutex<Option<InferenceEngine>>>);

impl Default for InferenceState {
    fn default() -> Self {
        Self(Arc::new(AsyncMutex::new(None)))
    }
}

/// Conversation ids with a generation currently active or queued — the
/// live signal `compute_status` uses for `in_progress` (FR-011). Populated
/// directly by `send_message` for now; once the real scheduler (US5) is
/// wired in as the sole path to inference, this can read the scheduler's
/// queue instead of being maintained separately.
#[derive(Default)]
pub struct ActiveGenerations(pub Mutex<HashSet<String>>);

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub id: String,
    pub workspace_id: Option<String>,
    pub title: String,
    pub created_at: i64,
    pub updated_at: i64,
    /// Computed live, never cached (FR-011): `in_progress` | `requires_action`
    /// | `failed` | `done`.
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub conversation_id: String,
    pub role: String,
    pub content_type: String,
    pub content: String,
    pub tool_name: Option<String>,
    pub created_at: i64,
    /// Set only once an assistant message finishes generating — frozen at
    /// `persisted_at - created_at` so the chat UI's elapsed-time badge
    /// survives a reload instead of resetting to zero.
    pub duration_ms: Option<i64>,
}

/// FR-011 status computation, in priority order: an active/queued
/// generation always wins (`in_progress`); otherwise the most recent
/// message decides — an `error` content_type is `failed`, an
/// `AskUserQuestion` tool call or a trailing real question mark (not part
/// of a URL) is `requires_action`, anything else is `done`. A user message
/// with no reply and nothing active is `failed` (send_message failed
/// before producing either an assistant reply or an active-generation
/// entry). An empty conversation is `done` — nothing pending.
fn compute_status(
    conn: &Connection,
    conversation_id: &str,
    active: &HashSet<String>,
) -> rusqlite::Result<String> {
    if active.contains(conversation_id) {
        return Ok("in_progress".to_string());
    }

    let latest: Option<(String, String, Option<String>, String)> = conn
        .query_row(
            "SELECT role, content_type, tool_name, content FROM messages
             WHERE conversation_id = ?1 ORDER BY sequence DESC LIMIT 1",
            [conversation_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .ok();

    let Some((role, content_type, tool_name, content)) = latest else {
        return Ok("done".to_string());
    };

    if role != "assistant" {
        // A user message with nothing generating and no reply yet.
        return Ok("failed".to_string());
    }
    if content_type == "error" {
        return Ok("failed".to_string());
    }
    if tool_name.as_deref() == Some("AskUserQuestion") {
        return Ok("requires_action".to_string());
    }
    if ends_with_real_question(&content) {
        return Ok("requires_action".to_string());
    }
    Ok("done".to_string())
}

/// A trailing `?` counts as "requires action" unless it's part of a URL
/// (e.g. "...see https://example.com?" should not read as a question).
fn ends_with_real_question(text: &str) -> bool {
    let trimmed = text.trim_end();
    if !trimmed.ends_with('?') {
        return false;
    }
    let last_word = trimmed.split_whitespace().next_back().unwrap_or("");
    !(last_word.starts_with("http://") || last_word.starts_with("https://"))
}

#[tauri::command]
#[specta::specta]
pub async fn create_conversation(
    app: AppHandle,
    db_cell: State<'_, DbCell>,
    workspace_id: Option<String>,
) -> Result<Conversation, String> {
    let conn = db_cell.get(&app).await?;
    let id = Uuid::now_v7().to_string();
    let now = now_ms();
    let title = "New conversation".to_string();

    conn.call({
        let id = id.clone();
        let workspace_id = workspace_id.clone();
        let title = title.clone();
        move |conn: &mut Connection| -> rusqlite::Result<()> {
            conn.execute(
                "INSERT INTO conversations (id, workspace_id, title, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![id, workspace_id, title, now, now],
            )?;
            Ok(())
        }
    })
    .await
    .map_err(|e| e.to_string())?;

    Ok(Conversation {
        id,
        workspace_id,
        title,
        created_at: now,
        updated_at: now,
        status: "done".to_string(),
    })
}

#[tauri::command]
#[specta::specta]
pub async fn list_messages(
    app: AppHandle,
    db_cell: State<'_, DbCell>,
    conversation_id: String,
) -> Result<Vec<Message>, String> {
    let conn = db_cell.get(&app).await?;
    conn.call(move |conn: &mut Connection| -> rusqlite::Result<Vec<Message>> {
        let mut stmt = conn.prepare(
            "SELECT id, conversation_id, role, content_type, content, tool_name, created_at, duration_ms
             FROM messages WHERE conversation_id = ?1 ORDER BY sequence ASC",
        )?;
        let rows = stmt
            .query_map([&conversation_id], |row| {
                Ok(Message {
                    id: row.get(0)?,
                    conversation_id: row.get(1)?,
                    role: row.get(2)?,
                    content_type: row.get(3)?,
                    content: row.get(4)?,
                    tool_name: row.get(5)?,
                    created_at: row.get(6)?,
                    duration_ms: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
    .await
    .map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Serialize, specta::Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct AssistantToken {
    pub conversation_id: String,
    pub message_id: String,
    pub token: String,
}

#[derive(Debug, Clone, Serialize, specta::Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct AssistantMessageComplete {
    pub conversation_id: String,
    pub message_id: String,
    pub duration_ms: i64,
}

/// Emitted when the worker fails a request after it's already been
/// dequeued (e.g. inference error, or the model was uninstalled between
/// submission and pickup) — without this, a mid-flight failure left the
/// chat UI stuck on "Generating…" forever with no feedback, the same class
/// of silent-hang bug the download-progress path had (downloader/mod.rs).
#[derive(Debug, Clone, Serialize, specta::Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct AssistantMessageError {
    pub conversation_id: String,
    pub message_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageResult {
    pub message_id: String,
    pub request_id: String,
    /// Id and creation timestamp of the assistant reply about to be
    /// generated — handed back synchronously so the UI can render its
    /// queued/generating placeholder (and start a refresh-safe elapsed
    /// timer) immediately, without waiting for the first streamed token.
    pub assistant_message_id: String,
    pub assistant_created_at: i64,
}

/// Resolves what `send_message` persists for the user message row —
/// `(content_type, content)` — and what text drives this turn's FR-012
/// title generation, from the `rich_content` IPC parameter (contracts/
/// rich-chat-input.md's `send_message` entry). Pure/sync and takes
/// `skills_dir` directly rather than an `AppHandle` so it stays
/// unit-testable without a running Tauri app, matching this codebase's
/// existing convention for `AppHandle`-adjacent logic (e.g.
/// `commands::workspaces`'s `resolve_search_scope`).
///
/// `None` reproduces today's exact behavior: `content_type = "text"`,
/// `content` persisted and used for the title verbatim, no JSON parsing.
/// `Some(json)` persists `content_type = "rich_text"` with `content=json`
/// verbatim, and derives the title from `expand_segments(...,
/// expand_skills: false)`'s output — the literal `/name` marker form, not
/// the raw JSON and not a fully-expanded skill injection, either of which
/// would make for a nonsensical auto-generated title (data-model.md's
/// Model-Text Expansion section). Returns `Err` for invalid JSON or (via
/// `expand_segments`) an unreadable `skill` segment (FR-014) — either way
/// the caller must not persist a broken row.
fn resolve_message_content(
    content: &str,
    rich_content: Option<&str>,
    skills_dir: &Path,
) -> Result<(String, String, String), String> {
    match rich_content {
        None => Ok(("text".to_string(), content.to_string(), content.to_string())),
        Some(json) => {
            let parsed: RichMessageContent =
                serde_json::from_str(json).map_err(|e| format!("invalid rich_content: {e}"))?;
            let title_source = expand_segments(&parsed.segments, skills_dir, false)?;
            Ok(("rich_text".to_string(), json.to_string(), title_source))
        }
    }
}

/// FR-006/FR-009/US5: submits a `GenerationRequest` to the single-flight
/// scheduler and returns immediately — the scheduler's worker
/// (`scheduler::worker`) is what actually runs inference, respects
/// focus-based priority, and can be cancelled mid-flight. Does not itself
/// await the result: the frontend gets progress via `assistant-token` /
/// `assistant-message-complete` / `assistant-message-error` events.
#[tauri::command]
#[specta::specta]
pub async fn send_message(
    app: AppHandle,
    db_cell: State<'_, DbCell>,
    active_generations: State<'_, ActiveGenerations>,
    scheduler: State<'_, crate::scheduler::Scheduler>,
    conversation_id: String,
    content: String,
    rich_content: Option<String>,
) -> Result<SendMessageResult, String> {
    let conn = db_cell.get(&app).await?.clone();
    let user_message_id = Uuid::now_v7().to_string();
    let request_id = Uuid::now_v7().to_string();
    let now = now_ms();

    // Resolved once and reused below for both this turn's own
    // `rich_content` (if any) and `load_history`'s expansion of any
    // `rich_text` rows from *earlier* turns in this conversation — the
    // same convention `commands::skills::list_skills` already uses
    // (`app.path().app_data_dir()?.join("skills")`).
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

    // `rich_content` is `None` for the common, plain-text-only case — that
    // path persists `content_type='text'`/`content=content` and titles off
    // `content` verbatim, exactly as it always has. `Some(json)` persists
    // `content_type='rich_text'` with `content=json` instead, and derives
    // the title from the segments' `expand_skills: false` text rather than
    // the raw JSON (contracts/rich-chat-input.md's `send_message` entry).
    let (content_type, persisted_content, title_source) =
        resolve_message_content(&content, rich_content.as_deref(), &skills_dir)?;

    // FR-012: title comes from the first message only, no model call.
    let title_update = if next_seq == 0 {
        Some(generate_title(&title_source))
    } else {
        None
    };

    conn.call({
        let conversation_id = conversation_id.clone();
        let user_message_id = user_message_id.clone();
        move |conn: &mut Connection| -> rusqlite::Result<()> {
            let tx = conn.transaction()?;
            tx.execute(
                "INSERT INTO messages (id, conversation_id, role, content_type, content, created_at, sequence) VALUES (?1, ?2, 'user', ?3, ?4, ?5, ?6)",
                rusqlite::params![user_message_id, conversation_id, content_type, persisted_content, now, next_seq],
            )?;
            if let Some(title) = &title_update {
                tx.execute(
                    "UPDATE conversations SET title = ?1 WHERE id = ?2",
                    rusqlite::params![title, conversation_id],
                )?;
            }
            tx.commit()?;
            Ok(())
        }
    })
    .await
    .map_err(|e| e.to_string())?;

    // Fail fast rather than queueing work that can never succeed — the
    // worker double-checks this too (the model could be uninstalled
    // between now and pickup), surfacing that rarer case via
    // `assistant-message-error` since nothing awaits this function's
    // result once it returns.
    let has_model: bool = conn
        .call(|conn: &mut Connection| -> rusqlite::Result<i64> {
            conn.query_row(
                "SELECT COUNT(*) FROM models WHERE is_active = 1",
                [],
                |row| row.get(0),
            )
        })
        .await
        .map(|n| n > 0)
        .unwrap_or(false);
    if !has_model {
        return Err("no active model installed".to_string());
    }

    let assistant_message_id = Uuid::now_v7().to_string();
    let assistant_created_at = now_ms();

    // Full history (including the user message just inserted above) so the
    // model sees prior turns instead of generating this reply in isolation.
    // `load_history` expands any `rich_text` row it finds (this turn's or
    // an earlier one) via `skills_dir`, resolved once above.
    let history = conn
        .call({
            let conversation_id = conversation_id.clone();
            let skills_dir = skills_dir.clone();
            move |conn: &mut Connection| load_history(conn, &conversation_id, &skills_dir)
        })
        .await
        .map_err(|e| e.to_string())?;
    let mut messages = vec![ChatMessage::system(CHAT_SYSTEM_PROMPT)];
    messages.extend(history);

    active_generations
        .0
        .lock()
        .unwrap()
        .insert(conversation_id.clone());

    let (result_tx, _result_rx) = tokio::sync::oneshot::channel();
    scheduler.submit(crate::scheduler::GenerationRequest {
        request_id: request_id.clone(),
        conversation_id: conversation_id.clone(),
        priority_conversation_id: conversation_id.clone(),
        messages,
        assistant_message_id: assistant_message_id.clone(),
        assistant_created_at,
        cancel: tokio_util::sync::CancellationToken::new(),
        result_tx,
    });

    let _ = app.emit(
        "generation-queue-update",
        crate::scheduler::GenerationQueueUpdate {
            request_id: request_id.clone(),
            conversation_id: conversation_id.clone(),
            state: "queued".to_string(),
            position: scheduler
                .queue_positions()
                .into_iter()
                .find(|(id, _)| *id == request_id)
                .map(|(_, pos)| pos),
        },
    );

    Ok(SendMessageResult {
        message_id: user_message_id,
        request_id,
        assistant_message_id,
        assistant_created_at,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn list_conversations(
    app: AppHandle,
    db_cell: State<'_, DbCell>,
    active_generations: State<'_, ActiveGenerations>,
    workspace_id: Option<String>,
) -> Result<Vec<Conversation>, String> {
    let conn = db_cell.get(&app).await?;
    let active = active_generations.0.lock().unwrap().clone();
    conn.call(
        move |conn: &mut Connection| -> rusqlite::Result<Vec<Conversation>> {
            // FR-007: excludes subagent-run conversations from the default result.
            let mut stmt = conn.prepare(
                "SELECT id, workspace_id, title, created_at, updated_at FROM conversations
             WHERE spawned_by_conversation_id IS NULL
             AND (?1 IS NULL OR workspace_id = ?1)
             ORDER BY updated_at DESC",
            )?;
            let rows = stmt
                .query_map([&workspace_id], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;

            rows.into_iter()
                .map(|(id, workspace_id, title, created_at, updated_at)| {
                    let status = compute_status(conn, &id, &active)?;
                    Ok(Conversation {
                        id,
                        workspace_id,
                        title,
                        created_at,
                        updated_at,
                        status,
                    })
                })
                .collect()
        },
    )
    .await
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::test_connection;

    fn insert_conversation(conn: &Connection, id: &str) {
        conn.execute(
            "INSERT INTO conversations (id, title, created_at, updated_at) VALUES (?1, 'x', 0, 0)",
            [id],
        )
        .unwrap();
    }

    fn insert_message(
        conn: &Connection,
        conv_id: &str,
        seq: i64,
        role: &str,
        content_type: &str,
        content: &str,
        tool_name: Option<&str>,
    ) {
        conn.execute(
            "INSERT INTO messages (id, conversation_id, role, content_type, content, tool_name, created_at, sequence) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7)",
            rusqlite::params![uuid::Uuid::now_v7().to_string(), conv_id, role, content_type, content, tool_name, seq],
        )
        .unwrap();
    }

    #[test]
    fn empty_conversation_is_done() {
        let conn = test_connection();
        insert_conversation(&conn, "c1");
        assert_eq!(
            compute_status(&conn, "c1", &HashSet::new()).unwrap(),
            "done"
        );
    }

    #[test]
    fn active_generation_wins_over_everything() {
        let conn = test_connection();
        insert_conversation(&conn, "c1");
        insert_message(&conn, "c1", 0, "assistant", "error", "boom", None);
        let mut active = HashSet::new();
        active.insert("c1".to_string());
        assert_eq!(compute_status(&conn, "c1", &active).unwrap(), "in_progress");
    }

    #[test]
    fn error_content_type_is_failed() {
        let conn = test_connection();
        insert_conversation(&conn, "c1");
        insert_message(&conn, "c1", 0, "user", "text", "hi", None);
        insert_message(&conn, "c1", 1, "assistant", "error", "boom", None);
        assert_eq!(
            compute_status(&conn, "c1", &HashSet::new()).unwrap(),
            "failed"
        );
    }

    #[test]
    fn ask_user_question_tool_call_requires_action() {
        let conn = test_connection();
        insert_conversation(&conn, "c1");
        insert_message(
            &conn,
            "c1",
            0,
            "assistant",
            "tool_call",
            "{}",
            Some("AskUserQuestion"),
        );
        assert_eq!(
            compute_status(&conn, "c1", &HashSet::new()).unwrap(),
            "requires_action"
        );
    }

    #[test]
    fn trailing_question_mark_requires_action() {
        let conn = test_connection();
        insert_conversation(&conn, "c1");
        insert_message(
            &conn,
            "c1",
            0,
            "assistant",
            "text",
            "Should I proceed?",
            None,
        );
        assert_eq!(
            compute_status(&conn, "c1", &HashSet::new()).unwrap(),
            "requires_action"
        );
    }

    #[test]
    fn question_mark_inside_url_is_not_requires_action() {
        let conn = test_connection();
        insert_conversation(&conn, "c1");
        insert_message(
            &conn,
            "c1",
            0,
            "assistant",
            "text",
            "see https://example.com?q=1",
            None,
        );
        assert_eq!(
            compute_status(&conn, "c1", &HashSet::new()).unwrap(),
            "done"
        );
    }

    #[test]
    fn trailing_bare_url_ending_in_question_mark_is_not_requires_action() {
        let conn = test_connection();
        insert_conversation(&conn, "c1");
        insert_message(
            &conn,
            "c1",
            0,
            "assistant",
            "text",
            "check this out https://example.com?",
            None,
        );
        assert_eq!(
            compute_status(&conn, "c1", &HashSet::new()).unwrap(),
            "done"
        );
    }

    #[test]
    fn normal_completion_is_done() {
        let conn = test_connection();
        insert_conversation(&conn, "c1");
        insert_message(&conn, "c1", 0, "user", "text", "hi", None);
        insert_message(&conn, "c1", 1, "assistant", "text", "Hello there.", None);
        assert_eq!(
            compute_status(&conn, "c1", &HashSet::new()).unwrap(),
            "done"
        );
    }

    #[test]
    fn unanswered_user_message_with_nothing_active_is_failed() {
        let conn = test_connection();
        insert_conversation(&conn, "c1");
        insert_message(&conn, "c1", 0, "user", "text", "hi", None);
        assert_eq!(
            compute_status(&conn, "c1", &HashSet::new()).unwrap(),
            "failed"
        );
    }

    // --- 009-rich-chat-input/US2: resolve_message_content ---

    #[test]
    fn none_rich_content_persists_as_plain_text_and_titles_off_the_raw_content() {
        let (content_type, persisted_content, title_source) =
            resolve_message_content("hello there", None, Path::new("/does/not/matter")).unwrap();

        assert_eq!(content_type, "text");
        assert_eq!(persisted_content, "hello there");
        assert_eq!(title_source, "hello there");
    }

    #[test]
    fn some_rich_content_persists_the_raw_json_verbatim_as_rich_text() {
        let json = serde_json::json!({
            "segments": [{"type": "text", "text": "hello there"}]
        })
        .to_string();

        let (content_type, persisted_content, _title_source) =
            resolve_message_content("hello there", Some(&json), Path::new("/does/not/matter"))
                .unwrap();

        assert_eq!(content_type, "rich_text");
        // Persisted verbatim -- not re-serialized, not the flat `content`
        // param (contracts/rich-chat-input.md).
        assert_eq!(persisted_content, json);
    }

    #[test]
    fn some_rich_content_titles_off_expand_segments_with_expand_skills_false() {
        // A skill segment must render as its literal `/name` marker for the
        // title, never the injected file content -- and must not touch the
        // filesystem doing so (no skill file exists at this skills_dir).
        let json = serde_json::json!({
            "segments": [
                {"type": "text", "text": "please use "},
                {"type": "skill", "id": "s1", "name": "reviewer"},
            ]
        })
        .to_string();

        let (_content_type, _persisted_content, title_source) =
            resolve_message_content("ignored", Some(&json), Path::new("/does/not/exist")).unwrap();

        assert_eq!(title_source, "please use /reviewer");
    }

    #[test]
    fn some_rich_content_with_invalid_json_is_an_error() {
        let result =
            resolve_message_content("ignored", Some("not json"), Path::new("/does/not/matter"));
        assert!(result.is_err());
    }

    #[test]
    fn some_rich_content_with_a_skill_that_does_not_exist_still_titles_fine_since_titles_never_read_the_filesystem(
    ) {
        // Title generation always uses `expand_skills: false`, which never
        // touches disk (rich_content.rs) -- so an unresolvable skill name
        // must not fail title generation, even though it would fail the
        // model-facing `expand_skills: true` expansion `load_history`/
        // `send_agent_message` perform separately.
        let dir = tempfile::tempdir().unwrap();
        let json = serde_json::json!({
            "segments": [{"type": "skill", "id": "s1", "name": "missing-skill"}]
        })
        .to_string();

        let (content_type, _persisted_content, title_source) =
            resolve_message_content("ignored", Some(&json), dir.path()).unwrap();

        assert_eq!(content_type, "rich_text");
        assert_eq!(title_source, "/missing-skill");
    }

    #[test]
    fn some_rich_content_with_empty_json_object_is_an_error() {
        // Missing the required `segments` field entirely -- must not be
        // silently treated as zero segments.
        let result = resolve_message_content("ignored", Some("{}"), Path::new("/does/not/matter"));
        assert!(result.is_err());
    }
}
