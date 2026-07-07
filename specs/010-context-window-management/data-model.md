# Data Model: Context Window Management for Chat and Agent Mode

## Overview

This feature introduces no new SQLite tables — it widens the existing `messages.content_type` CHECK constraint by one variant and adds four new rows to the existing `settings` key/value table. All other state (live usage, compaction state) is computed on demand from already-persisted data, never stored separately, per `research.md`'s "token accounting is always recomputed" decision.

## `ContextUsage` (new, Rust + TS type, not persisted)

Represents a point-in-time snapshot of how full a conversation's effective prompt is. Always computed fresh (never cached) by rendering the conversation's effective history + system prompt through the model's chat template and counting tokens.

```rust
pub struct ContextUsage {
    pub conversation_id: String,
    pub tokens_used: u32,
    pub token_budget: u32,       // InferenceEngine::context_window()
    pub state: ContextState,
}

pub enum ContextState {
    Normal,
    Warning,
    JustCompacted,
}
```

- `tokens_used`/`token_budget`: both raw token counts (not pre-divided into a percentage — division happens at render time so the UI can choose its own display format).
- `state`:
  - `Normal`: `tokens_used < warnThresholdPct * token_budget`
  - `Warning`: `tokens_used >= warnThresholdPct * token_budget`
  - `JustCompacted`: set only on the single `ContextUsage` value returned/emitted as the direct result of a compaction pass having just run (automatic or manual) — a transient, one-shot state for that emission only, not a state the value would organically be recomputed into on a later, unrelated read. A subsequent, ordinary `get_context_usage` call after a `JustCompacted` emission returns `Normal` or `Warning` based on the post-compaction token count, same as any other read.

## `ContextSettings` (new, Rust-internal, backed by existing `settings` table)

Not a new table — four new keys in the existing `settings(key, value, updated_at)` table, read via the existing `get_settings`/parsed by the new `context` module, written via the existing `update_setting`. See `research.md`'s threshold-defaults decision for the four keys and default values (`context.warnThresholdPct`, `context.compactThresholdPct`, `context.hardLimitPct`, `context.toolOutputOffloadChars`).

- Validation: each value must parse as an `f64` in `(0.0, 1.0]` for the three `*Pct` keys, and as a `usize > 0` for `toolOutputOffloadChars`. An unparseable or missing setting falls back to its documented default rather than erroring — this feature must not be able to brick a conversation just because a setting value was hand-edited into something invalid.
- Invariant enforced at read time (not stored as a separate constraint): `warnThresholdPct <= compactThresholdPct <= hardLimitPct`. If violated (e.g. a user sets warn above compact), the reader clamps `compactThresholdPct`/`hardLimitPct` up to at least `warnThresholdPct` rather than producing a nonsensical UI state.

## `messages.content_type` — extended

```sql
content_type TEXT NOT NULL CHECK (content_type IN ('text', 'tool_call', 'tool_result', 'error', 'rich_text', 'context_notice'))
```

### `messages.content` for a `context_notice` row

A JSON string, one of two shapes distinguished by `kind`:

```jsonc
// tier 1 — lightweight clearing occurred (informational only; see research.md
// for why this row is not load-bearing for tier-1 reconstruction)
{ "kind": "cleared", "clearedCount": 3, "notice": "3 old tool results cleared to save space" }

// tier 2 — summarization occurred (load-bearing: load_history_annotated
// splices `summary` in place of everything before this row)
{ "kind": "summarized", "summary": "<model-generated summary text>", "notice": "Conversation condensed to save space" }
```

- `role` is always `'assistant'` for a `context_notice` row (the `messages.role` CHECK only allows `'user' | 'assistant' | 'tool'`; there is no `'system'` role in this schema, matching how `error` rows are already persisted).
- `tool_name` is always `NULL` for this content_type.
- A `context_notice` row participates in `sequence` ordering exactly like any other message row, so it renders inline at the correct transcript position.

### `HistoryMessage` (new, replaces the plain-`ChatMessage`-only return of `load_history`)

```rust
pub struct HistoryMessage {
    pub chat: ChatMessage,       // existing type, unchanged
    pub content_type: String,    // drives tier-1's tool-result identification
    pub sequence: i64,
}
```

`storage::conversations::load_history_annotated(conn, conversation_id, skills_dir) -> Vec<HistoryMessage>` replaces the current `load_history`'s SQL (same query, `content_type` and `sequence` now also selected instead of discarded). Behavior beyond the raw column additions:

- `context_notice` rows are **excluded** from the returned `Vec<HistoryMessage>` themselves (same as `error` rows today) — they are not "said" by any role — **except** that when a `kind:"summarized"` row is encountered, everything with a lower `sequence` is dropped from the result and replaced by one synthesized `HistoryMessage { chat: ChatMessage::system(summary), content_type: "context_notice", sequence: <that row's sequence> }` at that position. If multiple `summarized` rows exist (a conversation compacted more than once), only content before the **most recent** one is spliced away — earlier summaries are themselves subject to being summarized away by a later pass, which is the expected steady-state (compaction of compactions).
- A thin wrapper `load_history(...) -> Vec<ChatMessage>` (for any call site that doesn't need `content_type`/`sequence`, if one remains) is `load_history_annotated(...).into_iter().map(|m| m.chat).collect()` — no duplicated SQL.

## `ContextNoticeDetail` (new, TS type, frontend rendering of a `context_notice` message)

```typescript
export type ContextNoticeDetail =
  | { kind: "cleared"; clearedCount: number; notice: string }
  | { kind: "summarized"; summary: string; notice: string };
```

Parsed from a `context_notice` message's `content` the same way `parseToolResultDetail` parses a `tool_result` row's content — a small sibling parser, since `context_notice` is not a tool result and doesn't belong in `ToolResultDetail`'s union.

## `ToolResultDetail` — extended for offloading (agent mode)

Existing `ReadDetail`/`BashDetail`/etc. (per `specs/004-tool-call-widgets/data-model.md`) each gain one new optional field so a widget can show a "view full output" affordance without changing the discriminated union's shape:

```typescript
export interface BashDetail {
  toolName: "Bash";
  command: string | null;
  timeoutMs: number | null;
  outcome: BashOutcome;
  offloadedTo: string | null; // NEW — absolute path to the full output file, if this
                               // result was large enough to be offloaded; null otherwise
}
```

Same `offloadedTo: string | null` field is added to `ReadDetail` (a giant file read can also exceed the threshold). Other tool detail shapes are unchanged — offloading in this feature's scope only applies where `model_text` can plausibly be large (`Bash` stdout/stderr, `Read` content); `Write`/`Edit`/`Glob`/`Grep`/`Task`/`AskUserQuestion` are not touched.

## Persistence

### `messages` table
No column changes beyond the widened `content_type` CHECK (migration `0004_context_notice_content_type.sql`, following the exact rebuild pattern of migration `0003`).

### `settings` table
No schema change. Four new keys seeded with their defaults on first read if absent (same lazy-default pattern already implicit in `get_settings`/`update_setting`'s upsert).

### Filesystem — offloaded tool-output files

`<app_data_dir>/tool-outputs/<conversation_id>/<tool_call_id>.txt` — plain UTF-8 text, the tool result's full `model_text` verbatim. Created lazily (directory created with the file on first offload for a given conversation). Not tracked in SQLite — the message row's `ToolResultDetail.offloadedTo` field is the only pointer to it, consistent with how attachments/rich-content already reference filesystem paths without a dedicated DB table.

### IPC surface

New commands (see `contracts/context-window-management.md` for full signatures):
- `get_context_usage(conversation_id) -> ContextUsage`
- `compact_conversation(conversation_id) -> ContextUsage`

New event:
- `context-usage-update` → `ContextUsage` payload (camelCase over the wire, per the existing `tauri-specta`/hand-written-`ipc.ts` convention).

### `load_history_annotated` (storage/conversations.rs)

See above — this is the one existing function whose signature/behavior changes; every current call site (`send_message`, `send_agent_message`) is updated to call it and adapt (either using `HistoryMessage` directly for the new compaction logic, or `.map(|m| m.chat)` where only the plain `ChatMessage` is needed).

## Frontend Types (src/lib/ipc.ts)

```typescript
export type ContextState = "normal" | "warning" | "justCompacted";

export interface ContextUsage {
  conversationId: string;
  tokensUsed: number;
  tokenBudget: number;
  state: ContextState;
}

export type ContextNoticeDetail =
  | { kind: "cleared"; clearedCount: number; notice: string }
  | { kind: "summarized"; summary: string; notice: string };
```

Plus the `offloadedTo: string | null` additions to `BashDetail`/`ReadDetail` above, and:

```typescript
commands.getContextUsage(conversationId: string): Promise<ContextUsage>
commands.compactConversation(conversationId: string): Promise<ContextUsage>

events.onContextUsageUpdate(cb: (p: ContextUsage) => void): Promise<UnlistenFn>
```

## Validation Rules

- `ContextUsage.tokensUsed` is never negative and, before any compaction runs, may legitimately exceed `tokenBudget` (the whole point of surfacing `state: "warning"`/triggering compaction) — the frontend must not clamp or hide an over-100% value, it should render it plainly (e.g. a full/overflowing bar) so the user understands *why* compaction is about to run.
- A `context_notice` row's `content` MUST be valid JSON matching one of the two `ContextNoticeDetail` shapes; a parse failure degrades to rendering the raw `notice`-less JSON as plain text (mirroring `parseToolResultDetail`'s existing degrade-gracefully convention) rather than throwing.
- `toolOutputOffloadChars` MUST be applied to the tool's `model_text` length (the string that would otherwise enter the prompt), not to any UI-only preview/display string — offloading is about what the *model* sees, independent of how a widget chooses to render it.
- `compact_conversation` MUST be a no-op (returns the current, unchanged `ContextUsage` with `state` left as `Normal`/`Warning`, not forced to `JustCompacted`) if invoked when there is nothing eligible to clear or summarize — it must not fabricate a compaction notice when nothing actually changed.
