use crate::commands::models::now_ms;
use crate::storage::DbCell;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

/// Conversation ids with a turn currently running, each mapped to that
/// turn's `CancellationToken` — the live signal `compute_status` uses for
/// `in_progress` (FR-011) AND the handle `stop_generation` fires to halt a
/// running turn. Populated by `send_agent_message` for the duration of each
/// turn (RAII guard, so every early-return clears the entry — normal
/// completion just drops the token, it does NOT `.cancel()`).
/// One running turn's live state in `ActiveGenerations`: the `CancellationToken`
/// `stop_generation` fires, plus `steers` — a FIFO queue of already-persisted,
/// rich-expanded user turns that arrived mid-turn via `steer_generation` and are
/// drained into the running agent loop at its next step boundary (see
/// `RealBackend::drain_steers` and `run_loop`). Membership of the map already
/// means "a regular, steerable turn is live", so the steer queue rides the same
/// per-turn lifecycle and RAII cleanup as the token.
#[derive(Default)]
pub struct ActiveGeneration {
    pub cancel: tokio_util::sync::CancellationToken,
    pub steers: Vec<String>,
}

#[derive(Default)]
pub struct ActiveGenerations(pub Mutex<HashMap<String, ActiveGeneration>>);

/// Conversation ids currently running a *standalone* `/compact` (the
/// `compact_conversation` command), which holds the single llama-server and is
/// NOT registered in `ActiveGenerations`. `steer_generation` consults this to
/// return `Rejected` (vs `NoActiveTurn`) so the frontend keeps the message
/// queued instead of dispatching it as a doomed new turn. Automatic per-turn
/// compaction inside a regular turn is unaffected — that runs while the
/// conversation IS in `ActiveGenerations`, so steers there queue normally.
#[derive(Default)]
pub struct CompactingConversations(pub Mutex<HashSet<String>>);

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

    crate::commands::agent::emit_conversations_changed(&app);

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
    // `compute_status` only needs the SET of in-progress ids, not their
    // tokens — project the map's keys into the `HashSet<String>` its
    // signature (unchanged) still takes.
    let active = active_generations
        .0
        .lock()
        .unwrap()
        .keys()
        .cloned()
        .collect::<HashSet<_>>();
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
    .map_err(|e| e.to_string())?;
    crate::commands::agent::emit_conversations_changed(&app);
    Ok(())
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
    .map_err(|e| e.to_string())?;
    crate::commands::agent::emit_conversations_changed(&app);
    Ok(())
}

/// Persists the user-set goal for a conversation (`storage::conversations::
/// set_conversation_goal`) — `send_agent_message` loads it back into
/// `Plan.goal` at the start of the conversation's NEXT turn (this command
/// itself does not touch any in-flight turn's already-running `PlanState`).
/// `goal: None` (or an empty string) clears it. The composer's edit/clear
/// affordance calls this directly (the "send as goal" path instead goes
/// through `send_agent_message`'s `set_goal` flag, unidirectional-flow).
/// Emits `ConversationGoalChanged` after persisting — the same event
/// `send_agent_message` emits — so the frontend's goal banner reacts to
/// both write paths identically instead of trusting its own optimistic
/// state.
#[tauri::command]
#[specta::specta]
pub async fn set_conversation_goal(
    app: AppHandle,
    db_cell: State<'_, DbCell>,
    conversation_id: String,
    goal: Option<String>,
) -> Result<(), String> {
    let conn = db_cell.get(&app).await?;
    let goal_conversation_id = conversation_id.clone();
    let goal_for_persist = goal.clone();
    conn.call(move |conn: &mut Connection| -> rusqlite::Result<()> {
        crate::storage::conversations::set_conversation_goal(
            conn,
            &goal_conversation_id,
            goal_for_persist.as_deref(),
        )
    })
    .await
    .map_err(|e| e.to_string())?;
    let _ = app.emit(
        "conversation-goal-changed",
        crate::commands::agent::ConversationGoalChanged {
            conversation_id,
            goal,
        },
    );
    Ok(())
}

/// A conversation's goal plus whether the observer has confirmed it met — the
/// load-path shape the composer's "Pursuing goal" / "Goal achieved" banner
/// reads on mount, so the achieved state survives a reload.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ConversationGoal {
    pub goal: Option<String>,
    pub achieved: bool,
}

/// Reads back the user-set goal for a conversation and whether it has been
/// achieved (`storage::conversations::get_conversation_goal_state`) — used by
/// the goal UI to populate its initial state and recover it across a
/// reload/remount, same as `get_active_plan` does for the plan tracker.
/// `goal: None` means no goal is set (a `NULL` column or a legacy empty string).
#[tauri::command]
#[specta::specta]
pub async fn get_conversation_goal(
    app: AppHandle,
    db_cell: State<'_, DbCell>,
    conversation_id: String,
) -> Result<ConversationGoal, String> {
    let conn = db_cell.get(&app).await?;
    conn.call(
        move |conn: &mut Connection| -> rusqlite::Result<ConversationGoal> {
            let (goal, achieved) =
                crate::storage::conversations::get_conversation_goal_state(conn, &conversation_id)?;
            Ok(ConversationGoal { goal, achieved })
        },
    )
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
        .contains_key(&conversation_id)
}

/// Stops a running agent turn: fires the `CancellationToken` this
/// conversation's in-flight generation is threaded onto, so
/// `LlamaServerClient::chat` returns `InferenceError::Cancelled`, the loop
/// halts with `AgentError::Cancelled`, and `send_agent_message` finalizes
/// the turn quietly (no persisted answer, no error banner). The entry is
/// deliberately left in the map — `send_agent_message`'s
/// `ActiveGenerationGuard` removes it when the turn unwinds — so a token
/// fired here is the SAME one the still-running turn holds. Cancelling an
/// unknown or already-finished id is a harmless no-op (nothing to fire).
#[tauri::command]
#[specta::specta]
pub fn stop_generation(active_generations: State<'_, ActiveGenerations>, conversation_id: String) {
    if let Some(entry) = active_generations.0.lock().unwrap().get(&conversation_id) {
        entry.cancel.cancel();
    }
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
    fn active_generation_holds_a_cancel_token_and_a_steer_queue() {
        // Guards the value-type refactor: `stop_generation` fires
        // `entry.cancel.cancel()`, and a steered message rides the same entry
        // in `steers` without disturbing the token.
        let gens = ActiveGenerations::default();
        let token = tokio_util::sync::CancellationToken::new();
        gens.0.lock().unwrap().insert(
            "c1".to_string(),
            ActiveGeneration {
                cancel: token.clone(),
                steers: vec!["steered!".to_string()],
            },
        );

        // The exact lookup + fire `stop_generation` performs.
        if let Some(entry) = gens.0.lock().unwrap().get("c1") {
            entry.cancel.cancel();
        }
        assert!(token.is_cancelled());

        let guard = gens.0.lock().unwrap();
        let entry = guard
            .get("c1")
            .expect("entry still present until RAII drop");
        assert_eq!(entry.steers, vec!["steered!".to_string()]);
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
    fn stopped_turn_marker_reply_is_done_not_failed() {
        // Generation-cancellation (Task 4.2b): a gracefully-stopped turn
        // persists the stopped marker as an assistant *text* reply (not an
        // `error` row, and not leaving a bare user message with no reply), so
        // a stopped conversation must read as the neutral `done`, never the
        // red `failed`. Assert against the real marker `send_agent_message`'s
        // cancel arm persists, so a marker change can't silently regress this.
        let conn = test_connection();
        insert_conversation(&conn, "c1");
        insert_message(&conn, "c1", 0, "user", "text", "hi", None);
        insert_message(
            &conn,
            "c1",
            1,
            "assistant",
            "text",
            crate::commands::agent::STOPPED_TURN_MARKER,
            None,
        );
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
