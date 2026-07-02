# Phase 1 Data Model: Doce v1.0 — Zero-Config Local Personal Agent

Storage: local SQLite (`rusqlite`), one file under the app's local data
directory. All entities below are local-only per constitution Principle II
(no entity in this model is synced or transmitted off-device by default).

## Workspace

A folder the user has opened for agent mode. Owns its own permission state.

| Field | Type | Notes |
|---|---|---|
| `id` | UUID (text) | Primary key |
| `path` | text | Absolute filesystem path; unique |
| `display_name` | text | Derived from folder name, editable |
| `created_at` | timestamp | First-open time |
| `last_opened_at` | timestamp | Updated on each open |

**Relationships**: one `Workspace` has many `PermissionGrant`s and many
`Conversation`s (agent-mode conversations are scoped to a workspace; chat-only
conversations have `workspace_id = NULL`).

**Validation rules**: `path` must exist and be a directory at grant/open
time; opening a path already registered reuses the existing row (updates
`last_opened_at`) rather than creating a duplicate.

## Conversation

A chat thread — either standalone chat (`workspace_id = NULL`) or an
agent-mode session tied to a workspace.

| Field | Type | Notes |
|---|---|---|
| `id` | UUID (text) | Primary key |
| `workspace_id` | UUID (text), nullable | FK → `Workspace.id`; NULL for chat-only |
| `title` | text | Derived from first message, editable |
| `created_at` | timestamp | |
| `updated_at` | timestamp | Bumped on each new message |

**Relationships**: one `Conversation` has many `Message`s.

## Message

An individual turn within a conversation.

| Field | Type | Notes |
|---|---|---|
| `id` | UUID (text) | Primary key |
| `conversation_id` | UUID (text) | FK → `Conversation.id` |
| `role` | text | `user` \| `assistant` \| `tool` |
| `content` | text | Markdown/plain text; tool calls/results serialized as structured JSON in this field |
| `created_at` | timestamp | |
| `sequence` | integer | Monotonic per-conversation ordering (streaming appends update the same row until finalized) |

**State transitions**: `assistant` messages are created in a `streaming`
sub-state (not persisted as separate rows — represented in-memory and
flushed to SQLite on stream completion or app-close checkpoint) then
finalized; this spec does not require partial-stream durability beyond
periodic checkpointing.

## Model

A local inference model matched to a hardware tier.

| Field | Type | Notes |
|---|---|---|
| `id` | text | Stable model identifier from the registry (e.g. source repo + quantization) |
| `hardware_tier` | text | Tier key this model was matched to at install time |
| `source_url` | text | Hugging Face source |
| `quantization` | text | e.g. `Q4_K_M` |
| `sha256` | text | Expected checksum, from registry |
| `local_path` | text | Filesystem path once installed |
| `capability_tags` | text (JSON array) | e.g. `["tool-calling", "coding-focused"]` |
| `installed_at` | timestamp, nullable | NULL while download is in progress |
| `is_active` | boolean | Exactly one model is active at a time in v1.0 |

**Relationships**: referenced by `Conversation` indirectly (the active model
at send-time is not stored per-message in v1.0; only the currently active
model matters for generation).

**Validation rules**: a `Model` row is only marked `installed_at` after
SHA-256 verification of the downloaded file succeeds (FR-003).

## PermissionGrant

A persisted trust decision for an action kind within a specific workspace.

| Field | Type | Notes |
|---|---|---|
| `id` | UUID (text) | Primary key |
| `workspace_id` | UUID (text) | FK → `Workspace.id` |
| `action_kind` | text | e.g. `shell:git`, `fs:write-outside-workspace` (taxonomy defined during implementation) |
| `scope` | text | `always` \| `once` (once-grants are not persisted as rows past the single approved action) |
| `granted_at` | timestamp | |
| `source` | text | `local-chat` \| `bridged-channel` — retained for the stricter bridged-channel bar even though no bridge ships in v1.0 |

**Validation rules**: a grant with `scope = 'always'` MUST be checked before
prompting again for the same `(workspace_id, action_kind)` pair (FR-013);
grants MUST NOT be looked up across different `workspace_id`s (FR-014).

## Skill

A filesystem-based capability pack (bundled default or user-added). Not
itself a SQLite entity — skills are discovered from disk (bundled skills
directory + a user skills directory) at agent-loop time, matching the
`SKILL.md`-style convention already used by `.claude/skills` in this
repository. A lightweight in-memory index (name, description, trigger
keywords) is built at startup for contextual matching; no persistence needed
beyond the files themselves.

## MCPServerConnection

A user-configured external MCP server.

| Field | Type | Notes |
|---|---|---|
| `id` | UUID (text) | Primary key |
| `name` | text | User-facing label |
| `transport` | text | e.g. `stdio` \| `http` (per `rmcp` transport support) |
| `config` | text (JSON) | Transport-specific connection config (command/args, or URL) |
| `enabled` | boolean | |
| `created_at` | timestamp | |

**Relationships**: tools exposed by an enabled `MCPServerConnection` are
surfaced to the agent orchestrator's tool-use loop at runtime; not
persisted per-message.

## Settings

Single-row key-value table for app-level settings (model override choice,
telemetry opt-in state — always `false` by default per Principle II, etc.).

| Field | Type | Notes |
|---|---|---|
| `key` | text | Primary key |
| `value` | text (JSON) | |
| `updated_at` | timestamp | |
