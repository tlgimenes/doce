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
///
/// ORDERING IS EMISSION ORDER, and `updated_at DESC` never discriminates.
/// `replace_memories` stamps every surviving row with the same `now`, so the
/// leading key is constant across the whole workspace and the tiebreak `id ASC`
/// is what actually orders the result. Rows are inserted in `contents` order
/// (the extraction model's emission order for that pass), and `Uuid::now_v7()`
/// is time-ordered AND monotonic within a millisecond -- so ascending `id`
/// reproduces insertion order. That `now_v7()` monotonicity is a real, load-
/// bearing dependency of this ordering, not an incidental detail: switching
/// `replace_memories` to `Uuid::new_v4()` would randomize the order of a
/// same-millisecond batch and flake `context::tests::
/// extraction_persists_the_emitted_set`. Emission order is the intended
/// behaviour (the model emits its most important facts first); the
/// `updated_at DESC` key is kept only because it costs nothing and would
/// become meaningful if rows were ever stamped individually.
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

/// The delete+insert half of a set swap, run INSIDE `tx` by both
/// `replace_memories` and `replace_memories_if_unchanged`. Rows are inserted in
/// `contents` order; see `load_memories`' ordering note for why that order is
/// what recall renders.
fn swap_set_in_tx(
    tx: &rusqlite::Transaction<'_>,
    workspace_id: &Option<String>,
    contents: &[String],
    now: i64,
) -> rusqlite::Result<()> {
    // Remember prior created_at per content so an unchanged fact keeps its age.
    let prior: std::collections::HashMap<String, i64> = {
        let mut stmt =
            tx.prepare("SELECT content, created_at FROM memories WHERE workspace_id IS ?1")?;
        let rows = stmt
            .query_map([workspace_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter().collect()
    };
    tx.execute(
        "DELETE FROM memories WHERE workspace_id IS ?1",
        [workspace_id],
    )?;
    for content in contents {
        let created_at = prior.get(content).copied().unwrap_or(now);
        tx.execute(
            "INSERT INTO memories (id, workspace_id, content, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                Uuid::now_v7().to_string(),
                workspace_id,
                content,
                created_at,
                now
            ],
        )?;
    }
    Ok(())
}

/// The workspace's current content strings as a sorted multiset -- the
/// comparison basis for `replace_memories_if_unchanged`'s compare-and-swap.
/// Sorted (not a `HashSet`) so duplicate rows can't compare equal to a
/// deduplicated expectation, and so row ORDER is deliberately NOT part of the
/// comparison: `load_memories`' order is emission order from whichever pass
/// wrote last, which says nothing about whether the SET a reader saw is still
/// the set on disk. Only membership matters for "did someone change this under
/// us".
fn current_contents_sorted_in_tx(
    tx: &rusqlite::Transaction<'_>,
    workspace_id: &Option<String>,
) -> rusqlite::Result<Vec<String>> {
    let mut stmt = tx.prepare("SELECT content FROM memories WHERE workspace_id IS ?1")?;
    let mut rows = stmt
        .query_map([workspace_id], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.sort();
    Ok(rows)
}

/// Swaps the workspace's whole memory set in ONE transaction. Content strings
/// that already existed keep their original `created_at`; everything gets the
/// new `updated_at`.
///
/// UNCONDITIONAL: last writer wins. Any caller whose `contents` were derived
/// from an earlier read of this same set (i.e. the extraction pass, whose read
/// and write straddle a multi-second LLM round-trip) must use
/// `replace_memories_if_unchanged` instead, or it will silently clobber a
/// concurrent writer's facts.
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
        swap_set_in_tx(&tx, &workspace_id, &contents, now)?;
        tx.commit()?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())
}

/// Compare-and-swap `replace_memories`: swaps the set only if the workspace's
/// current contents still match `expected` -- the set the caller actually read
/// and reasoned from. Returns `Ok(true)` if the swap happened, `Ok(false)` if
/// the set changed underneath the caller (no rows touched).
///
/// WHY THIS EXISTS (the lost-update window). `extract_and_persist_memories`
/// loads the set, spends multiple seconds in an LLM round-trip, then writes the
/// model's FULL replacement set. Two conversations in one workspace can compact
/// concurrently -- `commands::context`'s manual "Compact now" and `agent`'s
/// `maybe_compact` are not mutually excluded, and `ActiveGenerations` is
/// per-conversation with no global turn lock. Both read `[X]`, A writes
/// `[X, oxfmt]`, B writes `[X, gated]`, and "oxfmt" is gone FOREVER: A's span is
/// already condensed away and will never be re-extracted. So the read must be
/// validated at write time.
///
/// The re-read happens INSIDE the same transaction as the swap, so it cannot
/// itself be raced. Comparison is an order-insensitive multiset of content
/// strings (see `current_contents_sorted_in_tx`). Returning `Ok(false)` rather
/// than merging is deliberate: this is a full-set swap authored by a model that
/// never saw the other writer's facts, so there is no sound merge -- dropping
/// this pass loses at most the facts from one span, while writing it anyway
/// destroys another pass's committed work.
pub async fn replace_memories_if_unchanged(
    conn: &tokio_rusqlite::Connection,
    workspace_id: Option<&str>,
    expected: &[String],
    contents: &[String],
    now: i64,
) -> Result<bool, String> {
    let workspace_id = workspace_id.map(|s| s.to_string());
    let contents = contents.to_vec();
    let mut expected = expected.to_vec();
    expected.sort();
    conn.call(move |conn: &mut Connection| -> rusqlite::Result<bool> {
        let tx = conn.transaction()?;
        if current_contents_sorted_in_tx(&tx, &workspace_id)? != expected {
            // Dropping `tx` without `commit` rolls back; nothing was written
            // anyway, but the rollback is what makes the re-read atomic with
            // respect to a concurrent swap.
            return Ok(false);
        }
        swap_set_in_tx(&tx, &workspace_id, &contents, now)?;
        tx.commit()?;
        Ok(true)
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

    async fn sorted_contents(
        conn: &tokio_rusqlite::Connection,
        workspace_id: Option<&str>,
    ) -> Vec<String> {
        let mut c: Vec<String> = load_memories(conn, workspace_id)
            .await
            .unwrap()
            .into_iter()
            .map(|m| m.content)
            .collect();
        c.sort();
        c
    }

    /// IMPORTANT 1 (lost update). The exact interleaving from the finding:
    /// workspace W holds `[X]`; extraction A and extraction B both read `[X]`
    /// and go off to the model; A commits `[X, oxfmt]` first; B then tries to
    /// commit `[X, gated]`, which was authored in ignorance of "oxfmt". B must
    /// make NO change and report it, or "oxfmt" is destroyed permanently (A's
    /// span is already condensed and will never be re-extracted).
    ///
    /// The multi-second LLM round-trip is simulated by simply doing A's commit
    /// in between B's "read" (`expected`) and B's write -- which is precisely
    /// what the window is.
    #[tokio::test]
    async fn cas_makes_no_change_when_the_set_changed_between_read_and_write() {
        let conn = test_async_connection().await;
        seed_workspace(&conn, "w1").await;
        replace_memories(&conn, Some("w1"), &["X".to_string()], 5)
            .await
            .unwrap();

        // B reads the set, then heads off to the model.
        let b_read: Vec<String> = load_memories(&conn, Some("w1"))
            .await
            .unwrap()
            .into_iter()
            .map(|m| m.content)
            .collect();
        assert_eq!(b_read, vec!["X".to_string()]);

        // A finishes first and commits its own full replacement set.
        replace_memories(
            &conn,
            Some("w1"),
            &["X".to_string(), "oxfmt".to_string()],
            10,
        )
        .await
        .unwrap();

        // B comes back and tries to write the set it authored from `[X]`.
        let wrote = replace_memories_if_unchanged(
            &conn,
            Some("w1"),
            &b_read,
            &["X".to_string(), "gated".to_string()],
            20,
        )
        .await
        .unwrap();

        assert!(!wrote, "a stale CAS must report that it did not write");
        assert_eq!(
            sorted_contents(&conn, Some("w1")).await,
            vec!["X".to_string(), "oxfmt".to_string()],
            "the stale write must not clobber the winner's set -- 'oxfmt' survives, \
             'gated' was never written"
        );
    }

    #[tokio::test]
    async fn cas_writes_when_the_set_is_unchanged() {
        let conn = test_async_connection().await;
        seed_workspace(&conn, "w1").await;
        replace_memories(&conn, Some("w1"), &["X".to_string()], 5)
            .await
            .unwrap();

        let wrote = replace_memories_if_unchanged(
            &conn,
            Some("w1"),
            &["X".to_string()],
            &["X".to_string(), "oxfmt".to_string()],
            20,
        )
        .await
        .unwrap();

        assert!(wrote);
        assert_eq!(
            sorted_contents(&conn, Some("w1")).await,
            vec!["X".to_string(), "oxfmt".to_string()]
        );
        let keep = load_memories(&conn, Some("w1"))
            .await
            .unwrap()
            .into_iter()
            .find(|m| m.content == "X")
            .unwrap();
        assert_eq!(keep.created_at, 5, "a CAS swap still preserves created_at");
    }

    /// The comparison is a multiset of contents, NOT a sequence: row order is
    /// emission order from whichever pass wrote last and says nothing about
    /// whether the set changed. A reader that saw the same facts in a different
    /// order has not been raced and must be allowed to write.
    #[tokio::test]
    async fn cas_comparison_is_order_insensitive() {
        let conn = test_async_connection().await;
        seed_workspace(&conn, "w1").await;
        replace_memories(&conn, Some("w1"), &["a".to_string(), "b".to_string()], 5)
            .await
            .unwrap();

        let wrote = replace_memories_if_unchanged(
            &conn,
            Some("w1"),
            &["b".to_string(), "a".to_string()],
            &["c".to_string()],
            20,
        )
        .await
        .unwrap();

        assert!(
            wrote,
            "same set, different order, must still count as unchanged"
        );
        assert_eq!(
            sorted_contents(&conn, Some("w1")).await,
            vec!["c".to_string()]
        );
    }

    /// An empty expectation is a real expectation: a caller that read an empty
    /// set must still lose to a writer that populated it in the meantime.
    #[tokio::test]
    async fn cas_from_an_empty_read_loses_to_a_concurrent_populate() {
        let conn = test_async_connection().await;
        seed_workspace(&conn, "w1").await;

        replace_memories(&conn, Some("w1"), &["sibling".to_string()], 10)
            .await
            .unwrap();
        let wrote =
            replace_memories_if_unchanged(&conn, Some("w1"), &[], &["mine".to_string()], 20)
                .await
                .unwrap();

        assert!(!wrote);
        assert_eq!(
            sorted_contents(&conn, Some("w1")).await,
            vec!["sibling".to_string()]
        );
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
