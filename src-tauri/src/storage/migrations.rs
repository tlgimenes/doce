use rusqlite::Connection;

/// Ordered list of (version, sql) migrations, tracked via SQLite's built-in
/// `PRAGMA user_version` rather than a hand-rolled tracking table
/// (data-model.md "Schema conventions").
const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("migrations/0001_init.sql")),
    (2, include_str!("migrations/0002_message_duration.sql")),
    (
        3,
        include_str!("migrations/0003_rich_text_content_type.sql"),
    ),
    (
        4,
        include_str!("migrations/0004_context_notice_content_type.sql"),
    ),
    (5, include_str!("migrations/0005_message_token_count.sql")),
    (6, include_str!("migrations/0006_tool_call_id.sql")),
    (
        7,
        include_str!("migrations/0007_conversation_title_trigram_fts.sql"),
    ),
    (
        8,
        include_str!("migrations/0008_conversation_last_seen_at.sql"),
    ),
    (
        9,
        include_str!("migrations/0009_conversation_archived_at.sql"),
    ),
    (10, include_str!("migrations/0010_memories.sql")),
    (11, include_str!("migrations/0011_conversation_goal.sql")),
    (
        12,
        include_str!("migrations/0012_conversation_goal_achieved.sql"),
    ),
    (13, include_str!("migrations/0013_model_sources.sql")),
    (14, include_str!("migrations/0014_model_downloads.sql")),
];

pub fn apply_pending(conn: &mut Connection) -> rusqlite::Result<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;

    for (version, sql) in MIGRATIONS {
        if *version > current {
            let tx = conn.transaction()?;
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", version)?;
            tx.commit()?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Applies only the migrations up to and including `target_version`,
    /// leaving `conn` stopped mid-sequence. Lets a test seed data against
    /// an older schema (e.g. pre-0003) before continuing on to a later
    /// migration under test via a subsequent `apply_pending` call.
    fn apply_up_to(conn: &mut Connection, target_version: i64) {
        for (version, sql) in MIGRATIONS {
            if *version > target_version {
                break;
            }
            let tx = conn.transaction().unwrap();
            tx.execute_batch(sql).unwrap();
            tx.pragma_update(None, "user_version", version).unwrap();
            tx.commit().unwrap();
        }
    }

    fn insert_conversation(conn: &Connection, id: &str) {
        conn.execute(
            "INSERT INTO conversations (id, workspace_id, spawned_by_conversation_id, title, created_at, updated_at) VALUES (?1, NULL, NULL, 'Test', 0, 0)",
            [id],
        )
        .unwrap();
    }

    /// 009-rich-chat-input, T014: after `0003_rich_text_content_type`
    /// widens the CHECK constraint, a `rich_text` row must be insertable —
    /// this is red until that migration exists (the pre-0003 schema still
    /// rejects it).
    #[test]
    fn rich_text_content_type_is_accepted_after_migration() {
        let mut conn = Connection::open_in_memory().unwrap();
        apply_pending(&mut conn).unwrap();
        insert_conversation(&conn, "c1");

        let result = conn.execute(
            "INSERT INTO messages (id, conversation_id, role, content_type, content, created_at, sequence) VALUES ('m1', 'c1', 'user', 'rich_text', '{\"segments\":[]}', 0, 0)",
            [],
        );

        assert!(
            result.is_ok(),
            "expected 'rich_text' to satisfy messages.content_type's CHECK constraint, got {result:?}"
        );
    }

    /// A row inserted under the pre-0003 schema must keep its `rowid`
    /// (and stay findable via `messages_fts`) once the table-rebuild
    /// migration runs — `messages_fts` is external-content, keyed on
    /// `content_rowid='rowid'` against `messages`, so a renumbering would
    /// silently desync existing search results from their source rows.
    #[test]
    fn existing_row_keeps_its_rowid_and_fts_entry_across_the_rebuild() {
        let mut conn = Connection::open_in_memory().unwrap();
        apply_up_to(&mut conn, 2);
        insert_conversation(&conn, "c1");
        conn.execute(
            "INSERT INTO messages (id, conversation_id, role, content_type, content, created_at, sequence) VALUES ('m1', 'c1', 'user', 'text', 'a searchable snowdrift of text', 0, 0)",
            [],
        )
        .unwrap();

        let rowid_before: i64 = conn
            .query_row("SELECT rowid FROM messages WHERE id = 'm1'", [], |row| {
                row.get(0)
            })
            .unwrap();

        apply_pending(&mut conn).unwrap();

        let rowid_after: i64 = conn
            .query_row("SELECT rowid FROM messages WHERE id = 'm1'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(
            rowid_before, rowid_after,
            "rowid must survive the table rebuild"
        );

        let matched_rowid: i64 = conn
            .query_row(
                "SELECT rowid FROM messages_fts WHERE messages_fts MATCH 'snowdrift'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            matched_rowid, rowid_before,
            "pre-migration content must still be found via messages_fts, at its original rowid"
        );
    }

    /// After the rebuild, a newly-inserted row must still be picked up by
    /// `messages_fts` — proof the `messages_ai`/`messages_ad`/`messages_au`
    /// sync triggers (dropped along with the old table) were recreated on
    /// the rebuilt table.
    #[test]
    fn new_row_after_migration_is_indexed_by_fts_sync_triggers() {
        let mut conn = Connection::open_in_memory().unwrap();
        apply_pending(&mut conn).unwrap();
        insert_conversation(&conn, "c1");

        conn.execute(
            "INSERT INTO messages (id, conversation_id, role, content_type, content, created_at, sequence) VALUES ('m2', 'c1', 'user', 'text', 'a distinctive marmalade word', 0, 0)",
            [],
        )
        .unwrap();

        let rowid: i64 = conn
            .query_row("SELECT rowid FROM messages WHERE id = 'm2'", [], |row| {
                row.get(0)
            })
            .unwrap();
        let matched_rowid: i64 = conn
            .query_row(
                "SELECT rowid FROM messages_fts WHERE messages_fts MATCH 'marmalade'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            matched_rowid, rowid,
            "a post-migration insert must still be synced into messages_fts"
        );
    }

    /// 010-context-window-management: 'context_notice' must satisfy
    /// `content_type`'s CHECK constraint post-0004 — red until that
    /// migration exists, same style as 0003's own analogous test above.
    #[test]
    fn context_notice_content_type_is_accepted_after_migration() {
        let mut conn = Connection::open_in_memory().unwrap();
        apply_pending(&mut conn).unwrap();
        insert_conversation(&conn, "c1");

        let result = conn.execute(
            "INSERT INTO messages (id, conversation_id, role, content_type, content, created_at, sequence) VALUES ('m1', 'c1', 'assistant', 'context_notice', '{\"kind\":\"cleared\",\"clearedCount\":1,\"notice\":\"n\"}', 0, 0)",
            [],
        );

        assert!(
            result.is_ok(),
            "expected 'context_notice' to satisfy messages.content_type's CHECK constraint, got {result:?}"
        );
    }

    /// Same rowid/FTS-survival proof as `existing_row_keeps_its_rowid_and_fts_entry_across_the_rebuild`,
    /// specifically across the 0004 rebuild (applying up to 0003 first).
    #[test]
    fn existing_row_keeps_its_rowid_and_fts_entry_across_the_0004_rebuild() {
        let mut conn = Connection::open_in_memory().unwrap();
        apply_up_to(&mut conn, 3);
        insert_conversation(&conn, "c1");
        conn.execute(
            "INSERT INTO messages (id, conversation_id, role, content_type, content, created_at, sequence) VALUES ('m1', 'c1', 'user', 'text', 'a searchable snowdrift of text', 0, 0)",
            [],
        )
        .unwrap();

        let rowid_before: i64 = conn
            .query_row("SELECT rowid FROM messages WHERE id = 'm1'", [], |row| {
                row.get(0)
            })
            .unwrap();

        apply_pending(&mut conn).unwrap();

        let rowid_after: i64 = conn
            .query_row("SELECT rowid FROM messages WHERE id = 'm1'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(
            rowid_before, rowid_after,
            "rowid must survive the 0004 table rebuild"
        );

        let matched_rowid: i64 = conn
            .query_row(
                "SELECT rowid FROM messages_fts WHERE messages_fts MATCH 'snowdrift'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            matched_rowid, rowid_before,
            "pre-migration content must still be found via messages_fts, at its original rowid"
        );
    }

    #[test]
    fn existing_conversation_titles_are_backfilled_into_trigram_index() {
        let mut conn = Connection::open_in_memory().unwrap();
        apply_up_to(&mut conn, 6);
        insert_conversation(&conn, "c1");
        conn.execute(
            "UPDATE conversations SET title = 'database migration plan' WHERE id = 'c1'",
            [],
        )
        .unwrap();

        apply_pending(&mut conn).unwrap();

        let rowid: i64 = conn
            .query_row(
                "SELECT rowid FROM conversations WHERE id = 'c1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let matched_rowid: i64 = conn
            .query_row(
                "SELECT rowid FROM conversations_title_trigram WHERE conversations_title_trigram MATCH 'migrat'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            matched_rowid, rowid,
            "pre-0007 titles must be backfilled into the trigram FTS index"
        );
    }

    #[test]
    fn conversation_last_seen_at_is_backfilled_from_updated_at() {
        let mut conn = Connection::open_in_memory().unwrap();
        apply_up_to(&mut conn, 7);
        conn.execute(
            "INSERT INTO conversations (id, workspace_id, spawned_by_conversation_id, title, created_at, updated_at) VALUES ('c1', NULL, NULL, 'Seen test', 10, 42)",
            [],
        )
        .unwrap();

        apply_pending(&mut conn).unwrap();

        let last_seen_at: i64 = conn
            .query_row(
                "SELECT last_seen_at FROM conversations WHERE id = 'c1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(last_seen_at, 42);
    }

    #[test]
    fn conversation_last_seen_at_is_not_nullable_after_migration() {
        let mut conn = Connection::open_in_memory().unwrap();
        apply_pending(&mut conn).unwrap();

        conn.execute(
            "INSERT INTO conversations (id, title, created_at, updated_at) VALUES ('c1', 'x', 1, 2)",
            [],
        )
        .unwrap();

        let last_seen_at: i64 = conn
            .query_row(
                "SELECT last_seen_at FROM conversations WHERE id = 'c1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(last_seen_at, 0);
    }

    #[test]
    fn conversation_archived_at_defaults_to_null_after_migration() {
        let mut conn = Connection::open_in_memory().unwrap();
        apply_pending(&mut conn).unwrap();

        conn.execute(
            "INSERT INTO conversations (id, title, created_at, updated_at) VALUES ('c1', 'x', 1, 2)",
            [],
        )
        .unwrap();

        let archived_at: Option<i64> = conn
            .query_row(
                "SELECT archived_at FROM conversations WHERE id = 'c1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(archived_at, None);
    }

    /// 0011_conversation_goal: confirms the `goal` column exists and
    /// round-trips (insert a conversation with no goal, set one, read it
    /// back) -- the storage on top of which
    /// `storage::conversations::set_conversation_goal`/`get_conversation_goal`
    /// are built.
    #[test]
    fn conversation_goal_column_exists_and_round_trips() {
        let mut conn = Connection::open_in_memory().unwrap();
        apply_pending(&mut conn).unwrap();
        insert_conversation(&conn, "c1");

        let goal: Option<String> = conn
            .query_row(
                "SELECT goal FROM conversations WHERE id = 'c1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(goal, None, "goal defaults to NULL for an existing row");

        conn.execute(
            "UPDATE conversations SET goal = 'ship the login page' WHERE id = 'c1'",
            [],
        )
        .unwrap();

        let goal: Option<String> = conn
            .query_row(
                "SELECT goal FROM conversations WHERE id = 'c1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(goal.as_deref(), Some("ship the login page"));
    }

    #[test]
    fn existing_models_migrate_as_curated_without_losing_the_active_pointer() {
        let mut conn = Connection::open_in_memory().unwrap();
        apply_up_to(&mut conn, 12);
        conn.execute(
            "INSERT INTO models (id, hardware_tier, source_url, quantization, sha256, local_path, capability_tags, installed_at, is_active)\
             VALUES ('balanced', 'apple-silicon-16gb', 'https://example.test/model', 'Q4_K_M', 'sha', '/models/balanced.gguf', '[]', 42, 1)",
            [],
        )
        .unwrap();

        apply_pending(&mut conn).unwrap();

        let row: (String, Option<String>, String, i64) = conn
            .query_row(
                "SELECT source_kind, display_name, local_path, is_active FROM models WHERE id = 'balanced'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(row.0, "curated");
        assert_eq!(row.1, None);
        assert_eq!(row.2, "/models/balanced.gguf");
        assert_eq!(row.3, 1);
    }

    #[test]
    fn model_downloads_are_durable_and_keyed_once_per_model() {
        let mut conn = Connection::open_in_memory().unwrap();
        apply_pending(&mut conn).unwrap();
        conn.execute(
            "INSERT INTO models (id, hardware_tier, source_url, quantization, sha256, capability_tags, source_kind)\
             VALUES ('balanced', '32gb', 'https://example.test/model', 'Q4_K_M', 'sha', '[]', 'curated')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO model_downloads (model_id, state, bytes_downloaded, bytes_total, revision, updated_at)\
             VALUES ('balanced', 'paused', 42, 100, 3, 7)",
            [],
        )
        .unwrap();

        let row: (String, i64, i64, i64) = conn
            .query_row(
                "SELECT state, bytes_downloaded, bytes_total, revision FROM model_downloads WHERE model_id = 'balanced'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(row, ("paused".to_string(), 42, 100, 3));
        assert!(conn
            .execute(
                "INSERT INTO model_downloads (model_id, state, updated_at) VALUES ('balanced', 'queued', 8)",
                [],
            )
            .is_err());
        assert!(conn
            .execute(
                "UPDATE model_downloads SET state = 'preparing' WHERE model_id = 'balanced'",
                [],
            )
            .is_err());
    }
}
