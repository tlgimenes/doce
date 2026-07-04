use crate::agent::rich_content::{expand_segments, RichMessageContent};
use crate::inference::ChatMessage;
use rusqlite::Connection;
use std::path::Path;

const MAX_TITLE_LEN: usize = 60;

/// Builds the chat-template message history for a conversation: every
/// non-error message so far, oldest first, role-mapped from the
/// `messages` table's `role` column. Shared by the plain chat path
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
pub fn load_history(
    conn: &Connection,
    conversation_id: &str,
    skills_dir: &Path,
) -> rusqlite::Result<Vec<ChatMessage>> {
    let mut stmt = conn.prepare(
        "SELECT role, content_type, content FROM messages WHERE conversation_id = ?1 AND content_type != 'error' ORDER BY sequence ASC",
    )?;
    let rows = stmt
        .query_map([conversation_id], |row| {
            let role: String = row.get(0)?;
            let content_type: String = row.get(1)?;
            let content: String = row.get(2)?;
            Ok((role, content_type, content))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(rows
        .into_iter()
        .map(|(role, content_type, content)| {
            let text = if content_type == "rich_text" {
                expand_rich_text(&content, skills_dir)
            } else {
                // Every other content_type (today: just 'text' — 'error'
                // rows are excluded by the query above, and tool rows are
                // never fed through load_history's role-mapping today)
                // behaves exactly as before this feature existed: the raw
                // column value, untouched.
                content
            };
            match role.as_str() {
                "assistant" => ChatMessage::assistant(text),
                _ => ChatMessage::user(text),
            }
        })
        .collect())
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
                sequence INTEGER NOT NULL
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
        assert_eq!(history[0].content, "hi");
        assert_eq!(history[1].role, "assistant");
        assert_eq!(history[1].content, "hello");
        assert_eq!(history[2].role, "user");
        assert_eq!(history[2].content, "how are you");
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
        assert_eq!(history[0].content, "hi");
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
        assert_eq!(history[0].content, "plain {not json} message");
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
            history[0].content,
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
        assert_eq!(history[0].content, "earlier message");
        assert!(
            history[1].content.starts_with("[unable to load message:"),
            "expected a bracketed fallback marker, got: {:?}",
            history[1].content
        );
        assert_eq!(history[2].content, "later message");
    }

    #[test]
    fn rich_text_row_with_malformed_json_falls_back_to_a_marker() {
        let conn = setup_conn();
        insert_message(&conn, "c1", "user", "rich_text", "not valid json", 0);

        let skills_dir = empty_skills_dir();
        let history = load_history(&conn, "c1", skills_dir.path()).unwrap();
        assert_eq!(history.len(), 1);
        assert!(history[0].content.starts_with("[unable to load message:"));
    }
}
