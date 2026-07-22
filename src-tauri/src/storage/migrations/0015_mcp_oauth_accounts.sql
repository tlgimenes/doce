-- OAuth accounts an MCP server can be linked to. Holds ONLY non-secret
-- metadata for listing/displaying accounts; the access + refresh tokens live
-- in the macOS Keychain (keyed by this `id`), NEVER in this table nor in
-- `mcp_server_connections.config`.
CREATE TABLE mcp_oauth_accounts (
    id         TEXT PRIMARY KEY,
    provider   TEXT NOT NULL,
    client_id  TEXT NOT NULL,
    scopes     TEXT NOT NULL, -- JSON array of scope strings
    expires_at INTEGER NOT NULL, -- Unix-epoch ms of the access token's expiry
    created_at INTEGER NOT NULL
);
