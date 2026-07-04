use crate::commands::models::now_ms;
use crate::storage::DbCell;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, State};
use uuid::Uuid;

const FOLDER_SEARCH_RESULT_LIMIT: usize = 10;
const FOLDER_SEARCH_QUERY_MIN_LEN: usize = 2;

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub id: String,
    pub path: String,
    pub display_name: String,
    pub created_at: i64,
    pub last_opened_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FolderSearchResult {
    pub path: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FolderSearchPage {
    pub folders: Vec<FolderSearchResult>,
    pub truncated: bool,
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

#[tauri::command]
#[specta::specta]
pub async fn search_folders(
    query: String,
    max_results: Option<usize>,
) -> Result<FolderSearchPage, String> {
    let needle = query.trim();
    if needle.is_empty() {
        return Ok(FolderSearchPage {
            folders: Vec::new(),
            truncated: false,
        });
    }

    let wants_path_mode = needle.starts_with('/') || needle.starts_with('~');
    if !wants_path_mode && needle.len() < FOLDER_SEARCH_QUERY_MIN_LEN {
        return Ok(FolderSearchPage {
            folders: Vec::new(),
            truncated: false,
        });
    }

    let home_dir = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| "/".to_string());

    let (base, filter) = resolve_search_scope(needle, &home_dir);
    let wants_path_mode = needle.starts_with('/') || needle.starts_with('~');
    let exact_match = if wants_path_mode && !needle.ends_with('/') && needle != "~" && needle != "/"
    {
        Some(resolve_path_query(needle, &home_dir)).filter(|path| path.is_dir())
    } else {
        None
    };
    let mut folders = collect_direct_children(&base)?;

    if let Some(filter) = filter {
        let filter = filter.to_lowercase();
        folders.retain(|entry| entry.display_name.to_lowercase().contains(&filter));
    }

    folders.sort_unstable_by(|a, b| a.display_name.cmp(&b.display_name));
    if let Some(exact_path) = exact_match {
        let exact = exact_path.to_string_lossy().to_string();
        folders.retain(|folder| folder.path != exact);
        let display_name = exact_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("Home")
            .to_string();
        folders.insert(
            0,
            FolderSearchResult {
                path: exact,
                display_name,
            },
        );
    }

    let max_results = max_results.unwrap_or(FOLDER_SEARCH_RESULT_LIMIT).max(1);
    let truncated = folders.len() > max_results;
    if truncated {
        folders.truncate(max_results);
    }

    Ok(FolderSearchPage { folders, truncated })
}

fn resolve_search_scope(query: &str, home_dir: &str) -> (PathBuf, Option<String>) {
    if !query.starts_with('/') && !query.starts_with('~') {
        return (PathBuf::from(home_dir), Some(query.to_string()));
    }

    if query == "~" {
        return (PathBuf::from(home_dir), None);
    }

    if query == "/" {
        return (PathBuf::from("/"), None);
    }

    let full_query = if let Some(rest) = query.strip_prefix("~/") {
        let mut path = PathBuf::from(home_dir);
        path.push(rest);
        path
    } else {
        PathBuf::from(query)
    };

    if full_query.is_dir() {
        return (full_query, None);
    }

    let parent = full_query.parent().filter(|p| p.is_dir());
    let filter = full_query
        .file_name()
        .and_then(|value| value.to_str())
        .map(|segment| segment.to_lowercase())
        .filter(|segment| !segment.is_empty());

    (parent.unwrap_or(Path::new(home_dir)).to_path_buf(), filter)
}

fn resolve_path_query(query: &str, home_dir: &str) -> PathBuf {
    if let Some(rest) = query.strip_prefix("~/") {
        let mut path = PathBuf::from(home_dir);
        path.push(rest);
        path
    } else {
        PathBuf::from(query)
    }
}

fn collect_direct_children(base: &Path) -> Result<Vec<FolderSearchResult>, String> {
    let read_dir = match fs::read_dir(base) {
        Ok(reader) => reader,
        Err(_) => return Ok(Vec::new()),
    };
    let mut folders = Vec::new();

    for entry in read_dir {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(_) => continue,
        };
        if !file_type.is_dir() {
            continue;
        }

        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if name.is_empty() || name.starts_with('.') {
            continue;
        }

        folders.push(FolderSearchResult {
            path: path.to_string_lossy().to_string(),
            display_name: name.to_string(),
        });
    }

    Ok(folders)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn resolve_search_scope_uses_home_for_relative_input() {
        let (base, filter) = resolve_search_scope("doce", "/home/tester");

        assert_eq!(base, PathBuf::from("/home/tester"));
        assert_eq!(filter, Some("doce".to_string()));
    }

    #[test]
    fn resolve_search_scope_uses_full_path_and_no_filter_when_existing() {
        let dir = tempdir().unwrap();
        let query = dir.path().to_string_lossy().to_string();

        let (base, filter) = resolve_search_scope(&query, "/fallback");

        assert_eq!(base, dir.path().to_path_buf());
        assert_eq!(filter, None);
    }

    #[test]
    fn resolve_search_scope_uses_parent_and_filter_for_partial_full_path() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("projects")).unwrap();
        let query = format!("{}/projects/missing", dir.path().to_string_lossy());

        let (base, filter) = resolve_search_scope(&query, "/fallback");

        assert_eq!(base, dir.path().join("projects"));
        assert_eq!(filter, Some("missing".to_string()));
    }

    #[test]
    fn collect_direct_children_includes_only_direct_folders() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("alpha")).unwrap();
        fs::create_dir(dir.path().join("beta")).unwrap();
        fs::create_dir(dir.path().join(".hidden")).unwrap();
        fs::write(dir.path().join("note.txt"), "ignore").unwrap();

        let mut children = collect_direct_children(dir.path()).unwrap();
        children.sort_unstable_by(|a, b| a.display_name.cmp(&b.display_name));

        assert_eq!(children.len(), 2);
        assert_eq!(children[0].display_name, "alpha");
        assert_eq!(children[1].display_name, "beta");
    }

    #[tokio::test]
    async fn search_folders_includes_exact_existing_path_match_first() {
        let dir = tempdir().unwrap();
        let base = dir.path().join("code");
        let mesh = base.join("mesh");
        fs::create_dir_all(&mesh).unwrap();
        fs::create_dir(mesh.join("child")).unwrap();
        let base_path = base.to_string_lossy().into_owned();
        let query = format!("{}/mesh", base_path);
        let expected = query.clone();
        let result = search_folders(query, Some(10)).await.unwrap();

        assert_eq!(
            result.folders.first().map(|item| item.path.as_str()),
            Some(expected.as_str())
        );
        assert_eq!(
            result
                .folders
                .first()
                .map(|item| item.display_name.as_str()),
            Some("mesh")
        );
        assert_eq!(result.folders.len(), 2);
    }

    #[tokio::test]
    async fn search_folders_does_not_inject_exact_match_for_trailing_slash_queries() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("code");
        fs::create_dir_all(path.join("mesh")).unwrap();
        let path_str = path.to_string_lossy().into_owned();
        let query = format!("{}/mesh/", path_str);
        let expected = format!("{}/mesh/", path_str);
        let result = search_folders(query, Some(10)).await.unwrap();

        assert_ne!(
            result.folders.first().map(|item| item.path.as_str()),
            Some(expected.as_str())
        );
    }
}
