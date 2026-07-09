//! The single insert path for `messages` rows (2026-07-09 payload-files
//! design): sequence allocation, the INSERT itself, and the transcript
//! append live together so the transcript file and the table cannot drift
//! via a forgotten call site — previously 7+ sites each hand-rolled
//! `COALESCE(MAX(sequence), -1) + 1`.

use crate::context::transcript;
use rusqlite::Connection;
use uuid::Uuid;

pub struct NewMessage<'a> {
    pub conversation_id: &'a str,
    pub role: &'a str,
    pub content_type: &'a str,
    pub content: &'a str,
    pub tool_name: Option<&'a str>,
    pub tool_call_id: Option<&'a str>,
    pub model_text: Option<&'a str>,
    pub created_at: i64,
    pub duration_ms: Option<i64>,
    pub token_count: Option<i64>,
}

/// Allocates MAX(sequence)+1, inserts, best-effort-appends the transcript
/// entry. Returns the allocated sequence. `transcript_dir: None` (tests,
/// callers without an AppHandle) skips the append — heal_if_stale
/// regenerates on next conversation open, so a skipped append is never
/// corruption, only staleness.
pub fn insert(
    conn: &Connection,
    transcript_dir: Option<&std::path::Path>,
    msg: &NewMessage,
) -> rusqlite::Result<i64> {
    let seq: i64 = conn.query_row(
        "SELECT COALESCE(MAX(sequence), -1) + 1 FROM messages WHERE conversation_id = ?1",
        [msg.conversation_id],
        |row| row.get(0),
    )?;
    conn.execute(
        "INSERT INTO messages (id, conversation_id, role, content_type, content, tool_name, \
         created_at, sequence, tool_call_id, model_text, duration_ms, token_count) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        rusqlite::params![
            Uuid::now_v7().to_string(),
            msg.conversation_id,
            msg.role,
            msg.content_type,
            msg.content,
            msg.tool_name,
            msg.created_at,
            seq,
            msg.tool_call_id,
            msg.model_text,
            msg.duration_ms,
            msg.token_count,
        ],
    )?;
    if let Some(dir) = transcript_dir {
        // Same body-selection rule `transcript::regenerate` uses for a
        // persisted row (`row_body`, made `pub(crate)` for exactly this) —
        // reimplementing the match here would let the append path and the
        // regenerate path drift, breaking the byte-equivalence invariant
        // between an appended transcript and one rebuilt from scratch.
        let body = transcript::row_body(msg.content_type, msg.content, msg.model_text);
        let entry = transcript::render_entry(seq, msg.role, msg.content_type, msg.tool_name, &body);
        // Derived cache: an append failure is staleness, not corruption.
        let _ = transcript::append_entry(dir, msg.conversation_id, &entry);
    }
    Ok(seq)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::test_connection;

    fn seed_conversation(conn: &rusqlite::Connection, id: &str) {
        conn.execute(
            "INSERT INTO conversations (id, workspace_id, spawned_by_conversation_id, title, created_at, updated_at) \
             VALUES (?1, NULL, NULL, 'T', 0, 0)",
            [id],
        ).unwrap();
    }

    fn msg<'a>(ct: &'a str, content: &'a str) -> NewMessage<'a> {
        NewMessage {
            conversation_id: "c1",
            role: "user",
            content_type: ct,
            content,
            tool_name: None,
            tool_call_id: None,
            model_text: None,
            created_at: 0,
            duration_ms: None,
            token_count: None,
        }
    }

    #[test]
    fn allocates_sequences_and_appends_transcript() {
        let conn = test_connection();
        let dir = tempfile::tempdir().unwrap();
        seed_conversation(&conn, "c1");
        assert_eq!(
            insert(&conn, Some(dir.path()), &msg("text", "one")).unwrap(),
            0
        );
        assert_eq!(
            insert(&conn, Some(dir.path()), &msg("text", "two")).unwrap(),
            1
        );
        let t = std::fs::read_to_string(dir.path().join("c1.txt")).unwrap();
        assert_eq!(t, "[#0 user]\none\n\n[#1 user]\ntwo\n\n");
    }

    #[test]
    fn tool_rows_render_model_text_not_content() {
        let conn = test_connection();
        let dir = tempfile::tempdir().unwrap();
        seed_conversation(&conn, "c1");
        let m = NewMessage {
            role: "tool",
            content_type: "tool_result",
            content: r#"{"toolName":"Bash","big":"detail"}"#,
            tool_name: Some("Bash"),
            tool_call_id: Some("tc1"),
            model_text: Some("what the model saw"),
            ..msg("tool_result", "")
        };
        insert(&conn, Some(dir.path()), &m).unwrap();
        let t = std::fs::read_to_string(dir.path().join("c1.txt")).unwrap();
        assert_eq!(t, "[#0 Bash result]\nwhat the model saw\n\n");
    }

    #[test]
    fn none_transcript_dir_skips_the_append_but_inserts() {
        let conn = test_connection();
        seed_conversation(&conn, "c1");
        assert_eq!(insert(&conn, None, &msg("text", "one")).unwrap(), 0);
    }
}
