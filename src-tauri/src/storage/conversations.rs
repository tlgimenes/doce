use crate::agent::rich_content::{expand_segments, RichMessageContent};
use crate::inference::ChatMessage;
use rusqlite::Connection;
use std::path::Path;

const MAX_TITLE_LEN: usize = 60;

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
}

/// Builds the chat-template message history for a conversation: every
/// non-error message so far, oldest first, role-mapped from the
/// `messages` table's `role` column, still tagged with `content_type`/
/// `sequence`. Shared by the plain chat path
/// (`commands::conversations::send_message`) and agent mode
/// (`commands::agent::send_agent_message`) — without this, every reply
/// was generated with no memory of earlier turns in the same
/// conversation, on top of the separate missing-chat-template bug.
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
/// isn't real turn content) — *except* that the most recent such row whose
/// JSON `content` has `"kind":"summarized"` marks a splice point: every row
/// at or before its `sequence` is dropped from the result and replaced by a
/// single synthesized system-role message carrying that row's `summary`
/// field. This is what makes a persisted compaction pass correctly
/// reflected on every subsequent load (data-model.md), not just for the
/// turn it happened on. A second, later `summarized` notice supersedes the
/// first — only the most recent one is spliced.
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
    // any) — its sequence is the splice point, its embedded `summary` is
    // what replaces everything at-or-before it.
    let splice: Option<(i64, String)> = rows
        .iter()
        .filter(|(_, content_type, ..)| content_type == "context_notice")
        .filter_map(|(_, _, content, sequence, ..)| {
            let parsed: serde_json::Value = serde_json::from_str(content).ok()?;
            if parsed.get("kind")?.as_str()? == "summarized" {
                let summary = parsed.get("summary")?.as_str()?.to_string();
                Some((*sequence, summary))
            } else {
                None
            }
        })
        .max_by_key(|(sequence, _)| *sequence);

    let mut result = Vec::new();
    if let Some((splice_sequence, summary)) = &splice {
        result.push(HistoryMessage {
            chat: ChatMessage::system(summary.clone()),
            content_type: "context_notice".to_string(),
            sequence: *splice_sequence,
        });
    }

    let splice_sequence = splice.as_ref().map(|(s, _)| *s);
    for (role, content_type, content, sequence, tool_name, tool_call_id, model_text) in rows {
        if content_type == "context_notice" {
            continue;
        }
        if let Some(splice_sequence) = splice_sequence {
            if sequence <= splice_sequence {
                continue;
            }
        }

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
        });
    }

    Ok(result)
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

/// Persists a `context_notice` row (010-context-window-management) —
/// `kind_json` is the row's full JSON `content`
/// (`{"kind":"cleared",...}`/`{"kind":"summarized",...}`, see
/// data-model.md). Always `role='assistant'` (the `messages.role` CHECK has
/// no `'system'` value; this matches how `error` rows are already
/// persisted under `role='assistant'` too) and `tool_name=NULL`.
pub fn persist_context_notice(
    conn: &Connection,
    conversation_id: &str,
    now: i64,
    kind_json: &str,
) -> rusqlite::Result<()> {
    let seq: i64 = conn.query_row(
        "SELECT COALESCE(MAX(sequence), -1) + 1 FROM messages WHERE conversation_id = ?1",
        [conversation_id],
        |row| row.get(0),
    )?;
    conn.execute(
        "INSERT INTO messages (id, conversation_id, role, content_type, content, created_at, sequence) VALUES (?1, ?2, 'assistant', 'context_notice', ?3, ?4, ?5)",
        rusqlite::params![uuid::Uuid::now_v7().to_string(), conversation_id, kind_json, now, seq],
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
                sequence INTEGER NOT NULL, tool_name TEXT, tool_call_id TEXT, model_text TEXT
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

    #[test]
    fn a_summarized_notice_splices_out_everything_at_or_before_it() {
        let conn = setup_conn();
        insert_message(&conn, "c1", "user", "text", "old message 1", 0);
        insert_message(&conn, "c1", "assistant", "text", "old message 2", 1);
        insert_message(
            &conn,
            "c1",
            "assistant",
            "context_notice",
            r#"{"kind":"summarized","summary":"the gist of it","notice":"Conversation condensed to save space"}"#,
            2,
        );
        insert_message(&conn, "c1", "user", "text", "new message", 3);

        let skills_dir = empty_skills_dir();
        let history = load_history_annotated(&conn, "c1", skills_dir.path()).unwrap();

        assert_eq!(history.len(), 2);
        assert_eq!(history[0].chat.role, "system");
        assert_eq!(history[0].chat.text(), "the gist of it");
        assert_eq!(history[0].sequence, 2);
        assert_eq!(history[1].chat.text(), "new message");
        assert_eq!(history[1].sequence, 3);
    }

    #[test]
    fn a_cleared_notice_is_excluded_but_does_not_splice_anything() {
        let conn = setup_conn();
        insert_message(&conn, "c1", "user", "text", "message 1", 0);
        insert_message(
            &conn,
            "c1",
            "assistant",
            "context_notice",
            r#"{"kind":"cleared","clearedCount":2,"notice":"2 old tool results cleared to save space"}"#,
            1,
        );
        insert_message(&conn, "c1", "user", "text", "message 2", 2);

        let skills_dir = empty_skills_dir();
        let history = load_history_annotated(&conn, "c1", skills_dir.path()).unwrap();

        assert_eq!(history.len(), 2);
        assert_eq!(history[0].chat.text(), "message 1");
        assert_eq!(history[1].chat.text(), "message 2");
    }

    #[test]
    fn only_the_most_recent_summarized_notice_is_spliced() {
        let conn = setup_conn();
        insert_message(&conn, "c1", "user", "text", "ancient message", 0);
        insert_message(
            &conn,
            "c1",
            "assistant",
            "context_notice",
            r#"{"kind":"summarized","summary":"first summary","notice":"n"}"#,
            1,
        );
        insert_message(&conn, "c1", "user", "text", "middle message", 2);
        insert_message(
            &conn,
            "c1",
            "assistant",
            "context_notice",
            r#"{"kind":"summarized","summary":"second summary covers the first too","notice":"n"}"#,
            3,
        );
        insert_message(&conn, "c1", "user", "text", "recent message", 4);

        let skills_dir = empty_skills_dir();
        let history = load_history_annotated(&conn, "c1", skills_dir.path()).unwrap();

        assert_eq!(history.len(), 2);
        assert_eq!(
            history[0].chat.text(),
            "second summary covers the first too"
        );
        assert_eq!(history[0].sequence, 3);
        assert_eq!(history[1].chat.text(), "recent message");
    }

    #[test]
    fn persist_context_notice_appends_at_the_next_sequence() {
        let conn = setup_conn();
        insert_message(&conn, "c1", "user", "text", "hi", 0);

        persist_context_notice(
            &conn,
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
}
