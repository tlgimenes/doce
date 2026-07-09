use crate::commands::models::now_ms;
use rusqlite::Connection;
use uuid::Uuid;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum SubagentError {
    #[error("subagents cannot spawn further subagents (one-level nesting limit, FR-016)")]
    NestingLimitExceeded,
    #[error("parent conversation not found")]
    ParentNotFound,
    #[error("database error: {0}")]
    Db(String),
}

/// FR-015/FR-016: creates a fresh, isolated `Conversation` for a subagent
/// run — no parent history copied in (context isolation), tagged via
/// `spawned_by_conversation_id` so it's excluded from the main
/// conversation list and FTS5 search (FR-007/FR-030), and rejected
/// outright if the *parent* is itself a subagent (defense in depth
/// alongside the loop-level `Task`-call interception in `agent::run_loop`
/// — this check holds even if something calls this function directly,
/// bypassing the loop).
pub fn spawn_subagent(
    conn: &Connection,
    parent_conversation_id: &str,
    task_prompt: &str,
) -> Result<String, SubagentError> {
    let parent_spawned_by: Option<String> = conn
        .query_row(
            "SELECT spawned_by_conversation_id FROM conversations WHERE id = ?1",
            [parent_conversation_id],
            |row| row.get(0),
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => SubagentError::ParentNotFound,
            other => SubagentError::Db(other.to_string()),
        })?;

    if parent_spawned_by.is_some() {
        return Err(SubagentError::NestingLimitExceeded);
    }

    let subagent_id = Uuid::now_v7().to_string();
    let now = now_ms();

    conn.execute(
        "INSERT INTO conversations (id, spawned_by_conversation_id, title, created_at, updated_at, last_seen_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![subagent_id, parent_conversation_id, "(subagent)", now, now, now],
    )
    .map_err(|e| SubagentError::Db(e.to_string()))?;

    // `transcript_dir: None` -- this synchronous, `&Connection`-only
    // function has no `AppHandle` to resolve one from. The subagent
    // conversation is never re-opened through a user-facing entry point
    // (those are the only places `heal_if_stale` is normally wired), so the
    // spawn site itself (`commands::agent::execute_top_level_tool`, right
    // after this call succeeds) heals this transcript once, immediately;
    // every append after that keeps it complete.
    crate::storage::messages::insert(
        conn,
        None,
        &crate::storage::messages::NewMessage {
            conversation_id: &subagent_id,
            role: "user",
            content_type: "text",
            content: task_prompt,
            tool_name: None,
            tool_call_id: None,
            model_text: None,
            created_at: now,
            duration_ms: None,
            token_count: None,
        },
    )
    .map_err(|e| SubagentError::Db(e.to_string()))?;

    Ok(subagent_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::test_connection;

    fn insert_conversation(conn: &Connection, id: &str, spawned_by: Option<&str>) {
        conn.execute(
            "INSERT INTO conversations (id, title, created_at, updated_at, last_seen_at, spawned_by_conversation_id) VALUES (?1, 'x', 0, 0, 0, ?2)",
            rusqlite::params![id, spawned_by],
        )
        .unwrap();
    }

    #[test]
    fn spawns_an_isolated_conversation_tagged_with_the_parent() {
        let conn = test_connection();
        insert_conversation(&conn, "parent", None);

        let subagent_id = spawn_subagent(&conn, "parent", "go research X").unwrap();

        let spawned_by: Option<String> = conn
            .query_row(
                "SELECT spawned_by_conversation_id FROM conversations WHERE id = ?1",
                [&subagent_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(spawned_by.as_deref(), Some("parent"));
    }

    #[test]
    fn subagent_gets_a_fresh_context_not_the_parents_history() {
        let conn = test_connection();
        insert_conversation(&conn, "parent", None);
        conn.execute(
            "INSERT INTO messages (id, conversation_id, role, content_type, content, created_at, sequence) VALUES ('m1', 'parent', 'user', 'text', 'parent secret history', 0, 0)",
            [],
        )
        .unwrap();

        let subagent_id = spawn_subagent(&conn, "parent", "the actual task").unwrap();

        let messages: Vec<String> = conn
            .prepare("SELECT content FROM messages WHERE conversation_id = ?1 ORDER BY sequence")
            .unwrap()
            .query_map([&subagent_id], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();

        assert_eq!(messages, vec!["the actual task".to_string()]);
        assert!(!messages.iter().any(|m| m.contains("parent secret history")));
    }

    #[test]
    fn a_subagent_cannot_itself_spawn_a_subagent() {
        let conn = test_connection();
        insert_conversation(&conn, "parent", None);
        insert_conversation(&conn, "sub", Some("parent"));

        let err = spawn_subagent(&conn, "sub", "nested task").unwrap_err();
        assert_eq!(err, SubagentError::NestingLimitExceeded);
    }

    #[test]
    fn spawning_from_a_nonexistent_parent_is_a_clear_error() {
        let conn = test_connection();
        let err = spawn_subagent(&conn, "does-not-exist", "task").unwrap_err();
        assert_eq!(err, SubagentError::ParentNotFound);
    }

    #[test]
    fn healing_right_after_spawn_gives_the_subagent_transcript_its_entry_zero() {
        // `spawn_subagent` seeds the subagent's task-prompt row with
        // `transcript_dir: None` (see the comment above, in
        // `spawn_subagent` itself) -- no transcript file is written at
        // spawn time. `commands::agent::execute_top_level_tool` heals this
        // transcript once, right after a successful spawn; this proves that
        // heal actually produces entry #0 from exactly the row shape
        // `spawn_subagent` leaves behind, closing the hole a stale doc
        // comment used to paper over (the subagent conversation is never
        // re-opened through a user-facing entry point, so nothing else
        // would ever heal it).
        let conn = test_connection();
        insert_conversation(&conn, "parent", None);
        let subagent_id = spawn_subagent(&conn, "parent", "go research X").unwrap();

        let dir = tempfile::tempdir().unwrap();
        crate::context::transcript::heal_if_stale(&conn, dir.path(), &subagent_id).unwrap();

        let transcript = std::fs::read_to_string(crate::context::transcript::transcript_path(
            dir.path(),
            &subagent_id,
        ))
        .unwrap();
        assert!(
            transcript.contains("[#0 user]"),
            "healing must regenerate the missing entry #0, got: {transcript:?}"
        );
        assert!(
            transcript.contains("go research X"),
            "entry #0's body must be the task prompt, got: {transcript:?}"
        );
    }
}
