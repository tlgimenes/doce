//! Materialized conversation transcripts (2026-07-09 payload-files design):
//! a per-conversation text file of exactly what the model saw, one
//! `[#seq role]` entry per message row. A DERIVED, REGENERABLE cache of
//! SQLite — never authoritative, so every consistency question is answered
//! by `regenerate`. Entry bodies are `model_text` (bounded by payload
//! staging), so the file can never hand the model an unbounded line.

use std::path::{Path, PathBuf};

/// Write/Edit tool_call args embed whole files; cap them in the transcript.
pub const TRANSCRIPT_ARGS_CAP_CHARS: usize = 2000;

pub fn transcript_path(transcript_dir: &Path, conversation_id: &str) -> PathBuf {
    transcript_dir.join(format!("{conversation_id}.txt"))
}

pub fn render_entry(
    seq: i64,
    role: &str,
    content_type: &str,
    tool_name: Option<&str>,
    body: &str,
) -> String {
    let header = match (content_type, tool_name) {
        ("tool_call", Some(tool)) => format!("[#{seq} assistant → {tool}]"),
        ("tool_result", Some(tool)) => format!("[#{seq} {tool} result]"),
        ("error", _) => format!("[#{seq} error]"),
        ("context_notice", _) => format!("[#{seq} context-notice]"),
        ("text", _) | ("rich_text", _) => format!("[#{seq} {role}]"),
        (other, _) => format!("[#{seq} {role} {other}]"),
    };
    let body = if content_type == "tool_call" && body.chars().count() > TRANSCRIPT_ARGS_CAP_CHARS {
        let head: String = body.chars().take(TRANSCRIPT_ARGS_CAP_CHARS).collect();
        format!("{head}… [args truncated]")
    } else {
        body.to_string()
    };
    // Tool stdout routinely ends in "\n"; trimming it is what keeps the
    // "every entry ends in exactly one blank line" contract byte-exact.
    let body = body.trim_end_matches('\n');
    // The transcript is a rendered view, not byte-exact data: a body line
    // starting with "[#" would be indistinguishable from an entry header, so
    // it gets one leading space. This is what makes `last_file_seq`'s
    // line-anchored parse unspoofable.
    let body: String = body
        .lines()
        .map(|line| {
            if line.starts_with("[#") {
                format!(" {line}")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    if body.is_empty() {
        format!("{header}\n\n")
    } else {
        format!("{header}\n{body}\n\n")
    }
}

pub fn append_entry(
    transcript_dir: &Path,
    conversation_id: &str,
    entry: &str,
) -> std::io::Result<()> {
    std::fs::create_dir_all(transcript_dir)?;
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(transcript_path(transcript_dir, conversation_id))?;
    f.write_all(entry.as_bytes())
}

/// The per-row body: what the model saw. tool rows use `model_text`
/// (falling back to `content` for legacy rows persisted before model_text
/// existed); everything else uses `content`.
fn row_body(content_type: &str, content: &str, model_text: Option<&str>) -> String {
    match content_type {
        "tool_call" | "tool_result" => model_text.unwrap_or(content).to_string(),
        _ => content.to_string(),
    }
}

pub fn regenerate(
    conn: &rusqlite::Connection,
    transcript_dir: &Path,
    conversation_id: &str,
) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare(
        "SELECT sequence, role, content_type, content, tool_name, model_text \
         FROM messages WHERE conversation_id = ?1 ORDER BY sequence ASC",
    )?;
    let entries = stmt
        .query_map([conversation_id], |row| {
            let seq: i64 = row.get(0)?;
            let role: String = row.get(1)?;
            let content_type: String = row.get(2)?;
            let content: String = row.get(3)?;
            let tool_name: Option<String> = row.get(4)?;
            let model_text: Option<String> = row.get(5)?;
            Ok(render_entry(
                seq,
                &role,
                &content_type,
                tool_name.as_deref(),
                &row_body(&content_type, &content, model_text.as_deref()),
            ))
        })?
        .collect::<rusqlite::Result<Vec<String>>>()?;

    // Derived cache: IO failures must not fail the caller's DB work.
    let _ = std::fs::create_dir_all(transcript_dir);
    let tmp = transcript_dir.join(format!("{conversation_id}.txt.tmp"));
    if std::fs::write(&tmp, entries.concat()).is_ok() {
        let _ = std::fs::rename(&tmp, transcript_path(transcript_dir, conversation_id));
    }
    Ok(())
}

/// Last entry seq actually in the file, or None if missing/unparseable.
/// Anchored to line starts: only a line BEGINNING with "[#" can be a
/// header (`render_entry` space-escapes body lines that would collide),
/// so a body that merely quotes a header can't spoof this.
fn last_file_seq(path: &Path) -> Option<i64> {
    let content = std::fs::read_to_string(path).ok()?;
    let header = content.lines().rev().find(|line| line.starts_with("[#"))?;
    let rest = &header[2..];
    let end = rest.find(' ')?;
    rest[..end].parse().ok()
}

pub fn heal_if_stale(
    conn: &rusqlite::Connection,
    transcript_dir: &Path,
    conversation_id: &str,
) -> rusqlite::Result<()> {
    let max_seq: Option<i64> = conn.query_row(
        "SELECT MAX(sequence) FROM messages WHERE conversation_id = ?1",
        [conversation_id],
        |row| row.get(0),
    )?;
    let file_seq = last_file_seq(&transcript_path(transcript_dir, conversation_id));
    if file_seq != max_seq {
        regenerate(conn, transcript_dir, conversation_id)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::test_connection;

    #[allow(clippy::too_many_arguments)]
    fn insert_row(
        conn: &rusqlite::Connection,
        conv: &str,
        seq: i64,
        role: &str,
        ct: &str,
        content: &str,
        tool: Option<&str>,
        model_text: Option<&str>,
    ) {
        conn.execute(
            "INSERT INTO messages (id, conversation_id, role, content_type, content, tool_name, created_at, sequence, model_text) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7, ?8)",
            rusqlite::params![uuid::Uuid::now_v7().to_string(), conv, role, ct, content, tool, seq, model_text],
        ).unwrap();
    }

    fn seed_conversation(conn: &rusqlite::Connection, id: &str) {
        conn.execute(
            "INSERT INTO conversations (id, workspace_id, spawned_by_conversation_id, title, created_at, updated_at) \
             VALUES (?1, NULL, NULL, 'T', 0, 0)",
            [id],
        ).unwrap();
    }

    #[test]
    fn golden_entry_formats() {
        assert_eq!(
            render_entry(1, "user", "text", None, "hello"),
            "[#1 user]\nhello\n\n"
        );
        assert_eq!(
            render_entry(
                2,
                "assistant",
                "tool_call",
                Some("Bash"),
                r#"{"command":"ls"}"#
            ),
            "[#2 assistant → Bash]\n{\"command\":\"ls\"}\n\n"
        );
        assert_eq!(
            render_entry(3, "tool", "tool_result", Some("Bash"), "ok"),
            "[#3 Bash result]\nok\n\n"
        );
        assert_eq!(
            render_entry(4, "assistant", "error", None, "boom"),
            "[#4 error]\nboom\n\n"
        );
        // tool_call bodies are capped; others are not.
        let big = "x".repeat(5000);
        let capped = render_entry(5, "assistant", "tool_call", Some("Write"), &big);
        assert!(capped.len() < 2200 && capped.contains("… [args truncated]"));
        // A body ending in "\n" (tool stdout, routinely) still renders with
        // exactly one trailing blank line, not two.
        assert_eq!(
            render_entry(6, "tool", "tool_result", Some("Bash"), "ok\n"),
            "[#6 Bash result]\nok\n\n"
        );
        // A body line that would collide with an entry header gets one
        // leading space, so last_file_seq's line-anchored parse can't be
        // spoofed by quoted transcript text or grep output.
        assert_eq!(
            render_entry(
                7,
                "tool",
                "tool_result",
                Some("Grep"),
                "found:\n[#5 assistant]\nmore"
            ),
            "[#7 Grep result]\nfound:\n [#5 assistant]\nmore\n\n"
        );
        // An empty body renders as header + exactly one blank line.
        assert_eq!(render_entry(8, "user", "text", None, ""), "[#8 user]\n\n");
    }

    #[test]
    fn regenerate_then_append_matches_regenerate_from_scratch() {
        let conn = test_connection();
        let dir = tempfile::tempdir().unwrap();
        seed_conversation(&conn, "c1");
        insert_row(&conn, "c1", 0, "user", "text", "hi", None, None);
        insert_row(&conn, "c1", 1, "assistant", "text", "hello", None, None);
        regenerate(&conn, dir.path(), "c1").unwrap();
        // Now append rows 2 and 3 both ways and compare byte-for-byte.
        // Row 3 is a legacy tool row (model_text NULL), exercising the
        // content fallback in row_body.
        insert_row(
            &conn,
            "c1",
            2,
            "tool",
            "tool_result",
            "{}",
            Some("Bash"),
            Some("done"),
        );
        insert_row(
            &conn,
            "c1",
            3,
            "tool",
            "tool_result",
            "legacy output",
            Some("Bash"),
            None,
        );
        append_entry(
            dir.path(),
            "c1",
            &render_entry(2, "tool", "tool_result", Some("Bash"), "done"),
        )
        .unwrap();
        append_entry(
            dir.path(),
            "c1",
            &render_entry(3, "tool", "tool_result", Some("Bash"), "legacy output"),
        )
        .unwrap();
        let appended = std::fs::read_to_string(transcript_path(dir.path(), "c1")).unwrap();
        regenerate(&conn, dir.path(), "c1").unwrap();
        let rebuilt = std::fs::read_to_string(transcript_path(dir.path(), "c1")).unwrap();
        assert_eq!(appended, rebuilt);
        assert!(
            rebuilt.contains("[#3 Bash result]\nlegacy output"),
            "a legacy row's body must fall back to content when model_text is NULL"
        );
    }

    #[test]
    fn heal_regenerates_on_missing_stale_or_torn_files() {
        let conn = test_connection();
        let dir = tempfile::tempdir().unwrap();
        seed_conversation(&conn, "c1");
        insert_row(&conn, "c1", 0, "user", "text", "hi", None, None);
        // Missing file -> created.
        heal_if_stale(&conn, dir.path(), "c1").unwrap();
        let p = transcript_path(dir.path(), "c1");
        assert!(p.exists());
        let healthy = std::fs::read_to_string(&p).unwrap();
        // Fresh file + no new rows -> untouched (same content).
        heal_if_stale(&conn, dir.path(), "c1").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), healthy);
        // Torn tail -> rebuilt.
        std::fs::write(&p, format!("{healthy}[#gar")).unwrap();
        heal_if_stale(&conn, dir.path(), "c1").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), healthy);
        // Stale (missing latest row) -> rebuilt.
        insert_row(&conn, "c1", 1, "assistant", "text", "hello", None, None);
        heal_if_stale(&conn, dir.path(), "c1").unwrap();
        assert!(std::fs::read_to_string(&p)
            .unwrap()
            .contains("[#1 assistant]"));
    }

    #[test]
    fn heal_is_not_spoofed_by_a_body_that_quotes_a_header() {
        let conn = test_connection();
        let dir = tempfile::tempdir().unwrap();
        seed_conversation(&conn, "c1");
        for i in 0..4 {
            insert_row(&conn, "c1", i, "user", "text", &format!("m{i}"), None, None);
        }
        // Row 4's body quotes a future entry header on its own line -- e.g.
        // grep output over a transcript file, or a quoted transcript excerpt.
        insert_row(
            &conn,
            "c1",
            4,
            "tool",
            "tool_result",
            "{}",
            Some("Grep"),
            Some("matches:\n[#5 assistant]\nquoted"),
        );
        regenerate(&conn, dir.path(), "c1").unwrap();
        // A real row 5 lands in the DB but is never appended to the file --
        // the quoted "[#5 assistant]" must not make the stale file look
        // healthy.
        insert_row(
            &conn,
            "c1",
            5,
            "assistant",
            "text",
            "real reply",
            None,
            None,
        );
        heal_if_stale(&conn, dir.path(), "c1").unwrap();
        let content = std::fs::read_to_string(transcript_path(dir.path(), "c1")).unwrap();
        assert!(
            content.contains("[#5 assistant]\nreal reply"),
            "heal_if_stale must regenerate: a header quoted inside a body \
             must not mask a missing real entry"
        );
    }
}
