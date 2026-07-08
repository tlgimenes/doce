use crate::storage::DbCell;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use tauri::{AppHandle, State};

const FUZZY_TITLE_MIN_SCORE: f64 = 0.45;
const FUZZY_TITLE_CANDIDATE_LIMIT: i64 = 50;

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub conversation_id: String,
    pub title: String,
    /// FTS5 `snippet()` output with `<mark>...</mark>` around matches.
    pub excerpt: String,
    /// FTS5 `bm25()` — lower (more negative) is a better match.
    pub rank: f64,
}

fn trigrams(value: &str) -> Vec<String> {
    let chars: Vec<char> = value.trim().to_lowercase().chars().collect();
    if chars.len() < 3 {
        return Vec::new();
    }
    chars
        .windows(3)
        .map(|window| window.iter().collect::<String>())
        .collect()
}

fn quote_fts5_string(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn fuzzy_title_query(query: &str) -> Option<String> {
    let mut seen = HashSet::new();
    let grams = trigrams(query)
        .into_iter()
        .filter(|gram| seen.insert(gram.clone()))
        .map(|gram| quote_fts5_string(&gram))
        .collect::<Vec<_>>();
    if grams.is_empty() {
        None
    } else {
        Some(grams.join(" OR "))
    }
}

fn normalized_words(value: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    for ch in value.chars().flat_map(char::to_lowercase) {
        if ch.is_alphanumeric() {
            current.push(ch);
        } else if !current.is_empty() {
            words.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

fn trigram_set(value: &str) -> HashSet<String> {
    trigrams(value).into_iter().collect()
}

fn dice_similarity(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let intersection = a.intersection(b).count();
    (2.0 * intersection as f64) / (a.len() + b.len()) as f64
}

fn fuzzy_title_score(query: &str, title: &str) -> f64 {
    let query_words = normalized_words(query);
    if query_words.is_empty() {
        return 0.0;
    }
    let normalized_query = query_words.join(" ");
    let query_grams = trigram_set(&normalized_query);
    if query_grams.is_empty() {
        return 0.0;
    }

    let title_words = normalized_words(title);
    let mut candidates = title_words.clone();
    if !title_words.is_empty() {
        candidates.push(title_words.join(" "));
    }

    candidates
        .iter()
        .map(|candidate| dice_similarity(&query_grams, &trigram_set(candidate)))
        .fold(0.0, f64::max)
}

/// FR-029/FR-030: searches both conversation titles and message content via
/// FTS5, ranked by `bm25()`. Subagent-run conversations never surface here
/// — the FTS5 sync triggers (`0001_init.sql`) already exclude them from
/// being indexed at all, and this query re-checks
/// `spawned_by_conversation_id IS NULL` directly too, so the isolation
/// boundary holds even if a future migration changes the trigger logic.
fn search_impl(conn: &Connection, query: &str) -> rusqlite::Result<Vec<SearchResult>> {
    let mut best: HashMap<String, SearchResult> = HashMap::new();

    let mut title_stmt = conn.prepare(
        "SELECT c.id, c.title, snippet(conversations_fts, 0, '<mark>', '</mark>', '…', 8), bm25(conversations_fts)
         FROM conversations_fts
         JOIN conversations c ON c.rowid = conversations_fts.rowid
         WHERE conversations_fts MATCH ?1
           AND c.spawned_by_conversation_id IS NULL
           AND c.archived_at IS NULL",
    )?;
    let title_rows = title_stmt
        .query_map([query], |row| {
            Ok(SearchResult {
                conversation_id: row.get(0)?,
                title: row.get(1)?,
                excerpt: row.get(2)?,
                rank: row.get(3)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    for r in title_rows {
        best.entry(r.conversation_id.clone())
            .and_modify(|existing| {
                if r.rank < existing.rank {
                    *existing = r.clone();
                }
            })
            .or_insert(r);
    }

    let mut content_stmt = conn.prepare(
        "SELECT c.id, c.title, snippet(messages_fts, 0, '<mark>', '</mark>', '…', 8), bm25(messages_fts)
         FROM messages_fts
         JOIN messages m ON m.rowid = messages_fts.rowid
         JOIN conversations c ON c.id = m.conversation_id
         WHERE messages_fts MATCH ?1
           AND c.spawned_by_conversation_id IS NULL
           AND c.archived_at IS NULL",
    )?;
    let content_rows = content_stmt
        .query_map([query], |row| {
            Ok(SearchResult {
                conversation_id: row.get(0)?,
                title: row.get(1)?,
                excerpt: row.get(2)?,
                rank: row.get(3)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    for r in content_rows {
        best.entry(r.conversation_id.clone())
            .and_modify(|existing| {
                if r.rank < existing.rank {
                    *existing = r.clone();
                }
            })
            .or_insert(r);
    }

    if let Some(fuzzy_query) = fuzzy_title_query(query) {
        let mut fuzzy_stmt = conn.prepare(
            "SELECT c.id, c.title, bm25(conversations_title_trigram)
             FROM conversations_title_trigram
             JOIN conversations c ON c.rowid = conversations_title_trigram.rowid
             WHERE conversations_title_trigram MATCH ?1
               AND c.spawned_by_conversation_id IS NULL
               AND c.archived_at IS NULL
             ORDER BY bm25(conversations_title_trigram)
             LIMIT ?2",
        )?;
        let fuzzy_rows = fuzzy_stmt
            .query_map((&fuzzy_query, FUZZY_TITLE_CANDIDATE_LIMIT), |row| {
                let title: String = row.get(1)?;
                let score = fuzzy_title_score(query, &title);
                Ok((
                    SearchResult {
                        conversation_id: row.get(0)?,
                        title: title.clone(),
                        excerpt: title,
                        rank: 1.0 - score,
                    },
                    score,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        for (r, score) in fuzzy_rows {
            if score < FUZZY_TITLE_MIN_SCORE {
                continue;
            }
            best.entry(r.conversation_id.clone())
                .and_modify(|existing| {
                    if r.rank < existing.rank {
                        *existing = r.clone();
                    }
                })
                .or_insert(r);
        }
    }

    let mut results: Vec<SearchResult> = best.into_values().collect();
    results.sort_by(|a, b| {
        a.rank
            .partial_cmp(&b.rank)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(results)
}

#[tauri::command]
#[specta::specta]
pub async fn search_conversations(
    app: AppHandle,
    db_cell: State<'_, DbCell>,
    query: String,
) -> Result<Vec<SearchResult>, String> {
    let conn = db_cell.get(&app).await?;
    conn.call(
        move |conn: &mut Connection| -> rusqlite::Result<Vec<SearchResult>> {
            search_impl(conn, &query)
        },
    )
    .await
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::test_connection;

    fn insert_conversation(conn: &Connection, id: &str, title: &str, spawned_by: Option<&str>) {
        conn.execute(
            "INSERT INTO conversations (id, title, created_at, updated_at, spawned_by_conversation_id) VALUES (?1, ?2, 0, 0, ?3)",
            rusqlite::params![id, title, spawned_by],
        )
        .unwrap();
    }

    fn insert_message(conn: &Connection, conv_id: &str, seq: i64, content: &str) {
        conn.execute(
            "INSERT INTO messages (id, conversation_id, role, content_type, content, created_at, sequence) VALUES (?1, ?2, 'user', 'text', ?3, 0, ?4)",
            rusqlite::params![uuid::Uuid::now_v7().to_string(), conv_id, content, seq],
        )
        .unwrap();
    }

    #[test]
    fn finds_by_message_content() {
        let conn = test_connection();
        insert_conversation(&conn, "c1", "Some title", None);
        insert_message(
            &conn,
            "c1",
            0,
            "the quick brown fox jumps over the lazy dog",
        );

        let results = search_impl(&conn, "fox").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].conversation_id, "c1");
        assert!(results[0].excerpt.contains("<mark>fox</mark>"));
    }

    #[test]
    fn finds_by_title_when_content_does_not_match() {
        let conn = test_connection();
        insert_conversation(&conn, "c1", "database migration plan", None);

        let results = search_impl(&conn, "migration").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].conversation_id, "c1");
    }

    #[test]
    fn subagent_conversations_never_surface() {
        let conn = test_connection();
        insert_conversation(&conn, "parent", "parent conversation", None);
        insert_conversation(&conn, "sub", "subagent internal work", Some("parent"));
        insert_message(&conn, "sub", 0, "secret subagent-only keyword xyzzy123");

        let results = search_impl(&conn, "xyzzy123").unwrap();
        assert!(
            results.is_empty(),
            "subagent content leaked into search results"
        );
    }

    #[test]
    fn archived_conversations_never_surface() {
        let conn = test_connection();
        insert_conversation(&conn, "c1", "archive target", None);
        insert_message(&conn, "c1", 0, "hidden archived keyword");
        conn.execute(
            "UPDATE conversations SET archived_at = 100 WHERE id = 'c1'",
            [],
        )
        .unwrap();

        let title_results = search_impl(&conn, "archive").unwrap();
        assert!(
            title_results.is_empty(),
            "archived title leaked into search results"
        );

        let content_results = search_impl(&conn, "keyword").unwrap();
        assert!(
            content_results.is_empty(),
            "archived message content leaked into search results"
        );
    }

    #[test]
    fn no_match_returns_empty() {
        let conn = test_connection();
        insert_conversation(&conn, "c1", "title", None);
        insert_message(&conn, "c1", 0, "hello world");

        assert!(search_impl(&conn, "nonexistentterm").unwrap().is_empty());
    }

    #[test]
    fn ranks_stronger_matches_first() {
        let conn = test_connection();
        insert_conversation(&conn, "weak", "unrelated", None);
        insert_message(&conn, "weak", 0, "rust is mentioned once here");
        insert_conversation(&conn, "strong", "all about rust", None);
        insert_message(
            &conn,
            "strong",
            0,
            "rust rust rust rust everywhere, a rust conversation about rust",
        );

        let results = search_impl(&conn, "rust").unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].conversation_id, "strong");
    }

    #[test]
    fn fuzzy_title_match_finds_typo_when_exact_fts_misses() {
        let conn = test_connection();
        insert_conversation(&conn, "c1", "database migration plan", None);

        let exact_results = search_impl(&conn, "migraton").unwrap();
        assert_eq!(exact_results.len(), 1);
        assert_eq!(exact_results[0].conversation_id, "c1");
        assert!(exact_results[0].rank > 0.0);
    }

    #[test]
    fn fuzzy_title_search_keeps_subagent_conversations_hidden() {
        let conn = test_connection();
        insert_conversation(&conn, "parent", "parent conversation", None);
        insert_conversation(&conn, "sub", "internal migration notes", Some("parent"));

        let results = search_impl(&conn, "migraton").unwrap();
        assert!(
            results.is_empty(),
            "subagent title leaked into fuzzy search"
        );
    }
}
