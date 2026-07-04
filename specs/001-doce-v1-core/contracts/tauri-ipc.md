# IPC Contract: Frontend ↔ Rust Backend

doce has no external network API in v1.0 (per constitution Principle II).
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
| `send_message` | `{ conversationId: string, content: string }` | `{ messageId: string, requestId: string }` (queued immediately; assistant reply streams via event once its turn starts) | FR-006, FR-009, FR-024, FR-025 |
| `cancel_generation` | `{ requestId: string }` | `{ ok: true }` | FR-028 |
| `answer_user_question` | `{ toolCallId: string, answer: string \| string[] }` | `{ ok: true }` | FR-010 — resolves a pending `AskUserQuestion` tool call, letting the agent's tool-use loop continue |
| `set_focused_conversation` | `{ conversationId: string \| null }` | `{ ok: true }` | FR-026 — called whenever the frontend's active view changes, so the scheduler knows which conversation (and any of its subagents) to prioritize |
| `list_conversations` | `{ workspaceId?: string }` | `Conversation[]` (each including the computed `title`/`status` fields, FR-011/FR-012) | FR-007 — excludes subagent-run conversations (`spawned_by_conversation_id` set) from the result |
| `open_workspace` | `{ path: string }` | `Workspace` | FR-008 |
| `list_workspaces` | — | `Workspace[]` | FR-008 |
| `add_mcp_server` | `{ name: string, transport: "stdio" \| "http", config: object }` | `MCPServerConnection` | FR-018 |
| `list_mcp_servers` | — | `MCPServerConnection[]` | FR-018 |
| `list_skills` | — | `{ name: string, description: string, source: "bundled" \| "user" }[]` | FR-019 |
| `get_settings` | — | `Record<string, unknown>` | FR-020 |
| `update_setting` | `{ key: string, value: unknown }` | `{ ok: true }` | FR-005, FR-020 |
| `search_conversations` | `{ query: string }` | `{ conversationId: string, title: string, snippet: string, matchedMessageId?: string }[]` (ranked via FTS5 `bm25()`, excerpt via `snippet()`) | FR-029, FR-030 |

All commands return a typed error variant (`{ error: { code: string, message: string } }`)
on failure rather than throwing opaque strings, so the frontend can render
clear, plain-language error messages rather than opaque failures.

doce v1.0 has no permission/approval IPC surface: agent-mode file and shell
actions execute directly (FR-013), with no prompt/response round-trip
gating them. The one exception is `AskUserQuestion` (FR-010) — that's the
agent explicitly choosing to pause and ask, not a system-imposed approval
gate on an action it would otherwise take unprompted.

## Events (`emit` / `listen`)

| Event | Payload | Purpose |
|---|---|---|
| `model-install-progress` | `{ modelId: string, bytesDownloaded: number, bytesTotal: number, state: string }` | Drives onboarding download progress UI (FR-003, SC-002) |
| `assistant-token` | `{ conversationId: string, messageId: string, token: string }` | Streaming chat/agent responses (FR-006, User Story 2) |
| `assistant-message-complete` | `{ conversationId: string, messageId: string }` | Marks a streamed message finalized and persisted |
| `agent-activity` | `{ conversationId: string, kind: "file-diff" \| "shell-output" \| "subagent-status", detail: object }` | Live workspace view during agent tasks (FR-017, User Story 3). The `subagent-status` kind carries `{ state: "running" \| "complete", label?: string }` — a coarse "delegating a sub-task" indicator only; a subagent's intermediate tool calls are never sent to the frontend (FR-015, SC-008) |
| `ask-user-question` | `{ conversationId: string, toolCallId: string, header: string, question: string, options: { label: string, description?: string }[], multiSelect: boolean }` | The agent's tool-use loop called `AskUserQuestion` and is paused awaiting a reply via `answer_user_question` (FR-010) |
| `generation-queue-update` | `{ requestId: string, conversationId: string, priority: "focused" \| "background", state: "queued" \| "running" \| "canceled", position?: number }` | Drives the queued/running indicator so a waiting conversation is visibly queued, not indistinguishable from frozen. `priority` reflects whether `conversationId` was the focused conversation at the time of this update, not a fixed property of the request; for a subagent's requests, `conversationId` here is the *spawning* conversation, matching `priority_conversation_id` in `research.md` §25 (FR-025, FR-026, User Story 5) |

## Out of scope for this contract

- No HTTP/REST API — there is no remote client of this app in v1.0.
- The MCP client (`rmcp`) contract itself is the standard, externally-defined
  Model Context Protocol, not something this project defines; doce is a
  protocol *consumer* on that side, configured via `add_mcp_server`/
  `list_mcp_servers` above.
- WhatsApp bridging has no contract here — deferred per constitution Principle V.
- No command lists, inspects, or manually resumes a subagent run in v1.0 —
  see `research.md` §25's "Out of scope for v1.0."
