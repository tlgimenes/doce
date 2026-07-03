use crate::commands::models::now_ms;
use crate::storage::DbCell;
use rusqlite::Connection;
use std::collections::HashMap;
use tauri::{AppHandle, State};

/// `serde_json::Value` is deliberately not used in these signatures: its
/// self-referential `Object(Map<String, Value>)` structure sent
/// `tauri-specta`'s (pre-1.0) TypeScript type-graph builder into unbounded
/// recursion — a genuine stack overflow, not a clean error, and reproduced
/// 100% of the time once isolated. Values cross the specta boundary as
/// JSON-encoded strings instead; the frontend does `JSON.parse`/`stringify`,
/// same as it already must for the dynamic per-key settings shape.
#[tauri::command]
#[specta::specta]
pub async fn get_settings(
    app: AppHandle,
    db_cell: State<'_, DbCell>,
) -> Result<HashMap<String, String>, String> {
    let conn = db_cell.get(&app).await?;
    conn.call(
        |conn: &mut Connection| -> rusqlite::Result<HashMap<String, String>> {
            let mut stmt = conn.prepare("SELECT key, value FROM settings")?;
            let rows = stmt
                .query_map([], |row| {
                    let key: String = row.get(0)?;
                    let value: String = row.get(1)?;
                    Ok((key, value))
                })?
                .collect::<Result<HashMap<_, _>, _>>()?;
            Ok(rows)
        },
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn update_setting(
    app: AppHandle,
    db_cell: State<'_, DbCell>,
    key: String,
    value_json: String,
) -> Result<(), String> {
    let conn = db_cell.get(&app).await?;
    let now = now_ms();
    conn.call(move |conn: &mut Connection| -> rusqlite::Result<()> {
        conn.execute(
            "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            rusqlite::params![key, value_json, now],
        )?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())
}
