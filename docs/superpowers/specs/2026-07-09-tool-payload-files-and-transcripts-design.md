# Tool payload files, honest Read truncation, and materialized transcripts

**Date:** 2026-07-09
**Status:** Draft — pending user review
**Supersedes:** the offload behavior of 010-context-window-management User Story 3 (`context/offload.rs`)

## Problem

Three related problems with how tool output flows through the agent today:

1. **Offload coverage is partial and mis-denominated.** `offload_if_oversized`
   (`src-tauri/src/context/offload.rs`) truncates oversized tool results to a
   preview + Read pointer, but only on the top-level path
   (`handle_general_tool_call`). `SubagentBackend::execute_tool` and the
   parent-side `Task` result never offload, and the threshold is chars, not
   tokens. `fit_to_budget` documents the consequence: "a tool result that
   should have been offloaded but wasn't" (`context/mod.rs:535-541`). Bash's
   64KB `model_text` cap (~16k tokens) is twice the 8192-token window, and its
   truncation marker points at "the conversation transcript" — a place the
   model cannot Read.

2. **Read is offload-wrapped**, so reading a large file writes a copy of that
   file into `tool-outputs/` — a write on every read, duplicating data that
   already exists at a known path. Meanwhile `fs::read` has no byte cap: a
   single long line passes through whole (`agent/tools/fs.rs:18-31`).

3. **The model has no way to recall history that left its window.** Tier-1
   clearing destroys non-offloaded tool results irrecoverably; tier-2
   summarizing drops detail. The full history exists in SQLite, but no tool
   reaches it.

## Governing invariant

> **SQLite stores exactly what entered the model's context. Files store the
> canonical payloads that *might* enter it.**

Consequences, in both directions:

- A tool result's `model_text` column is precisely what the model saw for that
  call — whether that was inline content (small result) or a reference line
  (large result). Context rebuild (`load_history_annotated`) therefore remains
  a pure SQLite scan with **no file I/O on the per-turn hot path**.
- Bulk tool output lives in exactly one place: a payload file. It is never
  duplicated into `messages.content`. SQLite rows are bounded by
  construction.
- Replay fidelity is perfect: the context the model ran with is always
  rebuildable from SQLite alone.
- The transcript file (Piece 3) renders `model_text` per row, so it is
  literally "what the model saw," and is a **derived, regenerable cache** of
  SQLite — the same status as the FTS index. Payload files are **not**
  derived; they are the only copy of the bulk.

## Piece 1 — payload files for every data-tool result

### Storage rule (uniform, branch-free)

Every result from a data-producing tool — Bash, Grep, Glob, Write, Edit, and
future MCP tools — is written to:

```
<app_data_dir>/tool-outputs/<conversation_id>/<tool_call_id>.txt
```

**always**, whether or not it also appears inline. For Bash the payload is the
full untruncated stdout + stderr (the text that today survives only in
`detail`). One canonical location, one GC path, and a future file indexer
ingests everything with no "small results live elsewhere" split.

Write ordering: **payload file first, then SQLite row.** A crash leaves an
orphan file (harmless, swept by GC), never a row referencing a missing file.

### Presentation rule (thresholded)

Let `T = context.toolOutputOffloadTokens`, default derived as
`CONTEXT_WINDOW_TOKENS / 16` (= 512 today), overridable via the settings
table. Token counting uses the live model tokenizer via a
`count: impl Fn(&str) -> usize` closure so `offload.rs` stays
Tauri-independent and unit-testable (call sites already hold the
`InferenceEngine` — they call `annotate_with_token_count` on the adjacent
line). The char-based `context.toolOutputOffloadChars` setting is removed;
an existing user-set value in the settings table is dropped, not converted
(local app, one user, announce in release notes).

- `count(result) <= T` → `model_text` = the full result, no pointer noise.
- `count(result) > T` → `model_text` = a **status reference line** carrying
  cheap metadata but no content, e.g.:

  ```
  Bash: exit 0 — 48,213 bytes stdout, 0 bytes stderr → Read "<path>" to view
  Grep: 214 matches in 37 files → Read "<path>" to view
  ```

  The metadata answers "did it work / how big" — the common case — without a
  Read round-trip; the model spends a turn on Read only when it needs actual
  content. This matters because on local inference every round-trip costs
  seconds, which is also why small results inline at all.

### `detail` becomes pure metadata

`detail` carries structured metadata only: tool name, exit code, sizes,
`tokenCount`, and `payloadRef` (the payload file path). The bulk text is
**removed** from `detail` — the transcript UI renders metadata instantly and
lazy-loads content from the payload file on expand (aligned with the existing
widget-cost/progressive-rendering plan). This requires a small Tauri command
for the frontend to read a payload file, plus per-widget lazy-expand behavior.

`payloadRef` replaces `offloadedTo`. Readers (tier-1 clearing, widgets)
accept either key; legacy rows with inline bulk in `detail` continue to render
from it. **No data migration** — old conversations stay fat until deleted.

### Coverage

- **Top-level** (`handle_general_tool_call`): as today, with the new rule.
- **Subagents** (`SubagentBackend::execute_tool`): gains an `app_data_dir`
  field (plumbed from the spawn site, where the `AppHandle` exists — the
  backend itself keeps `app: None`). Payloads file under the **subagent's
  own** conversation id.
- **Carve-outs, each principled:**
  - **Read** never writes a copy; its `payloadRef` is the source path plus
    offset/limit (re-derivable — see Piece 2).
  - **Task** stays inline: `sub_final` is an answer meant for immediate
    consumption; forcing a Read per Task call adds a round-trip to every
    delegation. The subagent's transcript holds its full history anyway.
  - **AskUserQuestion / plan-tool replies** stay inline: short, fixed-shape
    control-flow strings persisted via `persist_plan_tool` /
    `handle_ask_user_question`. Cheap to add files later if uniformity wins.

### Failure handling

If the payload write fails, `model_text` falls back to a hard-truncated
preview plus an honest `[full output could not be saved: <err>]`, and
`payloadRef` is null. **Unbounded text never enters the window on any failure
path** (today's fallback passes the full text through —
`commands/agent.rs:1022`).

### Search consequence (accepted)

`messages_fts` indexes `messages.content`; with bulk removed, FTS covers
`model_text` + metadata only. Full-recall search over tool output returns
with the planned file indexer, whose ingest set is exactly `tool-outputs/` +
`transcripts/`. Interim recall loss is accepted. FTS triggers and the
subagent-exclusion logic are untouched.

## Piece 2 — Read unwrapped, honest truncation

- Read is excluded from payload-file writing at both call sites: no writes on
  the read path, no duplicate copies.
- `fs::read` gains two caps, both with honest markers in the style of the
  Bash truncation marker:
  - **Per-line clamp:** lines over 2000 chars are cut with `… [line
    truncated]`.
  - **Total cap:** output over ~8KB (~2k tokens of the 8192 window) stops
    with `[capped at <N> bytes — continue with offset=<next line>]`, so the
    model can page without guessing.
- **Restorability without copies:** a tier-1-cleared Read result's
  placeholder cites the original path + offset/limit from its paired
  `tool_call` row's arguments: `[Read result cleared; re-Read "<path>"
  (offset X, limit Y) to recover]`. The source file may have changed since —
  acceptable for a recall mechanism.

## Piece 3 — materialized transcript files

### Format

`<app_data_dir>/transcripts/<conversation_id>.txt` — rendered text entries,
one per message row, in sequence order:

```
[#41 user]
Run the test suite and fix failures

[#42 assistant → Bash]
{"command": "cargo test"}

[#43 Bash result]
Bash: exit 1 — 48,213 bytes stdout → Read "/…/tool-outputs/<conv>/<call>.txt" to view

[#44 assistant]
Three failures, all in context::tests…
```

- Entry body is the row's `model_text` — what the model saw. Tool-call args
  are capped at 2000 chars with a truncation marker (Write/Edit args can
  embed whole files).
- **Everything is included**, with markers: error rows (`[#n error]`) and
  context-notice rows (`[#n context-notice: summarized]` + summary text).
  Unlike `load_history_annotated`, the transcript never splices or drops —
  it is the recall record, not the context feed.
- Grep targets are stable (`^\[#42 `), and appends never shift earlier
  lines, so entry numbers are durable references.

### Write path and the insert-helper refactor

All message inserts collapse into one `storage::messages::insert` helper that
(a) allocates `MAX(sequence)+1`, (b) inserts the row, (c) appends the
rendered transcript entry. This replaces the 7+ duplicated `MAX(sequence)+1`
sites (in `storage/conversations.rs`, `commands/agent.rs`,
`scheduler/worker.rs`) — one choke point, so transcript and DB cannot drift
by a forgotten call site. Existing behavior — per-`tool_call_id` idempotency,
transactional pairing with `conversations.updated_at` — is preserved inside
the helper.

Transcript append is **best-effort**: on failure, log and continue;
regeneration heals.

### Healing (derived-cache discipline)

On first use of a conversation (agent start or history load), compare the
transcript file's last `[#seq` against SQLite's `MAX(sequence)`. Any mismatch
— missing file, torn tail, stale content — triggers full regeneration: render
all rows, write to a temp file, atomic rename. Regeneration is always safe
because SQLite is authoritative. At local scale (hundreds of conversations,
thousands of rows) this is milliseconds.

### Exposure to the model

One line in the agent system prompt (`agent/mod.rs`):

> This conversation's transcript — everything so far, including content no
> longer in your context — is at `<path>`. Read it to recall earlier work.

The wording says *transcript*, not *conversation*: it is a record to consult,
not the live exchange. Tier-1 cleared rows with a `payloadRef` cite it
directly (for Read rows that is the original source path + offset/limit);
rows without one (Task, plan, AskUserQuestion) cite
`entry #N in the transcript at <path>`. **Every cleared row becomes
restorable.** Tier-2 summarized spans remain recoverable the same way.

### Scope and isolation

- **All conversations get transcripts, including subagents** — subagents hit
  the same 8k window and benefit equally. Each agent's system prompt names
  only its **own** transcript path; the system never feeds one conversation's
  content into another's context (FR-015/SC-008 hold at the system level, as
  today).
- A determined agent could `ls` the transcripts directory via Bash — but Bash
  can already `sqlite3` the whole database today (`Read` is unsandboxed per
  FR-009; Bash runs unsandboxed). For a local single-user app this is an
  **accepted, documented boundary**, not a new enforcement surface.
- FTS, sidebar, search UI, and their subagent-exclusion filters are
  unchanged; SQLite remains the source of truth for all user-facing surfaces.

### Lifecycle

Conversation deletion GCs `transcripts/<id>.txt` and `tool-outputs/<id>/` —
this fixes the acknowledged wart in specs/010 (offload files "not tracked in
SQLite," never cleaned). Stale files for still-existing conversations are
harmless (healing regenerates; payload orphans are unreferenced). Archived
conversations keep their files.

## What this deliberately does not do

- **No JSONL message store; messages stay in SQLite.** Sequence allocation,
  idempotency, crash-healing, status polling, transactional `updated_at`
  pairing, and search joins all keep their transactional substrate.
- **No external search engine in this spec.** The payload + transcript files
  are shaped to be its ingest set later.
- **No new enforcement layer for cross-conversation file access** (accepted
  boundary, above).
- **No change to `worker.rs` scheduled assistant messages** — they are not
  tool results (they do flow through the shared insert helper).

## Dependency order

1. **Piece 1** — bounds every `model_text`; everything downstream assumes
   bounded rows.
2. **Piece 2** — Read caps (the recall path must itself be bounded).
3. **Piece 3** — transcripts render bounded `model_text`; system prompt line
   and cleared-row pointers land last.

Each piece is independently shippable and testable.

## Testing

**Unit:**
- Threshold boundary: at-threshold inlines, over-threshold references; token
  counting via injected closure.
- Payload file always written with exact original bytes; reference line
  format per tool (exit code, sizes, counts).
- Read carve-out: no file written; `payloadRef` = source + offset/limit.
- Failure fallback: payload write error → bounded `model_text`, null
  `payloadRef`.
- `fs::read`: long-line clamp marker; total-cap marker with correct
  continue-offset; offset/limit interaction.
- Transcript: golden-format render; append-then-regenerate idempotence
  (byte-identical output); healing on torn tail, missing file, stale file.
- Insert helper: `MAX(sequence)+1` semantics and per-`tool_call_id`
  idempotency preserved (port existing tests to the helper).

**Integration / e2e:**
- Oversized Bash turn: bounded `model_text` reference, payload file present,
  widget renders metadata instantly and lazy-loads content.
- Subagent run: payloads and transcript under the subagent's own id; parent
  transcript carries only the Task result (SC-008 regression guard).
- Cleared-row recovery: tier-1 clears a tool row → placeholder cites the
  payload file → agent Reads it back successfully.
- Existing `load_history_annotated` tests pass unchanged — the regression
  guard that model-facing context semantics did not move.

## Settings and constants

| Name | Kind | Default | Notes |
|---|---|---|---|
| `context.toolOutputOffloadTokens` | settings table | `CONTEXT_WINDOW_TOKENS / 16` (512) | replaces `toolOutputOffloadChars` |
| `READ_MAX_LINE_CHARS` | constant | 2000 | per-line clamp in `fs::read` |
| `READ_MAX_BYTES` | constant | 8192 | total Read output cap |
| `TRANSCRIPT_ARGS_CAP_CHARS` | constant | 2000 | tool-call args cap in transcript entries |
