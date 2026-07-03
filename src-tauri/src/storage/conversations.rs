use crate::inference::ChatMessage;
use rusqlite::Connection;

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
pub fn load_history(
    conn: &Connection,
    conversation_id: &str,
) -> rusqlite::Result<Vec<ChatMessage>> {
    let mut stmt = conn.prepare(
        "SELECT role, content FROM messages WHERE conversation_id = ?1 AND content_type != 'error' ORDER BY sequence ASC",
    )?;
    let rows = stmt
        .query_map([conversation_id], |row| {
            let role: String = row.get(0)?;
            let content: String = row.get(1)?;
            Ok((role, content))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(rows
        .into_iter()
        .map(|(role, content)| match role.as_str() {
            "assistant" => ChatMessage::assistant(content),
            _ => ChatMessage::user(content),
        })
        .collect())
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

    #[test]
    fn loads_history_in_sequence_order_with_roles_mapped() {
        let conn = setup_conn();
        insert_message(&conn, "c1", "user", "text", "hi", 0);
        insert_message(&conn, "c1", "assistant", "text", "hello", 1);
        insert_message(&conn, "c1", "user", "text", "how are you", 2);

        let history = load_history(&conn, "c1").unwrap();
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

        let history = load_history(&conn, "c1").unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].content, "hi");
    }

    #[test]
    fn empty_conversation_returns_empty_history() {
        let conn = setup_conn();
        assert!(load_history(&conn, "nonexistent").unwrap().is_empty());
    }
}
