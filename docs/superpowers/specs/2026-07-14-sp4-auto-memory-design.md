# SP4 — Auto-Memory Subsystem — Design

**Status:** Design (awaiting user spec-review)
**Date:** 2026-07-14
**Parent goal:** "State-of-the-art context management and prompt engineering on this repo, comparable to the Claude Code and Qwen Code repos." SP4 is the *durable cross-conversation memory* leg — the piece neither SP2 (in-conversation context management) nor SP3 (per-turn prompt engineering) covers: facts that should survive the end of a conversation and inform the next one in the same workspace.

**Relationship to prior sub-projects:**
- SP1 (harness cleanup/budgeting) — landed on `main`.
- SP2 (SOTA context management) — landed on `main`. SP4 piggybacks on its tier-2 compaction pass (`summarize_and_persist`).
- SP3 (prompt engineering) — on branch `sp3-prompt-engineering`, **benchmark-gated, not yet merged**. SP3 added the `AGENTS.md` project-instructions section to `plan_system_message`. SP4's recall injection sits in the *same slot*. **See "Sequencing & branch coordination" below — this is the one hard cross-project dependency.**

---

## 1. What this builds

A workspace-scoped memory store that:

1. **Extracts** durable facts from a conversation automatically, out-of-band, when that conversation compacts — reviewing the span being condensed *plus* the workspace's existing memories, and emitting an updated memory set. No per-turn cost; best-effort; never blocks or fails the agent turn.
2. **Recalls** those memories into every subsequent turn in the same workspace by folding a token-capped `# Memories` section into `messages[0]` (the stable system prompt), structurally outside the compaction window.

This mirrors the auto-memory this very repo's controller uses (the `MEMORY.md` + per-fact files under `~/.claude/.../memory/`), but implemented natively in doce for doce's own agent, keyed on doce workspaces.

**Locked design decisions** (from the user's two design forks):
- **Extraction = out-of-band pass.** A separate tool-free `Forbid`-mode `llama-server` call, distinct from the summarization call. It reviews the conversation span + existing memories and emits the updated memory set. Best-effort — a failure logs and leaves memories untouched.
- **Trigger = on-compaction.** Extraction runs inside tier-2 compaction (`summarize_and_persist`'s Accept arm), not per-turn.
- **Recall = all workspace memories.** `SELECT ... WHERE workspace_id = ?`, ordered, capped at a token budget, injected as `# Memories` in `messages[0]`.

---

## 2. Data model

New migration `0010_memories.sql` (append `(10, include_str!("migrations/0010_memories.sql"))` to the flat `MIGRATIONS` array in `src-tauri/src/storage/migrations.rs` — latest is currently `0009`).

```sql
-- SP4: durable per-workspace agent memory
CREATE TABLE memories (
  id            TEXT PRIMARY KEY,
  workspace_id  TEXT REFERENCES workspaces(id),
  content       TEXT NOT NULL,
  created_at    INTEGER NOT NULL,
  updated_at    INTEGER NOT NULL
);
CREATE INDEX idx_memories_workspace ON memories(workspace_id);
```

**Schema rationale:**
- `workspace_id` is **nullable** and mirrors `conversations.workspace_id` exactly (same `TEXT REFERENCES workspaces(id)` shape). A conversation with a NULL workspace (no workspace bound) recalls/extracts against the NULL bucket — never leaks across workspaces because the recall query filters on the conversation's own `workspace_id` value (including NULL via `IS`).
- No `key`/`kind`/dedup column. The extraction pass owns the *entire* memory set for a workspace and emits the full desired list each time (add/update/drop happens inside the model's reasoning, not via row-level upserts). Storage is therefore **replace-the-workspace-set** (see §4), which keeps the table a faithful projection of the last extraction rather than an append-only log that drifts.
- `content` is one fact per row, plain text. No frontmatter/structure at the storage layer — structure (if any) lives in the extraction prompt's output contract, but the row is just the fact string. This keeps recall rendering trivial and avoids a parse step.

**Scope key:** the conversation's `workspace_id`, resolved by `SELECT workspace_id FROM conversations WHERE id = ?1` in both the recall and extraction paths (both already have `conversation_id` in hand).

---

## 3. Recall path (injection into `messages[0]`)

**Where:** a new `# Memories` section folded into `plan_system_message` (`src-tauri/src/commands/agent.rs`), in the **same slot** as SP3's `project_instructions_section` (the `AGENTS.md` injection) — after the cwd line, as part of the immutable system prompt. Being in `messages[0]` places it structurally *outside* the compaction window: it is never summarized away, and it is byte-stable within a conversation so `inference::PromptSession`'s KV prefix survives turn boundaries (same invariant SP3's AGENTS.md block preserves).

**Why threaded as a parameter (not read inside `plan_system_message`):** `AGENTS.md` is read from *disk* synchronously inside `project_instructions_section`. Memories live in the *DB* and need an async query with a `conn`. So — unlike AGENTS.md — the rendered block is fetched by the async caller and **passed in** as a parameter, the same way `transcript_path` is already threaded through. Concretely:

- New helper `memories_section(conn, workspace_id) -> Option<String>`:
  - `SELECT content FROM memories WHERE workspace_id IS ?1 ORDER BY updated_at DESC, id` (use `IS` so a NULL workspace matches the NULL bucket).
  - Render as `# Memories\n\n- <content>\n- <content>\n...`.
  - Bound to `MEMORIES_MAX_TOKENS` using the **same re-measure shrink loop** SP3's `project_instructions_section` uses (drop whole trailing memories until the rendered block fits — never mid-fact truncation). Returns `None` when there are no memories, so a workspace with none injects nothing (zero prompt-byte delta — see gating note §6).
- `plan_system_message` gains a `memories: Option<&str>` parameter and splices the block into its existing slot. Every caller of `conversation_system_message` / `plan_system_message` (the live turn in `send_agent_message`, plus the usage/compaction accounting callers in `commands/context.rs`) fetches memories for the conversation's workspace and threads them in, so token accounting and the live prompt stay identical.

**Recall cap:** new constant in `src-tauri/src/context/limits.rs`:
```rust
/// Recalled workspace memories are capped at this share of the window,
/// mirroring PROJECT_INSTRUCTIONS_MAX_TOKENS. Injected once into messages[0],
/// outside the compaction window.
pub const MEMORIES_MAX_TOKENS: usize = CONTEXT_WINDOW_TOKENS / 8; // = 2048
```
(Same 1/8-of-window share as `PROJECT_INSTRUCTIONS_MAX_TOKENS`. AGENTS.md + memories together therefore cap at 1/4 of the window — acceptable; both are opt-in and small in practice.)

---

## 4. Extraction path (out-of-band, on compaction)

**Where:** inside `summarize_and_persist`'s **Accept arm** (`src-tauri/src/context/mod.rs`), after the summary and restored-file notices are persisted — the tier-2 pass that already has `conn`, `base_url`, `conversation_id`, and `to_summarize` (the exact span being condensed). Runs only when a summary was actually accepted (i.e., real compaction happened), so extraction cadence tracks compaction cadence.

**New function `extract_and_persist_memories(conn, base_url, conversation_id, to_summarize)`:**

1. Resolve `workspace_id` via `SELECT workspace_id FROM conversations WHERE id = ?1`.
2. Load existing memories for that workspace (same query as recall).
3. Build a `Forbid`-mode chat request — **exactly the summarization call's shape** (tools `None`, `tool_choice` `None`, flat `max_tokens` cap, fresh never-cancelled token — compaction is best-effort): system prompt = new `MEMORY_EXTRACTION_PROMPT`; body = the existing-memories block + the `to_summarize` span (same `.chat.clone()` mapping the summary uses).
4. Parse the model's output into a new memory set (output contract: one fact per line, or a fenced list — kept dead simple; a parse failure → treat as "no change", log, return).
5. **Guard, then replace-the-set** (mirroring `evaluate_summary`'s defensive posture): if the parsed set is empty/degenerate *and* existing memories are non-empty, **do nothing** (never let a bad extraction wipe good memories — the unsafe direction). Otherwise, in a single transaction: `DELETE FROM memories WHERE workspace_id IS ?` then insert the new set with fresh `updated_at`. Preserve `created_at` for facts whose `content` is unchanged (so "age" survives re-extraction).
6. **Best-effort throughout:** every failure (server error, parse failure, guard trip) logs and returns `Ok(())` — extraction must never fail or block compaction, which itself must never fail an agent turn. `summarize_and_persist` calls this with a `let _ = extract_and_persist_memories(...).await;`-style swallow (or logs the error), never `?`.

**New prompt `MEMORY_EXTRACTION_PROMPT`** (`src-tauri/src/context/limits.rs`, alongside `SUMMARIZATION_PROMPT`): instructs the model to review the existing memories and the conversation span, and emit the *full* updated set of durable, workspace-relevant facts (user preferences, project constraints, hard-won decisions — not transient task state, not anything already obvious from the code). Output contract: one fact per line, no commentary. Cap output at `SUMMARY_MAX_TOKENS` (reuse; the set is small). Kept tight (~300–400 tokens of instruction) and modeled on this repo's own memory guidance (durable facts, one per line, prefer updating over duplicating).

---

## 5. Testing

- **Migration:** `0010` applies cleanly on top of `0009`; `memories` table + index exist; existing migration-idempotency test extends to cover it.
- **Recall — `memories_section`:**
  - none → `None` (zero injection).
  - some → `# Memories` block with all facts, newest-first.
  - over-cap → shrink loop drops trailing facts, never mid-fact; non-ASCII fact respects the token weighting (same test shape as SP3 (c)'s CJK truncation test).
  - NULL workspace matches only the NULL bucket (no cross-workspace leak) — insert memories under workspace A and NULL, query NULL, get only NULL's.
- **`plan_system_message`:** with `memories: Some(block)`, the block appears in the same slot as AGENTS.md and the prompt is otherwise byte-identical to `memories: None`; with `None`, byte-identical to pre-SP4.
- **Extraction — `extract_and_persist_memories`** (against a stub/replayed `llama-server` like the existing summarization tests):
  - empty existing + model emits N → N rows persisted.
  - non-empty existing + model emits refined set → set replaced, unchanged facts keep `created_at`.
  - model emits empty + existing non-empty → **guard trips, existing preserved**.
  - server error / parse failure → `Ok(())`, memories untouched.
  - workspace resolution: extraction writes under the conversation's `workspace_id` (incl. NULL bucket).
- **Sacred-invariant regression:** `run_loop` byte-untouched; a workspace with no memories produces a byte-identical agent prompt to pre-SP4 (proves benchmark-inertness, §6).

---

## 6. Gating & benchmark-inertness

SP4 touches prompt bytes in two prompt-adjacent places — but both are **inert for the SP3 benchmark**, exactly as SP3's AGENTS.md injection was:

- **Recall injection** (`# Memories` in `messages[0]`): only renders when the workspace has memories. The benchmark's `tier4_planned` task runs in a scratch workspace with an empty `memories` table → `memories_section` returns `None` → **zero prompt-byte delta**. The §5 regression test locks this in.
- **`MEMORY_EXTRACTION_PROMPT`**: a *new, separate* out-of-band prompt (like `SUMMARIZATION_PROMPT`), never part of the agent turn the benchmark measures. It cannot affect `tier4_planned` scores.

**Therefore SP4 does not require its own benchmark gate for the stated goal** — it is behavior-adjacent but benchmark-inert. (If we later want to validate that *recalled* memories help rather than hurt real turns, that is a separate, future memory-specific benchmark — out of scope here.) SP4 lands un-gated, like SP2.

---

## 7. Sequencing & branch coordination

**The one hard cross-project dependency:** SP4's recall injection lives in the *same `plan_system_message` slot* as SP3's `AGENTS.md` `project_instructions_section`. That slot exists **only on the `sp3-prompt-engineering` branch**, not on `main`.

Options:
- **(Recommended) Build SP4 on top of `sp3-prompt-engineering`.** SP4 inherits the AGENTS.md slot, memories nestle in beside it, and both merge to `main` together once the SP3 benchmark passes. Cost: SP4 rides SP3's benchmark gate even though SP4 itself is inert — but SP4 *needs* SP3's slot anyway, so this is the natural order.
- **Build SP4 on `main` now.** Requires re-creating the injection slot independently; guarantees a `plan_system_message` merge conflict when SP3 later merges (both add a section to the same function). Resolvable but avoidable.

**Recommendation:** treat SP3-benchmark → merge as the prerequisite. Either (a) run the SP3 benchmark first, merge on pass, then build SP4 on the now-updated `main`; or (b) build SP4 on the `sp3-prompt-engineering` branch and let the single benchmark cover the merged stack. The user's call — but SP4 implementation should **not** start on `main` ahead of SP3 without accepting the merge conflict.

---

## 8. File-touch summary

| File | Change |
|---|---|
| `src-tauri/src/storage/migrations/0010_memories.sql` | **create** — `memories` table + index |
| `src-tauri/src/storage/migrations.rs` | add `(10, include_str!(...))` to `MIGRATIONS` |
| `src-tauri/src/storage/memories.rs` *(new)* or `conversations.rs` | `insert/replace/load` helpers for `memories` |
| `src-tauri/src/context/limits.rs` | `MEMORIES_MAX_TOKENS` const; `MEMORY_EXTRACTION_PROMPT` |
| `src-tauri/src/context/mod.rs` | `extract_and_persist_memories`; call it (swallowed) in `summarize_and_persist`'s Accept arm |
| `src-tauri/src/commands/agent.rs` | `memories_section` helper; `memories` param on `plan_system_message`; thread from `send_agent_message` |
| `src-tauri/src/commands/context.rs` | thread memories into usage/compaction accounting callers |

---

## 9. Open question for spec-review

None blocking. The one decision that needs the user's explicit sign-off is **§7 sequencing** — build SP4 on the SP3 branch vs. wait for SP3 to merge first. Everything else is locked by the two design forks the user already answered.
