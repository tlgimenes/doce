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

/// Registers a user-configured remote (streamable-HTTP) MCP server,
/// reachable by `url`. `config` is the JSON-encoded `{"url", "auth_token"?}`
/// shape parsed by `mcp::parse_config` for the `http` transport.
///
/// `auth_token`, if provided, is a bearer token attached verbatim as an
/// `Authorization: Bearer <token>` header on every request — a deliberate
/// minimal stub. There is NO OAuth acquisition/refresh flow; obtaining and
/// rotating tokens (e.g. Google OAuth) is a planned follow-up. Callers that
/// need auth must supply an already-valid token here.
#[tauri::command]
#[specta::specta]
pub async fn add_mcp_http_server(
    app: AppHandle,
    db_cell: State<'_, DbCell>,
    name: String,
    url: String,
    auth_token: Option<String>,
) -> Result<McpServerConnection, String> {
    let conn = db_cell.get(&app).await?;
    let now = now_ms();
    let config = match &auth_token {
        Some(token) => serde_json::json!({ "url": url, "auth_token": token }),
        None => serde_json::json!({ "url": url }),
    }
    .to_string();

    conn.call(move |conn: &mut Connection| -> rusqlite::Result<McpServerConnection> {
        let id = Uuid::now_v7().to_string();
        conn.execute(
            "INSERT INTO mcp_server_connections (id, name, transport, config, enabled, created_at) VALUES (?1, ?2, 'http', ?3, 1, ?4)",
            rusqlite::params![id, name, config, now],
        )?;
        Ok(McpServerConnection { id, name, transport: "http".to_string(), config, enabled: true, created_at: now })
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

/// FR-019: connects to a registered MCP server (stdio or http) and lists
/// the tools it exposes — a point-in-time capability query (e.g. for a
/// settings panel's "test connection" action), not a persistent session
/// wired into the agent loop's tool dispatch. Transport-agnostic: reads
/// the stored `transport`/`config`, parses it via `mcp::parse_config`, and
/// connects over whichever transport it selects.
#[tauri::command]
#[specta::specta]
pub async fn list_mcp_server_tools(
    app: AppHandle,
    db_cell: State<'_, DbCell>,
    server_id: String,
) -> Result<Vec<McpToolInfo>, String> {
    let conn = db_cell.get(&app).await?;
    let (transport, config_json): (String, String) = conn
        .call(
            move |conn: &mut Connection| -> rusqlite::Result<(String, String)> {
                conn.query_row(
                    "SELECT transport, config FROM mcp_server_connections WHERE id = ?1",
                    [&server_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
            },
        )
        .await
        .map_err(|e| e.to_string())?;

    let config = mcp::parse_config(&transport, &config_json).map_err(|e| e.to_string())?;
    let tools = mcp::list_tools(&config).await.map_err(|e| e.to_string())?;
    Ok(tools
        .into_iter()
        .map(|t| McpToolInfo {
            name: t.name,
            description: t.description,
        })
        .collect())
}
