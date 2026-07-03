# Data Model: Tool Call Widgets

No schema migration — `messages.content_type`/`messages.tool_name` already
exist (`001-doce-v1-core`). This document defines what actually goes into
`content` for `tool_call`/`tool_result` rows (currently unused in practice —
today only `'text'`/`'error'` rows are ever really written) and the new
Rust/TypeScript types that produce and consume it.

## `messages` row shapes (by `content_type`)

| `content_type` | `tool_name` | `content` (JSON) |
|---|---|---|
| `tool_call` | the tool's name | `{ "arguments": <raw tool_call arguments object, verbatim> }` |
| `tool_result` | same tool name as its paired `tool_call` | tool-specific `ToolResultDetail` (below) — self-sufficient, the widget renders from this row alone |

Both rows share the same `conversation_id` and are inserted back-to-back
(`tool_call` at sequence *n*, `tool_result` at sequence *n+1*) by the same
synchronous dispatch call (research.md § 5). The frontend renders the
widget from the `tool_result` row; the `tool_call` row renders nothing
standalone (research.md § 2/§ 5) except in the one degenerate case where a
`tool_call` exists with no following `tool_result` at all (e.g. the app
quit mid-call) — that renders as a generic "interrupted" fallback rather
than nothing.

## `ToolResultDetail` — one shape per tool, tagged by `tool_name`

Rust: `#[derive(Serialize, Deserialize, specta::Type)] #[serde(tag = "toolName", rename_all = "camelCase")]` enum in `src-tauri/src/agent/dispatch.rs`, serialized into `tool_result.content`. TypeScript: the mirrored discriminated union in `src/lib/ipc.ts` (or generated via `tauri-specta` like the rest of this project's IPC types — see `contracts/`).

### `Read`
```jsonc
{ "toolName": "Read", "filePath": "string", "offset": "number | null", "limit": "number | null", "outcome": { "ok": true, "content": "string", "truncated": "boolean" } | { "ok": false, "error": "string" } }
```
Widget: compact file-reference card — path (+ offset/limit if set), a
short content preview, truncated indicator if `truncated` (FR-004/FR-005).

### `Write`
```jsonc
{ "toolName": "Write", "filePath": "string", "contentPreview": "string", "byteCount": "number", "outcome": { "ok": true } | { "ok": false, "error": "string" } }
```
Widget: file-reference card visually distinct from `Read` and `Edit`
(FR-006) — path, size, a short preview.

### `Edit`
```jsonc
{ "toolName": "Edit", "filePath": "string", "oldString": "string", "newString": "string", "replaceAll": "boolean", "outcome": { "ok": true } | { "ok": false, "error": "string" } }
```
Widget: real diff (FR-002) — `diffLines(oldString, newString)` computed
client-side (research.md § 6); on `ok: false` (e.g. `old_string` not
found), shows a failed-edit state instead of an empty/misleading diff
(spec.md's Acceptance Scenario 2).

### `Bash`
```jsonc
{ "toolName": "Bash", "command": "string", "timeoutMs": "number | null", "outcome": { "ok": true, "exitCode": "number", "stdout": "string", "stderr": "string" } | { "ok": false, "error": "string" } }
```
Widget: terminal-style block (FR-003) — command, stdout/stderr visually
separated, success/failure from `exitCode == 0`, truncated/collapsible
past a length threshold (FR-004).

### `Glob` / `Grep`
```jsonc
// Glob
{ "toolName": "Glob", "pattern": "string", "path": "string", "matches": "string[]" }
// Grep
{ "toolName": "Grep", "pattern": "string", "path": "string", "glob": "string | null", "matches": [{ "path": "string", "lineNumber": "number", "line": "string" }] }
```
Widget: match-list (FR-007), truncated/collapsible past a length threshold
(FR-004) — these two never fail at the dispatch level today (an empty
result is a valid, non-error outcome: "No files matched"/"No matches
found"), so no `outcome`/`ok` wrapper is needed.

### `Task`
```jsonc
{ "toolName": "Task", "prompt": "string", "subagentConversationId": "string", "state": "running" | "complete" }
```
Widget: running/complete status only (FR-010) — never the subagent's own
tool calls (those persist under `subagentConversationId`'s own conversation
row, already isolated from the parent per `001`'s existing FR-015/SC-008,
unchanged by this feature). `state` is always `"complete"` by the time the
frontend can observe it in this pass (research.md § 2) — the field exists
for a future live pass, not exercised as `"running"` yet.

### `AskUserQuestion`
```jsonc
{
  "toolName": "AskUserQuestion",
  "questionId": "string",
  "header": "string",
  "question": "string",
  "options": [{ "label": "string", "description": "string" }],
  "multiSelect": "boolean",
  "answer": "string[] | null"
}
```
Widget: interactive prompt while `answer == null` (FR-008); once answered,
`answer_user_question` updates this same row's `content` in place (setting
`answer`), and the widget switches to a read-only "you chose: …" state
that no longer accepts input (FR-009). A stale/already-answered
`questionId` submitted again is rejected by `PendingQuestions::answer`
returning `false` (already unit-tested) — the command surfaces that as an
error rather than silently no-op-ing.

### Fallback (any tool without a dedicated shape above)
```jsonc
{ "toolName": "string", "arguments": "object", "outcome": { "ok": "boolean", "text": "string" } }
```
Widget: tool name + a readable rendering of its input/output (FR-011) —
never blank, broken, or silently dropped, including for a completely
unrecognized `toolName` (SC-004).

## Backend types (`src-tauri/src/agent/dispatch.rs`)

```rust
pub struct ToolOutcome {
    pub model_text: String,      // unchanged — fed back into the conversation, exactly today's format
    pub detail: serde_json::Value, // one of the shapes above, tagged by "toolName"
}
```
`execute(call, cwd) -> ToolOutcome` (was `-> String`) — every match arm
builds both fields from the same already-available data (arguments +
whatever the underlying `fs`/`bash`/`search` call returned), per
research.md § 4.

## Frontend types (`src/lib/ipc.ts`)

`Message.content` stays `string` (the DB column is `TEXT`) — for
`contentType === "tool_call" | "tool_result"`, callers `JSON.parse` it into
one of the `ToolResultDetail` shapes above (discriminated on `toolName`,
matching `message.toolName` redundantly for a fast dispatch without
parsing when only routing, not rendering, is needed).

## Validation rules

- A `tool_result` row's `content` MUST always parse as valid JSON matching
  its `tool_name`'s shape — enforced by construction (only
  `dispatch::execute`/the `AskUserQuestion` handler ever write one), not by
  a runtime check on read; a `JSON.parse` failure on the frontend degrades
  to the fallback widget rather than crashing the message list (defensive,
  since this is user-visible history, not a hot path).
- `AskUserQuestion`'s `answer` MUST NOT be set by anything other than
  `answer_user_question` succeeding against a still-pending `questionId`
  (FR-009's "no second answer" guarantee is enforced by
  `PendingQuestions::answer`'s existing one-shot-consume semantics, already
  unit-tested — this feature doesn't add new locking, it wires up what's
  there).
