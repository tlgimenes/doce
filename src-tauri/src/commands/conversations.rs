use crate::commands::models::now_ms;
use crate::inference::InferenceEngine;
use crate::storage::DbCell;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, State};
use tokio::sync::Mutex as AsyncMutex;
use uuid::Uuid;

pub struct InferenceState(pub Arc<AsyncMutex<Option<InferenceEngine>>>);

impl Default for InferenceState {
    fn default() -> Self {
        Self(Arc::new(AsyncMutex::new(None)))
    }
}

/// Conversation ids with a turn currently running — the live signal
/// `compute_status` uses for `in_progress` (FR-011). Populated by
/// `send_agent_message` for the duration of each turn (RAII guard, so
/// every early-return clears it).
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
    pub last_seen_at: i64,
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
    /// 010-context-window-management (UI refactor): the real tokenizer's
    /// count for this message's own text — input tokens for a user message,
    /// output tokens for an assistant reply — computed once at persistence
    /// time (mirrors `duration_ms`'s frozen-not-live pattern). `None` for
    /// rows persisted before a model was ever loaded, or for content_types
    /// this feature doesn't meter (tool_call/tool_result/error/context_notice).
    pub token_count: Option<i64>,
}

/// FR-011 status computation, in priority order: an active/queued
/// generation always wins (`in_progress`); otherwise the most recent
/// message decides — an `error` content_type is `failed`, an
/// `AskUserQuestion` tool call or a trailing real question mark (not part
/// of a URL) is `requires_action`, anything else is `done`. A user message
/// with no reply and nothing active is `failed` (the turn died before
/// producing either an assistant reply or an active-generation entry).
/// An empty conversation is `done` — nothing pending.
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
                "INSERT INTO conversations (id, workspace_id, title, created_at, updated_at, last_seen_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![id, workspace_id, title, now, now, now],
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
        last_seen_at: now,
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
            "SELECT id, conversation_id, role, content_type, content, tool_name, created_at, duration_ms, token_count
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
                    token_count: row.get(8)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
    .await
    .map_err(|e| e.to_string())
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
            list_conversations_in_conn(conn, workspace_id.as_deref(), &active)
        },
    )
    .await
    .map_err(|e| e.to_string())
}

fn list_conversations_in_conn(
    conn: &Connection,
    workspace_id: Option<&str>,
    active: &HashSet<String>,
) -> rusqlite::Result<Vec<Conversation>> {
    // FR-007: excludes subagent-run conversations from the default result.
    let mut stmt = conn.prepare(
        "SELECT id, workspace_id, title, created_at, updated_at, last_seen_at FROM conversations
         WHERE spawned_by_conversation_id IS NULL
         AND (?1 IS NULL OR workspace_id = ?1)
         AND archived_at IS NULL
         ORDER BY updated_at DESC",
    )?;
    let rows = stmt
        .query_map(rusqlite::params![workspace_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    rows.into_iter()
        .map(
            |(id, workspace_id, title, created_at, updated_at, last_seen_at)| {
                let status = compute_status(conn, &id, active)?;
                Ok(Conversation {
                    id,
                    workspace_id,
                    title,
                    created_at,
                    updated_at,
                    last_seen_at,
                    status,
                })
            },
        )
        .collect()
}

fn mark_conversation_seen_in_conn(
    conn: &Connection,
    conversation_id: &str,
    now: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE conversations
         SET last_seen_at = MAX(?1, updated_at)
         WHERE id = ?2",
        rusqlite::params![now, conversation_id],
    )?;
    Ok(())
}

fn archive_conversation_in_conn(
    conn: &Connection,
    conversation_id: &str,
    now: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE conversations
         SET archived_at = ?1
         WHERE id = ?2",
        rusqlite::params![now, conversation_id],
    )?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn mark_conversation_seen(
    app: AppHandle,
    db_cell: State<'_, DbCell>,
    conversation_id: String,
) -> Result<(), String> {
    let conn = db_cell.get(&app).await?;
    let now = now_ms();
    conn.call(move |conn: &mut Connection| -> rusqlite::Result<()> {
        mark_conversation_seen_in_conn(conn, &conversation_id, now)
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn archive_conversation(
    app: AppHandle,
    db_cell: State<'_, DbCell>,
    conversation_id: String,
) -> Result<(), String> {
    let conn = db_cell.get(&app).await?;
    let now = now_ms();
    conn.call(move |conn: &mut Connection| -> rusqlite::Result<()> {
        archive_conversation_in_conn(conn, &conversation_id, now)
    })
    .await
    .map_err(|e| e.to_string())
}

/// The reload-proof "is a turn genuinely running right now" signal, straight
/// from `ActiveGenerations` (the same source `compute_status`'s
/// `in_progress` uses). The frontend needs this because its own in-flight
/// tracking is in-memory webview state: a reload wipes it, and the
/// transcript alone can't distinguish "model is generating" (latest row is
/// a user message or a paired tool_result) from "turn finished" — exactly
/// the window that let a duplicate message through in production.
#[tauri::command]
#[specta::specta]
pub fn is_generation_active(
    active_generations: State<'_, ActiveGenerations>,
    conversation_id: String,
) -> bool {
    active_generations
        .0
        .lock()
        .unwrap()
        .contains(&conversation_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::test_connection;

    fn insert_conversation(conn: &Connection, id: &str) {
        conn.execute(
            "INSERT INTO conversations (id, title, created_at, updated_at, last_seen_at) VALUES (?1, 'x', 0, 0, 0)",
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
    fn mark_conversation_seen_in_conn_sets_marker_to_at_least_updated_at() {
        let conn = test_connection();
        conn.execute(
            "INSERT INTO conversations (id, title, created_at, updated_at, last_seen_at) VALUES ('c1', 'x', 1, 100, 2)",
            [],
        )
        .unwrap();

        mark_conversation_seen_in_conn(&conn, "c1", 50).unwrap();

        let last_seen_at: i64 = conn
            .query_row(
                "SELECT last_seen_at FROM conversations WHERE id = 'c1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(last_seen_at, 100);
    }

    #[test]
    fn mark_conversation_seen_in_conn_uses_now_when_it_is_newer_than_updated_at() {
        let conn = test_connection();
        conn.execute(
            "INSERT INTO conversations (id, title, created_at, updated_at, last_seen_at) VALUES ('c1', 'x', 1, 100, 2)",
            [],
        )
        .unwrap();

        mark_conversation_seen_in_conn(&conn, "c1", 150).unwrap();

        let last_seen_at: i64 = conn
            .query_row(
                "SELECT last_seen_at FROM conversations WHERE id = 'c1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(last_seen_at, 150);
    }

    #[test]
    fn archive_conversation_in_conn_sets_archived_at() {
        let conn = test_connection();
        insert_conversation(&conn, "c1");

        archive_conversation_in_conn(&conn, "c1", 123).unwrap();

        let archived_at: Option<i64> = conn
            .query_row(
                "SELECT archived_at FROM conversations WHERE id = 'c1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(archived_at, Some(123));
    }

    #[test]
    fn list_conversations_in_conn_skips_archived_rows() {
        let conn = test_connection();
        insert_conversation(&conn, "visible");
        insert_conversation(&conn, "archived");
        archive_conversation_in_conn(&conn, "archived", 123).unwrap();

        let conversations = list_conversations_in_conn(&conn, None, &HashSet::new()).unwrap();

        assert_eq!(conversations.len(), 1);
        assert_eq!(conversations[0].id, "visible");
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
}
