use crate::commands::models::now_ms;
use crate::mcp;
use crate::storage::DbCell;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct McpServerConnection {
    pub id: String,
    pub name: String,
    pub transport: String,
    pub config: String,
    pub enabled: bool,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct McpToolInfo {
    pub name: String,
    pub description: Option<String>,
}

/// FR-018: registers a user-configured stdio MCP server. `config` is the
/// JSON-encoded `{"command": "...", "args": [...]}` shape
/// (`data-model.md`'s `MCPServerConnection.config`).
#[tauri::command]
#[specta::specta]
pub async fn add_mcp_server(
    app: AppHandle,
    db_cell: State<'_, DbCell>,
    name: String,
    command: String,
    args: Vec<String>,
) -> Result<McpServerConnection, String> {
    let conn = db_cell.get(&app).await?;
    let now = now_ms();
    let config = serde_json::json!({"command": command, "args": args}).to_string();

    conn.call(move |conn: &mut Connection| -> rusqlite::Result<McpServerConnection> {
        let id = Uuid::now_v7().to_string();
        conn.execute(
            "INSERT INTO mcp_server_connections (id, name, transport, config, enabled, created_at) VALUES (?1, ?2, 'stdio', ?3, 1, ?4)",
            rusqlite::params![id, name, config, now],
        )?;
        Ok(McpServerConnection { id, name, transport: "stdio".to_string(), config, enabled: true, created_at: now })
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn list_mcp_servers(
    app: AppHandle,
    db_cell: State<'_, DbCell>,
) -> Result<Vec<McpServerConnection>, String> {
    let conn = db_cell.get(&app).await?;
    conn.call(|conn: &mut Connection| -> rusqlite::Result<Vec<McpServerConnection>> {
        let mut stmt =
            conn.prepare("SELECT id, name, transport, config, enabled, created_at FROM mcp_server_connections")?;
        let rows = stmt
            .query_map([], |row| {
                Ok(McpServerConnection {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    transport: row.get(2)?,
                    config: row.get(3)?,
                    enabled: row.get::<_, i64>(4)? != 0,
                    created_at: row.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
    .await
    .map_err(|e| e.to_string())
}

/// FR-019: connects to a registered stdio MCP server and lists the tools
/// it exposes — a point-in-time capability query (e.g. for a settings
/// panel's "test connection" action), not a persistent session wired into
/// the agent loop's tool dispatch (see `mcp::list_tools_stdio`'s doc
/// comment for why that further step isn't built in this pass).
#[tauri::command]
#[specta::specta]
pub async fn list_mcp_server_tools(
    app: AppHandle,
    db_cell: State<'_, DbCell>,
    server_id: String,
) -> Result<Vec<McpToolInfo>, String> {
    let conn = db_cell.get(&app).await?;
    let config_json: String = conn
        .call(move |conn: &mut Connection| -> rusqlite::Result<String> {
            conn.query_row(
                "SELECT config FROM mcp_server_connections WHERE id = ?1",
                [&server_id],
                |row| row.get(0),
            )
        })
        .await
        .map_err(|e| e.to_string())?;

    let config: serde_json::Value =
        serde_json::from_str(&config_json).map_err(|e| e.to_string())?;
    let command = config
        .get("command")
        .and_then(|v| v.as_str())
        .ok_or("missing command in config")?;
    let args: Vec<String> = config
        .get("args")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let tools = mcp::list_tools_stdio(command, &args)
        .await
        .map_err(|e| e.to_string())?;
    Ok(tools
        .into_iter()
        .map(|t| McpToolInfo {
            name: t.name,
            description: t.description,
        })
        .collect())
}
