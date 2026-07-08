pub mod conversations;
mod migrations;

use rusqlite::Connection;
use std::path::PathBuf;
use tauri::AppHandle;
use tokio::sync::OnceCell;
use tokio_rusqlite::Connection as AsyncConnection;

pub struct Db(pub AsyncConnection);

/// Lazily opens the database on first use rather than eagerly during
/// `.setup()`. Found necessary during implementation: eagerly spawning the
/// connection-open + migrations concurrently with `mount_events(app)` inside
/// `.setup()` correlated with an intermittent, hard-to-reproduce startup
/// crash ("thread 'main' has overflowed its stack") — see the note on
/// `run()` in `lib.rs`. Deferring all storage I/O until the first real
/// command call (which only happens once the webview has loaded and run its
/// JS, well after the native window/event-loop startup path has settled)
/// avoids doing any of this work during that fragile window.
pub struct DbCell(OnceCell<Db>);

impl DbCell {
    pub fn new() -> Self {
        Self(OnceCell::new())
    }

    pub async fn get(&self, app: &AppHandle) -> Result<&AsyncConnection, String> {
        self.0
            .get_or_try_init(|| async { open_and_migrate(app).await.map_err(|e| e.to_string()) })
            .await
            .map(|db| &db.0)
    }
}

impl Default for DbCell {
    fn default() -> Self {
        Self::new()
    }
}

/// Opens (creating if needed) the app's local SQLite database, applies
/// pending migrations, and sets the connection pragmas data-model.md
/// requires (WAL + foreign_keys, off by default in SQLite).
async fn open_and_migrate(app: &AppHandle) -> Result<Db, Box<dyn std::error::Error>> {
    let dir = db_path(app)
        .parent()
        .expect("db path always has a parent")
        .to_path_buf();
    std::fs::create_dir_all(&dir)?;
    let db_path = db_path(app);

    let conn = AsyncConnection::open(db_path.clone()).await?;
    conn.call(|conn: &mut Connection| -> rusqlite::Result<()> {
        // busy_timeout: don't fail outright on a transient write-lock
        // collision (e.g. WAL checkpointing) — retry for up to 5s first.
        conn.execute_batch(
            "PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;",
        )?;
        migrations::apply_pending(conn)?;
        // Crash recovery: a trailing unpaired tool_call row can only mean
        // a previous process died mid-tool (a live turn always persists
        // the result eventually, and no turn can be running yet — this is
        // the first DB access of the process). Heal before any command
        // reads, so no view ever sees a permanently-"in flight" turn.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        conversations::heal_interrupted_tool_calls(conn, now)?;
        Ok(())
    })
    .await?;

    Ok(Db(conn))
}

pub fn db_path(app: &AppHandle) -> PathBuf {
    use tauri::Manager;
    app.path()
        .app_local_data_dir()
        .expect("app local data dir must be resolvable")
        .join("doce.sqlite")
}

/// An in-memory, fully-migrated connection for integration tests — same
/// schema real app data gets, without touching the filesystem or going
/// through Tauri's `AppHandle`/async-connection machinery.
pub fn test_connection() -> Connection {
    let mut conn = Connection::open_in_memory().expect("open in-memory sqlite");
    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .expect("set pragmas");
    migrations::apply_pending(&mut conn).expect("apply migrations");
    conn
}

/// 004-tool-call-widgets: the async (`tokio_rusqlite`) counterpart of
/// `test_connection()` — for tests exercising code that talks to the DB
/// through the same async `Connection` real command handlers use (e.g.
/// `commands::agent`'s tool-call persistence), not just synchronous
/// storage-layer logic.
pub async fn test_async_connection() -> AsyncConnection {
    let conn = AsyncConnection::open_in_memory()
        .await
        .expect("open in-memory sqlite");
    conn.call(|conn: &mut Connection| -> rusqlite::Result<()> {
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        migrations::apply_pending(conn)
    })
    .await
    .expect("apply migrations");
    conn
}
