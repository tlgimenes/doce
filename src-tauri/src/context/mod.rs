//! Context-window management (010-context-window-management): live
//! per-conversation token accounting, tiered compaction (clear old tool
//! results, then summarize), and the settings that govern both. llama.cpp
//! has no server-side equivalent of these, so everything here is
//! reimplemented client-side against the app's own SQLite-persisted
//! conversation state (see research.md for why each piece is shaped this
//! way).
//!
//! Note on testability: unlike `apply_lightweight_clearing`/`ContextSettings`/
//! `exceeds` below (pure, unit-tested), `compute_usage`/`maybe_compact`/
//! `summarize_and_persist` all take a real `&InferenceEngine`, which (like
//! every other real-inference code path in this codebase — see
//! `inference::InferenceEngine::generate`'s own lack of a unit test) needs an
//! actually-loaded GGUF model to construct, unavailable in `cargo test`.
//! Their correctness is exercised by `quickstart.md`'s manual validation
//! pass against the real app instead.

pub mod limits;
pub mod offload;

use crate::inference::{ChatMessage, InferenceEngine, MessageContent};
use crate::storage::conversations::{load_history_annotated, persist_context_notice, HistoryMessage};
use limits::{
    PROTECTED_RECENT_MESSAGES, SUMMARIZATION_PROMPT, SUMMARY_MAX_TOKENS, TOOL_CLEARED_PLACEHOLDER,
    TOOL_KEEP_N,
};
use rusqlite::Connection;
use serde::Serialize;
use std::path::Path;

/// Live per-conversation context-usage snapshot. `state` mirrors this
/// codebase's existing convention for wire-level status fields
/// (`Conversation.status`, `GenerationQueueUpdate.state`) of a plain
/// `String` over a specta-derived Rust enum: one of `"normal"` /
/// `"warning"` / `"justCompacted"`.
#[derive(Debug, Clone, Serialize, specta::Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct ContextUsage {
    pub conversation_id: String,
    pub tokens_used: u32,
    pub token_budget: u32,
    pub state: String,
}

/// The four `context.*` settings keys (research.md's threshold-defaults
/// decision), read via the existing generic `settings` table rather than a
/// new schema. Default values live in `context::limits` (the single
/// source of truth for every context-budget-relative constant) --
/// aliased here as associated consts purely so existing call sites can
/// keep spelling them `ContextSettings::DEFAULT_...`.
#[derive(Debug, Clone, PartialEq)]
pub struct ContextSettings {
    pub warn_threshold_pct: f64,
    pub compact_threshold_pct: f64,
    pub hard_limit_pct: f64,
    pub tool_output_offload_chars: usize,
}

impl ContextSettings {
    pub const DEFAULT_WARN_THRESHOLD_PCT: f64 = limits::DEFAULT_WARN_THRESHOLD_PCT;
    pub const DEFAULT_COMPACT_THRESHOLD_PCT: f64 = limits::DEFAULT_COMPACT_THRESHOLD_PCT;
    pub const DEFAULT_HARD_LIMIT_PCT: f64 = limits::DEFAULT_HARD_LIMIT_PCT;
    pub const DEFAULT_TOOL_OUTPUT_OFFLOAD_CHARS: usize = limits::DEFAULT_TOOL_OUTPUT_OFFLOAD_CHARS;

    pub const KEY_WARN_THRESHOLD_PCT: &'static str = "context.warnThresholdPct";
    pub const KEY_COMPACT_THRESHOLD_PCT: &'static str = "context.compactThresholdPct";
    pub const KEY_HARD_LIMIT_PCT: &'static str = "context.hardLimitPct";
    pub const KEY_TOOL_OUTPUT_OFFLOAD_CHARS: &'static str = "context.toolOutputOffloadChars";

    /// Pure parse-with-defaults-and-clamping logic (data-model.md's
    /// Validation Rules), independent of the DB so it's unit-testable
    /// without a connection. An unparseable/missing/out-of-range value
    /// falls back to its default rather than erroring — a hand-edited
    /// settings row must not be able to brick a conversation.
    pub fn from_raw(raw: &std::collections::HashMap<String, String>) -> Self {
        let parse_pct = |key: &str, default: f64| -> f64 {
            raw.get(key)
                .and_then(|v| v.parse::<f64>().ok())
                .filter(|v| *v > 0.0 && *v <= 1.0)
                .unwrap_or(default)
        };
        let warn_threshold_pct = parse_pct(Self::KEY_WARN_THRESHOLD_PCT, Self::DEFAULT_WARN_THRESHOLD_PCT);
        let compact_threshold_pct_raw =
            parse_pct(Self::KEY_COMPACT_THRESHOLD_PCT, Self::DEFAULT_COMPACT_THRESHOLD_PCT);
        let hard_limit_pct_raw = parse_pct(Self::KEY_HARD_LIMIT_PCT, Self::DEFAULT_HARD_LIMIT_PCT);
        let tool_output_offload_chars = raw
            .get(Self::KEY_TOOL_OUTPUT_OFFLOAD_CHARS)
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|v| *v > 0)
            .unwrap_or(Self::DEFAULT_TOOL_OUTPUT_OFFLOAD_CHARS);

        // Invariant: warn <= compact <= hardLimit. Clamp up rather than
        // erroring if a hand-edited setting violates it.
        let compact_threshold_pct = compact_threshold_pct_raw.max(warn_threshold_pct);
        let hard_limit_pct = hard_limit_pct_raw.max(compact_threshold_pct);

        Self {
            warn_threshold_pct,
            compact_threshold_pct,
            hard_limit_pct,
            tool_output_offload_chars,
        }
    }

    /// No separate seeding step is needed for these four keys: an absent
    /// row simply falls back to its default inside `from_raw` above, the
    /// same lazy-default behavior `get_settings`/`update_setting` already
    /// rely on elsewhere in this codebase. A key only ever appears in the
    /// `settings` table once a user (or a future settings UI) explicitly
    /// writes it via `update_setting`.
    pub async fn load(conn: &tokio_rusqlite::Connection) -> Result<Self, String> {
        let raw = conn
            .call(|conn: &mut Connection| -> rusqlite::Result<std::collections::HashMap<String, String>> {
                let mut stmt = conn.prepare(
                    "SELECT key, value FROM settings WHERE key IN (?1, ?2, ?3, ?4)",
                )?;
                let rows = stmt
                    .query_map(
                        rusqlite::params![
                            Self::KEY_WARN_THRESHOLD_PCT,
                            Self::KEY_COMPACT_THRESHOLD_PCT,
                            Self::KEY_HARD_LIMIT_PCT,
                            Self::KEY_TOOL_OUTPUT_OFFLOAD_CHARS,
                        ],
                        |row| {
                            let key: String = row.get(0)?;
                            let value: String = row.get(1)?;
                            Ok((key, value))
                        },
                    )?
                    .collect::<Result<std::collections::HashMap<_, _>, _>>()?;
                Ok(rows)
            })
            .await
            .map_err(|e| e.to_string())?;

        Ok(Self::from_raw(&raw))
    }
}

/// Pure threshold check, shared by warn/compact classification.
fn exceeds(tokens_used: u32, token_budget: u32, pct: f64) -> bool {
    (tokens_used as f64) >= pct * (token_budget as f64)
}

fn classify_state(tokens_used: u32, token_budget: u32, settings: &ContextSettings) -> String {
    if exceeds(tokens_used, token_budget, settings.warn_threshold_pct) {
        "warning".to_string()
    } else {
        "normal".to_string()
    }
}

async fn load_history_via_conn(
    conn: &tokio_rusqlite::Connection,
    conversation_id: &str,
    skills_dir: &Path,
) -> Result<Vec<HistoryMessage>, String> {
    let conversation_id = conversation_id.to_string();
    let skills_dir = skills_dir.to_path_buf();
    conn.call(move |conn: &mut Connection| load_history_annotated(conn, &conversation_id, &skills_dir))
        .await
        .map_err(|e| e.to_string())
}

async fn persist_notice(
    conn: &tokio_rusqlite::Connection,
    conversation_id: &str,
    notice_json: String,
) -> Result<(), String> {
    let conversation_id = conversation_id.to_string();
    let now = crate::commands::models::now_ms();
    conn.call(move |conn: &mut Connection| persist_context_notice(conn, &conversation_id, now, &notice_json))
        .await
        .map_err(|e| e.to_string())
}

/// Renders `history` (prefixed with `system_prompt`) through the model's own
/// chat template and counts tokens — the exact prompt shape `generate()`
/// would actually decode for this conversation right now.
async fn usage_from_history(
    engine: &InferenceEngine,
    conversation_id: &str,
    history: &[HistoryMessage],
    system_prompt: &str,
    settings: &ContextSettings,
) -> Result<ContextUsage, String> {
    let mut messages = vec![ChatMessage::system(system_prompt)];
    messages.extend(history.iter().map(|m| m.chat.clone()));

    let rendered = engine.render_chat_prompt(&messages).map_err(|e| e.to_string())?;
    let tokens_used = engine.count_tokens(&rendered).map_err(|e| e.to_string())? as u32;
    let token_budget = engine.context_window();
    let state = classify_state(tokens_used, token_budget, settings);

    Ok(ContextUsage {
        conversation_id: conversation_id.to_string(),
        tokens_used,
        token_budget,
        state,
    })
}

/// Computes a conversation's current context usage from its persisted
/// history — always recomputed, never cached (research.md's "token
/// accounting is always recomputed" decision), so it's correct immediately
/// after a reopen (FR-014).
pub async fn compute_usage(
    conn: &tokio_rusqlite::Connection,
    engine: &InferenceEngine,
    conversation_id: &str,
    skills_dir: &Path,
    system_prompt: &str,
) -> Result<ContextUsage, String> {
    let settings = ContextSettings::load(conn).await?;
    let mut history = load_history_via_conn(conn, conversation_id, skills_dir).await?;
    // Tier 1 is a pure, idempotent, load-time transform (see
    // apply_lightweight_clearing's own doc comment) -- applying it here too
    // (not just inside maybe_compact/compact_in_memory) is what makes this
    // function actually report "what the effective prompt looks like right
    // now" instead of a raw, pre-tier-1 count that silently disagrees with
    // what compaction already achieved. Without this, get_context_usage and
    // emit_context_usage_update (both built on this function) would report
    // a persistently-too-high number even after tier 1 had genuinely
    // cleared old tool results -- found via real use, not speculatively.
    apply_lightweight_clearing(&mut history, TOOL_KEEP_N);
    usage_from_history(engine, conversation_id, &history, system_prompt, &settings).await
}

/// Tier 1: a pure, idempotent, load-time transform. Walking oldest-to-newest,
/// every `tool_call`/`tool_result` message beyond the most recent `keep_n`
/// such messages has its content replaced with a fixed placeholder. Returns
/// the count actually cleared. Independent of persistence — recomputable
/// fresh from `content_type`+order alone every time, which is why no
/// "cut marker" is needed for this tier to stay correct across reloads
/// (research.md).
pub fn apply_lightweight_clearing(history: &mut [HistoryMessage], keep_n: usize) -> usize {
    let tool_indices: Vec<usize> = history
        .iter()
        .enumerate()
        .filter(|(_, m)| m.content_type == "tool_call" || m.content_type == "tool_result")
        .map(|(i, _)| i)
        .collect();

    if tool_indices.len() <= keep_n {
        return 0;
    }

    let to_clear = &tool_indices[..tool_indices.len() - keep_n];
    for &i in to_clear {
        history[i].chat.content = MessageContent::Text(TOOL_CLEARED_PLACEHOLDER.to_string());
    }
    to_clear.len()
}

/// Tier 2: summarizes everything except the most recent `protected_recent`
/// messages via a real `generate()` call against the same loaded model, and
/// persists the result as a `context_notice` row (`kind:"summarized"`) that
/// `load_history_annotated` will splice in on every subsequent load.
/// Returns `Ok(None)` (no-op) when there's nothing eligible to summarize.
pub async fn summarize_and_persist(
    conn: &tokio_rusqlite::Connection,
    engine: &InferenceEngine,
    conversation_id: &str,
    history: &[HistoryMessage],
    protected_recent: usize,
) -> Result<Option<String>, String> {
    if history.len() <= protected_recent {
        return Ok(None);
    }

    let to_summarize = &history[..history.len() - protected_recent];
    let mut messages = vec![ChatMessage::system(SUMMARIZATION_PROMPT)];
    messages.extend(to_summarize.iter().map(|m| m.chat.clone()));

    let rendered = engine.render_chat_prompt(&messages).map_err(|e| e.to_string())?;
    let summary = engine
        .generate(&rendered, SUMMARY_MAX_TOKENS, false, |_| {}, || false)
        .map_err(|e| e.to_string())?;

    let notice_json = serde_json::json!({
        "kind": "summarized",
        "summary": summary,
        "notice": "Conversation condensed to save space",
    })
    .to_string();
    persist_notice(conn, conversation_id, notice_json).await?;

    Ok(Some(summary))
}

/// Orchestrates the tiered compaction pipeline: computes usage, and — if
/// `force` or the compaction threshold is crossed — runs tier 1
/// (lightweight clearing), and if that alone isn't enough, tier 2
/// (summarization). Returns the resulting usage with `state:"justCompacted"`
/// if either tier actually changed something, or the plain
/// `"normal"`/`"warning"` classification unchanged if there was nothing to
/// do (data-model.md's no-fabricated-notice rule — this is what makes a
/// manual "Compact now" on an already-small conversation a true no-op).
///
/// Deliberately does NOT enforce the hard-limit block itself — that's a
/// caller decision (`send_message`/`send_agent_message` block generation on
/// it; the manual `compact_conversation` command does not, since a user
/// explicitly asking to compact should see the resulting usage, not an
/// error, even if it's still high afterward).
pub async fn maybe_compact(
    conn: &tokio_rusqlite::Connection,
    engine: &InferenceEngine,
    conversation_id: &str,
    skills_dir: &Path,
    system_prompt: &str,
    force: bool,
) -> Result<ContextUsage, String> {
    let settings = ContextSettings::load(conn).await?;
    let mut history = load_history_via_conn(conn, conversation_id, skills_dir).await?;
    let mut usage = usage_from_history(engine, conversation_id, &history, system_prompt, &settings).await?;

    let over_compact_threshold =
        |u: &ContextUsage| exceeds(u.tokens_used, u.token_budget, settings.compact_threshold_pct);

    if !force && !over_compact_threshold(&usage) {
        return Ok(usage);
    }

    let mut changed = false;

    let cleared_count = apply_lightweight_clearing(&mut history, TOOL_KEEP_N);
    if cleared_count > 0 {
        changed = true;
        let plural = if cleared_count == 1 { "" } else { "s" };
        let notice_json = serde_json::json!({
            "kind": "cleared",
            "clearedCount": cleared_count,
            "notice": format!("{cleared_count} old tool result{plural} cleared to save space"),
        })
        .to_string();
        persist_notice(conn, conversation_id, notice_json).await?;
        usage = usage_from_history(engine, conversation_id, &history, system_prompt, &settings).await?;
    }

    if over_compact_threshold(&usage) {
        let summarized =
            summarize_and_persist(conn, engine, conversation_id, &history, PROTECTED_RECENT_MESSAGES).await?;
        if summarized.is_some() {
            changed = true;
            // summarize_and_persist just persisted a new context_notice row
            // that changes load_history_annotated's splice point -- reload
            // rather than trying to reconstruct the spliced view in memory.
            history = load_history_via_conn(conn, conversation_id, skills_dir).await?;
            usage = usage_from_history(engine, conversation_id, &history, system_prompt, &settings).await?;
        }
    }

    if changed {
        usage.state = "justCompacted".to_string();
    }

    Ok(usage)
}

async fn usage_from_chat_messages(
    engine: &InferenceEngine,
    conversation_id: &str,
    messages: &[ChatMessage],
    system_prompt: &str,
    settings: &ContextSettings,
) -> Result<ContextUsage, String> {
    let mut full = vec![ChatMessage::system(system_prompt)];
    full.extend(messages.iter().cloned());
    let rendered = engine.render_chat_prompt(&full).map_err(|e| e.to_string())?;
    let tokens_used = engine.count_tokens(&rendered).map_err(|e| e.to_string())? as u32;
    let token_budget = engine.context_window();
    let state = classify_state(tokens_used, token_budget, settings);
    Ok(ContextUsage {
        conversation_id: conversation_id.to_string(),
        tokens_used,
        token_budget,
        state,
    })
}

/// Tier 1, in-memory variant of `apply_lightweight_clearing` — same
/// keep-the-most-recent-`keep_n` algorithm, but matches directly on
/// `MessageContent::ToolResult` instead of a persisted `content_type`
/// (which a real in-memory tool result, live in `agent::run_loop`'s own
/// growing `Vec<ChatMessage>`, doesn't have until the turn finishes and
/// `commands::agent` writes its DB row) — no string-prefix guessing
/// needed now that this is a real, structured variant.
fn apply_lightweight_clearing_in_memory(messages: &mut [ChatMessage], keep_n: usize) -> usize {
    let tool_indices: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, m)| matches!(m.content, MessageContent::ToolResult { .. }))
        .map(|(i, _)| i)
        .collect();

    if tool_indices.len() <= keep_n {
        return 0;
    }

    let to_clear = &tool_indices[..tool_indices.len() - keep_n];
    for &i in to_clear {
        messages[i].content = MessageContent::Text(TOOL_CLEARED_PLACEHOLDER.to_string());
    }
    to_clear.len()
}

/// The mid-agent-loop counterpart to `maybe_compact`: called before *every*
/// turn inside `agent::run_loop`, not just once before the loop starts.
/// Without this, a single agent turn that makes several tool calls can
/// blow past the model's context window with nothing to stop it until the
/// *next* top-level message — which is exactly what let a raw
/// `NoKvCacheSlot` llama.cpp decode error reach the user instead of a
/// graceful compaction (found via real use, not speculatively). Mutates
/// `messages` in place — tier 1's placeholder swap, and tier 2's splice of
/// a persisted summary in place of the non-protected prefix — mirroring
/// `maybe_compact`'s two tiers, adapted to `agent::run_loop`'s own
/// `Vec<ChatMessage>` (which has no persisted `content_type` to key
/// tier 1's classifier off of, hence `apply_lightweight_clearing_in_memory`
/// above).
///
/// Deliberately simpler than `maybe_compact` in one respect: every
/// over-threshold turn re-runs both tiers against whatever `messages`
/// looks like *right now* (there is no way to persist a "already
/// compacted" marker back into `agent::run_loop`'s own growing history,
/// since it clones `messages` into this closure fresh on every turn) —
/// safe and correct (each render is independently brought under budget
/// before `generate()` is ever called on it), at the cost of a
/// long, tool-heavy single turn potentially producing more than one
/// `context_notice` row. Preventing the crash matters more than that
/// cosmetic edge case.
pub async fn compact_in_memory(
    conn: &tokio_rusqlite::Connection,
    engine: &InferenceEngine,
    conversation_id: &str,
    messages: &mut Vec<ChatMessage>,
    system_prompt: &str,
) -> Result<ContextUsage, String> {
    let settings = ContextSettings::load(conn).await?;
    let mut usage =
        usage_from_chat_messages(engine, conversation_id, messages, system_prompt, &settings).await?;

    let over_compact_threshold =
        |u: &ContextUsage| exceeds(u.tokens_used, u.token_budget, settings.compact_threshold_pct);

    if !over_compact_threshold(&usage) {
        return Ok(usage);
    }

    let mut changed = false;

    let cleared_count = apply_lightweight_clearing_in_memory(messages, TOOL_KEEP_N);
    if cleared_count > 0 {
        changed = true;
        let plural = if cleared_count == 1 { "" } else { "s" };
        let notice_json = serde_json::json!({
            "kind": "cleared",
            "clearedCount": cleared_count,
            "notice": format!("{cleared_count} old tool result{plural} cleared to save space"),
        })
        .to_string();
        persist_notice(conn, conversation_id, notice_json).await?;
        usage =
            usage_from_chat_messages(engine, conversation_id, messages, system_prompt, &settings).await?;
    }

    if over_compact_threshold(&usage) && messages.len() > PROTECTED_RECENT_MESSAGES {
        let split = messages.len() - PROTECTED_RECENT_MESSAGES;
        let mut summarize_prompt = vec![ChatMessage::system(SUMMARIZATION_PROMPT)];
        summarize_prompt.extend(messages[..split].iter().cloned());
        if let Ok(rendered) = engine.render_chat_prompt(&summarize_prompt) {
            if let Ok(summary) = engine.generate(&rendered, SUMMARY_MAX_TOKENS, false, |_| {}, || false) {
                let notice_json = serde_json::json!({
                    "kind": "summarized",
                    "summary": summary,
                    "notice": "Conversation condensed to save space",
                })
                .to_string();
                persist_notice(conn, conversation_id, notice_json).await?;

                let mut spliced = vec![ChatMessage::system(summary)];
                spliced.extend(messages[split..].iter().cloned());
                *messages = spliced;
                changed = true;
                usage = usage_from_chat_messages(engine, conversation_id, messages, system_prompt, &settings)
                    .await?;
            }
        }
    }

    if changed {
        usage.state = "justCompacted".to_string();
    }

    Ok(usage)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn history_message(content_type: &str, sequence: i64, content: &str) -> HistoryMessage {
        HistoryMessage {
            chat: ChatMessage::user(content),
            content_type: content_type.to_string(),
            sequence,
        }
    }

    // --- apply_lightweight_clearing ---

    #[test]
    fn no_tool_messages_clears_nothing() {
        let mut history = vec![
            history_message("text", 0, "hi"),
            history_message("text", 1, "hello"),
        ];
        assert_eq!(apply_lightweight_clearing(&mut history, TOOL_KEEP_N), 0);
        assert_eq!(history[0].chat.text(), "hi");
        assert_eq!(history[1].chat.text(), "hello");
    }

    #[test]
    fn exactly_keep_n_tool_messages_clears_nothing() {
        let mut history: Vec<HistoryMessage> = (0..TOOL_KEEP_N as i64)
            .map(|i| history_message("tool_result", i, "result"))
            .collect();
        assert_eq!(apply_lightweight_clearing(&mut history, TOOL_KEEP_N), 0);
        assert!(history.iter().all(|m| m.chat.text() == "result"));
    }

    #[test]
    fn keep_n_plus_three_tool_messages_clears_the_oldest_three() {
        let mut history: Vec<HistoryMessage> = (0..(TOOL_KEEP_N as i64 + 3))
            .map(|i| history_message("tool_result", i, &format!("result {i}")))
            .collect();

        let cleared = apply_lightweight_clearing(&mut history, TOOL_KEEP_N);
        assert_eq!(cleared, 3);

        for message in &history[0..3] {
            assert_eq!(message.chat.text(), TOOL_CLEARED_PLACEHOLDER);
        }
        for (i, message) in history.iter().enumerate().skip(3) {
            assert_eq!(message.chat.text(), format!("result {i}"));
        }
    }

    #[test]
    fn non_tool_messages_are_never_cleared() {
        // 5 tool_result messages, TOOL_KEEP_N of them protected -- the
        // oldest (5 - TOOL_KEEP_N) get cleared, regardless of the exact
        // constant's current value.
        let mut history = vec![history_message("text", 0, "old text stays")];
        history.extend((1..=5).map(|i| history_message("tool_result", i, &format!("r{}", i - 1))));

        let cleared = apply_lightweight_clearing(&mut history, TOOL_KEEP_N);
        assert_eq!(cleared, 5 - TOOL_KEEP_N);
        assert_eq!(history[0].chat.text(), "old text stays");
        for message in &history[1..=cleared] {
            assert_eq!(message.chat.text(), TOOL_CLEARED_PLACEHOLDER);
        }
        for message in &history[cleared + 1..] {
            assert_ne!(message.chat.text(), TOOL_CLEARED_PLACEHOLDER);
        }
    }

    // --- apply_lightweight_clearing_in_memory (mid-agent-loop tier 1) ---

    fn tool_result_message(i: i64) -> ChatMessage {
        ChatMessage::tool_result(format!("call-{i}"), "Bash", format!("result {i}"))
    }

    #[test]
    fn in_memory_no_tool_results_clears_nothing() {
        let mut messages = vec![ChatMessage::user("hi"), ChatMessage::assistant("hello")];
        assert_eq!(apply_lightweight_clearing_in_memory(&mut messages, TOOL_KEEP_N), 0);
        assert_eq!(messages[0].text(), "hi");
    }

    #[test]
    fn in_memory_keep_n_plus_three_tool_results_clears_the_oldest_three() {
        let mut messages: Vec<ChatMessage> =
            (0..(TOOL_KEEP_N as i64 + 3)).map(tool_result_message).collect();

        let cleared = apply_lightweight_clearing_in_memory(&mut messages, TOOL_KEEP_N);
        assert_eq!(cleared, 3);

        for message in &messages[0..3] {
            assert_eq!(message.text(), TOOL_CLEARED_PLACEHOLDER);
        }
        for (i, message) in messages.iter().enumerate().skip(3) {
            assert_eq!(message.text(), format!("<tool_response>result {i}</tool_response>"));
        }
    }

    #[test]
    fn in_memory_assistant_messages_are_never_mistaken_for_tool_results() {
        // A plain assistant reply happens to start with normal prose that
        // used to look like the old "Tool result for " string marker --
        // now structurally impossible to misclassify, since only
        // ChatMessage::tool_result ever produces a MessageContent::ToolResult
        // and ChatMessage::assistant never does, regardless of its text.
        // Kept as a regression guard against reintroducing string-based
        // classification.
        let mut messages = vec![
            ChatMessage::assistant("Tool result for you: I'm not actually a tool result"),
            tool_result_message(0),
            tool_result_message(1),
            tool_result_message(2),
            tool_result_message(3),
            tool_result_message(4),
        ];
        let cleared = apply_lightweight_clearing_in_memory(&mut messages, TOOL_KEEP_N);
        assert_eq!(
            cleared,
            5 - TOOL_KEEP_N,
            "only the 5 real ToolResult messages count toward clearing"
        );
        assert_eq!(
            messages[0].text(),
            "Tool result for you: I'm not actually a tool result",
            "an assistant Text message is never cleared, regardless of its text"
        );
    }

    // --- ContextSettings::from_raw ---

    #[test]
    fn missing_settings_fall_back_to_defaults() {
        let raw = std::collections::HashMap::new();
        let settings = ContextSettings::from_raw(&raw);
        assert_eq!(settings.warn_threshold_pct, ContextSettings::DEFAULT_WARN_THRESHOLD_PCT);
        assert_eq!(settings.compact_threshold_pct, ContextSettings::DEFAULT_COMPACT_THRESHOLD_PCT);
        assert_eq!(settings.hard_limit_pct, ContextSettings::DEFAULT_HARD_LIMIT_PCT);
        assert_eq!(
            settings.tool_output_offload_chars,
            ContextSettings::DEFAULT_TOOL_OUTPUT_OFFLOAD_CHARS
        );
    }

    #[test]
    fn unparseable_settings_fall_back_to_defaults() {
        let mut raw = std::collections::HashMap::new();
        raw.insert(ContextSettings::KEY_WARN_THRESHOLD_PCT.to_string(), "not a number".to_string());
        raw.insert(ContextSettings::KEY_TOOL_OUTPUT_OFFLOAD_CHARS.to_string(), "-5".to_string());
        let settings = ContextSettings::from_raw(&raw);
        assert_eq!(settings.warn_threshold_pct, ContextSettings::DEFAULT_WARN_THRESHOLD_PCT);
        assert_eq!(
            settings.tool_output_offload_chars,
            ContextSettings::DEFAULT_TOOL_OUTPUT_OFFLOAD_CHARS
        );
    }

    #[test]
    fn out_of_order_thresholds_are_clamped_up() {
        let mut raw = std::collections::HashMap::new();
        raw.insert(ContextSettings::KEY_WARN_THRESHOLD_PCT.to_string(), "0.9".to_string());
        raw.insert(ContextSettings::KEY_COMPACT_THRESHOLD_PCT.to_string(), "0.5".to_string());
        raw.insert(ContextSettings::KEY_HARD_LIMIT_PCT.to_string(), "0.6".to_string());
        let settings = ContextSettings::from_raw(&raw);
        assert_eq!(settings.warn_threshold_pct, 0.9);
        assert_eq!(settings.compact_threshold_pct, 0.9, "compact must be clamped up to at least warn");
        assert_eq!(settings.hard_limit_pct, 0.9, "hard limit must be clamped up to at least compact");
    }

    #[test]
    fn valid_settings_pass_through_unchanged() {
        let mut raw = std::collections::HashMap::new();
        raw.insert(ContextSettings::KEY_WARN_THRESHOLD_PCT.to_string(), "0.4".to_string());
        raw.insert(ContextSettings::KEY_COMPACT_THRESHOLD_PCT.to_string(), "0.6".to_string());
        raw.insert(ContextSettings::KEY_HARD_LIMIT_PCT.to_string(), "0.8".to_string());
        raw.insert(ContextSettings::KEY_TOOL_OUTPUT_OFFLOAD_CHARS.to_string(), "1500".to_string());
        let settings = ContextSettings::from_raw(&raw);
        assert_eq!(settings.warn_threshold_pct, 0.4);
        assert_eq!(settings.compact_threshold_pct, 0.6);
        assert_eq!(settings.hard_limit_pct, 0.8);
        assert_eq!(settings.tool_output_offload_chars, 1500);
    }

    // --- threshold math (exceeds/classify_state) ---

    #[test]
    fn classify_state_normal_below_warn_threshold() {
        let settings = ContextSettings::from_raw(&std::collections::HashMap::new());
        assert_eq!(classify_state(100, 2048, &settings), "normal");
    }

    #[test]
    fn classify_state_warning_at_or_above_warn_threshold() {
        let settings = ContextSettings::from_raw(&std::collections::HashMap::new());
        let warn_at = (settings.warn_threshold_pct * 2048.0).ceil() as u32;
        assert_eq!(classify_state(warn_at, 2048, &settings), "warning");
    }
}
