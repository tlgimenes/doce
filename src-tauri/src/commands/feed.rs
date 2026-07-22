//! Activity feed: persisted "cards" for the MUTATING/creative MCP tool calls
//! the agent makes. A card is a SIDE-EFFECT of the existing MCP tool dispatch
//! (`commands::agent::RealBackend::dispatch_mcp_tool`) — NOT a tool the model
//! calls — so the agent loop, its tools array, and the benchmark's byte
//! invariance are all untouched. Reads/queries never produce a card; only
//! names that look like an action do (see [`infer_card`]).
//!
//! The feed is an ADDITIVE surface: cards are reviewed and dismissed from the
//! Activity section, and the chat transcript is left exactly as it was.

use crate::commands::models::now_ms;
use crate::storage::DbCell;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

/// One persisted activity-feed card, mirroring a `feed_cards` row. Also the
/// payload of the `feed-card-created` event (emitted live when a card is
/// created mid-turn). `kind` is one of the `ActivityCard` frontend variants
/// (`draft`/`event`/`file`/`shell`); `status` is `pending` or `dismissed`.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
#[tauri_specta(event_name = "feed-card-created")]
pub struct FeedCard {
    pub id: String,
    pub conversation_id: Option<String>,
    pub kind: String,
    pub title: String,
    pub preview: String,
    pub source_tool: String,
    pub status: String,
    pub created_at: i64,
}

/// The pure, server-free result of the mutating-name heuristic + kind
/// inference — everything about a card that derives ONLY from the tool call,
/// so it's unit-testable without a live MCP server or DB. The row's `id`,
/// `conversation_id`, `status`, and `created_at` are added at persist time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferredCard {
    pub kind: String,
    pub title: String,
    pub preview: String,
}

/// Raw MCP tool-name substrings that mark a call as MUTATING/creative — the
/// only calls that surface a card. Everything else (searches, lists, gets,
/// reads, and any unrecognized name) returns `None` from [`infer_card`], so
/// the feed stays "things the agent did", not every query it ran.
const MUTATING_TOKENS: [&str; 12] = [
    "create", "draft", "compose", "propose", "update", "write", "add", "new", "send", "edit",
    "delete", "move",
];

/// Longest a card's `preview` may be — the model-facing result, truncated so
/// a chatty tool result can't bloat the row or the feed card. Kept small: the
/// card is a glance, not the full output.
const PREVIEW_MAX: usize = 240;

/// The mutating-name heuristic + kind inference, as one pure function so both
/// halves are testable without a live server. Returns `None` for a non-mutating
/// name (a read/list/get/search/etc.), `Some(InferredCard)` otherwise.
///
/// `title` is `"<server_name>: <raw_tool_name>"`; `preview` is the truncated
/// model-facing result; `kind` maps to an existing `ActivityCard` variant
/// (`draft`/`event`/`file`/`shell`), inferred from the tool name with `shell`
/// as the generic fallback.
pub fn infer_card(
    server_name: &str,
    raw_tool_name: &str,
    result_preview: &str,
) -> Option<InferredCard> {
    let name = raw_tool_name.to_lowercase();
    if !MUTATING_TOKENS.iter().any(|token| name.contains(token)) {
        return None;
    }
    Some(InferredCard {
        kind: infer_kind(&name),
        title: format!("{server_name}: {raw_tool_name}"),
        preview: truncate_preview(result_preview),
    })
}

/// Maps a (lowercased) tool name to an `ActivityCard` variant. Ordered so the
/// most specific surface wins: a "draft"/"compose" is a draft even if it also
/// touches a file; a calendar action is an event; a drive/doc action is a
/// file; everything else falls back to the generic `shell` card.
fn infer_kind(name: &str) -> String {
    if name.contains("draft") || name.contains("compose") {
        "draft"
    } else if name.contains("event") || name.contains("calendar") {
        "event"
    } else if name.contains("file") || name.contains("drive") || name.contains("doc") {
        "file"
    } else {
        "shell"
    }
    .to_string()
}

/// Truncates on a char boundary (never mid-UTF-8), appending an ellipsis when
/// it actually cut something.
fn truncate_preview(text: &str) -> String {
    if text.chars().count() <= PREVIEW_MAX {
        return text.to_string();
    }
    let truncated: String = text.chars().take(PREVIEW_MAX).collect();
    format!("{truncated}…")
}

/// Builds a full [`FeedCard`] from an [`InferredCard`] plus the row-level
/// fields assigned at persist time. Split out so the id/timestamp assembly is
/// covered by the same tests as the heuristic.
fn build_card(inferred: InferredCard, conversation_id: Option<String>) -> FeedCard {
    // The raw tool name, recovered from the `"<server>: <tool>"` title — the
    // `source_tool` column. Falls back to the whole title if there's no
    // separator (there always is, from `infer_card`).
    let source_tool = inferred
        .title
        .split_once(": ")
        .map(|(_, tool)| tool.to_string())
        .unwrap_or_else(|| inferred.title.clone());
    FeedCard {
        id: Uuid::now_v7().to_string(),
        conversation_id,
        kind: inferred.kind,
        title: inferred.title,
        preview: inferred.preview,
        source_tool,
        status: "pending".to_string(),
        created_at: now_ms(),
    }
}

/// Emits a card for a successful MCP call IF its name passes the mutating
/// heuristic — the side-effect hook `dispatch_mcp_tool` calls after a tool
/// returns. BEST-EFFORT by construction: a `None` heuristic result, a DB
/// failure, or a dropped event all just mean no card; NONE of them can fail
/// the tool call or the turn (the caller ignores the return, and every error
/// here is logged and swallowed).
pub async fn record_mcp_card(
    app: &AppHandle,
    conn: &tokio_rusqlite::Connection,
    conversation_id: &str,
    server_name: &str,
    raw_tool_name: &str,
    result_preview: &str,
) {
    let Some(inferred) = infer_card(server_name, raw_tool_name, result_preview) else {
        return;
    };
    let card = build_card(inferred, Some(conversation_id.to_string()));
    let insert = conn
        .call({
            let card = card.clone();
            move |conn: &mut Connection| -> rusqlite::Result<()> { insert_card(conn, &card) }
        })
        .await;
    if let Err(e) = insert {
        eprintln!("feed: failed to persist card for {server_name}:{raw_tool_name}: {e}");
        return;
    }
    // Live-append signal for a mounted Activity view. Best-effort like every
    // other `app.emit` in this codebase — a dropped event just means the next
    // `list_feed_cards` reconciles the feed.
    let _ = app.emit("feed-card-created", &card);
}

/// The single INSERT for a feed card — shared by the emit path and the tests.
fn insert_card(conn: &Connection, card: &FeedCard) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO feed_cards (id, conversation_id, kind, title, preview, source_tool, status, created_at)\
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![
            card.id,
            card.conversation_id,
            card.kind,
            card.title,
            card.preview,
            card.source_tool,
            card.status,
            card.created_at,
        ],
    )?;
    Ok(())
}

/// Reads a `feed_cards` row into a [`FeedCard`] — the shared row-mapper for
/// [`list_feed_cards`] and the tests.
fn row_to_card(row: &rusqlite::Row) -> rusqlite::Result<FeedCard> {
    Ok(FeedCard {
        id: row.get(0)?,
        conversation_id: row.get(1)?,
        kind: row.get(2)?,
        title: row.get(3)?,
        preview: row.get(4)?,
        source_tool: row.get(5)?,
        status: row.get(6)?,
        created_at: row.get(7)?,
    })
}

/// Lists feed cards, pending first then dismissed, newest first within each
/// group. `conversation_id: Some(id)` scopes to one conversation; `None`
/// returns every conversation's cards (the global Activity view).
#[tauri::command]
#[specta::specta]
pub async fn list_feed_cards(
    app: AppHandle,
    db_cell: State<'_, DbCell>,
    conversation_id: Option<String>,
) -> Result<Vec<FeedCard>, String> {
    let conn = db_cell.get(&app).await?;
    conn.call(
        move |conn: &mut Connection| -> rusqlite::Result<Vec<FeedCard>> {
            // Pending (status='pending') sorts before dismissed; within each,
            // newest (highest created_at) first.
            let order =
                "ORDER BY CASE WHEN status = 'pending' THEN 0 ELSE 1 END, created_at DESC, id DESC";
            let cols = "id, conversation_id, kind, title, preview, source_tool, status, created_at";
            match conversation_id {
                Some(cid) => {
                    let sql =
                        format!("SELECT {cols} FROM feed_cards WHERE conversation_id = ?1 {order}");
                    let mut stmt = conn.prepare(&sql)?;
                    let rows = stmt
                        .query_map([&cid], row_to_card)?
                        .collect::<Result<Vec<_>, _>>()?;
                    Ok(rows)
                }
                None => {
                    let sql = format!("SELECT {cols} FROM feed_cards {order}");
                    let mut stmt = conn.prepare(&sql)?;
                    let rows = stmt
                        .query_map([], row_to_card)?
                        .collect::<Result<Vec<_>, _>>()?;
                    Ok(rows)
                }
            }
        },
    )
    .await
    .map_err(|e| e.to_string())
}

/// Dismisses a card (sets `status = 'dismissed'`). Idempotent: dismissing an
/// already-dismissed or unknown id is a no-op success.
#[tauri::command]
#[specta::specta]
pub async fn dismiss_feed_card(
    app: AppHandle,
    db_cell: State<'_, DbCell>,
    id: String,
) -> Result<(), String> {
    let conn = db_cell.get(&app).await?;
    conn.call(move |conn: &mut Connection| -> rusqlite::Result<()> {
        conn.execute(
            "UPDATE feed_cards SET status = 'dismissed' WHERE id = ?1",
            [&id],
        )?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory_db() -> Connection {
        crate::storage::test_connection()
    }

    #[test]
    fn mutating_names_produce_a_card_with_the_right_kind() {
        // draft/compose → "draft"
        assert_eq!(
            infer_card("Gmail", "create_draft", "ok").unwrap().kind,
            "draft"
        );
        assert_eq!(
            infer_card("Gmail", "compose_message", "ok").unwrap().kind,
            "draft"
        );
        // event/calendar → "event"
        assert_eq!(
            infer_card("Google Calendar", "create_event", "ok")
                .unwrap()
                .kind,
            "event"
        );
        // file/drive/doc → "file"
        assert_eq!(
            infer_card("Google Drive", "create_file", "ok")
                .unwrap()
                .kind,
            "file"
        );
        assert_eq!(
            infer_card("Notion", "update_doc", "ok").unwrap().kind,
            "file"
        );
        // mutating but generic → "shell" fallback
        assert_eq!(
            infer_card("Slack", "send_message", "ok").unwrap().kind,
            "shell"
        );
        assert_eq!(
            infer_card("Pkg", "add_dependency", "ok").unwrap().kind,
            "shell"
        );
    }

    #[test]
    fn title_and_preview_are_built_from_the_call() {
        let card = infer_card("Gmail", "create_draft", "Subject: hi\nBody").unwrap();
        assert_eq!(card.title, "Gmail: create_draft");
        assert_eq!(card.preview, "Subject: hi\nBody");
    }

    #[test]
    fn read_like_names_never_produce_a_card() {
        for name in [
            "search_events",
            "list_files",
            "get_event",
            "read_file_content",
            "find_contact",
            "query_rows",
            "fetch_page",
            // anything unrecognized → None too
            "ping",
            "authorize",
        ] {
            assert!(
                infer_card("Svc", name, "ok").is_none(),
                "{name} should not surface a card"
            );
        }
    }

    #[test]
    fn preview_is_truncated_on_a_char_boundary() {
        let long = "x".repeat(PREVIEW_MAX + 50);
        let card = infer_card("Svc", "create_thing", &long).unwrap();
        assert!(card.preview.ends_with('…'));
        assert_eq!(card.preview.chars().count(), PREVIEW_MAX + 1);
    }

    #[test]
    fn insert_and_list_orders_pending_first_then_newest_first() {
        let conn = memory_db();
        let mk = |id: &str, status: &str, created: i64| FeedCard {
            id: id.to_string(),
            conversation_id: Some("c1".to_string()),
            kind: "shell".to_string(),
            title: "Svc: send_x".to_string(),
            preview: "p".to_string(),
            source_tool: "send_x".to_string(),
            status: status.to_string(),
            created_at: created,
        };
        insert_card(&conn, &mk("a", "pending", 100)).unwrap();
        insert_card(&conn, &mk("b", "dismissed", 300)).unwrap();
        insert_card(&conn, &mk("c", "pending", 200)).unwrap();

        let order =
            "ORDER BY CASE WHEN status = 'pending' THEN 0 ELSE 1 END, created_at DESC, id DESC";
        let cols = "id, conversation_id, kind, title, preview, source_tool, status, created_at";
        let sql = format!("SELECT {cols} FROM feed_cards {order}");
        let mut stmt = conn.prepare(&sql).unwrap();
        let ids: Vec<String> = stmt
            .query_map([], row_to_card)
            .unwrap()
            .map(|r| r.unwrap().id)
            .collect();
        // pending newest-first (c@200, a@100), then dismissed (b@300).
        assert_eq!(ids, vec!["c", "a", "b"]);
    }

    #[test]
    fn dismiss_flips_status_and_round_trips() {
        let conn = memory_db();
        let card = build_card(
            infer_card("Gmail", "create_draft", "hi").unwrap(),
            Some("c1".to_string()),
        );
        insert_card(&conn, &card).unwrap();

        conn.execute(
            "UPDATE feed_cards SET status = 'dismissed' WHERE id = ?1",
            [&card.id],
        )
        .unwrap();

        let status: String = conn
            .query_row(
                "SELECT status FROM feed_cards WHERE id = ?1",
                [&card.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "dismissed");
    }
}
