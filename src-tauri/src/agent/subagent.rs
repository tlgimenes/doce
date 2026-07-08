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

    conn.execute(
        "INSERT INTO messages (id, conversation_id, role, content_type, content, created_at, sequence) VALUES (?1, ?2, 'user', 'text', ?3, ?4, 0)",
        rusqlite::params![Uuid::now_v7().to_string(), subagent_id, task_prompt, now],
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
}
