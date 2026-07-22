//! Tauri commands for the OAuth engine: connect an OAuth account (runs the
//! full desktop PKCE flow), list/remove accounts, and register an MCP server
//! linked to an account.
//!
//! Tokens (access + refresh) are written to the macOS Keychain via the managed
//! [`crate::oauth::OAuthTokenStore`]; only non-secret metadata lands in SQLite
//! (`mcp_oauth_accounts`). Nothing here hardcodes any provider credentials —
//! `client_id`/`client_secret` are user-supplied params.

use crate::commands::mcp::McpServerConnection;
use crate::oauth::{self, now_ms};
use crate::storage::DbCell;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use uuid::Uuid;

/// The non-secret view of a connected OAuth account, mirrored from the
/// `mcp_oauth_accounts` metadata row. Tokens are never included.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OAuthAccount {
    pub id: String,
    pub provider: String,
    pub client_id: String,
    pub scopes: Vec<String>,
    /// Unix-epoch ms of the current access token's expiry.
    pub expires_at: i64,
    pub created_at: i64,
}

/// Runs the whole desktop OAuth flow for `provider` (currently `google`):
/// opens the system browser, waits for the loopback redirect, exchanges the
/// code, stores the tokens in the Keychain, and records the account metadata.
/// `client_id`/`client_secret` are user-supplied (registering the OAuth client
/// is human-gated); `scopes` empty falls back to the provider's defaults.
///
/// This awaits the interactive browser consent, so it does not return until
/// the user authorizes (or the flow errors).
#[tauri::command]
#[specta::specta]
pub async fn connect_oauth_account(
    app: AppHandle,
    db_cell: State<'_, DbCell>,
    token_store: State<'_, oauth::OAuthTokenStore>,
    provider: String,
    client_id: String,
    client_secret: Option<String>,
    scopes: Vec<String>,
) -> Result<OAuthAccount, String> {
    // A blank `client_id` means "use the built-in client" — fall back to the
    // credentials baked in at build time (if any). A non-empty `client_id`
    // keeps the bring-your-own path unchanged.
    let (client_id, client_secret) = oauth::resolve_client_credentials(
        &client_id,
        client_secret,
        oauth::google::builtin_client(),
    )
    .map_err(|e| e.to_string())?;

    let config = oauth::provider_config(&provider, client_id, client_secret, scopes)
        .map_err(|e| e.to_string())?;

    let flow = oauth::begin_flow(config.clone())
        .await
        .map_err(|e| e.to_string())?;
    let tokens = oauth::await_callback(flow)
        .await
        .map_err(|e| e.to_string())?;

    let id = Uuid::now_v7().to_string();
    let now = now_ms();
    let account = OAuthAccount {
        id: id.clone(),
        provider: provider.clone(),
        client_id: config.client_id.clone(),
        scopes: tokens.scopes.clone(),
        expires_at: tokens.expires_at,
        created_at: now,
    };

    // Tokens -> Keychain (the full self-contained credential blob).
    token_store
        .put_credential(&id, config, tokens)
        .map_err(|e| e.to_string())?;

    // Metadata -> SQLite (no secrets).
    let conn = db_cell.get(&app).await?;
    let scopes_json = serde_json::to_string(&account.scopes).map_err(|e| e.to_string())?;
    let insert = account.clone();
    conn.call(move |conn: &mut Connection| -> rusqlite::Result<()> {
        conn.execute(
            "INSERT INTO mcp_oauth_accounts (id, provider, client_id, scopes, expires_at, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![insert.id, insert.provider, insert.client_id, scopes_json, insert.expires_at, insert.created_at],
        )?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?;

    Ok(account)
}

/// Whether this build ships a built-in Google OAuth client (both
/// `DOCE_GOOGLE_CLIENT_ID` and `DOCE_GOOGLE_CLIENT_SECRET` were injected at
/// build time). The Connect UI calls this on mount: when `true` it offers a
/// one-click "Continue with Google" (no credential fields) and keeps the
/// bring-your-own client as an advanced fallback; when `false` the BYO fields
/// are required.
#[tauri::command]
#[specta::specta]
pub fn google_oauth_builtin_available() -> bool {
    oauth::google::builtin_client().is_some()
}

#[tauri::command]
#[specta::specta]
pub async fn list_oauth_accounts(
    app: AppHandle,
    db_cell: State<'_, DbCell>,
) -> Result<Vec<OAuthAccount>, String> {
    let conn = db_cell.get(&app).await?;
    conn.call(|conn: &mut Connection| -> rusqlite::Result<Vec<OAuthAccount>> {
        let mut stmt = conn.prepare(
            "SELECT id, provider, client_id, scopes, expires_at, created_at FROM mcp_oauth_accounts ORDER BY created_at",
        )?;
        let rows = stmt
            .query_map([], |row| {
                let scopes_json: String = row.get(3)?;
                let scopes = serde_json::from_str(&scopes_json).unwrap_or_default();
                Ok(OAuthAccount {
                    id: row.get(0)?,
                    provider: row.get(1)?,
                    client_id: row.get(2)?,
                    scopes,
                    expires_at: row.get(4)?,
                    created_at: row.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
    .await
    .map_err(|e| e.to_string())
}

/// Removes an OAuth account: deletes both the Keychain tokens and the SQLite
/// metadata row. Best-effort on the Keychain (a missing entry is not an error).
#[tauri::command]
#[specta::specta]
pub async fn remove_oauth_account(
    app: AppHandle,
    db_cell: State<'_, DbCell>,
    token_store: State<'_, oauth::OAuthTokenStore>,
    id: String,
) -> Result<(), String> {
    token_store
        .delete_credential(&id)
        .map_err(|e| e.to_string())?;

    let conn = db_cell.get(&app).await?;
    let id_owned = id.clone();
    conn.call(move |conn: &mut Connection| -> rusqlite::Result<()> {
        conn.execute("DELETE FROM mcp_oauth_accounts WHERE id = ?1", [&id_owned])?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())
}

/// The shared insert for an oauth-linked HTTP MCP server row: stores the
/// `{"url","oauth_account_id"}` config shape (transport `http`, enabled) — NO
/// token is stored in the config; it is fetched (and refreshed) from the
/// Keychain on each connect. Used by both [`add_mcp_oauth_server`] and
/// [`add_google_workspace_servers`] so they share one insert path.
fn insert_mcp_oauth_server(
    conn: &mut Connection,
    name: &str,
    url: &str,
    oauth_account_id: &str,
    now: i64,
) -> rusqlite::Result<McpServerConnection> {
    let id = Uuid::now_v7().to_string();
    let config =
        serde_json::json!({ "url": url, "oauth_account_id": oauth_account_id }).to_string();
    conn.execute(
        "INSERT INTO mcp_server_connections (id, name, transport, config, enabled, created_at) VALUES (?1, ?2, 'http', ?3, 1, ?4)",
        rusqlite::params![id, name, config, now],
    )?;
    Ok(McpServerConnection {
        id,
        name: name.to_string(),
        transport: "http".to_string(),
        config,
        enabled: true,
        created_at: now,
    })
}

/// Registers a remote (streamable-HTTP) MCP server whose bearer token is
/// resolved from an OAuth account at connect time. Stores the
/// `{"url","oauth_account_id"}` config shape — NO token is stored in the
/// config; it is fetched (and refreshed) from the Keychain on each connect.
#[tauri::command]
#[specta::specta]
pub async fn add_mcp_oauth_server(
    app: AppHandle,
    db_cell: State<'_, DbCell>,
    name: String,
    url: String,
    oauth_account_id: String,
) -> Result<McpServerConnection, String> {
    let conn = db_cell.get(&app).await?;
    let now = now_ms();
    conn.call(
        move |conn: &mut Connection| -> rusqlite::Result<McpServerConnection> {
            insert_mcp_oauth_server(conn, &name, &url, &oauth_account_id, now)
        },
    )
    .await
    .map_err(|e| e.to_string())
}

/// A Google Workspace preset choice, for a UI to render the available hosted
/// servers. Read-only view over [`oauth::google_workspace::SERVICES`].
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GoogleWorkspaceServiceInfo {
    /// Stable key to pass back to [`add_google_workspace_servers`].
    pub key: String,
    /// Human-readable name (also what gets written as the server name).
    pub display_name: String,
    /// Google's documented hosted MCP endpoint (loopback acceptance UNVERIFIED).
    pub url: String,
    /// Least-privilege scopes this server needs.
    pub scopes: Vec<String>,
}

/// Lists the Google Workspace MCP server presets doce ships (Gmail, Calendar,
/// Drive — Keep has no hosted endpoint). Pure read over the static table; the
/// Connections UI calls this to render the choices.
#[tauri::command]
#[specta::specta]
pub fn list_google_workspace_services() -> Vec<GoogleWorkspaceServiceInfo> {
    oauth::google_workspace::SERVICES
        .iter()
        .map(|s| GoogleWorkspaceServiceInfo {
            key: s.key.to_string(),
            display_name: s.display_name.to_string(),
            url: s.url.to_string(),
            scopes: s.scopes.iter().map(|s| s.to_string()).collect(),
        })
        .collect()
}

/// Registers one or more Google Workspace hosted MCP servers for an existing
/// OAuth account, from the static preset. Each requested `service_keys` entry
/// is looked up in [`oauth::google_workspace`] and inserted via the SAME path
/// as [`add_mcp_oauth_server`] (`{"url","oauth_account_id"}`, transport `http`,
/// enabled). The preset `display_name` is used as the server name so the
/// matching curated skill lights up. All keys are validated up front — an
/// unknown key errors (naming the valid keys) BEFORE any row is written.
#[tauri::command]
#[specta::specta]
pub async fn add_google_workspace_servers(
    app: AppHandle,
    db_cell: State<'_, DbCell>,
    oauth_account_id: String,
    service_keys: Vec<String>,
) -> Result<Vec<McpServerConnection>, String> {
    // Resolve every key first so an unknown one fails before any insert.
    let mut resolved: Vec<(&'static str, &'static str)> = Vec::with_capacity(service_keys.len());
    for key in &service_keys {
        match oauth::google_workspace::lookup(key) {
            Some(svc) => resolved.push((svc.display_name, svc.url)),
            None => {
                return Err(format!(
                    "unknown Google Workspace service key {:?}; valid keys: {}",
                    key,
                    oauth::google_workspace::valid_keys().join(", ")
                ));
            }
        }
    }

    let conn = db_cell.get(&app).await?;
    let now = now_ms();
    conn.call(
        move |conn: &mut Connection| -> rusqlite::Result<Vec<McpServerConnection>> {
            let mut created = Vec::with_capacity(resolved.len());
            for (name, url) in resolved {
                created.push(insert_mcp_oauth_server(
                    conn,
                    name,
                    url,
                    &oauth_account_id,
                    now,
                )?);
            }
            Ok(created)
        },
    )
    .await
    .map_err(|e| e.to_string())
}
