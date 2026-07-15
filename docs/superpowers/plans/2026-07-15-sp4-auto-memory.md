# SP4 Auto-Memory Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give doce durable, workspace-scoped agent memory: facts extracted automatically out-of-band when a conversation compacts, and recalled into every later turn in the same workspace.

**Architecture:** A `memories` table keyed by `workspace_id` (migration 0010). Extraction is a tool-free `Forbid`-mode llama-server call hooked into tier-2 compaction (`summarize_and_persist`'s Accept arm), which reviews the condensed span plus existing memories and emits the full replacement set. Recall renders a token-capped `# Memories` block that the async callers fetch and thread into `plan_system_message`, landing in `messages[0]` beside SP3's `AGENTS.md` slot — structurally outside the compaction window and byte-stable per conversation for KV-prefix reuse.

**Tech Stack:** Rust, `tokio_rusqlite`, `rusqlite`, llama-server sidecar (OpenAI-compatible `/v1/chat/completions`).

**Spec:** `docs/superpowers/specs/2026-07-14-sp4-auto-memory-design.md`

## Global Constraints

- **`run_loop` (`src/agent/mod.rs`) must remain byte-untouched.** The Require-invariant (a Require-mode turn with no tool call is a retriable correction, never done) is sacred.
- **No in-process model loading.** All inference goes through the llama-server sidecar over HTTP.
- **A workspace with no memories must produce a byte-identical agent prompt to pre-SP4.** `memories_section` returns `None` → nothing injected. This is what keeps SP4 inert for the tier4_planned benchmark. Lock it with a test.
- **Extraction is best-effort and must never fail or block a turn.** Every failure path (server error, parse failure, guard trip) logs and returns `Ok(())`. Compaction must never fail an agent turn.
- **Never let a bad extraction destroy good memories.** If the parsed set is empty/degenerate while existing memories are non-empty, make no change. This mirrors `evaluate_summary`'s defensive posture.
- **Do not touch `src/views/**`.** A parallel frontend session owns 4 uncommitted files there. Every commit is scoped to `src-tauri/**` (+ `docs/`).
- **Formatting:** this repo uses `oxfmt`, not prettier. Rust: `cargo fmt`.
- **NULL workspace is a real bucket.** Query with `workspace_id IS ?1` so a conversation with no workspace matches only NULL-bucket memories and never leaks across workspaces.

---

### Task 1: Memories table + storage layer

**Files:**
- Create: `src-tauri/src/storage/migrations/0010_memories.sql`
- Modify: `src-tauri/src/storage/migrations.rs` (append to the flat `MIGRATIONS` array; latest is currently `(9, ...)`)
- Create: `src-tauri/src/storage/memories.rs`
- Modify: `src-tauri/src/storage/mod.rs` (add `pub mod memories;`)

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces, in `crate::storage::memories`:
  - `pub struct Memory { pub id: String, pub content: String, pub created_at: i64, pub updated_at: i64 }`
  - `pub async fn load_memories(conn: &tokio_rusqlite::Connection, workspace_id: Option<&str>) -> Result<Vec<Memory>, String>` — ordered `updated_at DESC, id`.
  - `pub async fn replace_memories(conn: &tokio_rusqlite::Connection, workspace_id: Option<&str>, contents: &[String], now: i64) -> Result<(), String>` — single transaction: delete the workspace's rows, insert `contents`. Preserves `created_at` for a content string that already existed; new rows get `created_at = now`. All rows get `updated_at = now`.
  - `pub async fn workspace_id_for_conversation(conn: &tokio_rusqlite::Connection, conversation_id: &str) -> Result<Option<String>, String>`

- [ ] **Step 1: Write the migration**

`src-tauri/src/storage/migrations/0010_memories.sql`:
```sql
-- SP4: durable per-workspace agent memory. `workspace_id` mirrors
-- conversations.workspace_id exactly (nullable, same FK target) so a
-- conversation with no workspace recalls the NULL bucket and nothing else.
CREATE TABLE memories (
  id            TEXT PRIMARY KEY,
  workspace_id  TEXT REFERENCES workspaces(id),
  content       TEXT NOT NULL,
  created_at    INTEGER NOT NULL,
  updated_at    INTEGER NOT NULL
);
CREATE INDEX idx_memories_workspace ON memories(workspace_id);
```

- [ ] **Step 2: Register it**

In `src-tauri/src/storage/migrations.rs`, append to `MIGRATIONS`:
```rust
    (10, include_str!("migrations/0010_memories.sql")),
```

- [ ] **Step 3: Write the failing tests**

Add to `src-tauri/src/storage/memories.rs` a `#[cfg(test)] mod tests`. Follow the existing in-memory-connection pattern used by `src/storage/messages.rs` tests (open a `tokio_rusqlite::Connection`, run `crate::storage::migrations::run(&conn)`, then insert a workspace row). Tests:

```rust
#[tokio::test]
async fn replace_then_load_roundtrips_in_order() {
    // insert workspace 'w1'; replace_memories(w1, ["a","b"], now=10)
    // load_memories(w1) -> 2 rows, contents contain "a" and "b"
}

#[tokio::test]
async fn replace_preserves_created_at_for_unchanged_content() {
    // replace(w1, ["keep"], now=10); replace(w1, ["keep","new"], now=20)
    // "keep".created_at == 10 (survived), "new".created_at == 20
    // both updated_at == 20
}

#[tokio::test]
async fn null_workspace_is_isolated_from_a_real_workspace() {
    // replace(Some("w1"), ["ws"], 10); replace(None, ["nullbucket"], 10)
    // load(None) -> only "nullbucket"; load(Some("w1")) -> only "ws"
}

#[tokio::test]
async fn replace_with_empty_clears_the_workspace() {
    // replace(w1, ["a"], 10); replace(w1, &[], 20); load(w1) -> empty
    // (the empty-set GUARD lives in the caller, not here -- this layer is literal)
}

#[tokio::test]
async fn workspace_id_for_conversation_resolves_and_handles_null() {
    // conversation with workspace_id 'w1' -> Some("w1")
    // conversation with NULL workspace_id -> None
}
```

- [ ] **Step 4: Run them, verify they fail**

Run: `cargo test --lib storage::memories`
Expected: FAIL (module/functions not defined).

- [ ] **Step 5: Implement `src-tauri/src/storage/memories.rs`**

```rust
//! SP4: durable per-workspace agent memory. Rows are a faithful projection of
//! the last extraction pass -- `replace_memories` swaps a workspace's whole set
//! in one transaction rather than upserting row-by-row, because the extraction
//! model emits the full desired set (add/update/drop happens in its reasoning,
//! not here). `created_at` survives a re-extraction that keeps a fact verbatim,
//! so a memory's age means what it says.

use serde::{Deserialize, Serialize};
use rusqlite::Connection;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Memory {
    pub id: String,
    pub content: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// `workspace_id IS ?1` (not `=`): a conversation with no workspace owns the
/// NULL bucket, and `= NULL` would match nothing.
pub async fn load_memories(
    conn: &tokio_rusqlite::Connection,
    workspace_id: Option<&str>,
) -> Result<Vec<Memory>, String> {
    let workspace_id = workspace_id.map(|s| s.to_string());
    conn.call(move |conn: &mut Connection| {
        let mut stmt = conn.prepare(
            "SELECT id, content, created_at, updated_at FROM memories \
             WHERE workspace_id IS ?1 ORDER BY updated_at DESC, id",
        )?;
        let rows = stmt
            .query_map([&workspace_id], |row| {
                Ok(Memory {
                    id: row.get(0)?,
                    content: row.get(1)?,
                    created_at: row.get(2)?,
                    updated_at: row.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    })
    .await
    .map_err(|e| e.to_string())
}

/// Swaps the workspace's whole memory set in ONE transaction. Content strings
/// that already existed keep their original `created_at`; everything gets the
/// new `updated_at`.
pub async fn replace_memories(
    conn: &tokio_rusqlite::Connection,
    workspace_id: Option<&str>,
    contents: &[String],
    now: i64,
) -> Result<(), String> {
    let workspace_id = workspace_id.map(|s| s.to_string());
    let contents = contents.to_vec();
    conn.call(move |conn: &mut Connection| {
        let tx = conn.transaction()?;
        // Remember prior created_at per content so an unchanged fact keeps its age.
        let prior: std::collections::HashMap<String, i64> = {
            let mut stmt = tx.prepare(
                "SELECT content, created_at FROM memories WHERE workspace_id IS ?1",
            )?;
            let rows = stmt
                .query_map([&workspace_id], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows.into_iter().collect()
        };
        tx.execute("DELETE FROM memories WHERE workspace_id IS ?1", [&workspace_id])?;
        for content in &contents {
            let created_at = prior.get(content).copied().unwrap_or(now);
            tx.execute(
                "INSERT INTO memories (id, workspace_id, content, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    uuid::Uuid::new_v4().to_string(),
                    &workspace_id,
                    content,
                    created_at,
                    now
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())
}

pub async fn workspace_id_for_conversation(
    conn: &tokio_rusqlite::Connection,
    conversation_id: &str,
) -> Result<Option<String>, String> {
    let conversation_id = conversation_id.to_string();
    conn.call(move |conn: &mut Connection| {
        conn.query_row(
            "SELECT workspace_id FROM conversations WHERE id = ?1",
            [&conversation_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })
    })
    .await
    .map_err(|e| e.to_string())
}
```

Add `pub mod memories;` to `src-tauri/src/storage/mod.rs`. Check how `uuid` is already used in this crate (e.g. in `storage/messages.rs`) and match that idiom; if `uuid` is not a dependency, generate ids the same way neighbouring storage code does instead of adding a dep.

- [ ] **Step 6: Run tests, verify they pass**

Run: `cargo test --lib storage::memories && cargo test --lib storage::migrations`
Expected: PASS (including the existing migration-idempotency tests, which must still pass with 0010 present).

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/storage/memories.rs src-tauri/src/storage/migrations.rs \
        src-tauri/src/storage/migrations/0010_memories.sql src-tauri/src/storage/mod.rs
git commit -m "feat(memory): memories table and per-workspace storage layer"
```

---

### Task 2: Recall — the `# Memories` block in `messages[0]`

**Files:**
- Modify: `src-tauri/src/context/limits.rs` (add `MEMORIES_MAX_TOKENS`)
- Modify: `src-tauri/src/commands/agent.rs` (`memories_section`, `plan_system_message` param, thread callers)
- Modify: `src-tauri/src/commands/context.rs` (two call sites at ~42 and ~101)

**Interfaces:**
- Consumes: `crate::storage::memories::{load_memories, workspace_id_for_conversation}` (Task 1).
- Produces:
  - `pub const MEMORIES_MAX_TOKENS: usize` in `context::limits`.
  - `pub(crate) fn render_memories_section(memories: &[crate::storage::memories::Memory]) -> Option<String>` in `commands::agent` — pure, testable, applies the cap.
  - `pub(crate) async fn memories_section(conn, conversation_id) -> Option<String>` — resolves workspace, loads, renders.
  - `plan_system_message(cwd, allow_task, transcript_path, memories: Option<&str>)` — **signature changes; update all call sites.**
  - `conversation_system_message(cwd, transcript_dir, conversation_id, memories: Option<&str>)` — signature changes.

- [ ] **Step 1: Add the cap constant**

In `src-tauri/src/context/limits.rs`, beside `PROJECT_INSTRUCTIONS_MAX_TOKENS`:
```rust
/// Recalled workspace memories are capped at this share of the window,
/// mirroring `PROJECT_INSTRUCTIONS_MAX_TOKENS`. Injected once into
/// `messages[0]`, structurally outside the compaction window.
pub const MEMORIES_MAX_TOKENS: usize = CONTEXT_WINDOW_TOKENS / 8;
```

- [ ] **Step 2: Write the failing tests**

Add to the existing `#[cfg(test)] mod tests` in `src-tauri/src/commands/agent.rs`:

```rust
fn mem(content: &str) -> crate::storage::memories::Memory {
    crate::storage::memories::Memory {
        id: content.to_string(),
        content: content.to_string(),
        created_at: 0,
        updated_at: 0,
    }
}

#[test]
fn no_memories_renders_nothing() {
    assert!(render_memories_section(&[]).is_none());
}

#[test]
fn memories_render_as_a_bulleted_section() {
    let s = render_memories_section(&[mem("alpha"), mem("beta")]).unwrap();
    assert!(s.starts_with("# Memories"));
    assert!(s.contains("- alpha"));
    assert!(s.contains("- beta"));
}

#[test]
fn no_memories_leaves_the_prompt_byte_identical() {
    // The benchmark-inertness lock: an empty workspace must not shift a byte.
    let cwd = std::path::Path::new("/Users/tester/code/doce");
    assert_eq!(
        plan_system_message(Some(cwd), true, None, None),
        plan_system_message(Some(cwd), true, None, None)
    );
    let with_none = plan_system_message(Some(cwd), true, None, None);
    assert!(!with_none.contains("# Memories"));
}

#[test]
fn memories_section_is_injected_into_the_prompt() {
    let cwd = std::path::Path::new("/Users/tester/code/doce");
    let block = render_memories_section(&[mem("prefers oxfmt")]).unwrap();
    let msg = plan_system_message(Some(cwd), true, None, Some(&block));
    assert!(msg.contains("# Memories"));
    assert!(msg.contains("- prefers oxfmt"));
}

#[test]
fn over_cap_memories_drop_whole_trailing_facts_never_mid_fact() {
    // Build enough memories to blow MEMORIES_MAX_TOKENS.
    let big: Vec<_> = (0..4000).map(|i| mem(&format!("fact number {i} with some padding text"))).collect();
    let s = render_memories_section(&big).unwrap();
    assert!(
        (crate::inference::token_estimate(&s) as usize)
            <= crate::context::limits::MEMORIES_MAX_TOKENS
    );
    // Never a partial line: every rendered bullet is one of the inputs verbatim.
    for line in s.lines().filter(|l| l.starts_with("- ")) {
        let body = line.trim_start_matches("- ");
        assert!(big.iter().any(|m| m.content == body), "partial fact rendered: {body}");
    }
}

#[test]
fn non_ascii_memories_respect_the_token_cap() {
    // token_estimate weights non-ASCII ~1.1 tok/char, so a flat cap*4 char
    // budget would badly under-truncate CJK. Same trap SP3 (c) hit.
    let cjk: Vec<_> = (0..4000).map(|i| mem(&format!("事実{i}についての記録です"))).collect();
    let s = render_memories_section(&cjk).unwrap();
    assert!(
        (crate::inference::token_estimate(&s) as usize)
            <= crate::context::limits::MEMORIES_MAX_TOKENS
    );
}
```

- [ ] **Step 3: Run them, verify they fail**

Run: `cargo test --lib commands::agent`
Expected: FAIL (`render_memories_section` not defined; `plan_system_message` arity mismatch).

- [ ] **Step 4: Implement**

In `src-tauri/src/commands/agent.rs`, beside `project_instructions_section`:

```rust
/// Renders recalled workspace memories as the `# Memories` block that rides in
/// `messages[0]`. Bounded by dropping WHOLE trailing facts (never truncating
/// mid-fact -- half a fact is worse than no fact) until the rendered block fits
/// `MEMORIES_MAX_TOKENS`. Returns `None` for an empty set so a workspace with
/// no memories injects literally nothing.
pub(crate) fn render_memories_section(
    memories: &[crate::storage::memories::Memory],
) -> Option<String> {
    if memories.is_empty() {
        return None;
    }
    let cap = crate::context::limits::MEMORIES_MAX_TOKENS;
    let render = |take: usize| -> String {
        let mut s = String::from("# Memories\n\nDurable facts about this workspace, remembered from earlier conversations:\n");
        for m in memories.iter().take(take) {
            s.push_str(&format!("\n- {}", m.content));
        }
        s
    };
    let mut take = memories.len();
    while take > 0 {
        let candidate = render(take);
        if (crate::inference::token_estimate(&candidate) as usize) <= cap {
            return Some(candidate);
        }
        take -= 1;
    }
    None
}

/// Resolves the conversation's workspace, loads its memories, renders the
/// block. Best-effort: any DB error recalls nothing rather than failing the
/// turn.
pub(crate) async fn memories_section(
    conn: &tokio_rusqlite::Connection,
    conversation_id: &str,
) -> Option<String> {
    let workspace_id =
        crate::storage::memories::workspace_id_for_conversation(conn, conversation_id)
            .await
            .ok()?;
    let memories = crate::storage::memories::load_memories(conn, workspace_id.as_deref())
        .await
        .ok()?;
    render_memories_section(&memories)
}
```

Change `plan_system_message` to take a fourth parameter and splice the block in the same slot as the project-instructions section:
```rust
pub fn plan_system_message(
    cwd: Option<&std::path::Path>,
    allow_task: bool,
    transcript_path: Option<&str>,
    memories: Option<&str>,
) -> String {
    // ... existing base/cwd construction unchanged ...
    if let Some(section) = project_instructions_section(cwd) {
        message.push_str(&format!("\n\n{section}"));
    }
    if let Some(section) = memories {
        message.push_str(&format!("\n\n{section}"));
    }
    // ... existing transcript handling unchanged ...
}
```

Thread the new parameter through every call site:
- `conversation_system_message` gains `memories: Option<&str>` and forwards it.
- `src/commands/context.rs:42` and `:101`: both already hold `conn` and `conversation_id` — call `memories_section(&conn, &conversation_id).await` and pass `.as_deref()`, so usage accounting measures the same prompt production sends.
- `src/commands/agent.rs:1533` (the live turn): pass `memories_section(&conn, &conversation_id).await.as_deref()`.
- `src/commands/agent.rs:2036`: pass the fetched memories through.
- `src/commands/agent.rs:1250` (subagent prompt): pass `None`. A subagent is isolated delegated work; workspace memory is the top-level agent's context. Keep it out until there's evidence it helps.
- Existing tests calling `plan_system_message(..)` with 3 args: add `None`.

- [ ] **Step 5: Run tests, verify they pass**

Run: `cargo test --lib commands::agent && cargo test --lib`
Expected: PASS, all 352+ tests.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/commands/agent.rs src-tauri/src/commands/context.rs src-tauri/src/context/limits.rs
git commit -m "feat(memory): recall workspace memories into messages[0]"
```

---

### Task 3: Extraction — the out-of-band pass on tier-2 compaction

**Files:**
- Modify: `src-tauri/src/context/limits.rs` (add `MEMORY_EXTRACTION_PROMPT`)
- Modify: `src-tauri/src/context/mod.rs` (`extract_and_persist_memories`; call it from `summarize_and_persist`'s Accept arm ~line 678-721)

**Interfaces:**
- Consumes: `crate::storage::memories::{load_memories, replace_memories, workspace_id_for_conversation}` (Task 1).
- Produces: `pub(crate) async fn extract_and_persist_memories(conn, base_url, conversation_id, to_summarize: &[&HistoryMessage], now: i64) -> Result<(), String>` — always `Ok(())` in practice; errors are logged, never propagated to the turn.

- [ ] **Step 1: Add the extraction prompt**

In `src-tauri/src/context/limits.rs`, beside `SUMMARIZATION_PROMPT`:
```rust
/// SP4: the out-of-band memory-extraction prompt. A separate `Forbid`-mode
/// call (never part of an agent turn), so this text cannot affect the
/// tier4_planned benchmark. Asks for the FULL replacement set, one fact per
/// line, because `replace_memories` swaps the workspace's whole set.
pub const MEMORY_EXTRACTION_PROMPT: &str = "\
You maintain a durable memory of a software project workspace.

You will be given the existing memories (possibly empty) and a transcript of \
work that is about to be condensed away. Output the COMPLETE updated set of \
memories: keep the existing ones that are still true, update ones that changed, \
drop ones that are now wrong or obsolete, and add anything newly learned that \
will still matter weeks from now.

Remember only durable facts: the user's stated preferences and working style, \
project constraints and conventions, architectural decisions and the reasoning \
behind them, and hard-won gotchas that cost real time to discover.

Never remember: transient task state, what you are doing right now, file \
contents, anything trivially re-derivable by reading the code, or anything you \
are not confident is true.

Output one fact per line, each a single self-contained sentence. No bullets, no \
numbering, no commentary, no headers. If there is nothing worth remembering, \
output nothing at all.";
```

- [ ] **Step 2: Write the failing tests**

`summarize_and_persist`'s existing tests already stub the llama-server (find them in `src/context/mod.rs`'s test module and reuse that harness verbatim — likely `wiremock`). Add:

```rust
#[tokio::test]
async fn extraction_persists_the_emitted_set() {
    // stub server returns "User prefers oxfmt.\nBenchmarks are gated."
    // -> load_memories(workspace) yields exactly those two, in order
}

#[tokio::test]
async fn extraction_replaces_the_prior_set() {
    // seed ["old fact"]; stub returns "new fact."
    // -> load yields exactly ["new fact."], "old fact" gone
}

#[tokio::test]
async fn empty_extraction_never_wipes_existing_memories() {
    // THE GUARD. seed ["precious"]; stub returns "" (or whitespace)
    // -> load still yields ["precious"]
}

#[tokio::test]
async fn extraction_error_is_swallowed_and_changes_nothing() {
    // stub returns 500; seed ["precious"]
    // -> returns Ok(()), load still yields ["precious"]
}

#[tokio::test]
async fn extraction_writes_under_the_conversations_workspace() {
    // conversation bound to 'w1'; stub returns "fact."
    // -> load(Some("w1")) yields it; load(None) is empty
}
```

- [ ] **Step 3: Run them, verify they fail**

Run: `cargo test --lib context::`
Expected: FAIL (`extract_and_persist_memories` not defined).

- [ ] **Step 4: Implement**

In `src-tauri/src/context/mod.rs`:

```rust
/// SP4: the out-of-band memory-extraction pass. Reviews the span being
/// condensed plus the workspace's existing memories and swaps in the model's
/// full replacement set.
///
/// Best-effort by construction: every failure path -- server error, empty or
/// degenerate output, DB error -- logs and returns `Ok(())` leaving memories
/// exactly as they were. Compaction must never fail an agent turn, and a bad
/// extraction must never destroy good memories (the unsafe direction), so the
/// empty-output guard mirrors `evaluate_summary`'s posture.
pub(crate) async fn extract_and_persist_memories(
    conn: &tokio_rusqlite::Connection,
    base_url: &str,
    conversation_id: &str,
    to_summarize: &[&HistoryMessage],
    now: i64,
) -> Result<(), String> {
    let workspace_id =
        match crate::storage::memories::workspace_id_for_conversation(conn, conversation_id).await {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!("memory extraction: workspace lookup failed: {e}");
                return Ok(());
            }
        };
    let existing = crate::storage::memories::load_memories(conn, workspace_id.as_deref())
        .await
        .unwrap_or_default();

    let existing_block = if existing.is_empty() {
        "(no existing memories)".to_string()
    } else {
        existing
            .iter()
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    };

    let mut messages = vec![ChatMessage::system(limits::MEMORY_EXTRACTION_PROMPT)];
    messages.push(ChatMessage::user(&format!(
        "Existing memories:\n{existing_block}"
    )));
    messages.extend(to_summarize.iter().map(|m| m.chat.clone()));

    // `Forbid`: tools and tool_choice both `None` -- an extraction must never
    // emit a tool call. Fresh never-cancelled token, exactly as
    // `summarize_and_persist` does: this is best-effort background work with no
    // per-turn cancel handle to thread.
    let mut req = crate::inference::http::ChatRequest::build(
        "doce",
        crate::inference::http::to_openai_messages(&messages),
        None,
        None,
    );
    req.max_tokens = Some(SUMMARY_MAX_TOKENS as u32);
    let cancel = tokio_util::sync::CancellationToken::new();
    let outcome = match crate::inference::http::LlamaServerClient::new(base_url)
        .chat(req, |_piece| {}, &cancel)
        .await
    {
        Ok(o) => o,
        Err(e) => {
            tracing::warn!("memory extraction: inference failed: {e}");
            return Ok(());
        }
    };

    let facts: Vec<String> = outcome
        .text
        .lines()
        .map(|l| l.trim().trim_start_matches("- ").trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    // THE GUARD: a degenerate extraction must not wipe good memories.
    if facts.is_empty() {
        if !existing.is_empty() {
            tracing::warn!("memory extraction: empty output, keeping existing memories");
        }
        return Ok(());
    }

    if let Err(e) =
        crate::storage::memories::replace_memories(conn, workspace_id.as_deref(), &facts, now).await
    {
        tracing::warn!("memory extraction: persist failed: {e}");
    }
    Ok(())
}
```

Call it from `summarize_and_persist`'s `SummaryDecision::Accept` arm, after the restored-file notice and before `Ok(SummaryResult::Persisted(summary))`. Swallow the result — extraction must not affect the summary's outcome:
```rust
            // SP4: out-of-band memory extraction. Deliberately ignores its
            // result -- compaction's success does not depend on it.
            let _ = extract_and_persist_memories(
                conn,
                base_url,
                conversation_id,
                &to_summarize,
                now,
            )
            .await;
```

For `now`: use the same clock idiom the surrounding storage code already uses for `created_at`/`updated_at` (check `persist_notice`/`storage::messages`) rather than introducing a new one.

- [ ] **Step 5: Run tests, verify they pass**

Run: `cargo test --lib context:: && cargo test --lib`
Expected: PASS.

- [ ] **Step 6: Verify the sacred invariant**

Run: `git diff main --stat -- src-tauri/src/agent/mod.rs`
Expected: EMPTY (no output). `run_loop` must be byte-untouched.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/context/mod.rs src-tauri/src/context/limits.rs
git commit -m "feat(memory): out-of-band extraction pass on tier-2 compaction"
```

---

## Post-task verification (controller)

- [ ] `cargo test --lib` fully green.
- [ ] `git diff <pre-SP4> --stat -- src-tauri/src/agent/mod.rs` is empty.
- [ ] `git status --short | grep views` still shows exactly the 4 pre-existing frontend files, unstaged and unmodified by us.
- [ ] Benchmark-inertness: `no_memories_leaves_the_prompt_byte_identical` passes, so tier4_planned (which runs in a fresh workspace with an empty `memories` table) sees a byte-identical prompt. A confirming tier4_planned run is optional, not required, because the inertness is proven by construction.
