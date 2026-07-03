use rusqlite::Connection;

/// Ordered list of (version, sql) migrations, tracked via SQLite's built-in
/// `PRAGMA user_version` rather than a hand-rolled tracking table
/// (data-model.md "Schema conventions").
const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("migrations/0001_init.sql")),
    (2, include_str!("migrations/0002_message_duration.sql")),
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
