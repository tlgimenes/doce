//! SP4: durable per-workspace agent memory. Rows are a faithful projection of
//! the last extraction pass -- `replace_memories` swaps a workspace's whole set
//! in one transaction rather than upserting row-by-row, because the extraction
//! model emits the full desired set (add/update/drop happens in its reasoning,
//! not here). `created_at` survives a re-extraction that keeps a fact verbatim,
//! so a memory's age means what it says.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Memory {
    pub id: String,
    pub content: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// `workspace_id IS ?1` (not `=`): a conversation with no workspace owns the
/// NULL bucket, and `= NULL` would match nothing.
pub async fn load_memories(
    conn: &tokio_rusqlite::Connection,
    workspace_id: Option<&str>,
) -> Result<Vec<Memory>, String> {
    let workspace_id = workspace_id.map(|s| s.to_string());
    conn.call(
        move |conn: &mut Connection| -> rusqlite::Result<Vec<Memory>> {
            let mut stmt = conn.prepare(
                "SELECT id, content, created_at, updated_at FROM memories \
             WHERE workspace_id IS ?1 ORDER BY updated_at DESC, id",
            )?;
            let rows = stmt
                .query_map([&workspace_id], |row| {
                    Ok(Memory {
                        id: row.get(0)?,
                        content: row.get(1)?,
                        created_at: row.get(2)?,
                        updated_at: row.get(3)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        },
    )
    .await
    .map_err(|e| e.to_string())
}

/// Swaps the workspace's whole memory set in ONE transaction. Content strings
/// that already existed keep their original `created_at`; everything gets the
/// new `updated_at`.
pub async fn replace_memories(
    conn: &tokio_rusqlite::Connection,
    workspace_id: Option<&str>,
    contents: &[String],
    now: i64,
) -> Result<(), String> {
    let workspace_id = workspace_id.map(|s| s.to_string());
    let contents = contents.to_vec();
    conn.call(move |conn: &mut Connection| -> rusqlite::Result<()> {
        let tx = conn.transaction()?;
        // Remember prior created_at per content so an unchanged fact keeps its age.
        let prior: std::collections::HashMap<String, i64> = {
            let mut stmt =
                tx.prepare("SELECT content, created_at FROM memories WHERE workspace_id IS ?1")?;
            let rows = stmt
                .query_map([&workspace_id], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows.into_iter().collect()
        };
        tx.execute(
            "DELETE FROM memories WHERE workspace_id IS ?1",
            [&workspace_id],
        )?;
        for content in &contents {
            let created_at = prior.get(content).copied().unwrap_or(now);
            tx.execute(
                "INSERT INTO memories (id, workspace_id, content, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    Uuid::now_v7().to_string(),
                    &workspace_id,
                    content,
                    created_at,
                    now
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())
}

pub async fn workspace_id_for_conversation(
    conn: &tokio_rusqlite::Connection,
    conversation_id: &str,
) -> Result<Option<String>, String> {
    let conversation_id = conversation_id.to_string();
    conn.call(move |conn: &mut Connection| {
        conn.query_row(
            "SELECT workspace_id FROM conversations WHERE id = ?1",
            [&conversation_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })
    })
    .await
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::test_async_connection;

    async fn seed_workspace(conn: &tokio_rusqlite::Connection, id: &str) {
        let id = id.to_string();
        conn.call(move |conn: &mut Connection| {
            conn.execute(
                "INSERT INTO workspaces (id, path, display_name, created_at, last_opened_at) \
                 VALUES (?1, ?1, 'Test workspace', 0, 0)",
                [&id],
            )
        })
        .await
        .unwrap();
    }

    async fn seed_conversation(
        conn: &tokio_rusqlite::Connection,
        id: &str,
        workspace_id: Option<&str>,
    ) {
        let id = id.to_string();
        let workspace_id = workspace_id.map(|s| s.to_string());
        conn.call(move |conn: &mut Connection| {
            conn.execute(
                "INSERT INTO conversations (id, workspace_id, spawned_by_conversation_id, title, created_at, updated_at) \
                 VALUES (?1, ?2, NULL, 'Test', 0, 0)",
                rusqlite::params![&id, &workspace_id],
            )
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn replace_then_load_roundtrips_in_order() {
        let conn = test_async_connection().await;
        seed_workspace(&conn, "w1").await;

        replace_memories(&conn, Some("w1"), &["a".to_string(), "b".to_string()], 10)
            .await
            .unwrap();

        let loaded = load_memories(&conn, Some("w1")).await.unwrap();
        assert_eq!(loaded.len(), 2);
        let contents: Vec<&str> = loaded.iter().map(|m| m.content.as_str()).collect();
        assert!(contents.contains(&"a"));
        assert!(contents.contains(&"b"));
    }

    #[tokio::test]
    async fn replace_preserves_created_at_for_unchanged_content() {
        let conn = test_async_connection().await;
        seed_workspace(&conn, "w1").await;

        replace_memories(&conn, Some("w1"), &["keep".to_string()], 10)
            .await
            .unwrap();
        replace_memories(
            &conn,
            Some("w1"),
            &["keep".to_string(), "new".to_string()],
            20,
        )
        .await
        .unwrap();

        let loaded = load_memories(&conn, Some("w1")).await.unwrap();
        let keep = loaded.iter().find(|m| m.content == "keep").unwrap();
        let new = loaded.iter().find(|m| m.content == "new").unwrap();
        assert_eq!(
            keep.created_at, 10,
            "unchanged content keeps its created_at"
        );
        assert_eq!(new.created_at, 20, "new content gets now as created_at");
        assert_eq!(keep.updated_at, 20);
        assert_eq!(new.updated_at, 20);
    }

    #[tokio::test]
    async fn null_workspace_is_isolated_from_a_real_workspace() {
        let conn = test_async_connection().await;
        seed_workspace(&conn, "w1").await;

        replace_memories(&conn, Some("w1"), &["ws".to_string()], 10)
            .await
            .unwrap();
        replace_memories(&conn, None, &["nullbucket".to_string()], 10)
            .await
            .unwrap();

        let null_loaded = load_memories(&conn, None).await.unwrap();
        assert_eq!(null_loaded.len(), 1);
        assert_eq!(null_loaded[0].content, "nullbucket");

        let w1_loaded = load_memories(&conn, Some("w1")).await.unwrap();
        assert_eq!(w1_loaded.len(), 1);
        assert_eq!(w1_loaded[0].content, "ws");
    }

    #[tokio::test]
    async fn replace_with_empty_clears_the_workspace() {
        let conn = test_async_connection().await;
        seed_workspace(&conn, "w1").await;

        replace_memories(&conn, Some("w1"), &["a".to_string()], 10)
            .await
            .unwrap();
        replace_memories(&conn, Some("w1"), &[], 20).await.unwrap();

        let loaded = load_memories(&conn, Some("w1")).await.unwrap();
        assert!(loaded.is_empty());
    }

    #[tokio::test]
    async fn workspace_id_for_conversation_resolves_and_handles_null() {
        let conn = test_async_connection().await;
        seed_workspace(&conn, "w1").await;
        seed_conversation(&conn, "c1", Some("w1")).await;
        seed_conversation(&conn, "c2", None).await;

        assert_eq!(
            workspace_id_for_conversation(&conn, "c1").await.unwrap(),
            Some("w1".to_string())
        );
        assert_eq!(
            workspace_id_for_conversation(&conn, "c2").await.unwrap(),
            None
        );
    }
}
