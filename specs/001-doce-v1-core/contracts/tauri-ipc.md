# IPC Contract: Frontend ↔ Rust Backend

Doce has no external network API in v1.0 (per constitution Principle II).
The only interface contract is the Tauri IPC boundary between the React
frontend and the Rust backend: `invoke` commands for request/response calls,
and `emit`/`listen` events for backend→frontend streaming. This contract is
what `tests/frontend` and Tauri e2e tests validate against.

## Commands (`invoke`)

| Command | Request | Response | Maps to |
|---|---|---|---|
| `get_hardware_profile` | — | `{ tier: string, ramGb: number, chip: string, diskFreeGb: number }` | FR-001 |
| `start_model_install` | `{ modelId?: string }` (omitted = auto-match tier) | `{ modelId: string, resumed: boolean }` | FR-002, FR-003 |
| `get_model_install_status` | `{ modelId: string }` | `{ state: "downloading" \| "verifying" \| "installed" \| "failed", bytesDownloaded: number, bytesTotal: number }` | FR-003 |
| `list_models` | — | `Model[]` | FR-005 |
| `set_active_model` | `{ modelId: string }` | `{ ok: true }` | FR-005 |
| `create_conversation` | `{ workspaceId?: string }` | `Conversation` | FR-006, FR-008 |
| `send_message` | `{ conversationId: string, content: string }` | `{ messageId: string }` (assistant reply streams via event) | FR-006, FR-009 |
| `list_conversations` | `{ workspaceId?: string }` | `Conversation[]` | FR-007 |
| `open_workspace` | `{ path: string }` | `Workspace` | FR-008 |
| `list_workspaces` | — | `Workspace[]` | FR-008 |
| `respond_to_permission_prompt` | `{ promptId: string, decision: "allow-once" \| "allow-always" \| "deny" }` | `{ ok: true }` | FR-012, FR-013 |
| `list_permission_grants` | `{ workspaceId: string }` | `PermissionGrant[]` | FR-014 |
| `add_mcp_server` | `{ name: string, transport: "stdio" \| "http", config: object }` | `MCPServerConnection` | FR-015 |
| `list_mcp_servers` | — | `MCPServerConnection[]` | FR-015 |
| `list_skills` | — | `{ name: string, description: string, source: "bundled" \| "user" }[]` | FR-016 |
| `get_settings` | — | `Record<string, unknown>` | FR-017 |
| `update_setting` | `{ key: string, value: unknown }` | `{ ok: true }` | FR-005, FR-017 |

All commands return a typed error variant (`{ error: { code: string, message: string } }`)
on failure rather than throwing opaque strings, so the frontend can render
plain-language messages consistent with Principle IV's approval-prompt
language requirement.

## Events (`emit` / `listen`)

| Event | Payload | Purpose |
|---|---|---|
| `model-install-progress` | `{ modelId: string, bytesDownloaded: number, bytesTotal: number, state: string }` | Drives onboarding download progress UI (FR-003, SC-002) |
| `assistant-token` | `{ conversationId: string, messageId: string, token: string }` | Streaming chat/agent responses (FR-006, User Story 2) |
| `assistant-message-complete` | `{ conversationId: string, messageId: string }` | Marks a streamed message finalized and persisted |
| `agent-activity` | `{ conversationId: string, kind: "file-diff" \| "shell-output", detail: object }` | Live workspace view during agent tasks (FR-011, User Story 3) |
| `permission-prompt` | `{ promptId: string, workspaceId: string, actionKind: string, description: string }` | Triggers the plain-language approval UI (FR-012) |
| `permission-grant-updated` | `{ workspaceId: string, actionKind: string, scope: "always" }` | Confirms persisted "always allow" state (FR-013) |

## Out of scope for this contract

- No HTTP/REST API — there is no remote client of this app in v1.0.
- The MCP client (`rmcp`) contract itself is the standard, externally-defined
  Model Context Protocol, not something this project defines; Doce is a
  protocol *consumer* on that side, configured via `add_mcp_server`/
  `list_mcp_servers` above.
- WhatsApp bridging has no contract here — deferred per constitution Principle VI.
