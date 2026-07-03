# Phase 1 Data Model: Doce v1.0 — Zero-Config Local Personal Agent

Storage: local SQLite (`rusqlite`/`tokio-rusqlite`), one file under the
app's local data directory. All entities below are local-only per
constitution Principle II (no entity in this model is synced or
transmitted off-device by default).

## Schema conventions

- **IDs**: every primary key below is a UUIDv7, stored as `TEXT`. UUIDv7
  embeds a timestamp prefix, so inserts land roughly in order in each
  table's B-tree — unlike random UUIDv4, which would scatter inserts
  across the tree and degrade over time on the highest-volume table
  (`Message`, which grows with chat history). This gets most of the
  insert-locality benefit of an `INTEGER PRIMARY KEY` (rowid) while
  keeping one ID format usable everywhere, including across the IPC
  boundary (`contracts/tauri-ipc.md`'s `conversationId`, `requestId`, etc.).
- **Timestamps**: `INTEGER`, Unix epoch milliseconds. Compact, sorts
  correctly with a plain numeric comparison, trivial to convert in Rust.
- **Connection setup**: `PRAGMA journal_mode = WAL` (better commit/crash
  behavior for a desktop app, independent of the single-connection design
  in `research.md` §4) and `PRAGMA foreign_keys = ON` (off by default in
  SQLite for backward compatibility — every FK relationship described
  below depends on this being set explicitly at connection-open time, not
  assumed).
- **Migrations**: tracked via SQLite's built-in `PRAGMA user_version`
  (no hand-rolled tracking table needed), applied from numbered `.sql`
  files (`0001_init.sql`, `0002_...`) in a transaction at startup, per the
  migration runner decision in `research.md` §4.

## Workspace

A folder the user has opened for agent mode — the working project context
for that agent session. Not a permission or security boundary in v1.0 (see
constitution Principle V): agent actions are not restricted to it.

| Field | Type | Notes |
|---|---|---|
| `id` | UUID (text) | Primary key |
| `path` | text | Absolute filesystem path; `UNIQUE` constraint (enforces the "reuse existing row" validation rule below at the database level, not just in application code) |
| `display_name` | text | Derived from folder name, editable |
| `created_at` | integer (unix ms) | First-open time |
| `last_opened_at` | integer (unix ms) | Updated on each open |

**Relationships**: one `Workspace` has many `Conversation`s (agent-mode
conversations are tied to a workspace; chat-only conversations have
`workspace_id = NULL`).

**Validation rules**: `path` must exist and be a directory at open time;
opening a path already registered reuses the existing row (updates
`last_opened_at`) rather than creating a duplicate.

## Conversation

A chat thread — either standalone chat (`workspace_id = NULL`), an
agent-mode session tied to a workspace, or an isolated subagent run
spawned by another conversation's agent loop (`spawned_by_conversation_id`
set). A subagent-run row uses the same schema as any other conversation
(it's the same tool-use loop, just with a fresh, isolated context) —
subagents are not a separate entity type.

| Field | Type | Notes |
|---|---|---|
| `id` | UUID (text) | Primary key |
| `workspace_id` | UUID (text), nullable | FK → `Workspace.id`; NULL for chat-only |
| `spawned_by_conversation_id` | UUID (text), nullable | FK → `Conversation.id`; set only for a subagent run, pointing at whichever conversation spawned it. NULL for user-facing conversations. Never chained more than one level deep (FR-016). |
| `title` | text | The user's first message, truncated to a fixed max length at a word boundary (FR-012) — no model inference involved; editable afterward |
| `created_at` | integer (unix ms) | |
| `updated_at` | integer (unix ms) | Bumped on each new message |

**Validation rules**: a row with `spawned_by_conversation_id` set MUST be
excluded from `list_conversations`' default result (it is not user-facing)
but remains queryable/persisted like any other conversation. The schema
does not itself prevent chaining `spawned_by_conversation_id` more than
one level deep; the one-level nesting cap (FR-016) is enforced by the
agent orchestrator refusing to expose the subagent-spawning tool to a
conversation that is itself a subagent run, not by a database constraint.

**Relationships**: one `Conversation` has many `Message`s.

**Computed field (not a column)**: `status` — `done` \| `requires_action`
\| `failed` \| `in_progress` (FR-011). Computed at query time, never
persisted:
1. If a `Generation Request` is currently active or queued for this
   conversation (checked against the in-memory scheduler, not this table)
   → `in_progress`.
2. Else, look at the conversation's latest **assistant-authored** `Message`
   (never the user's own last message, which could itself end in "?" and
   otherwise be misread as the assistant asking something): if
   `content_type = 'error'` → `failed`.
3. Else, if that message has `content_type = 'tool_call'` and
   `tool_name = 'AskUserQuestion'`, or its last text segment (for
   `content_type = 'text'`) ends in a `?` outside any `https?://\S+`
   match → `requires_action`.
4. Else → `done`.

This is computed in the `list_conversations`/`get_conversation` command
handlers, not cached in a column, so it can never drift out of sync with
the state it describes.

## Message

An individual turn within a conversation.

| Field | Type | Notes |
|---|---|---|
| `id` | UUID (text) | Primary key |
| `conversation_id` | UUID (text) | FK → `Conversation.id` |
| `role` | text | `user` \| `assistant` \| `tool` |
| `content_type` | text | `text` \| `tool_call` \| `tool_result` \| `error` — discriminates what shape `content` holds, so the frontend renderer doesn't have to sniff it. `error` marks a tool execution or generation outcome that failed unrecoverably — this is what `status` computation's `failed` case (FR-011) checks for directly, rather than inferring "an error outcome" from ambiguous heuristics on `tool_result` content |
| `content` | text | Markdown/plain text (when `content_type = 'text'`); structured JSON (when `content_type` is `tool_call`/`tool_result`/`error`) |
| `tool_name` | text, nullable | Set only when `content_type = 'tool_call'` (e.g. `Read`, `Bash`, `AskUserQuestion`). A denormalized copy of the tool name that's already inside the `content` JSON — kept as its own column so the `status` computation's "does this end in an `AskUserQuestion` call" check (FR-011) is a plain indexed comparison, not a per-row JSON parse. |
| `created_at` | integer (unix ms) | |
| `sequence` | integer | Monotonic per-conversation ordering (streaming appends update the same row until finalized) |

**State transitions**: `assistant` messages are created in a `streaming`
sub-state (not persisted as separate rows — represented in-memory and
flushed to SQLite on stream completion or app-close checkpoint) then
finalized; this spec does not require partial-stream durability beyond
periodic checkpointing.

**Indexes**: `(conversation_id, sequence)` — the hot-path query is "load a
conversation's messages in order."

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
| `installed_at` | integer (unix ms), nullable | NULL while download is in progress |
| `is_active` | boolean | Exactly one model is active at a time in v1.0 |

**Relationships**: referenced by `Conversation` indirectly (the active model
at send-time is not stored per-message in v1.0; only the currently active
model matters for generation).

**Validation rules**: a `Model` row is only marked `installed_at` after
SHA-256 verification of the downloaded file succeeds (FR-003). The
"exactly one active model" invariant is enforced by the database itself
via a partial unique index — `CREATE UNIQUE INDEX ... ON models(is_active)
WHERE is_active = 1` — rather than trusted to application code alone.

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
| `created_at` | integer (unix ms) | |

**Relationships**: tools exposed by an enabled `MCPServerConnection` are
surfaced to the agent orchestrator's tool-use loop at runtime; not
persisted per-message.

## Settings

Single-row key-value table for app-level settings (telemetry opt-in state
— always `false` by default per Principle II, MCP/skills preferences,
etc.). Does **not** duplicate the active-model selection: `Model.is_active`
(below) is the single source of truth for which model is in use, enforced
by a partial unique index — an earlier draft of this document ambiguously
described Settings as also holding a "model override choice," which would
have created two places the same fact could disagree after
`set_active_model` runs. `Settings` never stores anything about which
model is active.

| Field | Type | Notes |
|---|---|---|
| `key` | text | Primary key |
| `value` | text (JSON) | |
| `updated_at` | integer (unix ms) | |

## Search (FTS5 virtual tables, FR-029/FR-030)

Not a persisted entity of its own — a search index derived from
`Conversation` and `Message`, kept in sync via triggers rather than
duplicating data. Two SQLite FTS5 external-content tables:

```sql
CREATE VIRTUAL TABLE messages_fts USING fts5(
  content, content='messages', content_rowid='rowid'
);
CREATE VIRTUAL TABLE conversations_fts USING fts5(
  title, content='conversations', content_rowid='rowid'
);
```

`content_rowid` references each source table's implicit SQLite rowid
(present as long as `messages`/`conversations` are not declared `WITHOUT
ROWID`) — this is the standard FTS5 external-content pattern: the FTS
index stores only the searchable text, the source tables remain the
single source of truth, and `INSERT`/`UPDATE`/`DELETE` triggers on
`messages`/`conversations` keep both FTS tables synchronized.

**Validation rules**: the sync triggers MUST exclude rows where the
owning conversation has `spawned_by_conversation_id IS NOT NULL` — a
subagent run's messages are never indexed, so they can never surface in a
search result (FR-030, SC-009), matching the same isolation boundary
already enforced on `list_conversations`.

**Query shape**: rank via FTS5's built-in `bm25()` function; generate the
highlighted excerpt via `snippet()` — both are SQLite built-ins, no custom
ranking/highlighting logic needed. A search spans both tables (title
matches and content matches), with results merged and ranked per
conversation before returning.
