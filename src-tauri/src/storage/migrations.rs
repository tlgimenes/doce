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
}
