use crate::commands::models::now_ms;
use crate::storage::DbCell;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tauri::{AppHandle, State};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub id: String,
    pub path: String,
    pub display_name: String,
    pub created_at: i64,
    pub last_opened_at: i64,
}

/// FR-008: opens a folder for agent mode. Reuses the existing row (and
/// bumps `last_opened_at`) if this path was opened before, rather than
/// creating a duplicate — enforced at the database level too (`path
/// UNIQUE`, `data-model.md`), not just here.
#[tauri::command]
#[specta::specta]
pub async fn open_workspace(
    app: AppHandle,
    db_cell: State<'_, DbCell>,
    path: String,
) -> Result<Workspace, String> {
    if !Path::new(&path).is_dir() {
        return Err(format!("{path} does not exist or is not a directory"));
    }
    let display_name = Path::new(&path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.clone());

    let conn = db_cell.get(&app).await?;
    let now = now_ms();

    conn.call({
        let path = path.clone();
        let display_name = display_name.clone();
        move |conn: &mut Connection| -> rusqlite::Result<Workspace> {
            let existing: Option<String> =
                conn.query_row("SELECT id FROM workspaces WHERE path = ?1", [&path], |row| row.get(0)).ok();

            let id = if let Some(id) = existing {
                conn.execute("UPDATE workspaces SET last_opened_at = ?1 WHERE id = ?2", rusqlite::params![now, id])?;
                id
            } else {
                let id = Uuid::now_v7().to_string();
                conn.execute(
                    "INSERT INTO workspaces (id, path, display_name, created_at, last_opened_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                    rusqlite::params![id, path, display_name, now, now],
                )?;
                id
            };

            conn.query_row(
                "SELECT id, path, display_name, created_at, last_opened_at FROM workspaces WHERE id = ?1",
                [&id],
                |row| {
                    Ok(Workspace {
                        id: row.get(0)?,
                        path: row.get(1)?,
                        display_name: row.get(2)?,
                        created_at: row.get(3)?,
                        last_opened_at: row.get(4)?,
                    })
                },
            )
        }
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn list_workspaces(
    app: AppHandle,
    db_cell: State<'_, DbCell>,
) -> Result<Vec<Workspace>, String> {
    let conn = db_cell.get(&app).await?;
    conn.call(|conn: &mut Connection| -> rusqlite::Result<Vec<Workspace>> {
        let mut stmt = conn.prepare(
            "SELECT id, path, display_name, created_at, last_opened_at FROM workspaces ORDER BY last_opened_at DESC",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(Workspace {
                    id: row.get(0)?,
                    path: row.get(1)?,
                    display_name: row.get(2)?,
                    created_at: row.get(3)?,
                    last_opened_at: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
    .await
    .map_err(|e| e.to_string())
}
