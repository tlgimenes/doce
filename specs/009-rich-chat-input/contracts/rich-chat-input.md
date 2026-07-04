# Contract: Rich Chat Input

This feature modifies two existing Tauri commands (adds one optional
parameter to each) and introduces no new commands or events — skill
content resolution and model-text expansion happen entirely inside the
existing `send_agent_message`/`send_message` calls, and the native
file-picker button uses `@tauri-apps/plugin-dialog`'s `open()` directly
(already available, no wrapper command needed — same pattern
`FolderPicker.tsx` already uses for folder selection).

## `send_agent_message` (modified)

| Input | Output | Notes |
|---|---|---|
| `{ conversationId: string, content: string, richContent?: string }` | `string` (the assistant's final text) — unchanged | `richContent`, when present, is a JSON-serialized `RichMessageContent` (`data-model.md`). When present, the persisted row is `content_type='rich_text'` with `content=richContent` verbatim (not the flat `content` param); `content` in that case is only ever used as a UI-side fallback/plain-text mirror, not persisted twice. When absent, behavior is byte-for-byte what exists today. Errors (including a `skill` segment whose file can't be read — FR-014) surface as `Err(string)`, same as any other pre-inference failure this command already returns today. |

## `send_message` (modified, plain-chat path)

| Input | Output | Notes |
|---|---|---|
| `{ conversationId: string, content: string, richContent?: string }` | `SendMessageResult` — unchanged | Identical `richContent` handling to `send_agent_message`, including title generation now deriving from `expand_segments(..., expand_skills: false)` rather than the raw `content` string when `richContent` is present (data-model.md's Model-Text Expansion) — without this, a rich message's auto-generated title would show raw JSON. |

## `read_attached_file` (new command)

| Input | Output | Notes |
|---|---|---|
| `{ path: string }` | `{ data: string, mimeType: string, name: string }` on success, `Err(string)` otherwise | `data` is base64 (no `data:` prefix, matching the `attachment` segment's shape in data-model.md). `path` comes from `@tauri-apps/plugin-dialog`'s `open()` (image-filtered, `directory: false`) or a drag-and-drop/paste-derived `File`'s path. research.md: `@tauri-apps/plugin-fs` is not installed in this project — reading the selected file's bytes goes through this purpose-built command instead of adding a new plugin dependency and a broader filesystem-read capability grant than this feature needs. Requires no new entry in `capabilities/default.json` (it's a plain `#[tauri::command]`, not a plugin permission). |

## `list_skills` (existing, unmodified — reused as the picker's data source)

| Input | Output | Notes |
|---|---|---|
| `()` | `SkillSummary[]` (`{ name, description }`) | Already implemented (`001-doce-v1-core`). The "/" skill picker (FR-010) calls this directly — no new command needed. Selecting an item only needs the skill's `name` (`data-model.md`'s `skill` segment); content resolution happens backend-side, at send time, inside `expand_segments` — the frontend never reads a skill's file content directly. |

## `RichInput` component contract (frontend-internal, not IPC — documented here since it's this feature's central interface)

| Prop | Type | Notes |
|---|---|---|
| `onSubmit` | `(content: string, richContent?: RichMessageContent) => void` | Mirrors the two-parameter shape the IPC commands take, so each of the three composing surfaces (`EmptyState.tsx`, `Chat.tsx`, `Workspace.tsx`) just forwards these straight into its own existing `commands.sendAgentMessage`/`commands.sendMessage` call — no per-surface serialization logic. `richContent` is `undefined` for a plain-text-only message (data-model.md's "common case" — no JSON wrapper). |
| `skillsEnabled` | `boolean` | Gates whether typing "/" opens the skill picker (FR-010/FR-011). `true` for `EmptyState.tsx` and `Workspace.tsx` (agent-mode surfaces); `false` for `Chat.tsx` (plain conversations — per the existing constitution, no tool/skill access there). Paste-collapse and attachment chips are **not** gated by this prop — they're available on all three surfaces (spec.md's FR-003/FR-006 have no agent-mode restriction). |
| `disabled` | `boolean` | Toggled via Tiptap's `editor.setEditable()`, not remounting — matches the ref-based mutable-config pattern from `~/code/mesh` (research.md), preserving in-progress composition (open skill picker, cursor position, undo history) across a streaming/disabled transition exactly as the plain `<textarea>`'s existing `disabled` prop does today. |
| `placeholder` | `string` | Per-surface placeholder text — each of the three surfaces already has its own today ("What do you want to work on?" / "Message Doce…" / "Describe a task…"); unchanged by this feature. |

Every existing `data-testid` on the three composing surfaces (`empty-state-input`/`empty-state-submit`, `chat-input`/`chat-send`, `agent-input`) is preserved on `RichInput`'s outer container and submit button — e2e specs that already exercise these surfaces continue to work unchanged (matching the discipline already established for the `Button`/design-system migration in `008-shared-design-system`'s T019).

## `UserMessageContent` component contract (frontend-internal, read-only rendering path)

Parallel to `004-tool-call-widgets`' `MessageContent.tsx` → `ToolWidget` dispatch: `MessageContent.tsx` gains one more branch — a `content_type='rich_text'` user message dispatches to `UserMessageContent`, which parses `content` as `RichMessageContent` and renders each segment through the same chip components `RichInput`'s live editor uses (`PastedTextNode`/`AttachmentNode`/skill chip), mounted read-only (no editing, no expand-on-click for pasted text, no re-triggering a skill picker) — mirroring how `~/code/mesh`'s `message/user.tsx` mounts a second, `editable: false` Tiptap instance with the identical node extensions so the same chip visuals render non-interactively, rather than maintaining a second, hand-written rendering implementation that could drift from the live one.
