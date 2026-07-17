use crate::agent::rich_content::{expand_segments, RichMessageContent};
use crate::inference::{ChatMessage, MessageContent};
use rusqlite::Connection;
use std::path::Path;

const MAX_TITLE_LEN: usize = 60;

/// The most recent `summarized` context_notice, as `load_history_annotated`
/// reads it back. The two sequences are different numbers with different
/// jobs and the whole of BUG 1 (2026-07-15) was conflating them — see that
/// function's own comments.
struct SummarySplice {
    /// The notice ROW's own sequence. Always the last sequence in the
    /// conversation at the moment it was written (`messages::insert`
    /// allocates `MAX(sequence) + 1`). Only used to date the restored-file
    /// notice that belongs to this same compaction pass.
    notice_sequence: i64,
    /// The sequence of the LAST message the summary actually covers
    /// (`context::messages_to_summarize`'s span end). THE SPLICE POINT.
    through_sequence: i64,
    summary: String,
}

/// One row of conversation history, still tagged with its `content_type`
/// and `sequence` (010-context-window-management) — `load_history`'s plain
/// `ChatMessage` discards both, which is fine for callers that only need
/// the rendered prompt, but the compaction pipeline needs `content_type` to
/// find tool_call/tool_result rows (tier 1) and `sequence` to order/splice
/// against persisted `context_notice` rows (tier 2).
#[derive(Debug, Clone)]
pub struct HistoryMessage {
    pub chat: ChatMessage,
    pub content_type: String,
    pub sequence: i64,
    /// True for a `tool_call`/`tool_result` row whose persisted JSON
    /// `detail` blob (data-model.md) carries `"plan": true` — the
    /// plan-machine tools (`commands::agent::persist_plan_tool`), parsed
    /// once here at load time (`parse_tool_row_flags`) since `chat`'s own
    /// reconstruction above discards `content` in favor of the plain
    /// model-facing text (`model_text`). Tier 1
    /// (`context::apply_lightweight_clearing`) reads this to clear a plan
    /// row under `PLAN_KEEP_N` rather than `TOOL_KEEP_N`. Always `false`
    /// for every other content type — nothing reads it for those.
    pub plan: bool,
    /// The `detail.payloadRef` path for a `tool_result` row — the payload
    /// file every staged result now writes
    /// (`context::payload::stage_tool_result`), or, for a `Read` row, the
    /// source file itself — parsed the same way as `plan`. Falls back to
    /// the legacy `detail.offloadedTo` key for a row persisted before this
    /// rename. Tier 1 uses this to clear the row to a restorable
    /// `Read`-able pointer instead of the plain placeholder. Always `None`
    /// for every other content type, and for a tool row with neither key.
    ///
    /// Replaces this struct's former `raw_content: String` field (a review
    /// finding: keeping every row's full raw JSON around for the lifetime
    /// of every loaded history duplicated large chat turns and untruncated
    /// detail blobs in memory just so a later pass could parse it once
    /// more) — these two parsed facts are the only thing tier 1 actually
    /// needs out of that JSON, so they're computed once here and the
    /// string itself is dropped.
    pub payload_ref: Option<String>,
    /// The `messages.tool_name` column for a `tool_call`/`tool_result` row;
    /// `None` for other content types. Used by
    /// `context::most_recent_read_path` to find the most-recent `Read`
    /// result after a compaction.
    pub tool_name: Option<String>,
}

/// Parses a tool row's persisted `detail` JSON (data-model.md) for the two
/// flags tier 1 needs — `"plan"` and `"payloadRef"` (falling back to the
/// legacy `"offloadedTo"` key for a row persisted before that rename) —
/// once here at load time rather than keeping the raw JSON string around
/// for `apply_lightweight_clearing` to re-parse later. `content` is only
/// ever `detail`-shaped JSON for a `tool_call`/`tool_result` row; callers
/// must not invoke this for any other content type.
fn parse_tool_row_flags(content: &str) -> (bool, Option<String>) {
    let parsed: Option<serde_json::Value> = serde_json::from_str(content).ok();
    let plan = parsed
        .as_ref()
        .and_then(|v| v.get("plan"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let payload_ref = parsed
        .as_ref()
        .and_then(|v| v.get("payloadRef").or_else(|| v.get("offloadedTo")))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    (plan, payload_ref)
}

/// Builds the chat-template message history for a conversation: every
/// non-error message so far, oldest first, role-mapped from the
/// `messages` table's `role` column, still tagged with `content_type`/
/// `sequence`. Used by `commands::agent::send_agent_message` — without
/// this, every reply was generated with no memory of earlier turns in the
/// same conversation, on top of the separate missing-chat-template bug.
/// `content_type = 'error'` rows are UI-only failure notices, not real
/// assistant output, so they're excluded rather than fed back to the
/// model as if it had said them.
///
/// 009-rich-chat-input User Story 2: a `content_type = 'rich_text'` row is
/// parsed and expanded via `expand_segments(..., expand_skills: true)`
/// (see `expand_rich_text` below) rather than fed to the model as raw
/// JSON. This is what keeps a skill/paste/attachment selected on an
/// earlier turn influencing every later turn — without it, that context
/// would silently stop applying the moment a new message is sent, since
/// only the current turn's own message would ever get expanded.
/// `skills_dir` is threaded in (rather than resolved internally) because
/// this function is synchronous `&Connection`-only, called from inside a
/// `conn.call(...)` closure with no `AppHandle` in scope — callers resolve
/// it the same way `commands::skills::list_skills` already does
/// (`app.path().app_data_dir()?.join("skills")`).
///
/// 010-context-window-management: a `content_type = 'context_notice'` row
/// is never itself returned as an ordinary history entry (like `error`, it
/// isn't real turn content) — *except* that it is how BOTH compaction tiers
/// reach the model, since this function is the only thing that stands
/// between the persisted `messages` rows and the prompt
/// (`commands::agent::send_agent_message` seeds from `load_history`, a thin
/// wrapper over this):
///
/// * **Tier 2 (`"kind":"summarized"`)** marks a splice point. Every row at
///   or before the notice's `throughSequence` — the sequence of the last
///   message the summary actually covers (`context::summarized_notice_json`)
///   — is dropped and replaced by a synthesized system-role message
///   carrying the `summary` field, EXCEPT the keep-first genuine user
///   message (see `keep_first_sequence` below). A second, later
///   `summarized` notice supersedes the first — only the most recent one is
///   spliced.
/// * **Tier 1 (`"kind":"cleared"`)** replays a persisted clearing: every
///   `ClearedRow` in every `cleared` notice names a tool row's `sequence`
///   and the placeholder text tier 1 substituted for its content
///   (`context::cleared_notice_json`). Splices nothing.
///
/// The resulting shape after a tier-2 compaction is exactly:
/// `[keep-first user message] + [summary] + [restored file, if any] +
/// [the messages after the summarized span]`
/// — i.e. the summary replaces exactly the span `context::
/// messages_to_summarize` selected, no more and no less. That function is
/// the authority on what the summary covers: it deliberately excludes the
/// first genuine user message and the most recent
/// `PROTECTED_RECENT_MESSAGES`, so those are precisely the messages the
/// summary does NOT describe and which therefore have to survive verbatim
/// or be lost outright.
///
/// The user's own view of the conversation is never affected by any of
/// this: `commands::conversations::list_messages` reads the `messages`
/// table directly, so the transcript UI keeps showing every message and
/// every tool result in full, whatever the model is being seeded with.
pub fn load_history_annotated(
    conn: &Connection,
    conversation_id: &str,
    skills_dir: &Path,
) -> rusqlite::Result<Vec<HistoryMessage>> {
    let mut stmt = conn.prepare(
        "SELECT role, content_type, content, sequence, tool_name, tool_call_id, model_text FROM messages WHERE conversation_id = ?1 AND content_type != 'error' ORDER BY sequence ASC",
    )?;
    let rows = stmt
        .query_map([conversation_id], |row| {
            let role: String = row.get(0)?;
            let content_type: String = row.get(1)?;
            let content: String = row.get(2)?;
            let sequence: i64 = row.get(3)?;
            let tool_name: Option<String> = row.get(4)?;
            let tool_call_id: Option<String> = row.get(5)?;
            let model_text: Option<String> = row.get(6)?;
            Ok((
                role,
                content_type,
                content,
                sequence,
                tool_name,
                tool_call_id,
                model_text,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    // Find the most recent `context_notice` row of kind "summarized" (if
    // any): its embedded `summary` replaces the span it covers, and that
    // span ENDS at `throughSequence`.
    //
    // `notice_sequence` (the row's own) and `through_sequence` (the span's
    // end) are emphatically NOT the same number and must not be conflated:
    // `storage::messages::insert` allocates `COALESCE(MAX(sequence), -1) + 1`,
    // so a notice ALWAYS lands last, past every real message. Splicing at
    // the notice's own sequence -- which this did until 2026-07-15 -- meant
    // `sequence <= splice_sequence` was true for every row that existed, so
    // every tier-2 compaction silently deleted the whole conversation (18
    // messages reloaded as 1) while reporting `state:"justCompacted"` and a
    // usage drop. `notice_sequence` still has one job, below: dating the
    // restored-file notice that belongs to THIS compaction pass.
    //
    // BACKWARD COMPATIBILITY: a notice persisted by that buggy code has no
    // `throughSequence`, and falls back to the notice row's own sequence --
    // today's (wrong) behavior, deliberately. Those conversations are
    // already destroyed: the rows are unreachable, the user has long since
    // been shown a condensed conversation, and resurrecting them now would
    // hand the model an unbounded history it was never sized for. The one
    // thing the fallback DOES recover is the keep-first task statement,
    // which is exempted below regardless of which splice point applies --
    // the summary never described it under either code path.
    let splice: Option<SummarySplice> = rows
        .iter()
        .filter(|(_, content_type, ..)| content_type == "context_notice")
        .filter_map(|(_, _, content, sequence, ..)| {
            let parsed: serde_json::Value = serde_json::from_str(content).ok()?;
            if parsed.get("kind")?.as_str()? != "summarized" {
                return None;
            }
            Some(SummarySplice {
                notice_sequence: *sequence,
                through_sequence: parsed
                    .get("throughSequence")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(*sequence),
                summary: parsed.get("summary")?.as_str()?.to_string(),
            })
        })
        .max_by_key(|s| s.notice_sequence);

    // The one row at-or-before the splice point that SURVIVES it: the first
    // genuine user message -- the task statement. `context::
    // messages_to_summarize` excludes it from every span it hands the model
    // (its own doc comment: a summarization pass "must never be the thing
    // that makes the model forget what it was asked to do"), so the summary
    // does not describe it, and dropping it here would leave nothing at all
    // standing in for what the user asked. Matched with the same predicate
    // `messages_to_summarize` itself uses, against the raw row.
    let keep_first_sequence: Option<i64> = splice.as_ref().and_then(|splice| {
        rows.iter()
            .find(|(role, content_type, ..)| is_genuine_user_row(role, content_type))
            .map(|(_, _, _, sequence, _, _, _)| *sequence)
            .filter(|sequence| *sequence <= splice.through_sequence)
    });

    // FR-3 (restore-recent-file): the restored-file notice `context::
    // summarize_and_persist` persists right after a `summarized` notice, if
    // any -- its `restored` field is the most-recently-`Read` file's
    // ACTUAL content, re-read fresh at compaction time. Only one is ever
    // relevant: the one persisted for THIS splice's own compaction pass.
    // That is `sequence > notice_sequence` -- the NOTICE ROW's own sequence,
    // NOT `through_sequence`: a restored-file row is persisted immediately
    // after its summary, so it is the next row after the notice, while
    // `through_sequence` points far back into the conversation and would
    // match every restored-file row ever written for it, including the
    // superseded ones this rule exists to exclude.
    let restored_file: Option<(i64, String)> = splice.as_ref().and_then(|splice| {
        rows.iter()
            .filter(|(_, content_type, ..)| content_type == "context_notice")
            .filter_map(|(_, _, content, sequence, ..)| {
                if *sequence <= splice.notice_sequence {
                    return None;
                }
                let parsed: serde_json::Value = serde_json::from_str(content).ok()?;
                if parsed.get("kind")?.as_str()? == "restoredFile" {
                    let restored = parsed.get("restored")?.as_str()?.to_string();
                    Some((*sequence, restored))
                } else {
                    None
                }
            })
            .max_by_key(|(sequence, _)| *sequence)
    });

    // Collected before `rows` is consumed below; applied after, once every
    // surviving row has been rebuilt.
    let cleared = cleared_rows(
        rows.iter()
            .filter(|(_, content_type, ..)| content_type == "context_notice")
            .map(|(_, _, content, ..)| content.as_str()),
    );

    let mut result = Vec::new();
    for (role, content_type, content, sequence, tool_name, tool_call_id, model_text) in rows {
        if content_type == "context_notice" {
            continue;
        }
        // The summarized span, replaced below by the summary itself -- minus
        // the keep-first task statement, which no summary ever covered.
        if let Some(splice) = &splice {
            if sequence <= splice.through_sequence && Some(sequence) != keep_first_sequence {
                continue;
            }
        }

        // Parsed once, before `content` is consumed/moved below, only for
        // the two content types whose `content` is actually `detail`-shaped
        // JSON -- tier 1 (`context::apply_lightweight_clearing`) reads
        // these two fields back off the resulting `HistoryMessage` rather
        // than re-parsing raw JSON itself.
        let (plan, payload_ref) = if content_type == "tool_call" || content_type == "tool_result" {
            parse_tool_row_flags(&content)
        } else {
            (false, None)
        };
        // `tool_name` is moved into `name` below inside the tool_call/
        // tool_result arms -- cloned here first so the resulting
        // `HistoryMessage` still carries it (`context::most_recent_read_path`
        // needs it after a compaction, FR-3).
        let tool_name_for_history = tool_name.clone();

        // 010-context-window-management (structured tool calls): a
        // `tool_call`/`tool_result` row reconstructs into the same
        // MessageContent::ToolUse/ToolResult variant `agent::run_loop`
        // pushes live, rather than feeding the raw persisted JSON back to
        // the model as if it were plain text -- a real, pre-existing bug
        // (a reloaded conversation showed the model malformed-looking JSON
        // blobs for its own past tool activity that it never actually
        // produced in that shape). `tool_call_id` being `None` only
        // happens for a row persisted before migration 0006 -- falls back
        // to a synthetic per-row id (never reused, so still safe to treat
        // as a real call/result pair) rather than crashing on old data.
        let chat = if content_type == "tool_call" {
            let id = tool_call_id.unwrap_or_else(|| format!("legacy-{sequence}"));
            let name = tool_name.unwrap_or_default();
            let arguments = serde_json::from_str::<serde_json::Value>(&content)
                .ok()
                .and_then(|v| v.get("arguments").cloned())
                .unwrap_or(serde_json::Value::Null);
            ChatMessage::tool_use(id, name, arguments)
        } else if content_type == "tool_result" {
            let id = tool_call_id.unwrap_or_else(|| format!("legacy-{sequence}"));
            let name = tool_name.unwrap_or_default();
            // `model_text` only exists from migration 0006 onward — an
            // older row falls back to its raw `content` (its own prior
            // behavior), still better than silently losing the row.
            let text = model_text.unwrap_or(content);
            ChatMessage::tool_result(id, name, text)
        } else {
            let text = if content_type == "rich_text" {
                expand_rich_text(&content, skills_dir)
            } else {
                // 'text' behaves exactly as before this feature existed:
                // the raw column value, untouched.
                content
            };
            match role.as_str() {
                "assistant" => ChatMessage::assistant(text),
                _ => ChatMessage::user(text),
            }
        };
        result.push(HistoryMessage {
            chat,
            content_type,
            sequence,
            plan,
            payload_ref,
            tool_name: tool_name_for_history,
        });
    }

    if let Some(splice) = &splice {
        // The summary, then the restored file if there is one -- the model
        // sees the restored file's real content immediately following the
        // condensed summary that dropped it, not buried among the ordinary
        // turns that follow.
        let mut synthesized: Vec<String> = vec![splice.summary.clone()];
        synthesized.extend(restored_file.iter().map(|(_, restored)| restored.clone()));

        // These rows stand in for the span they replaced, so they take that
        // span's own trailing sequences -- free by construction (every row
        // at-or-before `through_sequence` was just dropped, bar keep-first)
        // and strictly between the keep-first row and the first surviving
        // turn, which is what keeps the reloaded history ordered by
        // `sequence` the way every caller reads it. Their own rows'
        // sequences would be useless here: `messages::insert` allocated
        // those past the end of the conversation.
        //
        // The clamp is belt-and-braces: a restored-file note only exists
        // when the span contained a `Read` tool_result (`context::
        // most_recent_read_path`), which is always preceded by its own
        // tool_call row in that same span, so a two-row block always has two
        // sequences of span to land in above keep-first. If that ever stops
        // holding, the summary must still never be given the task
        // statement's own sequence, or worse a lower one -- the reloaded
        // history would then read as though the summary preceded the request
        // it summarizes.
        let first_sequence = (splice.through_sequence - (synthesized.len() as i64 - 1))
            .max(keep_first_sequence.map_or(i64::MIN, |k| k + 1));
        let at = result
            .iter()
            .position(|m| m.sequence > splice.through_sequence)
            .unwrap_or(result.len());
        let rows = synthesized
            .into_iter()
            .enumerate()
            .map(|(i, text)| HistoryMessage {
                chat: ChatMessage::system(text),
                content_type: "context_notice".to_string(),
                sequence: first_sequence + i as i64,
                plan: false,
                payload_ref: None,
                tool_name: None,
            })
            .collect::<Vec<_>>();
        result.splice(at..at, rows);
    }

    // Tier 1's clearing, replayed: a tool row named by any `cleared` notice
    // carries the placeholder that notice recorded, not its real content.
    // This is the ONLY thing that applies tier 1 to what the model receives
    // -- `context::maybe_compact` clears a local copy of the history and
    // then drops it, so before this replay existed the notice ("3 old tool
    // results cleared to save space") and the usage drop it reported were
    // both fiction: every byte was still seeded on the next turn. A notice
    // written before this field existed carries no `cleared` array and
    // replays nothing, which is exactly right -- that clearing never
    // happened.
    if !cleared.is_empty() {
        for message in &mut result {
            if message.content_type != "tool_call" && message.content_type != "tool_result" {
                continue;
            }
            if let Some(placeholder) = cleared.get(&message.sequence) {
                message.chat.content = MessageContent::Text(placeholder.clone());
            }
        }
    }

    Ok(result)
}

/// Every `(sequence, placeholder)` tier 1 has ever cleared in this
/// conversation, unioned across all its `cleared` notices
/// (`context::cleared_notice_json`). A row is only ever cleared once, so
/// later notices never contradict earlier ones; a notice whose JSON doesn't
/// parse, or predates the `cleared` array, simply contributes nothing.
fn cleared_rows<'a>(
    notice_contents: impl Iterator<Item = &'a str>,
) -> std::collections::HashMap<i64, String> {
    let mut cleared = std::collections::HashMap::new();
    for content in notice_contents {
        let Ok(parsed) = serde_json::from_str::<serde_json::Value>(content) else {
            continue;
        };
        if parsed.get("kind").and_then(|k| k.as_str()) != Some("cleared") {
            continue;
        }
        let Some(rows) = parsed.get("cleared") else {
            continue;
        };
        let Ok(rows) = serde_json::from_value::<Vec<crate::context::ClearedRow>>(rows.clone())
        else {
            continue;
        };
        for row in rows {
            cleared.insert(row.sequence, row.placeholder);
        }
    }
    cleared
}

/// True for a `messages` row that reconstructs into a genuine user-authored
/// turn — the "task statement" sense of user message. Deliberately distinct
/// from a `tool_result` row, which this function rebuilds with
/// `chat.role == "user"` too (see `ChatMessage::tool_result`) but which is
/// never something the user said.
///
/// Takes the raw `role`/`content_type` so the splice above can apply it to a
/// row it has not rebuilt yet, and `context::is_genuine_user_message` (which
/// applies it to the rebuilt `HistoryMessage`, where `chat.role` is already
/// `"user"`/`"assistant"`) delegates here rather than restating it: tier 2
/// picks the span with that one, the splice keeps the survivor with this
/// one, and if the two ever disagreed the message keep-first exists to
/// protect would be summarized away or dropped.
pub fn is_genuine_user_row(role: &str, content_type: &str) -> bool {
    // Mirrors this function's own role mapping above, where every
    // non-assistant `text`/`rich_text` row becomes a `ChatMessage::user`.
    (content_type == "text" || content_type == "rich_text") && role != "assistant"
}

/// Thin wrapper over `load_history_annotated` for callers that only need
/// the rendered prompt, not `content_type`/`sequence` — no SQL duplicated.
pub fn load_history(
    conn: &Connection,
    conversation_id: &str,
    skills_dir: &Path,
) -> rusqlite::Result<Vec<ChatMessage>> {
    Ok(load_history_annotated(conn, conversation_id, skills_dir)?
        .into_iter()
        .map(|m| m.chat)
        .collect())
}

/// What the model (and the transcript) is told about a tool call that was
/// interrupted by the app closing — actionable ("re-run it"), not just a
/// bare failure notice.
const INTERRUPTED_TOOL_TEXT: &str =
    "Error: interrupted — the app closed before this tool call finished. The tool did not run to completion; re-run it if its result is still needed.";

/// Crash recovery, run once at DB open (storage::open_and_migrate): pairs
/// every conversation whose *latest* message is a still-unpaired
/// `tool_call` row with an interrupted-error `tool_result`. Such a row can
/// only mean the app died (or was restarted) mid-tool — a live turn always
/// persists the result before anything else lands, and this runs before
/// any new turn can start. Without this, two things strand permanently:
/// the frontend treats a trailing unpaired tool_call as "turn in flight"
/// and keeps the composer disabled forever, and an orphaned
/// AskUserQuestion can never be answered (PendingQuestions is in-memory,
/// empty after restart) so its conversation reads `requires_action` with
/// no way to act. Returns how many conversations were healed.
pub fn heal_interrupted_tool_calls(conn: &Connection, now: i64) -> rusqlite::Result<usize> {
    struct OrphanedToolCall {
        conversation_id: String,
        tool_call_id: Option<String>,
        tool_name: Option<String>,
        content: String,
        sequence: i64,
    }

    let orphans: Vec<OrphanedToolCall> = conn
        .prepare(
            "SELECT m.conversation_id, m.tool_call_id, m.tool_name, m.content, m.sequence
             FROM messages m
             WHERE m.content_type = 'tool_call'
               AND m.sequence = (SELECT MAX(sequence) FROM messages WHERE conversation_id = m.conversation_id)",
        )?
        .query_map([], |row| {
            Ok(OrphanedToolCall {
                conversation_id: row.get(0)?,
                tool_call_id: row.get(1)?,
                tool_name: row.get(2)?,
                content: row.get(3)?,
                sequence: row.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    for orphan in &orphans {
        let arguments = serde_json::from_str::<serde_json::Value>(&orphan.content)
            .ok()
            .and_then(|v| v.get("arguments").cloned())
            .unwrap_or(serde_json::Value::Null);
        let detail = interrupted_tool_result_detail(
            orphan.tool_name.as_deref().unwrap_or(""),
            &arguments,
            orphan.tool_call_id.as_deref().unwrap_or(""),
        );
        conn.execute(
            "INSERT INTO messages (id, conversation_id, role, content_type, content, tool_name, created_at, sequence, tool_call_id, model_text) VALUES (?1, ?2, 'tool', 'tool_result', ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                uuid::Uuid::now_v7().to_string(),
                orphan.conversation_id,
                detail.to_string(),
                orphan.tool_name,
                now,
                orphan.sequence + 1,
                orphan.tool_call_id,
                INTERRUPTED_TOOL_TEXT,
            ],
        )?;
    }

    Ok(orphans.len())
}

/// The interrupted `tool_result`'s widget-facing `detail`, mirroring the
/// exact per-tool shape `agent::dispatch`'s own error arms produce (from
/// the orphaned call's persisted arguments) — these shapes are what the
/// existing result widgets already render in production, so a healed row
/// needs no frontend special-casing.
fn interrupted_tool_result_detail(
    tool_name: &str,
    arguments: &serde_json::Value,
    tool_call_id: &str,
) -> serde_json::Value {
    let a = |key: &str| {
        arguments
            .get(key)
            .cloned()
            .unwrap_or(serde_json::Value::Null)
    };
    let error = INTERRUPTED_TOOL_TEXT;
    match tool_name {
        "Read" => serde_json::json!({
            "toolName": "Read", "filePath": a("file_path"), "offset": a("offset"), "limit": a("limit"), "interrupted": true,
            "outcome": {"ok": false, "error": error},
        }),
        "Write" => serde_json::json!({
            "toolName": "Write", "filePath": a("file_path"), "contentPreview": "", "byteCount": 0, "interrupted": true,
            "outcome": {"ok": false, "error": error},
        }),
        "Edit" => serde_json::json!({
            "toolName": "Edit", "filePath": a("file_path"), "oldString": a("old_string"),
            "newString": a("new_string"), "replaceAll": a("replace_all"), "interrupted": true,
            "outcome": {"ok": false, "error": error},
        }),
        "Bash" => serde_json::json!({
            "toolName": "Bash", "command": a("command"), "timeoutMs": a("timeout"), "interrupted": true,
            "outcome": {"ok": false, "error": error},
        }),
        "Glob" => serde_json::json!({
            "toolName": "Glob", "pattern": a("pattern"), "path": a("path"), "matches": [], "interrupted": true,
        }),
        "Grep" => serde_json::json!({
            "toolName": "Grep", "pattern": a("pattern"), "path": a("path"), "glob": a("glob"),
            "matches": [], "truncated": false, "skippedOversized": 0, "interrupted": true,
        }),
        "Task" => serde_json::json!({
            "toolName": "Task", "prompt": a("prompt"), "subagentConversationId": "", "state": "complete", "interrupted": true,
        }),
        // An empty answer is the honest representation of "never answered"
        // — the answered-question widget renders it as such.
        "AskUserQuestion" => serde_json::json!({
            "toolName": "AskUserQuestion", "questionId": tool_call_id, "header": a("header"),
            "question": a("question"),
            "options": arguments.get("options").cloned().unwrap_or(serde_json::json!([])),
            "multiSelect": a("multiSelect"), "answer": [], "interrupted": true,
        }),
        other => serde_json::json!({
            "toolName": other, "arguments": arguments, "interrupted": true,
            "outcome": {"ok": false, "error": error},
        }),
    }
}

/// Persists a `context_notice` row (010-context-window-management) —
/// `kind_json` is the row's full JSON `content`
/// (`{"kind":"cleared",...}`/`{"kind":"summarized",...}`, see
/// data-model.md). Always `role='assistant'` (the `messages.role` CHECK has
/// no `'system'` value; this matches how `error` rows are already
/// persisted under `role='assistant'` too) and `tool_name=NULL`.
pub fn persist_context_notice(
    conn: &Connection,
    transcript_dir: Option<&Path>,
    conversation_id: &str,
    now: i64,
    kind_json: &str,
) -> rusqlite::Result<()> {
    crate::storage::messages::insert(
        conn,
        transcript_dir,
        &crate::storage::messages::NewMessage {
            conversation_id,
            role: "assistant",
            content_type: "context_notice",
            content: kind_json,
            tool_name: None,
            tool_call_id: None,
            model_text: None,
            created_at: now,
            duration_ms: None,
            token_count: None,
        },
    )?;
    Ok(())
}

/// Expands one `rich_text` row's JSON `content` into the text the model
/// should see, per data-model.md's `load_history` section.
///
/// Failure mode (deliberate, since data-model.md doesn't dictate one):
/// malformed JSON or a `skill` segment naming a skill that's since been
/// renamed/deleted (FR-014) does **not** fail the whole history load —
/// `load_history`'s caller (`send_agent_message`/`send_message`) would
/// have no way to recover a whole conversation just because one old
/// message references a now-stale skill, and a single conversation should
/// not become permanently unloadable over it. Instead, only *this* row's
/// text is replaced with a short bracketed marker naming the failure; the
/// rest of the conversation's history (before and after this row) loads
/// normally, roles still alternate correctly, and the turn structure the
/// model sees stays intact. This is a page-level substitution rather than
/// a per-segment one because `expand_segments` itself is all-or-nothing
/// (`Result<String, String>` for the whole segment list, per
/// `agent/rich_content.rs`) — surfacing exactly which segment failed
/// isn't information this call site has available to it.
fn expand_rich_text(content: &str, skills_dir: &Path) -> String {
    serde_json::from_str::<RichMessageContent>(content)
        .map_err(|e| format!("malformed rich_text content: {e}"))
        .and_then(|parsed| expand_segments(&parsed.segments, skills_dir, true))
        .unwrap_or_else(|e| format!("[unable to load message: {e}]"))
}

/// Auto-generated conversation title (FR-012): truncates the first user
/// message at a word boundary around 60 chars, no model call involved.
/// Collapses internal whitespace/newlines so a multi-line first message
/// doesn't produce a title with embedded line breaks.
pub fn generate_title(first_message: &str) -> String {
    let normalized: String = first_message
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let trimmed = normalized.trim();

    if trimmed.is_empty() {
        return "New conversation".to_string();
    }
    if trimmed.chars().count() <= MAX_TITLE_LEN {
        return trimmed.to_string();
    }

    // Truncate at the last word boundary at-or-before MAX_TITLE_LEN chars,
    // falling back to a hard char-boundary cut if the first "word" alone
    // already exceeds the limit (e.g. one long unbroken token).
    let mut cut = 0;
    let mut last_space = None;
    for (byte_idx, ch) in trimmed.char_indices() {
        let char_count = trimmed[..byte_idx].chars().count();
        if char_count >= MAX_TITLE_LEN {
            break;
        }
        if ch == ' ' {
            last_space = Some(byte_idx);
        }
        cut = byte_idx + ch.len_utf8();
    }

    let truncated = match last_space {
        Some(space_idx) if space_idx > 0 => &trimmed[..space_idx],
        _ => &trimmed[..cut],
    };

    format!("{truncated}…")
}

/// Persists (or clears) a conversation's user-set goal (0011_conversation_goal
/// migration's `conversations.goal` column) — the single source of truth
/// `send_agent_message` reads at task start to populate `Plan.goal`
/// (`commands::agent::send_agent_message`, near its `PlanState::default()`).
/// `goal: None` or `Some("")` both clear the column to `NULL`, so a caller
/// never has to special-case an empty string versus "no goal" itself.
pub fn set_conversation_goal(
    conn: &Connection,
    conversation_id: &str,
    goal: Option<&str>,
) -> rusqlite::Result<()> {
    let goal = goal.filter(|g| !g.is_empty());
    conn.execute(
        "UPDATE conversations SET goal = ?1 WHERE id = ?2",
        rusqlite::params![goal, conversation_id],
    )?;
    Ok(())
}

/// Reads a conversation's goal back — `None` for both an unset (`NULL`)
/// column and a legacy empty-string value, so every caller sees the same
/// "no goal" signal regardless of how it got there.
pub fn get_conversation_goal(
    conn: &Connection,
    conversation_id: &str,
) -> rusqlite::Result<Option<String>> {
    let goal: Option<String> = conn.query_row(
        "SELECT goal FROM conversations WHERE id = ?1",
        [conversation_id],
        |row| row.get(0),
    )?;
    Ok(goal.filter(|g| !g.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inference::MessageContent;

    #[test]
    fn short_message_used_verbatim() {
        assert_eq!(generate_title("hello there"), "hello there");
    }

    #[test]
    fn empty_message_falls_back() {
        assert_eq!(generate_title(""), "New conversation");
        assert_eq!(generate_title("   "), "New conversation");
    }

    #[test]
    fn long_message_truncates_at_word_boundary() {
        let msg = "Can you help me refactor this really long function that has way too many responsibilities crammed into it";
        let title = generate_title(msg);
        assert!(title.ends_with('…'));
        assert!(title.chars().count() <= MAX_TITLE_LEN + 1);

        let body = title.trim_end_matches('…');
        let words: Vec<&str> = msg.split_whitespace().collect();
        // The truncated body must be an exact prefix run of whole words
        // from the original message, not a mid-word cut.
        let mut prefix = String::new();
        let mut matched = false;
        for (i, w) in words.iter().enumerate() {
            if i > 0 {
                prefix.push(' ');
            }
            prefix.push_str(w);
            if prefix == body {
                matched = true;
                break;
            }
        }
        assert!(
            matched,
            "title body {body:?} is not a whole-word prefix of the source message"
        );
    }

    #[test]
    fn collapses_internal_whitespace_and_newlines() {
        let msg = "line one\nline two\n\nline three";
        assert_eq!(generate_title(msg), "line one line two line three");
    }

    #[test]
    fn single_long_token_hard_cuts() {
        let msg = "a".repeat(200);
        let title = generate_title(&msg);
        assert!(title.ends_with('…'));
        assert!(title.chars().count() <= MAX_TITLE_LEN + 1);
    }

    fn setup_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE messages (
                id TEXT PRIMARY KEY, conversation_id TEXT NOT NULL, role TEXT NOT NULL,
                content_type TEXT NOT NULL, content TEXT NOT NULL, created_at INTEGER NOT NULL,
                sequence INTEGER NOT NULL, tool_name TEXT, tool_call_id TEXT, model_text TEXT,
                duration_ms INTEGER, token_count INTEGER
            );",
        )
        .unwrap();
        conn
    }

    fn insert_message(
        conn: &Connection,
        conv_id: &str,
        role: &str,
        content_type: &str,
        content: &str,
        seq: i64,
    ) {
        conn.execute(
            "INSERT INTO messages (id, conversation_id, role, content_type, content, created_at, sequence) VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6)",
            rusqlite::params![format!("{conv_id}-{seq}"), conv_id, role, content_type, content, seq],
        )
        .unwrap();
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_tool_message(
        conn: &Connection,
        conv_id: &str,
        role: &str,
        content_type: &str,
        content: &str,
        seq: i64,
        tool_name: &str,
        tool_call_id: &str,
        model_text: Option<&str>,
    ) {
        conn.execute(
            "INSERT INTO messages (id, conversation_id, role, content_type, content, created_at, sequence, tool_name, tool_call_id, model_text) VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?7, ?8, ?9)",
            rusqlite::params![format!("{conv_id}-{seq}"), conv_id, role, content_type, content, seq, tool_name, tool_call_id, model_text],
        )
        .unwrap();
    }

    // --- crash recovery: heal_interrupted_tool_calls ---

    #[test]
    fn heal_pairs_a_trailing_orphaned_tool_call_with_an_interrupted_error_result() {
        let conn = setup_conn();
        insert_message(&conn, "c1", "user", "text", "find the needle", 0);
        insert_tool_message(
            &conn,
            "c1",
            "assistant",
            "tool_call",
            r#"{"arguments":{"pattern":"needle","path":"/tmp"}}"#,
            1,
            "Grep",
            "tc1",
            None,
        );

        let healed = heal_interrupted_tool_calls(&conn, 42).unwrap();
        assert_eq!(healed, 1);

        let (role, content_type, tool_name, tool_call_id, model_text, content): (
            String,
            String,
            String,
            String,
            String,
            String,
        ) = conn
            .query_row(
                "SELECT role, content_type, tool_name, tool_call_id, model_text, content FROM messages WHERE conversation_id = 'c1' ORDER BY sequence DESC LIMIT 1",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(role, "tool");
        assert_eq!(content_type, "tool_result");
        assert_eq!(tool_name, "Grep");
        assert_eq!(tool_call_id, "tc1");
        assert!(
            model_text.contains("interrupted"),
            "the model must be told the tool never finished, got: {model_text:?}"
        );
        // The detail must be the same shape dispatch's own Grep arm
        // produces, so the existing result widget renders it untouched.
        let detail: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(detail["toolName"], "Grep");
        assert_eq!(detail["pattern"], "needle");
        assert_eq!(detail["matches"], serde_json::json!([]));
        // The widget-visible interruption marker — without it the healed
        // row renders as a successful empty search.
        assert_eq!(detail["interrupted"], true);

        // Model history is now a well-formed call/result pair again.
        let skills_dir = empty_skills_dir();
        let history = load_history(&conn, "c1", skills_dir.path()).unwrap();
        assert_eq!(history.len(), 3);
    }

    #[test]
    fn heal_leaves_paired_tool_calls_and_ordinary_latest_messages_alone() {
        let conn = setup_conn();
        // c1: completed pair — latest message is the tool_result.
        insert_tool_message(
            &conn,
            "c1",
            "assistant",
            "tool_call",
            r#"{"arguments":{"command":"ls"}}"#,
            0,
            "Bash",
            "tc1",
            None,
        );
        insert_tool_message(
            &conn,
            "c1",
            "tool",
            "tool_result",
            r#"{"toolName":"Bash","outcome":{"ok":true}}"#,
            1,
            "Bash",
            "tc1",
            Some("ok"),
        );
        // c2: latest message is a plain assistant answer.
        insert_message(&conn, "c2", "assistant", "text", "all done", 0);

        let healed = heal_interrupted_tool_calls(&conn, 42).unwrap();
        assert_eq!(healed, 0);

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 3, "no rows may be added or removed");
    }

    #[test]
    fn heal_gives_an_orphaned_ask_user_question_an_empty_answer_result() {
        // After a restart, PendingQuestions is empty — the persisted
        // pending question can never be answered, so leaving it "latest"
        // would strand the conversation in requires_action forever.
        let conn = setup_conn();
        insert_tool_message(
            &conn,
            "c1",
            "assistant",
            "tool_call",
            r#"{"arguments":{"header":"Pick","question":"Which?","options":[{"label":"A","description":""}],"multiSelect":false,"questionId":"q1"}}"#,
            0,
            "AskUserQuestion",
            "q1",
            None,
        );

        assert_eq!(heal_interrupted_tool_calls(&conn, 42).unwrap(), 1);

        let content: String = conn
            .query_row(
                "SELECT content FROM messages WHERE conversation_id = 'c1' AND content_type = 'tool_result'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let detail: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(detail["toolName"], "AskUserQuestion");
        assert_eq!(detail["questionId"], "q1");
        assert_eq!(detail["answer"], serde_json::json!([]));
        assert_eq!(detail["interrupted"], true);
    }

    #[test]
    fn reloaded_tool_call_and_result_reconstruct_as_structured_messages_not_raw_json() {
        // Regression: this used to feed the raw persisted JSON straight
        // back to the model as plain text on every reload after the very
        // first turn -- a real bug found live, not speculatively. The
        // model never actually produced that JSON shape itself; only the
        // *first* turn (built fresh in-memory by `agent::run_loop`) ever
        // looked right.
        let conn = setup_conn();
        insert_tool_message(
            &conn,
            "c1",
            "assistant",
            "tool_call",
            r#"{"arguments":{"command":"ls ."}}"#,
            0,
            "Bash",
            "call-1",
            None,
        );
        insert_tool_message(
            &conn,
            "c1",
            "tool",
            "tool_result",
            r#"{"toolName":"Bash","outcome":{"ok":true,"stdout":"a.txt\n"}}"#,
            1,
            "Bash",
            "call-1",
            Some("a.txt"),
        );

        let skills_dir = empty_skills_dir();
        let history = load_history_annotated(&conn, "c1", skills_dir.path()).unwrap();
        assert_eq!(history.len(), 2);

        match &history[0].chat.content {
            MessageContent::ToolUse { id, name, input } => {
                assert_eq!(id, "call-1");
                assert_eq!(name, "Bash");
                assert_eq!(input, &serde_json::json!({"command": "ls ."}));
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
        match &history[1].chat.content {
            MessageContent::ToolResult {
                tool_use_id,
                tool_name,
                content,
            } => {
                assert_eq!(tool_use_id, "call-1");
                assert_eq!(tool_name, "Bash");
                assert_eq!(content, "a.txt");
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
        // The rendered text the model actually sees matches exactly what
        // it would have seen live in the same turn, not a raw JSON dump.
        assert_eq!(
            history[1].chat.text(),
            "<tool_response>a.txt</tool_response>"
        );
    }

    // --- HistoryMessage.plan/payload_ref: parsed once at load time from a
    // tool row's `content` JSON (replacing the former raw_content: String
    // field a review finding on this feature flagged as duplicating every
    // row's full content in memory for the lifetime of every loaded
    // history) ---

    #[test]
    fn a_tool_call_rows_plan_marker_is_parsed_from_its_own_content() {
        // The regression this covers: a plan-machine tool's CALL row used
        // to persist with no "plan" marker at all (only its paired RESULT
        // row carried one), so this row's `plan` field must reflect
        // whatever is actually in ITS OWN content, not its paired row's.
        let conn = setup_conn();
        insert_tool_message(
            &conn,
            "c1",
            "assistant",
            "tool_call",
            r#"{"arguments":{"goal":"g","steps":["a"]},"plan":true}"#,
            0,
            "CreatePlan",
            "call-1",
            None,
        );

        let skills_dir = empty_skills_dir();
        let history = load_history_annotated(&conn, "c1", skills_dir.path()).unwrap();
        assert_eq!(history.len(), 1);
        assert!(
            history[0].plan,
            "the call row's own plan marker must be parsed, not defaulted to false"
        );
        assert_eq!(history[0].payload_ref, None);
    }

    #[test]
    fn a_tool_result_rows_payload_ref_is_parsed_from_its_own_content() {
        let conn = setup_conn();
        insert_tool_message(
            &conn,
            "c1",
            "tool",
            "tool_result",
            r#"{"toolName":"Bash","payloadRef":"/tmp/payload.txt"}"#,
            0,
            "Bash",
            "call-1",
            Some("preview..."),
        );

        let skills_dir = empty_skills_dir();
        let history = load_history_annotated(&conn, "c1", skills_dir.path()).unwrap();
        assert_eq!(history.len(), 1);
        assert!(!history[0].plan);
        assert_eq!(history[0].payload_ref.as_deref(), Some("/tmp/payload.txt"));
    }

    #[test]
    fn a_tool_result_rows_payload_ref_falls_back_to_the_legacy_offloaded_to_key() {
        let conn = setup_conn();
        insert_tool_message(
            &conn,
            "c1",
            "tool",
            "tool_result",
            r#"{"toolName":"Read","offloadedTo":"/tmp/offload.txt"}"#,
            0,
            "Read",
            "call-1",
            Some("preview..."),
        );

        let skills_dir = empty_skills_dir();
        let history = load_history_annotated(&conn, "c1", skills_dir.path()).unwrap();
        assert_eq!(history.len(), 1);
        assert!(!history[0].plan);
        assert_eq!(history[0].payload_ref.as_deref(), Some("/tmp/offload.txt"));
    }

    #[test]
    fn parse_tool_row_flags_reads_payload_ref_with_offloaded_to_fallback() {
        let (_, new_key) = parse_tool_row_flags(r#"{"payloadRef": "/p/new.txt"}"#);
        assert_eq!(new_key.as_deref(), Some("/p/new.txt"));
        let (_, legacy) = parse_tool_row_flags(r#"{"offloadedTo": "/p/old.txt"}"#);
        assert_eq!(legacy.as_deref(), Some("/p/old.txt"));
        let (_, both) =
            parse_tool_row_flags(r#"{"payloadRef": "/p/new.txt", "offloadedTo": "/p/old.txt"}"#);
        assert_eq!(both.as_deref(), Some("/p/new.txt"), "payloadRef wins");
    }

    #[test]
    fn tool_rows_with_no_plan_or_offloaded_to_key_parse_to_false_and_none() {
        let conn = setup_conn();
        insert_tool_message(
            &conn,
            "c1",
            "assistant",
            "tool_call",
            r#"{"arguments":{"command":"ls"}}"#,
            0,
            "Bash",
            "call-1",
            None,
        );
        insert_tool_message(
            &conn,
            "c1",
            "tool",
            "tool_result",
            r#"{"toolName":"Bash","outcome":{"ok":true}}"#,
            1,
            "Bash",
            "call-1",
            Some("ok"),
        );

        let skills_dir = empty_skills_dir();
        let history = load_history_annotated(&conn, "c1", skills_dir.path()).unwrap();
        assert_eq!(history.len(), 2);
        for message in &history {
            assert!(!message.plan);
            assert_eq!(message.payload_ref, None);
        }
    }

    #[test]
    fn non_tool_rows_never_parse_plan_or_offloaded_to_even_if_their_text_looks_like_json() {
        // A 'text'/'rich_text'/spliced-'context_notice' row's `content` is
        // never `detail`-shaped JSON -- parse_tool_row_flags must never run
        // against it, even in the pathological case where the plain text
        // itself happens to parse as JSON containing these same keys.
        let conn = setup_conn();
        insert_message(
            &conn,
            "c1",
            "user",
            "text",
            r#"{"plan": true, "offloadedTo": "/tmp/should-not-be-read.txt"}"#,
            0,
        );

        let skills_dir = empty_skills_dir();
        let history = load_history_annotated(&conn, "c1", skills_dir.path()).unwrap();
        assert_eq!(history.len(), 1);
        assert!(!history[0].plan);
        assert_eq!(history[0].payload_ref, None);
    }

    /// No test exercises a real skill directory except the two rich-text
    /// ones below, but every call site needs *some* `&Path` — an empty
    /// tempdir stands in for "no skills directory contents needed."
    fn empty_skills_dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn loads_history_in_sequence_order_with_roles_mapped() {
        let conn = setup_conn();
        insert_message(&conn, "c1", "user", "text", "hi", 0);
        insert_message(&conn, "c1", "assistant", "text", "hello", 1);
        insert_message(&conn, "c1", "user", "text", "how are you", 2);

        let skills_dir = empty_skills_dir();
        let history = load_history(&conn, "c1", skills_dir.path()).unwrap();
        assert_eq!(history.len(), 3);
        assert_eq!(history[0].role, "user");
        assert_eq!(history[0].text(), "hi");
        assert_eq!(history[1].role, "assistant");
        assert_eq!(history[1].text(), "hello");
        assert_eq!(history[2].role, "user");
        assert_eq!(history[2].text(), "how are you");
    }

    #[test]
    fn excludes_error_messages_and_other_conversations() {
        let conn = setup_conn();
        insert_message(&conn, "c1", "user", "text", "hi", 0);
        insert_message(&conn, "c1", "assistant", "error", "inference failed", 1);
        insert_message(&conn, "c2", "user", "text", "unrelated conversation", 0);

        let skills_dir = empty_skills_dir();
        let history = load_history(&conn, "c1", skills_dir.path()).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].text(), "hi");
    }

    #[test]
    fn empty_conversation_returns_empty_history() {
        let conn = setup_conn();
        let skills_dir = empty_skills_dir();
        assert!(load_history(&conn, "nonexistent", skills_dir.path())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn text_row_is_passed_through_unexpanded() {
        // A 'text' row must behave exactly as before this feature existed
        // — no JSON parsing, no expansion, even if it happens to contain
        // JSON-shaped content.
        let conn = setup_conn();
        insert_message(&conn, "c1", "user", "text", "plain {not json} message", 0);

        let skills_dir = empty_skills_dir();
        let history = load_history(&conn, "c1", skills_dir.path()).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].text(), "plain {not json} message");
    }

    #[test]
    fn rich_text_row_is_expanded_with_skills_resolved_from_skills_dir() {
        let conn = setup_conn();
        let skills_dir = empty_skills_dir();
        let skill_dir = skills_dir.path().join("reviewer");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: reviewer\ndescription: Reviews things\n---\n\nReview instructions.",
        )
        .unwrap();

        let rich_content = serde_json::json!({
            "segments": [
                {"type": "text", "text": "please use "},
                {"type": "skill", "id": "s1", "name": "reviewer"},
                {"type": "text", "text": " for this"},
            ]
        })
        .to_string();
        insert_message(&conn, "c1", "user", "rich_text", &rich_content, 0);

        let history = load_history(&conn, "c1", skills_dir.path()).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(
            history[0].text(),
            "please use \n<skill name=\"reviewer\">\n---\nname: reviewer\ndescription: Reviews things\n---\n\nReview instructions.\n</skill>\n for this"
        );
    }

    #[test]
    fn rich_text_row_with_a_stale_skill_reference_falls_back_to_a_marker_without_failing_the_whole_load(
    ) {
        // The skill named in this old message no longer exists on disk
        // (renamed/deleted since the message was sent — FR-014). This must
        // not error the whole `load_history` call: an older message in the
        // same conversation, and the rest of this row's siblings, must
        // still load.
        let conn = setup_conn();
        insert_message(&conn, "c1", "user", "text", "earlier message", 0);
        let rich_content = serde_json::json!({
            "segments": [
                {"type": "text", "text": "please use "},
                {"type": "skill", "id": "s1", "name": "long-gone-skill"},
            ]
        })
        .to_string();
        insert_message(&conn, "c1", "user", "rich_text", &rich_content, 1);
        insert_message(&conn, "c1", "assistant", "text", "later message", 2);

        let skills_dir = empty_skills_dir();
        let history = load_history(&conn, "c1", skills_dir.path()).unwrap();

        assert_eq!(history.len(), 3);
        assert_eq!(history[0].text(), "earlier message");
        assert!(
            history[1].text().starts_with("[unable to load message:"),
            "expected a bracketed fallback marker, got: {:?}",
            history[1].text()
        );
        assert_eq!(history[2].text(), "later message");
    }

    #[test]
    fn rich_text_row_with_malformed_json_falls_back_to_a_marker() {
        let conn = setup_conn();
        insert_message(&conn, "c1", "user", "rich_text", "not valid json", 0);

        let skills_dir = empty_skills_dir();
        let history = load_history(&conn, "c1", skills_dir.path()).unwrap();
        assert_eq!(history.len(), 1);
        assert!(history[0].text().starts_with("[unable to load message:"));
    }

    // --- 010-context-window-management: load_history_annotated splicing ---

    #[test]
    fn annotated_history_with_no_notices_matches_load_history_exactly() {
        let conn = setup_conn();
        insert_message(&conn, "c1", "user", "text", "hi", 0);
        insert_message(&conn, "c1", "assistant", "tool_call", "{}", 1);
        insert_message(&conn, "c1", "tool", "tool_result", "{}", 2);
        insert_message(&conn, "c1", "assistant", "text", "hello", 3);

        let skills_dir = empty_skills_dir();
        let annotated = load_history_annotated(&conn, "c1", skills_dir.path()).unwrap();
        assert_eq!(annotated.len(), 4);
        assert_eq!(annotated[0].content_type, "text");
        assert_eq!(annotated[0].sequence, 0);
        assert_eq!(annotated[1].content_type, "tool_call");
        assert_eq!(annotated[3].chat.text(), "hello");
    }

    /// Seeds one row through production's ONLY insert path
    /// (`storage::messages::insert`), which allocates `MAX(sequence) + 1`.
    /// Returns the sequence it allocated.
    ///
    /// Every fixture below builds on this rather than `insert_message`'s
    /// hand-picked sequence, because the sequences are the whole subject
    /// here: the tests these replaced hand-placed a `summarized` notice
    /// mid-conversation with a message AFTER it -- a row shape production
    /// cannot produce -- and so asserted about a survivor that could not
    /// exist, staying green for the entire life of a splice that deleted
    /// every real message in the conversation.
    fn seed_row(conn: &Connection, role: &str, content_type: &str, content: &str) -> i64 {
        crate::storage::messages::insert(
            conn,
            None,
            &crate::storage::messages::NewMessage {
                conversation_id: "c1",
                role,
                content_type,
                content,
                tool_name: None,
                tool_call_id: None,
                model_text: None,
                created_at: 0,
                duration_ms: None,
                token_count: None,
            },
        )
        .unwrap()
    }

    /// Persists a context_notice the way `context::persist_notice` does --
    /// through `persist_context_notice`, this module's own production path --
    /// and returns the sequence it landed at.
    fn seed_notice(conn: &Connection, notice_json: &str) -> i64 {
        persist_context_notice(conn, None, "c1", 0, notice_json).unwrap();
        conn.query_row(
            "SELECT MAX(sequence) FROM messages WHERE conversation_id = 'c1'",
            [],
            |row| row.get(0),
        )
        .unwrap()
    }

    /// THE INVARIANT THAT MAKES `throughSequence` NECESSARY, pinned rather
    /// than described. A `summarized` notice can only ever be written by
    /// `persist_context_notice` -> `messages::insert`, which allocates
    /// `COALESCE(MAX(sequence), -1) + 1`, so it ALWAYS lands after every
    /// message in the conversation. That is why the splice point has to be
    /// recorded in the notice's payload and can never be read off the notice
    /// row itself: "drop everything at or before this row" and "drop
    /// everything the summary covers" are the same sentence only if the row
    /// sits at the end of the span, and it never does -- it sits at the end
    /// of the CONVERSATION.
    #[test]
    fn a_summarized_notice_is_always_allocated_the_last_sequence() {
        let conn = setup_conn();
        seed_row(&conn, "user", "text", "the task statement");
        seed_row(&conn, "assistant", "text", "a reply");
        seed_row(&conn, "user", "text", "the most recent turn");

        let notice_sequence = seed_notice(&conn, &crate::context::summarized_notice_json("s", 1));

        assert_eq!(
            notice_sequence, 3,
            "the notice landed at MAX(sequence)+1, past every message it summarizes"
        );
        let last_real: i64 = conn
            .query_row(
                "SELECT MAX(sequence) FROM messages WHERE conversation_id = 'c1' AND content_type != 'context_notice'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(
            notice_sequence > last_real,
            "no message can ever follow a context_notice, so a splice at the notice's own \
             sequence ({notice_sequence}) drops the entire conversation (last real message: \
             {last_real})"
        );
    }

    #[test]
    fn a_summarized_notice_splices_out_exactly_the_span_it_covers() {
        let conn = setup_conn();
        let task = seed_row(&conn, "user", "text", "the task statement");
        seed_row(&conn, "assistant", "text", "summarized message 1");
        let span_end = seed_row(&conn, "assistant", "text", "summarized message 2");
        seed_row(&conn, "user", "text", "protected recent turn");
        // Production's own notice payload, at the span's end -- exactly what
        // `context::summarize_and_persist`'s Accept arm writes.
        seed_notice(
            &conn,
            &crate::context::summarized_notice_json("the gist of it", span_end),
        );

        let skills_dir = empty_skills_dir();
        let history = load_history_annotated(&conn, "c1", skills_dir.path()).unwrap();

        // [keep-first] + [summary] + [what came after the span]
        let texts: Vec<String> = history.iter().map(|m| m.chat.text()).collect();
        assert_eq!(
            texts,
            vec![
                "the task statement",
                "the gist of it",
                "protected recent turn"
            ],
            "the summary must replace exactly the span it covers -- no more (the task statement \
             and the recent turn are not in it, so nothing stands in for them) and no less"
        );
        assert_eq!(history[1].chat.role, "system");
        assert_eq!(history[1].content_type, "context_notice");
        assert_eq!(history[0].sequence, task);
        assert_eq!(
            history[1].sequence, span_end,
            "the summary stands where the span it replaced ended"
        );
        let sequences: Vec<i64> = history.iter().map(|m| m.sequence).collect();
        assert!(
            sequences.windows(2).all(|w| w[0] < w[1]),
            "the reloaded history must stay strictly ordered by sequence, got {sequences:?}"
        );
    }

    /// The task statement is the one message at-or-before the splice point
    /// that survives. `context::messages_to_summarize` excludes it from
    /// every span, so the summary does not describe it: dropping it here
    /// would erase what the user asked for with nothing standing in for it,
    /// which is exactly what shipped until 2026-07-15.
    #[test]
    fn the_keep_first_user_message_survives_the_splice_but_later_user_turns_in_the_span_do_not() {
        let conn = setup_conn();
        seed_row(&conn, "user", "text", "the task statement");
        seed_row(&conn, "assistant", "text", "a reply");
        let span_end = seed_row(&conn, "user", "text", "a later user turn, inside the span");
        seed_notice(
            &conn,
            &crate::context::summarized_notice_json("the gist", span_end),
        );

        let skills_dir = empty_skills_dir();
        let history = load_history_annotated(&conn, "c1", skills_dir.path()).unwrap();

        let texts: Vec<String> = history.iter().map(|m| m.chat.text()).collect();
        assert_eq!(
            texts,
            vec!["the task statement", "the gist"],
            "only the FIRST genuine user message is exempt -- a later user turn inside the span \
             is summarized like anything else"
        );
    }

    /// A `tool_result` row reconstructs with `chat.role == "user"` but is
    /// never the task statement -- if the exemption matched it, the real
    /// keep-first message would be dropped and a tool result kept in its
    /// place.
    #[test]
    fn a_leading_tool_result_is_never_mistaken_for_the_keep_first_message() {
        let conn = setup_conn();
        seed_row(&conn, "tool", "tool_result", "{}");
        seed_row(&conn, "user", "text", "the task statement");
        let span_end = seed_row(&conn, "assistant", "text", "a reply");
        seed_notice(
            &conn,
            &crate::context::summarized_notice_json("the gist", span_end),
        );

        let skills_dir = empty_skills_dir();
        let history = load_history_annotated(&conn, "c1", skills_dir.path()).unwrap();

        let texts: Vec<String> = history.iter().map(|m| m.chat.text()).collect();
        assert_eq!(texts, vec!["the task statement", "the gist"]);
    }

    /// BACKWARD COMPATIBILITY: a notice written before `throughSequence`
    /// existed falls back to the notice row's own sequence -- the old
    /// behavior, deliberately. Those conversations are already condensed as
    /// far as the user has been told; resurrecting their rows now would hand
    /// the model a history it was never sized for. The keep-first message is
    /// exempt regardless of which splice point applies, because no summary
    /// ever covered it under either code path.
    #[test]
    fn a_legacy_summarized_notice_without_a_through_sequence_splices_at_its_own_row() {
        let conn = setup_conn();
        seed_row(&conn, "user", "text", "the task statement");
        seed_row(&conn, "assistant", "text", "old message");
        seed_notice(
            &conn,
            r#"{"kind":"summarized","summary":"legacy summary","notice":"n"}"#,
        );

        let skills_dir = empty_skills_dir();
        let history = load_history_annotated(&conn, "c1", skills_dir.path()).unwrap();

        let texts: Vec<String> = history.iter().map(|m| m.chat.text()).collect();
        assert_eq!(texts, vec!["the task statement", "legacy summary"]);
    }

    // --- FR-3 (restore-recent-file): a `restoredFile` notice, persisted by
    // `context::summarize_and_persist` right after a `summarized` notice,
    // renders as a second synthesized system message spliced in right after
    // the summary. ---

    #[test]
    fn a_restored_file_notice_renders_right_after_the_summary() {
        let conn = setup_conn();
        seed_row(&conn, "user", "text", "the task statement");
        // A restored-file notice is only ever written for a span that
        // contained a `Read` tool_result (`context::most_recent_read_path`),
        // and a result row is always preceded by its own call row -- so the
        // span this fixture summarizes is shaped the way the only span that
        // can produce this notice really is.
        seed_row(&conn, "assistant", "tool_call", "{}");
        let span_end = seed_row(&conn, "tool", "tool_result", "{}");
        seed_row(&conn, "user", "text", "protected recent turn");
        seed_notice(
            &conn,
            &crate::context::summarized_notice_json("the gist of it", span_end),
        );
        seed_notice(
            &conn,
            &crate::context::restored_file_notice_json(
                "/tmp/a.rs",
                "Current contents of `/tmp/a.rs`:\nfn main() {}",
            ),
        );

        let skills_dir = empty_skills_dir();
        let history = load_history_annotated(&conn, "c1", skills_dir.path()).unwrap();

        let texts: Vec<String> = history.iter().map(|m| m.chat.text()).collect();
        assert_eq!(
            texts,
            vec![
                "the task statement",
                "the gist of it",
                "Current contents of `/tmp/a.rs`:\nfn main() {}",
                "protected recent turn",
            ],
            "the restored file's real content belongs immediately after the summary that dropped \
             it, not buried among the turns that follow"
        );
        assert_eq!(history[2].chat.role, "system");
        let sequences: Vec<i64> = history.iter().map(|m| m.sequence).collect();
        assert!(
            sequences.windows(2).all(|w| w[0] < w[1]),
            "the reloaded history must stay strictly ordered by sequence, got {sequences:?}"
        );
    }

    #[test]
    fn a_restored_file_notice_from_a_superseded_summary_is_dropped() {
        // The restored-file notice paired with the FIRST (now-superseded)
        // summary must not leak into a load spliced against the SECOND,
        // later summary -- same "only the most recent one" rule the
        // `summarized` notice itself already follows. This is what dates a
        // restored-file notice by the NOTICE row's sequence rather than by
        // `throughSequence`, which points far enough back to match every
        // restored-file row the conversation ever had.
        let conn = setup_conn();
        seed_row(&conn, "user", "text", "the task statement");
        let first_span_end = seed_row(&conn, "assistant", "text", "ancient message");
        seed_notice(
            &conn,
            &crate::context::summarized_notice_json("first summary", first_span_end),
        );
        seed_notice(
            &conn,
            &crate::context::restored_file_notice_json("/tmp/old.rs", "stale restored content"),
        );
        let second_span_end = seed_row(&conn, "assistant", "text", "middle message");
        seed_notice(
            &conn,
            &crate::context::summarized_notice_json(
                "second summary covers the first too",
                second_span_end,
            ),
        );
        seed_row(&conn, "user", "text", "recent message");

        let skills_dir = empty_skills_dir();
        let history = load_history_annotated(&conn, "c1", skills_dir.path()).unwrap();

        let texts: Vec<String> = history.iter().map(|m| m.chat.text()).collect();
        assert_eq!(
            texts,
            vec![
                "the task statement",
                "second summary covers the first too",
                "recent message"
            ],
        );
        assert!(
            !texts.iter().any(|t| t == "stale restored content"),
            "a restored-file notice tied to a superseded summary must not render"
        );
    }

    #[test]
    fn no_restored_file_notice_means_the_summary_renders_alone() {
        let conn = setup_conn();
        seed_row(&conn, "user", "text", "the task statement");
        let span_end = seed_row(&conn, "assistant", "text", "old message");
        seed_row(&conn, "user", "text", "new message");
        seed_notice(
            &conn,
            &crate::context::summarized_notice_json("the gist of it", span_end),
        );

        let skills_dir = empty_skills_dir();
        let history = load_history_annotated(&conn, "c1", skills_dir.path()).unwrap();

        let texts: Vec<String> = history.iter().map(|m| m.chat.text()).collect();
        assert_eq!(
            texts,
            vec!["the task statement", "the gist of it", "new message"],
            "no restored-file notice was persisted -- only the summary stands in for the span"
        );
    }

    #[test]
    fn a_cleared_notice_is_excluded_but_does_not_splice_anything() {
        let conn = setup_conn();
        seed_row(&conn, "user", "text", "message 1");
        seed_notice(&conn, &crate::context::cleared_notice_json(&[]));
        seed_row(&conn, "user", "text", "message 2");

        let skills_dir = empty_skills_dir();
        let history = load_history_annotated(&conn, "c1", skills_dir.path()).unwrap();

        let texts: Vec<String> = history.iter().map(|m| m.chat.text()).collect();
        assert_eq!(texts, vec!["message 1", "message 2"]);
    }

    // --- 010-context-window-management tier 1: a `cleared` notice replays
    // the clearing onto the rows it names. This replay is the ONLY thing
    // that applies tier 1 to what the model receives. ---

    #[test]
    fn a_cleared_notice_replays_its_placeholders_onto_the_tool_rows_it_names() {
        let conn = setup_conn();
        seed_row(&conn, "user", "text", "the task statement");
        let cleared_seq = seed_row(&conn, "tool", "tool_result", "{}");
        let kept_seq = seed_row(&conn, "tool", "tool_result", "{}");
        seed_notice(
            &conn,
            &crate::context::cleared_notice_json(&[crate::context::ClearedRow {
                sequence: cleared_seq,
                placeholder: crate::context::limits::TOOL_CLEARED_PLACEHOLDER.to_string(),
            }]),
        );

        let skills_dir = empty_skills_dir();
        let history = load_history_annotated(&conn, "c1", skills_dir.path()).unwrap();

        let cleared = history
            .iter()
            .find(|m| m.sequence == cleared_seq)
            .expect("a cleared row is still a row -- tier 1 replaces its content, never drops it");
        assert_eq!(
            cleared.chat.text(),
            crate::context::limits::TOOL_CLEARED_PLACEHOLDER,
            "the row the notice named must carry the placeholder the notice recorded -- without \
             this the model is seeded with every byte the notice claimed to free"
        );
        assert_eq!(
            cleared.content_type, "tool_result",
            "a cleared row keeps its own content_type, so tier 1's own keep_n populations still \
             count it"
        );
        let kept = history.iter().find(|m| m.sequence == kept_seq).unwrap();
        assert!(
            !crate::context::limits::is_tool_cleared_placeholder(&kept.chat.text()),
            "a row no notice names must be untouched"
        );
    }

    /// BACKWARD COMPATIBILITY: a `cleared` notice written before the rows
    /// were recorded replays nothing -- correctly, since that clearing never
    /// actually happened to anything the model saw.
    #[test]
    fn a_legacy_cleared_notice_without_recorded_rows_replays_nothing() {
        let conn = setup_conn();
        seed_row(&conn, "tool", "tool_result", "{}");
        seed_notice(
            &conn,
            r#"{"kind":"cleared","clearedCount":1,"notice":"1 old tool result cleared to save space"}"#,
        );

        let skills_dir = empty_skills_dir();
        let history = load_history_annotated(&conn, "c1", skills_dir.path()).unwrap();

        assert_eq!(history.len(), 1);
        assert!(!crate::context::limits::is_tool_cleared_placeholder(
            &history[0].chat.text()
        ));
    }

    #[test]
    fn only_the_most_recent_summarized_notice_is_spliced() {
        let conn = setup_conn();
        seed_row(&conn, "user", "text", "the task statement");
        let first_span_end = seed_row(&conn, "assistant", "text", "ancient message");
        seed_notice(
            &conn,
            &crate::context::summarized_notice_json("first summary", first_span_end),
        );
        let second_span_end = seed_row(&conn, "assistant", "text", "middle message");
        seed_notice(
            &conn,
            &crate::context::summarized_notice_json(
                "second summary covers the first too",
                second_span_end,
            ),
        );
        seed_row(&conn, "user", "text", "recent message");

        let skills_dir = empty_skills_dir();
        let history = load_history_annotated(&conn, "c1", skills_dir.path()).unwrap();

        let texts: Vec<String> = history.iter().map(|m| m.chat.text()).collect();
        assert_eq!(
            texts,
            vec![
                "the task statement",
                "second summary covers the first too",
                "recent message"
            ],
            "a superseded summary must not render, and the newer splice must still cover \
             everything the older one did"
        );
        assert_eq!(history[1].sequence, second_span_end);
    }

    #[test]
    fn persist_context_notice_appends_at_the_next_sequence() {
        let conn = setup_conn();
        insert_message(&conn, "c1", "user", "text", "hi", 0);

        persist_context_notice(
            &conn,
            None,
            "c1",
            1000,
            r#"{"kind":"cleared","clearedCount":1,"notice":"n"}"#,
        )
        .unwrap();

        let (content_type, sequence): (String, i64) = conn
            .query_row(
                "SELECT content_type, sequence FROM messages WHERE conversation_id = 'c1' ORDER BY sequence DESC LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(content_type, "context_notice");
        assert_eq!(sequence, 1);
    }

    // --- set_conversation_goal / get_conversation_goal (0011_conversation_goal) ---
    //
    // Unlike the rest of this file's tests, these need the REAL migrated
    // `conversations` table (goal included) rather than `setup_conn`'s
    // hand-rolled `messages`-only schema, so they run migrations for real.

    fn setup_conn_with_conversation(id: &str) -> Connection {
        let conn = crate::storage::test_connection();
        conn.execute(
            "INSERT INTO conversations (id, workspace_id, spawned_by_conversation_id, title, created_at, updated_at) VALUES (?1, NULL, NULL, 'Test', 0, 0)",
            [id],
        )
        .unwrap();
        conn
    }

    #[test]
    fn get_conversation_goal_is_none_before_anything_is_set() {
        let conn = setup_conn_with_conversation("c1");
        assert_eq!(get_conversation_goal(&conn, "c1").unwrap(), None);
    }

    #[test]
    fn set_then_get_conversation_goal_round_trips() {
        let conn = setup_conn_with_conversation("c1");
        set_conversation_goal(&conn, "c1", Some("ship the login page")).unwrap();
        assert_eq!(
            get_conversation_goal(&conn, "c1").unwrap().as_deref(),
            Some("ship the login page")
        );
    }

    #[test]
    fn set_conversation_goal_with_none_clears_it() {
        let conn = setup_conn_with_conversation("c1");
        set_conversation_goal(&conn, "c1", Some("ship the login page")).unwrap();
        set_conversation_goal(&conn, "c1", None).unwrap();
        assert_eq!(get_conversation_goal(&conn, "c1").unwrap(), None);
    }

    #[test]
    fn set_conversation_goal_with_empty_string_also_clears_it() {
        let conn = setup_conn_with_conversation("c1");
        set_conversation_goal(&conn, "c1", Some("ship the login page")).unwrap();
        set_conversation_goal(&conn, "c1", Some("")).unwrap();
        assert_eq!(
            get_conversation_goal(&conn, "c1").unwrap(),
            None,
            "an empty string must read back as no goal, same as NULL"
        );
    }

    #[test]
    fn set_conversation_goal_only_touches_the_named_conversation() {
        let conn = setup_conn_with_conversation("c1");
        conn.execute(
            "INSERT INTO conversations (id, workspace_id, spawned_by_conversation_id, title, created_at, updated_at) VALUES ('c2', NULL, NULL, 'Test 2', 0, 0)",
            [],
        )
        .unwrap();
        set_conversation_goal(&conn, "c1", Some("goal for c1")).unwrap();
        assert_eq!(get_conversation_goal(&conn, "c2").unwrap(), None);
    }
}
