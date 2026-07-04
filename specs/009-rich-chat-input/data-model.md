# Data Model: Rich Chat Input

## Overview

This feature introduces one new persisted shape — a user message's rich content — plus the derivation logic that turns it into what the model actually sees. Everything else (conversations, plain-text messages, tool messages) is untouched.

## `RichMessageContent`

The structured content of a user message authored via the new input, when it contains at least one non-plain-text segment. Serialized as JSON and stored in `messages.content` when `messages.content_type = 'rich_text'`.

A message with **only** plain text (no pasted-text chip, no attachment, no skill marker) is **not** wrapped in this shape — it persists exactly as today, `content_type = 'text'`, `content` = the raw string. `RichMessageContent` exists only for messages that actually need it, so the common case (a short typed message) has zero storage or behavioral change from what exists today.

```ts
export interface RichMessageContent {
  segments: RichTextSegment[];
}

export type RichTextSegment =
  | { type: "text"; text: string }
  | { type: "pastedText"; id: string; text: string; lineCount: number }
  | { type: "attachment"; id: string; name: string; mimeType: string; data: string; isImage: boolean }
  | { type: "skill"; id: string; name: string };
```

- **Order is meaningful**: `segments` preserves the message exactly as authored — a skill marker typed mid-sentence stays mid-sentence when expanded for the model (FR-012's "at the point of selection").
- **`text`**: an ordinary run of typed characters. Consecutive typing between chips.
- **`pastedText`**: FR-003's collapsed-paste chip. `text` is the **full, uncollapsed** original paste (FR-005 — the model always sees the whole thing; `lineCount` is display-only, computed once at paste time via `text.split("\n").length`.
- **`attachment`**: FR-006/FR-007's image/file chip. `data` is base64 (no `data:` prefix, matching the local-only, never-sent-to-model design in FR-009 — `data` is used for local rendering/hover-preview only and is never part of any model-facing text). `isImage` drives the hover-preview vs. plain-filename rendering (FR-007 vs FR-008).
- **`skill`**: FR-010's "/" mention. Deliberately carries only `name`, not the skill's content — content is resolved fresh from disk at the point of use (see Model-Text Expansion below), matching FR-014's "can no longer be read **at send time**" language, which commits to resolve-at-use rather than resolve-and-snapshot-at-selection. A side effect worth being explicit about: replaying an old conversation's history re-reads the skill file as it exists **now**, not as it was when originally sent — if the user has since edited that skill, a resent/continued conversation sees the edited version. This is a deliberate, minimal design (skills are local files the user controls; the freshest version is more useful than a silently stale one) and is recorded here as an accepted consequence, not an oversight.
- **Every non-`text` variant carries an `id`** (`crypto.randomUUID()`, matching `~/code/mesh`'s convention) — a stable React key and click-target identity for the expand/hover/remove interactions each chip type needs.

## Model-Text Expansion

Two representations are derived from `RichMessageContent`, both computed by one function (single source of truth, avoids the two ever drifting):

```rust
fn expand_segments(segments: &[RichTextSegment], skills_dir: &Path, expand_skills: bool) -> Result<String, String>
```

- **`expand_skills = true`** — "what the model sees" (FR-013). Used when this turn is actually sent to inference (`send_agent_message`/`send_message`, and `load_history` replaying **every prior** `rich_text` row into context on every subsequent turn — not just the newest one).
  - `text`, `pastedText` → their text, verbatim, in place.
  - `attachment` → `[attached image: {name}]` or `[attached file: {name}]` per `isImage` (FR-009 — bytes never included, for images **or** non-image attachments; a non-image file's actual text content is deliberately out of scope for this pass — the paste-collapse path already covers "I want the agent to see this text," so a plain filename placeholder for both attachment kinds is the consistent, contamination-safe default. Revisit only if a real need for the agent to read an attached file's content emerges, as a distinct, deliberate feature.).
  - `skill` → read `{skills_dir}/{name}/SKILL.md` (matching `skills::discover_skills`'s existing directory convention) and inline its content wrapped as `\n<skill name="{name}">\n{content}\n</skill>\n`. Read failure → `Err` (FR-014 — the caller surfaces this as the turn's send error, same as any other pre-inference failure today).
- **`expand_skills = false`** — "display/title text." Used only by `generate_title` (`storage/conversations.rs`), which today calls `generate_title(&content)` directly on the raw message string; that call must not receive a `rich_text` row's raw JSON verbatim (a title built from `{"segments":[{"type":...` would be visibly broken) nor the fully-expanded model text (a title built from an entire injected skill file would be nonsensical and enormous). In this mode, `skill` renders as the literal marker text `/{name}` instead of its file content; `text`/`pastedText`/`attachment` render identically to the `true` case. `generate_title` then truncates this exactly as it already does for plain messages today — no change to `generate_title` itself, only to what's passed into it.

Both modes share the same `attachment`/`text`/`pastedText` handling — only the `skill` variant's behavior differs. This keeps the two call sites (send-time and title-generation) from silently diverging as the segment types evolve.

## Persistence

### `messages.content_type` — extended

Adds `'rich_text'` to the existing `CHECK (content_type IN ('text', 'tool_call', 'tool_result', 'error'))` constraint on `messages` (`storage/migrations/0001_init.sql`). SQLite cannot alter a `CHECK` constraint in place, so this requires a table-rebuild migration (`0003_rich_text_content_type.sql`):

1. Create `messages_new` with the widened constraint (schema otherwise identical, including the existing `duration_ms` column from `0002_message_duration.sql`).
2. `INSERT INTO messages_new (rowid, ...) SELECT rowid, ... FROM messages` — **rowid must be copied explicitly**, not left to auto-assign, because `messages_fts` is an external-content FTS5 table keyed on `content_rowid='messages'`; a rowid renumbering would silently desync existing search results from their source rows.
3. `DROP TABLE messages` (this also drops the `messages_ai`/`messages_ad`/`messages_au` triggers defined `ON messages` — they do not survive a rename and must be recreated).
4. `ALTER TABLE messages_new RENAME TO messages`.
5. Recreate `idx_messages_conversation_sequence` and the three FTS5 sync triggers, verbatim from `0001_init.sql`.
6. No `messages_fts` rebuild needed — its shadow tables are untouched by dropping/recreating the content table by the same name with the same rowids; only the sync triggers (which govern **future** writes) need restoring.

### `messages.content` for a `rich_text` row

`JSON.stringify({ segments })` — matching the existing, established convention from `004-tool-call-widgets`, where `tool_result` rows already store a JSON blob discriminated by `content_type`. `messages_fts` will index this raw JSON verbatim, same imperfection `tool_call`/`tool_result` rows already have today (a search for a word inside a pasted-text chip won't match as cleanly as it would for plain text) — an accepted, pre-existing-pattern limitation, not a regression this feature introduces, and out of scope to fix here.

### IPC surface (both `send_agent_message` and `send_message`)

Both commands gain one new parameter:

```rust
rich_content: Option<String>,  // JSON-serialized RichMessageContent; None for a plain-text-only message
```

- `None` — today's exact behavior, unchanged: `content_type='text'`, `content` = the flat string, no expansion step.
- `Some(json)` — persist `content_type='rich_text'`, `content=json`; call `expand_segments(..., expand_skills: true)` to produce the text actually fed to `ChatMessage::user(...)` for this turn; call `expand_segments(..., expand_skills: false)` in place of the raw string wherever `generate_title` is invoked.

`send_message` (plain-chat path) receives the identical parameter and expansion treatment as `send_agent_message`, even though its composing surface (`Chat.tsx`) never opens the skill picker (FR-011) — a plain conversation's rich content can still contain `pastedText`/`attachment` segments (those aren't agent-mode-gated), and reusing one `expand_segments` implementation for both call sites means a stray `skill` segment (which the UI never produces there, but nothing prevents defensively) behaves identically rather than being a second, subtly different code path.

### `load_history` (`storage/conversations.rs`)

Currently maps every non-`assistant`, non-`error` row straight to `ChatMessage::user(content)`. Extended to check `content_type`: a `'rich_text'` row is parsed as `RichMessageContent` and passed through `expand_segments(..., expand_skills: true)` before becoming a `ChatMessage`; every other `content_type` is handled exactly as today. This is what makes skill/paste/attachment content available not just on the turn it was sent, but on every subsequent turn that replays this conversation's history — without it, a skill selected three turns ago would silently stop influencing the agent the moment a new message is sent.

## Frontend Types (`src/lib/ipc.ts`)

Mirrors the Rust shape exactly, following the file's own established convention (see `ReadDetail`/`WriteDetail`/etc. from `004-tool-call-widgets`):

```ts
export interface RichTextSegmentText { type: "text"; text: string }
export interface RichTextSegmentPastedText { type: "pastedText"; id: string; text: string; lineCount: number }
export interface RichTextSegmentAttachment {
  type: "attachment"; id: string; name: string; mimeType: string; data: string; isImage: boolean;
}
export interface RichTextSegmentSkill { type: "skill"; id: string; name: string }

export type RichTextSegment =
  | RichTextSegmentText
  | RichTextSegmentPastedText
  | RichTextSegmentAttachment
  | RichTextSegmentSkill;

export interface RichMessageContent {
  segments: RichTextSegment[];
}
```

`UserMessageContent.tsx` parses a `content_type='rich_text'` message's `content` (`JSON.parse` → `RichMessageContent`) the same defensively-degrading way `parseToolResultDetail` already does for tool messages — a parse failure or unrecognized segment `type` falls back to rendering the raw string rather than throwing into the message list.

## Validation Rules

- A `pastedText` segment is only created client-side when a paste crosses the ~10-line/~500-char threshold (FR-003) — below it, the pasted text becomes an ordinary `text` segment (or is merged into an adjacent one). Not a persisted-side validation; enforced entirely by the editor's paste handler.
- `expand_segments` returns `Err` (not a partially-expanded string) if any `skill` segment's file can't be read — the caller must not silently drop a broken skill reference and send an incomplete turn (FR-014).
- `RichMessageContent.segments` must not be empty — a message with zero segments has nothing to send; the existing empty/whitespace-only submit guard (already present in all three composing surfaces) covers this before a `rich_content` payload is ever constructed.
