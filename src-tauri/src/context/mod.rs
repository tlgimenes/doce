//! Context-window management (010-context-window-management): live
//! per-conversation token accounting, tiered compaction (clear old tool
//! results, then summarize), and the settings that govern both. llama.cpp
//! has no server-side equivalent of these, so everything here is
//! reimplemented client-side against the app's own SQLite-persisted
//! conversation state (see research.md for why each piece is shaped this
//! way).
//!
//! Note on testability: `apply_lightweight_clearing`/`ContextSettings`/
//! `exceeds` below are pure and directly unit-tested. `compute_usage`/
//! `maybe_compact` need a live DB connection (and, for `maybe_compact`, a
//! running `llama-server` — its `summarize_and_persist` generates through the
//! HTTP client at a `base_url`), so they are driven end-to-end by
//! `tests/real_model_smoke.rs`'s `#[ignore]`d real-model tests instead.
//!
//! This comment used to claim those two were untestable and left to
//! `quickstart.md`'s manual validation pass. They were not, and the first
//! pass that actually ran them found both halves of the persistence
//! pipeline broken (2026-07-15): tier 2's splice dropped the entire
//! conversation, and tier 1's clearing never reached the model at all. What
//! each tier PERSISTS is now pinned by fast unit tests over the real persist
//! path (`post_compaction_history_contract` below, and
//! `storage::conversations`'s splice tests) — a real-model test is the right
//! place to answer "does the model produce a usable summary", never the only
//! thing standing between a persistence bug and a shipped release.
//!
//! Where a real function needs a pure core unit-tested on its own, that core
//! is split out (`fit_turn_to_budget`/`fit_to_budget`;
//! `summarize_and_persist`/`messages_to_summarize`), the same way each time.

pub mod limits;
pub mod payload;
pub mod transcript;

use crate::agent::dispatch::ToolOutcome;
use crate::inference::{token_estimate, ChatMessage, MessageContent};
use crate::storage::conversations::{
    load_history_annotated, persist_context_notice, HistoryMessage,
};
use limits::{
    PROTECTED_RECENT_MESSAGES, SUMMARIZATION_PROMPT, SUMMARY_MAX_TOKENS, TOOL_CLEARED_PLACEHOLDER,
    TOOL_KEEP_N,
};
use rusqlite::Connection;
use serde::Serialize;
use std::path::Path;

/// Live per-conversation context-usage snapshot. `state` mirrors this
/// codebase's existing convention for wire-level status fields
/// (`Conversation.status`) of a plain
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
    pub tool_output_offload_tokens: usize,
}

impl ContextSettings {
    pub const DEFAULT_WARN_THRESHOLD_PCT: f64 = limits::DEFAULT_WARN_THRESHOLD_PCT;
    pub const DEFAULT_COMPACT_THRESHOLD_PCT: f64 = limits::DEFAULT_COMPACT_THRESHOLD_PCT;
    pub const DEFAULT_HARD_LIMIT_PCT: f64 = limits::DEFAULT_HARD_LIMIT_PCT;
    pub const DEFAULT_TOOL_OUTPUT_OFFLOAD_TOKENS: usize =
        limits::DEFAULT_TOOL_OUTPUT_OFFLOAD_TOKENS;

    pub const KEY_WARN_THRESHOLD_PCT: &'static str = "context.warnThresholdPct";
    pub const KEY_COMPACT_THRESHOLD_PCT: &'static str = "context.compactThresholdPct";
    pub const KEY_HARD_LIMIT_PCT: &'static str = "context.hardLimitPct";
    pub const KEY_TOOL_OUTPUT_OFFLOAD_TOKENS: &'static str = "context.toolOutputOffloadTokens";

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
        let warn_threshold_pct = parse_pct(
            Self::KEY_WARN_THRESHOLD_PCT,
            Self::DEFAULT_WARN_THRESHOLD_PCT,
        );
        let compact_threshold_pct_raw = parse_pct(
            Self::KEY_COMPACT_THRESHOLD_PCT,
            Self::DEFAULT_COMPACT_THRESHOLD_PCT,
        );
        let hard_limit_pct_raw = parse_pct(Self::KEY_HARD_LIMIT_PCT, Self::DEFAULT_HARD_LIMIT_PCT);
        let tool_output_offload_tokens = raw
            .get(Self::KEY_TOOL_OUTPUT_OFFLOAD_TOKENS)
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|v| *v > 0)
            .unwrap_or(Self::DEFAULT_TOOL_OUTPUT_OFFLOAD_TOKENS);

        // Invariant: warn <= compact <= hardLimit. Clamp up rather than
        // erroring if a hand-edited setting violates it.
        let compact_threshold_pct = compact_threshold_pct_raw.max(warn_threshold_pct);
        let hard_limit_pct = hard_limit_pct_raw.max(compact_threshold_pct);

        Self {
            warn_threshold_pct,
            compact_threshold_pct,
            hard_limit_pct,
            tool_output_offload_tokens,
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
                            Self::KEY_TOOL_OUTPUT_OFFLOAD_TOKENS,
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
    conn.call(move |conn: &mut Connection| {
        load_history_annotated(conn, &conversation_id, &skills_dir)
    })
    .await
    .map_err(|e| e.to_string())
}

async fn persist_notice(
    conn: &tokio_rusqlite::Connection,
    transcript_dir: Option<std::path::PathBuf>,
    conversation_id: &str,
    notice_json: String,
) -> Result<(), String> {
    let conversation_id = conversation_id.to_string();
    let now = crate::commands::models::now_ms();
    conn.call(move |conn: &mut Connection| {
        persist_context_notice(
            conn,
            transcript_dir.as_deref(),
            &conversation_id,
            now,
            &notice_json,
        )
    })
    .await
    .map_err(|e| e.to_string())
}

/// The server's last authoritative prompt-token count for a conversation and
/// the history length (count of OpenAI-shaped messages) it corresponded to.
/// Recorded by the live backends' `generate` (post `chat_result_to_turn_outcome`,
/// from the SSE trailer's `usage.prompt_tokens`) and consulted by their
/// `measure` and by the reopen/UI `usage_from_history`/`usage_from_fitted_messages`
/// paths (`authoritative_prompt_tokens` below) -- FR-2's "prefer the server's
/// truth as the base, chars/4 only for the delta since" policy. In-memory
/// (session-scoped): a restart resets to pure `chars/4` estimation, which is
/// safe (identical to today's behavior).
#[derive(Debug, Clone)]
pub struct ObservedUsage {
    pub prompt_tokens: u32,
    pub at_len: usize,
}

/// Per-conversation last observed usage. Mirrors `CompactionFailures`'s exact
/// shape: a bare `Mutex<HashMap<..>>` newtype, `.manage()`d in `lib.rs`, read
/// from Tauri commands via `State<'_, CompactionState>`'s `observed_usage`
/// field and from the live backends via a plain borrow.
pub struct LastObservedUsage(
    pub std::sync::Mutex<std::collections::HashMap<String, ObservedUsage>>,
);

impl Default for LastObservedUsage {
    fn default() -> Self {
        Self(std::sync::Mutex::new(std::collections::HashMap::new()))
    }
}

/// FR-2: prefers the server's authoritative `prompt_tokens` (`observed`) as
/// the base and adds only the estimated `chars/4` delta of messages appended
/// since that observation, rather than re-estimating the whole prompt from
/// scratch every time. Falls back to a full estimate when unobserved, OR when
/// the history has shrunk to at-or-below the observed length (a compaction/
/// reload invalidated the base -- `at_len > all_openai_msgs.len()`) -- never
/// underflows/panics on that slice.
pub fn authoritative_prompt_tokens(
    observed: Option<&ObservedUsage>,
    all_openai_msgs: &[serde_json::Value],
    estimate: impl Fn(&str) -> u32,
) -> u32 {
    let full =
        |slice: &[serde_json::Value]| estimate(&serde_json::to_string(slice).unwrap_or_default());
    match observed {
        Some(o) if o.at_len <= all_openai_msgs.len() => {
            o.prompt_tokens + full(&all_openai_msgs[o.at_len..])
        }
        _ => full(all_openai_msgs),
    }
}

/// Estimates the token usage of `history` (prefixed with `system_prompt`) —
/// prefers the server's last authoritative `prompt_tokens` (`observed`) as
/// the base (`authoritative_prompt_tokens`), falling back to a chars/4
/// heuristic (`inference::token_estimate`) over the OpenAI `messages` shape
/// the llama-server sidecar actually decodes (`to_openai_messages`), NOT the
/// old in-process dialect render. The server reports authoritative usage, so
/// this local number only has to be close enough to drive the compaction
/// TRIGGER (safe if it fires a bit early).
async fn usage_from_history(
    conversation_id: &str,
    history: &[HistoryMessage],
    system_prompt: &str,
    settings: &ContextSettings,
    observed: Option<&ObservedUsage>,
) -> Result<ContextUsage, String> {
    let mut messages = vec![ChatMessage::system(system_prompt)];
    messages.extend(history.iter().map(|m| m.chat.clone()));

    let openai_messages = crate::inference::http::to_openai_messages(&messages);
    let tokens_used = authoritative_prompt_tokens(observed, &openai_messages, token_estimate);
    let token_budget = crate::inference::CONTEXT_WINDOW_TOKENS;
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
    conversation_id: &str,
    skills_dir: &Path,
    system_prompt: &str,
    observed: Option<&ObservedUsage>,
) -> Result<ContextUsage, String> {
    let settings = ContextSettings::load(conn).await?;
    // Tier 1's clearing is already IN this history: `load_history_annotated`
    // replays whatever tier 1 persisted, so what loads here is exactly what
    // `commands::agent::send_agent_message` seeds the model with, placeholders
    // and all. This function used to re-run `apply_lightweight_clearing`
    // itself, on the reasoning that tier 1 was a pure load-time transform and
    // this was the only way the number could reflect it -- but no load path
    // applied it, so the clearing existed ONLY in the local copies this
    // function and `maybe_compact` threw away. That made this the more
    // dangerous half of the same bug: it reported the cleared estimate (~770
    // tokens on the real fixture) for a prompt that was really ~5.1k, i.e. it
    // under-reported by ~6.6x, in the direction that lets an over-budget
    // prompt through `send_agent_message`'s hard-limit check.
    let history = load_history_via_conn(conn, conversation_id, skills_dir).await?;
    usage_from_history(
        conversation_id,
        &history,
        system_prompt,
        &settings,
        observed,
    )
    .await
}

/// Tier 1: a pure, idempotent, load-time transform. Walking oldest-to-newest,
/// every `tool_call`/`tool_result` message beyond the most recent `keep_n`
/// such messages has its content replaced with a placeholder — the
/// restorable pointer text (`limits::tool_cleared_placeholder_with_pointer`)
/// when the row's `payload_ref` names a `Read`-able path, else the
/// plain `TOOL_CLEARED_PLACEHOLDER`. Plan-marked rows (`plan == true` — the
/// plan-machine tools, `commands::agent::persist_plan_tool`, which stamps
/// this marker onto BOTH its call and result row) are their own, stricter
/// population: cleared beyond the most recent `limits::PLAN_KEEP_N`
/// regardless of `keep_n`, since a plan row only ever echoes state the
/// always-regenerated system/state prompt already carries in full. Returns
/// the count actually cleared (both populations combined). Independent of
/// persistence — recomputable fresh from `content_type`+order (+ each row's
/// own `plan`/`payload_ref`, themselves parsed once at load time by
/// `storage::conversations::load_history_annotated`) alone every time,
/// which is why no "cut marker" is needed for this tier to stay correct
/// across reloads (research.md).
///
/// `transcript_path`, when `Some`, names this conversation's own
/// materialized transcript (`context::transcript::transcript_path`) — the
/// recovery route a cleared row with no `payload_ref` of its own now cites
/// (`limits::tool_cleared_placeholder_transcript`) instead of the bare
/// `TOOL_CLEARED_PLACEHOLDER`, since every row (staged or not) always has
/// an entry there. A row WITH a `payload_ref` still prefers that pointer
/// regardless — it's the more specific file (the exact staged content, not
/// the whole conversation).
///
/// Returns one [`ClearedRow`] per row it ACTUALLY changed — a row some
/// earlier pass already cleared (`limits::is_tool_cleared_placeholder`) is
/// counted by `keep_n`'s populations but never re-cleared and never
/// reported, which is what makes calling this on an
/// already-replayed history (every load applies the persisted clearing now
/// — see `storage::conversations::load_history_annotated`) a true no-op
/// instead of an endless "N old tool results cleared" notice every turn.
pub fn apply_lightweight_clearing(
    history: &mut [HistoryMessage],
    keep_n: usize,
    transcript_path: Option<&str>,
) -> Vec<ClearedRow> {
    let tool_rows: Vec<(usize, bool, Option<String>, i64)> = history
        .iter()
        .enumerate()
        .filter(|(_, m)| m.content_type == "tool_call" || m.content_type == "tool_result")
        .map(|(i, m)| (i, m.plan, m.payload_ref.clone(), m.sequence))
        .collect();

    let plan_indices: Vec<usize> = tool_rows
        .iter()
        .filter(|(_, plan, _, _)| *plan)
        .map(|(i, _, _, _)| *i)
        .collect();
    let plan_to_clear: &[usize] = if plan_indices.len() > limits::PLAN_KEEP_N {
        &plan_indices[..plan_indices.len() - limits::PLAN_KEEP_N]
    } else {
        &[]
    };

    let regular_indices: Vec<usize> = tool_rows
        .iter()
        .filter(|(_, plan, _, _)| !*plan)
        .map(|(i, _, _, _)| *i)
        .collect();
    let regular_to_clear: &[usize] = if regular_indices.len() > keep_n {
        &regular_indices[..regular_indices.len() - keep_n]
    } else {
        &[]
    };

    let mut cleared = Vec::new();
    for (i, _, payload_ref, sequence) in &tool_rows {
        if !(plan_to_clear.contains(i) || regular_to_clear.contains(i)) {
            continue;
        }
        // Already a placeholder (this conversation's persisted clearing,
        // replayed at load) -- leave it exactly as it is and say nothing.
        if limits::is_tool_cleared_placeholder(&history[*i].chat.text()) {
            continue;
        }
        let placeholder = match (payload_ref, transcript_path) {
            (Some(path), _) => limits::tool_cleared_placeholder_with_pointer(path),
            (None, Some(tp)) => limits::tool_cleared_placeholder_transcript(tp, *sequence),
            (None, None) => TOOL_CLEARED_PLACEHOLDER.to_string(),
        };
        history[*i].chat.content = MessageContent::Text(placeholder.clone());
        cleared.push(ClearedRow {
            sequence: *sequence,
            placeholder,
        });
    }
    cleared
}

/// One row tier 1 actually cleared: the row's `sequence` and the EXACT
/// placeholder text substituted for its content. Persisted verbatim into the
/// `cleared` context_notice (`cleared_notice_json`) and replayed verbatim on
/// every subsequent load (`storage::conversations::load_history_annotated`).
///
/// Why the substituted text is recorded rather than recomputed at load: it is
/// not a function of the row alone. `tool_cleared_placeholder_transcript`
/// embeds this app install's transcript directory — an ENVIRONMENT fact the
/// load path has no access to (`load_history_annotated` takes a `Connection`,
/// not an `AppHandle`), and the reason tier 1's clearing was recomputed into a
/// dropped local copy instead of being applied to what the model actually
/// receives. Recording it makes the notice a complete record of what tier 1
/// did, and guarantees the next turn's prompt carries byte-exactly the text
/// `maybe_compact` measured when it reported the usage drop.
#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearedRow {
    pub sequence: i64,
    pub placeholder: String,
}

/// The `cleared` context_notice's full JSON `content` (data-model.md), built
/// in one place so `maybe_compact` (which writes it) and
/// `storage::conversations::load_history_annotated` (which replays it) can
/// never drift, and so a test can build the row production builds without
/// hand-typing the shape.
pub fn cleared_notice_json(cleared: &[ClearedRow]) -> String {
    let plural = if cleared.len() == 1 { "" } else { "s" };
    let count = cleared.len();
    serde_json::json!({
        "kind": "cleared",
        "clearedCount": count,
        // The durable half: WHICH rows were cleared, and to what. Without
        // this the notice was pure narration -- it claimed a clearing no
        // load path ever applied, so the model kept receiving every byte
        // the notice said had been freed.
        "cleared": cleared,
        "notice": format!("{count} old tool result{plural} cleared to save space"),
    })
    .to_string()
}

/// The `summarized` context_notice's full JSON `content` (data-model.md) —
/// same single-source-of-truth reasoning as `cleared_notice_json`.
///
/// `through_sequence` is the sequence of the LAST message the summary
/// covers (`messages_to_summarize`'s span end), and is the splice point
/// `load_history_annotated` cuts at. It CANNOT be derived from the notice
/// row's own sequence: `storage::messages::insert` always allocates
/// `MAX(sequence) + 1`, so this row always lands last, and splicing at its
/// own sequence dropped every message in the conversation — including the
/// keep-first task statement and the protected recent turns, the exact two
/// things `messages_to_summarize` refuses to summarize and which therefore
/// have nothing standing in for them once dropped.
pub fn summarized_notice_json(summary: &str, through_sequence: i64) -> String {
    serde_json::json!({
        "kind": "summarized",
        "summary": summary,
        "throughSequence": through_sequence,
        "notice": "Conversation condensed to save space",
    })
    .to_string()
}

/// The `restoredFile` context_notice's full JSON `content` (FR-3) — same
/// single-source-of-truth reasoning as `cleared_notice_json`. Carries no
/// sequence of its own: `load_history_annotated` dates it by its ROW's
/// sequence, which is always the one right after the `summarized` notice it
/// belongs to.
pub fn restored_file_notice_json(path: &str, restored: &str) -> String {
    serde_json::json!({
        "kind": "restoredFile",
        "path": path,
        "restored": restored,
        "notice": "Restored the most-recent file after condensing",
    })
    .to_string()
}

/// True for a `HistoryMessage` that is a genuine user-authored turn (a
/// `text`/`rich_text` row with role `"user"`) — deliberately distinct from
/// a `tool_result` row, which also reconstructs with `chat.role == "user"`
/// (see `ChatMessage::tool_result`'s own doc comment) but is never "the
/// task statement".
///
/// Delegates to `storage::conversations::is_genuine_user_row` rather than
/// spelling the rule out again: `load_history_annotated` has to apply the
/// SAME rule to the raw `messages` row (to know which row survives a
/// summary's splice), and two copies of "what counts as the task
/// statement" that disagree would mean tier 2 summarizing away the one
/// message keep-first exists to protect.
fn is_genuine_user_message(message: &HistoryMessage) -> bool {
    crate::storage::conversations::is_genuine_user_row(&message.chat.role, &message.content_type)
}

/// The pure span-selection logic behind tier 2: everything in `history`
/// except the most recent `protected_recent` messages, *and* except the
/// first genuine user message (the task statement) — OpenHands' "keep-
/// first" behavior, applied here because a summarization pass is
/// generative, lossy compression by a small model, the riskiest single
/// step in the whole compaction pipeline; it must never be the thing that
/// makes the model forget what it was asked to do. Split out from
/// `summarize_and_persist` (which needs a real model server — this
/// file's own testability note at the top) purely so this part is
/// unit-testable on its own, the same split `fit_turn_to_budget`/
/// `fit_to_budget` already established.
fn messages_to_summarize(
    history: &[HistoryMessage],
    protected_recent: usize,
) -> Vec<&HistoryMessage> {
    if history.len() <= protected_recent {
        return Vec::new();
    }
    let recent_cutoff = history.len() - protected_recent;
    let first_user_index = history.iter().position(is_genuine_user_message);

    history[..recent_cutoff]
        .iter()
        .enumerate()
        .filter(|(i, _)| Some(*i) != first_user_index)
        .map(|(_, m)| m)
        .collect()
}

/// Why a candidate summary was rejected (or accepted). The rejection cases
/// mirror qwen-code's COMPRESSION_FAILED_* guards.
#[derive(Debug, PartialEq)]
pub enum SummaryDecision {
    Accept,
    RejectEmpty,     // empty after trimming
    RejectTruncated, // the summarization completion hit its max_tokens
    RejectInflated,  // the summary is not smaller than what it replaces
}

/// Decides whether a candidate summary is safe to apply. `finish_reason` is
/// the server's stop reason for the summarization call; `pre_tokens` is the
/// estimated size of the history span being summarized; `post_tokens` the
/// estimated size of the summary that would replace it. Checked in this
/// order -- empty beats truncated beats inflated -- so a pathological
/// candidate that is simultaneously empty-after-trim AND reports
/// `finish_reason: "length"` (e.g. the model emitted nothing but pure
/// whitespace/stop tokens before hitting its cap) is reported as the more
/// fundamental failure (`RejectEmpty`) rather than the truncation guard.
pub fn evaluate_summary(
    summary: &str,
    finish_reason: Option<&str>,
    pre_tokens: u32,
    post_tokens: u32,
) -> SummaryDecision {
    if summary.trim().is_empty() {
        return SummaryDecision::RejectEmpty;
    }
    if finish_reason == Some("length") {
        return SummaryDecision::RejectTruncated;
    }
    if post_tokens >= pre_tokens {
        return SummaryDecision::RejectInflated;
    }
    SummaryDecision::Accept
}

/// The outcome of a `summarize_and_persist` attempt -- distinguishes "there
/// was nothing eligible to summarize" (`messages_to_summarize` came back
/// empty, a true no-op) from "the model's candidate summary was rejected"
/// (`evaluate_summary`). Both leave history completely untouched, but only
/// a rejection should count against `CompactionFailures` -- `maybe_compact`
/// is the sole caller that needs to tell the two apart.
pub enum SummaryResult {
    Persisted(String),
    NothingToSummarize,
    Rejected(SummaryDecision),
}

/// Consecutive failed auto-compaction attempts per conversation. Guards
/// against burning llama-server round-trips on a model that can't produce a
/// usable summary: after `limits::MAX_CONSECUTIVE_COMPACTION_FAILURES`,
/// auto-compaction NOOPs (`breaker_open`) until a FORCED compaction
/// succeeds and resets the count. In-memory (session-scoped) -- a restart
/// resets it, which is fine. Mirrors `commands::conversations::ActiveGenerations`'s
/// shape exactly: a bare `Mutex<HashMap<..>>` newtype, `.manage()`d in
/// `lib.rs`, read from Tauri commands via `State<'_, CompactionState>`'s
/// `failures` field.
pub struct CompactionFailures(pub std::sync::Mutex<std::collections::HashMap<String, u32>>);

impl Default for CompactionFailures {
    fn default() -> Self {
        Self(std::sync::Mutex::new(std::collections::HashMap::new()))
    }
}

/// Bundles `CompactionFailures` and `LastObservedUsage` into a single
/// `.manage()`d value. `send_agent_message` was already at ten total params
/// (State included) -- specta's `SpectaFn` arity ceiling -- before FR-2;
/// adding `LastObservedUsage` as its own eleventh param blew past it
/// (`cargo build` fails: "the trait bound ... SpectaFn<_> is not
/// satisfied"). Bundling these two small, closely-related, session-scoped
/// in-memory maps behind one `State` buys back a slot. `compact_conversation`/
/// `get_context_usage` (comfortably under the ceiling either way) take the
/// SAME bundle -- not separate `CompactionFailures`/`LastObservedUsage`
/// states of their own -- so every reader/writer across the app shares the
/// exact same underlying `Mutex`es; two independently-managed instances of
/// either map would silently drift out of sync (e.g. a turn's observed
/// `prompt_tokens` recorded by `send_agent_message`'s `RealBackend` would
/// never be visible to `get_context_usage`'s reopen snapshot).
#[derive(Default)]
pub struct CompactionState {
    pub failures: CompactionFailures,
    pub observed_usage: LastObservedUsage,
}

/// Pure breaker decision (unit-tested): true when auto-compaction
/// (`force == false`) should skip the summarize call entirely because this
/// conversation has already hit `limits::MAX_CONSECUTIVE_COMPACTION_FAILURES`
/// consecutive rejections. A forced run (manual "Compact now") always
/// ignores the breaker -- that's the recovery path a successful forced
/// compaction resets the counter through.
pub fn breaker_open(failures: u32, force: bool) -> bool {
    !force && failures >= limits::MAX_CONSECUTIVE_COMPACTION_FAILURES
}

/// The source path of the most-recently-`Read` file in a summarized span —
/// the last `tool_result` row whose `tool_name == "Read"`, returning its
/// `payload_ref` (which, for a `Read` row, IS the source file path the SP1
/// carve-out stored — see `HistoryMessage::payload_ref`'s own doc comment).
/// `None` when the span contains no `Read`. Used by `summarize_and_persist`
/// to restore that file's CURRENT contents right after a compaction (FR-3),
/// so the agent doesn't lose the file it was working on to a lossy summary.
///
/// Takes `&[&HistoryMessage]` rather than `&[HistoryMessage]`: its one real
/// caller already holds `messages_to_summarize`'s `Vec<&HistoryMessage>`
/// (the summarized span, borrowed out of the full history) -- this avoids
/// cloning that span just to call in.
pub fn most_recent_read_path(summarized: &[&HistoryMessage]) -> Option<String> {
    summarized
        .iter()
        .rev()
        .find(|m| m.content_type == "tool_result" && m.tool_name.as_deref() == Some("Read"))
        .and_then(|m| m.payload_ref.clone())
}

/// Builds the post-summary restored-file note body: the file's CURRENT
/// content (re-read fresh from disk by the caller), inlined whole when it
/// fits `cap_tokens`, else a head+tail window plus a truncation note.
/// NEVER a "Read ... to view" reference line -- the whole point of FR-3 is
/// to carry the file's REAL content across the compaction ("not reference
/// files" -- user directive), the same way an ordinary inlined tool result
/// already does for a fresh `Read`.
///
/// `estimate` is the same `chars/4` heuristic (`inference::token_estimate`)
/// used everywhere else in this module, so `cap_tokens` (typically
/// `DEFAULT_TOOL_OUTPUT_OFFLOAD_TOKENS`) means the same thing here it does
/// for an ordinary tool result. Char-boundary-safe: only ever indexes via
/// `.chars()`, never a byte slice, so this never panics on a multi-byte
/// UTF-8 file regardless of where the cap falls.
pub fn bounded_restore_body(
    path: &str,
    content: &str,
    cap_tokens: usize,
    estimate: impl Fn(&str) -> u32,
) -> String {
    let header = format!("Current contents of `{path}`:\n");
    if (estimate(content) as usize) <= cap_tokens {
        return format!("{header}{content}");
    }
    // Head+tail window: split the cap between the start and end of the file
    // so both the imports/signature region and the end are preserved. Size
    // each half by chars against a chars/4-consistent budget; join with a
    // note naming how much was dropped.
    let budget_chars = cap_tokens.saturating_mul(4);
    let half = budget_chars / 2;
    let head: String = content.chars().take(half).collect();
    let tail: String = content
        .chars()
        .rev()
        .take(half)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let dropped = content
        .chars()
        .count()
        .saturating_sub(head.chars().count() + tail.chars().count());
    format!("{header}{head}\n… [{dropped} chars truncated] …\n{tail}")
}

/// Tier 2: summarizes everything except the most recent `protected_recent`
/// messages and the first user message (`messages_to_summarize`) by
/// generating through the supervised `llama-server` (the HTTP client at
/// `base_url`, in `Forbid` mode — no tools, no `tool_choice` — exactly how
/// production's `RealBackend` calls it, minus tools). A candidate summary is
/// screened by `evaluate_summary` before anything is persisted -- guards
/// against a small local model returning an empty, truncated, or bloated
/// summary that would otherwise silently corrupt or grow the context. On
/// ANY rejection, history is left completely untouched: no notice is
/// persisted, nothing else changes. Only on acceptance is the result
/// persisted as a `context_notice` row (`kind:"summarized"`) that
/// `load_history_annotated` will splice in on every subsequent load.
/// Returns `SummaryResult::NothingToSummarize` (no-op) when there's nothing
/// eligible to summarize.
pub async fn summarize_and_persist(
    conn: &tokio_rusqlite::Connection,
    transcript_dir: Option<std::path::PathBuf>,
    base_url: &str,
    conversation_id: &str,
    history: &[HistoryMessage],
    protected_recent: usize,
) -> Result<SummaryResult, String> {
    let to_summarize = messages_to_summarize(history, protected_recent);
    if to_summarize.is_empty() {
        return Ok(SummaryResult::NothingToSummarize);
    }

    let mut messages = vec![ChatMessage::system(SUMMARIZATION_PROMPT)];
    messages.extend(to_summarize.iter().map(|m| m.chat.clone()));
    // The request must NEVER end on an assistant message: `to_summarize` is an
    // arbitrary slice of history and routinely ends with one, which the chat
    // template treats as a prefill to CONTINUE rather than context to act on --
    // the model then closes out that sentence and `evaluate_summary` happily
    // accepts the echo as a summary. Ending on a user turn is what makes the
    // system prompt the thing being answered. See `SUMMARIZATION_FINAL_TURN`.
    messages.push(ChatMessage::user(limits::SUMMARIZATION_FINAL_TURN));

    // `Forbid`: tools and tool_choice both `None` (a summary must never be
    // able to emit a tool call). Compaction is best-effort, so a fresh,
    // never-cancelled token — there is no per-turn cancel handle to thread
    // here the way a live agent turn has.
    let mut req = crate::inference::http::ChatRequest::build(
        "doce",
        crate::inference::http::to_openai_messages(&messages),
        None,
        None,
    );
    // Flat cap, NOT `clamp_output_tokens` -- this is a `Forbid`-mode call
    // over an already-bounded prompt, not an agent turn sized against the
    // live window (restore-output-cap task; see `SUMMARY_MAX_TOKENS`'s doc
    // comment).
    req.max_tokens = Some(SUMMARY_MAX_TOKENS as u32);
    // Reasoning is pure overhead on a reformatting job like this one, and
    // spends the output budget before any content is emitted (see
    // `disable_thinking`).
    req.disable_thinking();
    let cancel = tokio_util::sync::CancellationToken::new();
    let outcome = crate::inference::http::LlamaServerClient::new(base_url)
        .chat(req, |_piece| {}, &cancel)
        .await
        .map_err(|e| e.to_string())?;

    let summary = outcome.text.trim().to_string();
    // The same chars/4-over-the-request-shape estimate everything else in
    // this module uses (`usage_from_history`/`usage_from_fitted_messages`),
    // applied to just the span being replaced -- `evaluate_summary`'s
    // `pre_tokens`/`post_tokens` are only ever compared to each other, so
    // both sides using the same heuristic is what makes the comparison
    // meaningful.
    let pre_tokens = token_estimate(
        &serde_json::to_string(&crate::inference::http::to_openai_messages(
            &to_summarize
                .iter()
                .map(|m| m.chat.clone())
                .collect::<Vec<_>>(),
        ))
        .unwrap_or_default(),
    );
    let post_tokens = token_estimate(&summary);
    // `ChatOutcome::finish_reason` is a plain `String` ("" sentinel for "the
    // server never sent one" — see its own doc comment), not `Option<...>`;
    // normalize the empty-string sentinel to `None` here so
    // `evaluate_summary`'s `Some("length")` comparison only ever means a
    // real, observed truncation.
    let finish_reason = if outcome.finish_reason.is_empty() {
        None
    } else {
        Some(outcome.finish_reason.as_str())
    };

    match evaluate_summary(&summary, finish_reason, pre_tokens, post_tokens) {
        SummaryDecision::Accept => {
            // The span's END is what the summary stands in for, so that is
            // what the notice records and what `load_history_annotated`
            // splices at -- NOT this notice row's own sequence, which
            // `messages::insert` always allocates past the end of the
            // conversation (see `summarized_notice_json`). `to_summarize` is
            // non-empty (checked at the top), so the `unwrap_or` is
            // unreachable; -1 would splice nothing, the safe direction.
            let through_sequence = to_summarize.last().map(|m| m.sequence).unwrap_or(-1);
            let notice_json = summarized_notice_json(&summary, through_sequence);
            // Cloned (not moved) -- the restored-file notice below still
            // needs `transcript_dir` for its own `persist_notice` call.
            persist_notice(conn, transcript_dir.clone(), conversation_id, notice_json).await?;

            // FR-3 (restore-recent-file): a summary names files but drops
            // their contents, so the agent often loses the file it was
            // working on. Re-read the single most-recently-`Read` file in
            // the summarized span FRESH from disk (current contents, not
            // the stale pre-summary snapshot) and persist its ACTUAL
            // content -- never a path/reference line, "not reference
            // files" -- as a second notice right after the summary.
            // Missing/unreadable file (deleted, renamed, permission error
            // since the Read happened) is a silent no-op: the summary
            // still names it, and there is nothing else useful to
            // restore. Exactly one restored-file note per compaction.
            if let Some(path) = most_recent_read_path(&to_summarize) {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    let restored = bounded_restore_body(
                        &path,
                        &content,
                        limits::DEFAULT_TOOL_OUTPUT_OFFLOAD_TOKENS,
                        token_estimate,
                    );
                    let restore_notice_json = restored_file_notice_json(&path, &restored);
                    persist_notice(conn, transcript_dir, conversation_id, restore_notice_json)
                        .await?;
                }
            }

            // SP4: out-of-band memory extraction. Deliberately ignores its
            // result -- compaction's success does not depend on it.
            let _ = extract_and_persist_memories(
                conn,
                base_url,
                conversation_id,
                &to_summarize,
                crate::commands::models::now_ms(),
            )
            .await;

            Ok(SummaryResult::Persisted(summary))
        }
        rejected => Ok(SummaryResult::Rejected(rejected)),
    }
}

/// SP4: the out-of-band memory-extraction pass. Reviews the span being
/// condensed plus the workspace's existing memories and swaps in the model's
/// full replacement set.
///
/// Best-effort by construction: every failure path -- server error, a failed
/// read of the existing set, a truncated completion, a parse failure, a
/// concurrent writer, or empty/degenerate output -- logs and returns `Ok(())`
/// leaving memories exactly as they were. Compaction must never fail an agent
/// turn, and a bad extraction must never destroy good memories (the unsafe
/// direction), so the truncation backstop, the shape check, and the
/// empty-output guard all mirror `evaluate_summary`'s posture; a failed read of
/// the existing set is treated as "bail out", never as "existing == empty"
/// (that would let a set built in ignorance of the real memories wipe them);
/// and the write is a compare-and-swap against the set actually read, so a
/// sibling conversation that compacted during this call's LLM round-trip is
/// never clobbered.
///
/// The transport and the DB live here; every DECISION about what the model
/// actually returned lives in [`parse_extraction_output`], which is pure and
/// directly unit-tested. This function is the glue: resolve the workspace,
/// read the prior set, call the model, parse, and either log a refusal or
/// compare-and-swap the new set in.
///
/// `pub` (not `pub(crate)`) so `tests/real_model_smoke.rs` can drive this
/// exact path against a REAL llama-server -- the only thing that can answer
/// whether the model actually obeys `MEMORY_EXTRACTION_PROMPT`'s contract, as
/// opposed to whether we parse a response that already assumes it does.
pub async fn extract_and_persist_memories(
    conn: &tokio_rusqlite::Connection,
    base_url: &str,
    conversation_id: &str,
    to_summarize: &[&HistoryMessage],
    now: i64,
) -> Result<(), String> {
    // LATENT HAZARD -- subagent conversations and the NULL bucket. A subagent
    // is created with `workspace_id` NULL (`agent::subagent::spawn`), so this
    // resolution yields `None` and every write below lands in the SHARED,
    // GLOBAL NULL bucket -- the same bucket every other workspace-less
    // conversation writes to and recalls from. That is harmless only because
    // the subagent path never reaches `maybe_compact` today, so it never gets
    // here. The recall side is already explicitly guarded (agent.rs passes
    // `plan_system_message(..., None)` for subagents, with a comment); the
    // write side is guarded only by that reachability accident. If subagents
    // ever gain compaction, decide FIRST what an isolated subagent's memories
    // mean -- most likely: skip extraction entirely when `workspace_id` is
    // `None` and the conversation has a `spawned_by_conversation_id`, or
    // inherit the parent's workspace -- because an isolated subagent silently
    // cross-contaminating the global NULL bucket is the failure mode here.
    let workspace_id = match crate::storage::memories::workspace_id_for_conversation(
        conn,
        conversation_id,
    )
    .await
    {
        Ok(w) => w,
        Err(e) => {
            eprintln!("[memory-extraction] workspace lookup failed: {e}");
            return Ok(());
        }
    };
    // A transient read failure here must NOT be treated as "no existing
    // memories" (that was the original, wrong, `.unwrap_or_default()`
    // mandate): the multi-second LLM round-trip below sits between this read
    // and the eventual `replace_memories` write, so a real DB error here
    // would otherwise have the model build a replacement set in ignorance of
    // the true prior set, which `replace_memories` would then use to WIPE
    // it. Never extract from ignorance of the prior set -- bail out instead.
    let existing =
        match crate::storage::memories::load_memories(conn, workspace_id.as_deref()).await {
            Ok(m) => m,
            Err(e) => {
                eprintln!("[memory-extraction] loading existing memories failed: {e}");
                return Ok(());
            }
        };

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
    messages.push(ChatMessage::user(format!(
        "Existing memories:\n{existing_block}"
    )));
    messages.extend(to_summarize.iter().map(|m| m.chat.clone()));
    // Never end on an assistant message -- the trailing-assistant prefill that
    // had this call echoing the span's last message back as a durable "memory".
    // See `EXTRACTION_FINAL_TURN`, and `summarize_and_persist`'s twin of this.
    messages.push(ChatMessage::user(limits::EXTRACTION_FINAL_TURN));

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
    // The reasoning block was measured consuming this call's ENTIRE 1024-token
    // budget (empty content, `finish_reason:"length"`, nothing persisted). See
    // `disable_thinking`.
    req.disable_thinking();
    let cancel = tokio_util::sync::CancellationToken::new();
    let outcome = match crate::inference::http::LlamaServerClient::new(base_url)
        .chat(req, |_piece| {}, &cancel)
        .await
    {
        Ok(o) => o,
        Err(e) => {
            eprintln!("[memory-extraction] inference failed: {e}");
            return Ok(());
        }
    };

    // `ChatOutcome::finish_reason` is a plain `String` ("" sentinel for "the
    // server never sent one" -- see its own doc comment), not `Option<...>`;
    // normalize the empty-string sentinel to `None` here exactly as
    // `summarize_and_persist` does before comparing to `Some("length")`.
    // (`parse_extraction_output` is total over `Some("")` too, but the
    // normalization stays here so the two callers read identically.)
    let finish_reason = if outcome.finish_reason.is_empty() {
        None
    } else {
        Some(outcome.finish_reason.as_str())
    };

    // Every decision about what came back is made here, purely.
    let facts = match parse_extraction_output(&outcome.text, finish_reason) {
        ExtractionOutcome::Facts(facts) => facts,
        ExtractionOutcome::Rejected(reason) => {
            match reason {
                ExtractionRejection::Truncated => {
                    eprintln!("[memory-extraction] truncated output, keeping existing memories");
                }
                ExtractionRejection::MajorityUnshaped { unshaped, total } => {
                    eprintln!(
                        "[memory-extraction] {unshaped}/{total} lines are not fact-shaped; \
                         treating as a parse failure and keeping existing memories"
                    );
                }
                // Only worth a line when there was something to lose --
                // "the model had nothing to add and there was nothing there"
                // is the normal, uninteresting case.
                ExtractionRejection::Empty => {
                    if !existing.is_empty() {
                        eprintln!("[memory-extraction] empty output, keeping existing memories");
                    }
                }
            }
            return Ok(());
        }
    };

    // COMPARE-AND-SWAP, not a blind write: `existing` was read before a
    // multi-second LLM round-trip, and a sibling conversation in this same
    // workspace can compact and commit its own full replacement set during it
    // (manual "Compact now" and `maybe_compact` share no lock). `facts` was
    // authored in ignorance of any such set, so writing it unconditionally
    // would destroy the sibling's facts permanently -- its span is already
    // condensed and will never be re-extracted. See
    // `storage::memories::replace_memories_if_unchanged`.
    let expected: Vec<String> = existing.iter().map(|m| m.content.clone()).collect();
    match crate::storage::memories::replace_memories_if_unchanged(
        conn,
        workspace_id.as_deref(),
        &expected,
        &facts,
        now,
    )
    .await
    {
        Ok(true) => {}
        Ok(false) => {
            eprintln!("[memory-extraction] memory set changed under us; skipping write");
        }
        Err(e) => {
            eprintln!("[memory-extraction] persist failed: {e}");
        }
    }
    Ok(())
}

/// The facts an extraction earned the right to persist, or the reason the
/// whole pass is refused. Refusal is always "leave the existing set exactly as
/// it is" -- never a partial write.
#[derive(Debug, PartialEq, Eq)]
pub enum ExtractionOutcome {
    Facts(Vec<String>),
    Rejected(ExtractionRejection),
}

/// Why an extraction was refused. Carried out of [`parse_extraction_output`]
/// rather than logged inside it, so the decision stays pure and the caller
/// owns the (context-dependent) logging.
#[derive(Debug, PartialEq, Eq)]
pub enum ExtractionRejection {
    /// The completion hit the output cap (`finish_reason:"length"`).
    Truncated,
    /// Nothing fact-shaped survived -- including the model correctly emitting
    /// nothing at all.
    Empty,
    /// Most of what came back wasn't fact-shaped, so the model didn't do the
    /// task. Counts are for the log line only.
    MajorityUnshaped { unshaped: usize, total: usize },
}

/// THE complete extraction-output decision: the model's raw text plus its
/// `finish_reason` in, either the facts to persist or the reason we refuse to
/// out. Pure, synchronous, and total -- every guard protecting the memory set
/// lives here, so each is directly unit-testable without an HTTP round-trip.
/// [`extract_and_persist_memories`] is the only caller and does nothing with
/// this beyond logging a rejection or CAS-writing the facts.
///
/// `finish_reason` is `Option` because the server may not send one; both
/// `None` and the `Some("")` sentinel `ChatOutcome` uses mean "never sent" and
/// must NOT read as truncation. Only an observed `Some("length")` does.
pub fn parse_extraction_output(text: &str, finish_reason: Option<&str>) -> ExtractionOutcome {
    // TRUNCATION BACKSTOP, first: a truncated response is not evidence about
    // anything downstream. The prompt asks for the FULL replacement set every
    // time, so the expected output grows as memories accumulate -- a truncated
    // response would silently drop every memory not yet re-emitted AND persist
    // a half-finished sentence as a durable fact. `MEMORY_EXTRACTION_PROMPT`'s
    // own self-cap is meant to keep this call clear of `SUMMARY_MAX_TOKENS`,
    // but this backstop is what actually refuses to persist if it ever doesn't
    // (mirrors `evaluate_summary`'s `RejectTruncated`).
    if finish_reason == Some("length") {
        return ExtractionOutcome::Rejected(ExtractionRejection::Truncated);
    }

    // Every non-empty line the model emitted, bullet prefix stripped. This is
    // the denominator of the majority test below, so it is counted BEFORE
    // dedup and before the shape check.
    let candidates: Vec<String> = text
        .lines()
        .map(|l| l.trim().trim_start_matches("- ").trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    // Shape check, then dedup, first-seen order preserved: `replace_memories`
    // inserts `contents` verbatim (no dedup of its own), so a model that
    // repeats a line would otherwise create duplicate rows.
    let mut seen = std::collections::HashSet::new();
    let facts: Vec<String> = candidates
        .iter()
        .filter(|l| is_plausible_fact(l))
        .filter(|l| seen.insert((*l).clone()))
        .cloned()
        .collect();

    // PARSE FAILURE => NO CHANGE (spec §4 step 4). If MOST of what came back
    // isn't fact-shaped, the model didn't do the task -- and the task is to
    // emit a faithful FULL replacement set. Persisting the surviving minority
    // would swap the workspace's whole set for whatever fragments happened to
    // pass a syntactic filter, silently dropping every real memory that the
    // confused model failed to re-emit. A model that mostly emitted garbage has
    // not earned that trust, so the whole pass is discarded. `candidates` is
    // never empty here in a way that matters: 0 rejected of 0 is not a
    // majority, and the empty case falls through to the empty-output guard
    // below, which is the older and narrower of the two.
    let unshaped = candidates.iter().filter(|l| !is_plausible_fact(l)).count();
    if unshaped * 2 > candidates.len() {
        return ExtractionOutcome::Rejected(ExtractionRejection::MajorityUnshaped {
            unshaped,
            total: candidates.len(),
        });
    }

    // THE GUARD: a degenerate extraction must not wipe good memories.
    if facts.is_empty() {
        return ExtractionOutcome::Rejected(ExtractionRejection::Empty);
    }

    ExtractionOutcome::Facts(facts)
}

/// Is this line plausibly a durable fact, rather than the model failing to
/// follow `MEMORY_EXTRACTION_PROMPT`'s "no commentary, no headers" contract?
///
/// `pub` so `tests/real_model_smoke.rs` can assert the REAL model's output
/// against the very check that was written blind against it.
///
/// Every line that passes becomes a DURABLE row, and a bad row is sticky and
/// self-reinforcing: the next pass feeds the existing set back in under "keep
/// the existing ones that are still true", so the model keeps it, and it rides
/// in `messages[0]` of every turn in the workspace forever. There is no UI and
/// no clear command -- the only recovery is manual sqlite surgery. So the bias
/// is: when in doubt, DROP. A dropped real fact costs one fact until the next
/// compaction re-learns it; a persisted preamble costs forever.
///
/// The rules are deliberately syntactic and few -- this cannot judge whether a
/// sentence is TRUE, only whether it is shaped like a fact at all:
pub fn is_plausible_fact(line: &str) -> bool {
    // 1. Trailing ':' -- a preamble or header, never a self-contained fact.
    // This is the observed 4B failure: "Here is the updated set of
    // memories:" as line 1, which then becomes permanent. A real fact is a
    // sentence; sentences don't end in a colon.
    if line.ends_with(':') {
        return false;
    }
    // 2. Length bounds, in CHARS (not bytes -- a CJK fact is short in chars
    // and long in bytes, and truncating on bytes could also split a
    // codepoint). Below the floor there is no room for a self-contained
    // sentence, so it's a fragment, a list marker, or a stray word ("Yes",
    // "Memories", "1."). Above the ceiling it's prose, a pasted code block,
    // or a run-on -- the prompt asks for <=20 words, so ~300 chars is
    // already far past disobedient. Both bounds are loose on purpose: they
    // are backstops against garbage, not enforcement of the prompt.
    let len = line.chars().count();
    if !(limits::MEMORY_FACT_MIN_CHARS..=limits::MEMORY_FACT_MAX_CHARS).contains(&len) {
        return false;
    }
    true
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
///
/// `failures` is the caller's `CompactionFailures` circuit breaker (one
/// counter per conversation). If tier 2 is reached and `breaker_open`
/// returns true (auto-compaction, `force == false`, already at
/// `limits::MAX_CONSECUTIVE_COMPACTION_FAILURES` consecutive rejections),
/// the summarize call is skipped entirely and `state` is set to the warn
/// state `"compactionStalled"` — tier 1 still ran above, so a partial
/// clearing is not lost. A `Rejected` summary increments the counter
/// (history is left untouched either way — `summarize_and_persist`'s own
/// contract); a `Persisted` summary (whether auto or forced) resets it to 0.
#[allow(clippy::too_many_arguments)]
pub async fn maybe_compact(
    conn: &tokio_rusqlite::Connection,
    transcript_dir: Option<std::path::PathBuf>,
    base_url: &str,
    conversation_id: &str,
    skills_dir: &Path,
    system_prompt: &str,
    force: bool,
    failures: &CompactionFailures,
    observed_usage: &LastObservedUsage,
) -> Result<ContextUsage, String> {
    let settings = ContextSettings::load(conn).await?;
    let mut history = load_history_via_conn(conn, conversation_id, skills_dir).await?;
    let observed = observed_usage
        .0
        .lock()
        .unwrap()
        .get(conversation_id)
        .cloned();
    let mut usage = usage_from_history(
        conversation_id,
        &history,
        system_prompt,
        &settings,
        observed.as_ref(),
    )
    .await?;

    let over_compact_threshold = |u: &ContextUsage| {
        exceeds(
            u.tokens_used,
            u.token_budget,
            settings.compact_threshold_pct,
        )
    };

    if !force && !over_compact_threshold(&usage) {
        return Ok(usage);
    }

    let mut changed = false;

    // Unlike `compute_usage`, this caller has `transcript_dir` in hand, so
    // a cleared row with no `payload_ref` of its own gets the more useful
    // restorable placeholder (its own transcript entry) instead of the
    // plain "gone for good" one.
    let transcript_path = transcript_dir.as_ref().map(|dir| {
        transcript::transcript_path(dir, conversation_id)
            .display()
            .to_string()
    });
    let cleared = apply_lightweight_clearing(&mut history, TOOL_KEEP_N, transcript_path.as_deref());
    if !cleared.is_empty() {
        changed = true;
        // The notice carries the clearing itself (which rows, replaced by
        // what text), not just a sentence about it -- `load_history_annotated`
        // replays it on every subsequent load, which is what makes the
        // mutation above reach the model at all. Without the replay this
        // whole arm mutated a local `history` that was dropped on return:
        // the notice, and the usage drop recomputed from it, were both
        // fiction.
        persist_notice(
            conn,
            transcript_dir.clone(),
            conversation_id,
            cleared_notice_json(&cleared),
        )
        .await?;
        // Same invalidation, and for the same reason, as the tier-2 arm
        // below: `authoritative_prompt_tokens` uses the server's last
        // observed `prompt_tokens` as a base and only estimates messages
        // APPENDED since. A clearing changes message CONTENT without
        // changing the message COUNT, so every cleared row sits inside that
        // observed prefix -- the base would still price the full, uncleared
        // tool results and this recompute would return a byte-identical
        // number, i.e. tier 1 could never lower usage on any turn after the
        // first. Harmless only while the clearing never reached the prompt;
        // now that it does, the observation genuinely describes a prompt
        // that no longer exists.
        observed_usage.0.lock().unwrap().remove(conversation_id);
        usage =
            usage_from_history(conversation_id, &history, system_prompt, &settings, None).await?;
    }

    if over_compact_threshold(&usage) {
        let n = failures
            .0
            .lock()
            .unwrap()
            .get(conversation_id)
            .copied()
            .unwrap_or(0);
        if breaker_open(n, force) {
            // Auto-compaction only -- a forced run always ignores the
            // breaker (see this function's own doc comment). Tier 1 already
            // ran above, so any partial clearing it did is kept; only the
            // llama-server round-trip is skipped.
            usage.state = "compactionStalled".to_string();
            return Ok(usage);
        }

        match summarize_and_persist(
            conn,
            transcript_dir.clone(),
            base_url,
            conversation_id,
            &history,
            PROTECTED_RECENT_MESSAGES,
        )
        .await?
        {
            SummaryResult::Persisted(_) => {
                changed = true;
                failures.0.lock().unwrap().remove(conversation_id);
                // A summary replaces history wholesale, so any prior
                // authoritative-usage observation for this conversation is
                // stale -- force a fresh full estimate until the next
                // `generate` re-observes (FR-2). `usage_from_history` below
                // would already fall back safely (`authoritative_prompt_tokens`'s
                // stale/shrunk guard), but this makes the invalidation
                // explicit and covers callers after this function returns too.
                observed_usage.0.lock().unwrap().remove(conversation_id);
                // summarize_and_persist just persisted a new context_notice row
                // that changes load_history_annotated's splice point -- reload
                // rather than trying to reconstruct the spliced view in memory.
                history = load_history_via_conn(conn, conversation_id, skills_dir).await?;
                usage =
                    usage_from_history(conversation_id, &history, system_prompt, &settings, None)
                        .await?;
            }
            SummaryResult::NothingToSummarize => {}
            SummaryResult::Rejected(_) => {
                // History is untouched (summarize_and_persist's own
                // contract on any rejection) -- only the failure count
                // moves, so a persistently-bad model stops burning
                // round-trips once the breaker opens.
                *failures
                    .0
                    .lock()
                    .unwrap()
                    .entry(conversation_id.to_string())
                    .or_insert(0) += 1;
            }
        }
    }

    if changed {
        usage.state = "justCompacted".to_string();
    }

    Ok(usage)
}

/// The mid-agent-loop counterpart to `maybe_compact`'s *replacement*:
/// called before *every* turn inside `agent::run_loop`, not just once
/// before the loop starts. Without this, a single agent turn that makes
/// several tool calls can blow past the model's context window with
/// nothing to stop it until the *next* top-level message — which is
/// exactly what let a raw `NoKvCacheSlot` llama.cpp decode error reach the
/// user instead of a graceful fit (found via real use, not speculatively).
///
/// Superseded the old tier-1/tier-2 `compact_in_memory` (placeholder-clear
/// then maybe-summarize, both making a judgment about what's worth
/// keeping): this is purely mechanical — `fit_to_budget`'s own doc comment
/// explains why judgment belongs to the outer loop's own goal/plan/
/// observations state instead. No persisted notice, no summarization
/// `generate()` call, no DB dependency at all — engine + messages in,
/// fitted messages out — which is also what makes this callable directly
/// from the real-model agent task tests (`tests/agent_tasks.rs`)
/// instead of that suite reimplementing its own version of this step.
pub fn fit_turn_to_budget(messages: &[ChatMessage]) -> Result<Vec<ChatMessage>, String> {
    // The reserve covers BOTH per-turn costs `messages` doesn't contain
    // yet: the output tokens generation may still produce, and the plan
    // hosts' state-tail message (`PlanState::state_tail`), which every
    // plan backend pushes AFTER this fit has already run -- see
    // `limits::STATE_TAIL_RESERVE_TOKENS` for the overflow this closes.
    // Pure, estimate-only (`token_estimate` per message, no render, no
    // recount loop): the llama-server owns the exact count now, and the B1a
    // output clamp guarantees request validity regardless of this estimate's
    // error, so `fit_to_budget`'s single greedy pass replaces the old
    // render-then-recount trim.
    let budget = crate::inference::CONTEXT_WINDOW_TOKENS
        .saturating_sub(limits::AGENT_TURN_MAX_OUTPUT_TOKENS + limits::STATE_TAIL_RESERVE_TOKENS);
    let costs: Vec<u32> = messages.iter().map(|m| token_estimate(&m.text())).collect();
    Ok(fit_to_budget(messages, &costs, budget, 1))
}

/// `ContextUsage` for an already-fully-assembled message list (system
/// prompt included as the first element, e.g. `fit_turn_to_budget`'s own
/// output) — unlike `usage_from_chat_messages`/`usage_from_history`, does
/// not prepend a second system message, since this one's already there.
pub fn usage_from_fitted_messages(
    conversation_id: &str,
    messages: &[ChatMessage],
    settings: &ContextSettings,
    observed: Option<&ObservedUsage>,
) -> Result<ContextUsage, String> {
    // Same right-shape estimate as `usage_from_history` (`to_openai_messages`
    // is what the server decodes), just over an already-assembled message
    // list rather than a persisted history -- prefers the server's last
    // authoritative `prompt_tokens` (`observed`) as the base (FR-2), falling
    // back to the full chars/4 estimate when unobserved/stale
    // (`authoritative_prompt_tokens`).
    let openai_messages = crate::inference::http::to_openai_messages(messages);
    let tokens_used = authoritative_prompt_tokens(observed, &openai_messages, token_estimate);
    let token_budget = crate::inference::CONTEXT_WINDOW_TOKENS;
    let state = classify_state(tokens_used, token_budget, settings);
    Ok(ContextUsage {
        conversation_id: conversation_id.to_string(),
        tokens_used,
        token_budget,
        state,
    })
}

/// Purely mechanical, judgment-free context fit: given each message's
/// already-known token cost, keeps `pinned_prefix` messages unconditionally
/// (the system prompt, always first), then greedily keeps as many of the
/// *most recent* remaining messages as fit within `budget`, working
/// backward from the newest. No placeholder-clearing, no summarization
/// call, no persisted notice — this layer makes no judgment about what's
/// worth remembering; that call belongs to the outer loop's own
/// goal/plan/observations state (derived from the full persisted history
/// via retrieval), not here.
///
/// A single message costing more than what's left of the budget is
/// *skipped*, not treated as a stopping point — an oversized message
/// (e.g. a tool result that should have been offloaded but wasn't) must
/// not cost every older message its place too, including ones that would
/// easily have fit; found via real-model testing, where an oversized
/// tool result being the *most recent* message otherwise dropped the
/// user's own task out of the prompt entirely.
pub fn fit_to_budget(
    messages: &[ChatMessage],
    token_costs: &[u32],
    budget: u32,
    pinned_prefix: usize,
) -> Vec<ChatMessage> {
    debug_assert_eq!(messages.len(), token_costs.len());
    let pinned = pinned_prefix.min(messages.len());
    let pinned_tokens: u32 = token_costs[..pinned].iter().sum();
    let mut remaining_budget = budget.saturating_sub(pinned_tokens);

    let mut keep = vec![false; messages.len() - pinned];
    for (i, &cost) in token_costs[pinned..].iter().enumerate().rev() {
        if cost <= remaining_budget {
            remaining_budget -= cost;
            keep[i] = true;
        }
    }

    let mut result = messages[..pinned].to_vec();
    result.extend(
        messages[pinned..]
            .iter()
            .zip(keep.iter())
            .filter(|(_, &k)| k)
            .map(|(m, _)| m.clone()),
    );
    result
}

/// Names the four tool results whose size varies enough for a cost badge
/// to be worth showing (`Write`/`Edit`/`Task`/`AskUserQuestion` are small
/// and roughly fixed-cost, so a badge there would just be noise) — see
/// the widget-cost-and-progressive-rendering design doc's scope decision.
fn wants_token_count(tool_name: &str) -> bool {
    matches!(tool_name, "Read" | "Bash" | "Grep" | "Glob")
}

/// Merges a computed token count into `detail`'s `tokenCount` field — pure
/// JSON manipulation, split out from the token-counting itself so it's
/// unit-testable without a loaded model.
fn merge_token_count(mut detail: serde_json::Value, token_count: usize) -> serde_json::Value {
    if let Some(obj) = detail.as_object_mut() {
        obj.insert("tokenCount".to_string(), serde_json::json!(token_count));
    }
    detail
}

/// Annotates a tool result with its estimated token cost — the same chars/4
/// heuristic (`inference::token_estimate`) `fit_to_budget`/the context usage
/// gauge now use, so this badge and the budget math agree. Applied only to
/// the four tool results whose size varies enough to matter
/// (`wants_token_count`); every other tool's `detail` passes through
/// unchanged. Called right after `dispatch::execute()` returns, before
/// persistence.
pub fn annotate_with_token_count(outcome: ToolOutcome) -> ToolOutcome {
    let tool_name = outcome
        .detail
        .get("toolName")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !wants_token_count(tool_name) {
        return outcome;
    }
    let token_count = token_estimate(&outcome.model_text) as usize;
    ToolOutcome {
        model_text: outcome.model_text,
        detail: merge_token_count(outcome.detail, token_count),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn history_message(content_type: &str, sequence: i64, content: &str) -> HistoryMessage {
        HistoryMessage {
            chat: ChatMessage::user(content),
            content_type: content_type.to_string(),
            sequence,
            plan: false,
            payload_ref: None,
            tool_name: None,
        }
    }

    /// A `text` row authored by the ASSISTANT. `history_message` above always
    /// builds a user turn, and the trailing-assistant prefill hazard the
    /// request-shape tests at the bottom of this module pin only exists when a
    /// span ENDS on an assistant message -- so those fixtures need this.
    fn assistant_history_message(sequence: i64, content: &str) -> HistoryMessage {
        HistoryMessage {
            chat: ChatMessage::assistant(content),
            content_type: "text".to_string(),
            sequence,
            plan: false,
            payload_ref: None,
            tool_name: None,
        }
    }

    /// A tool row with an explicit `plan`/`payload_ref` — the two fields
    /// `storage::conversations::load_history_annotated` parses once at load
    /// time from a real row's `content` JSON (see that module's own tests
    /// for coverage of the parsing itself); `history_message` above
    /// deliberately defaults both to their "never a plan/staged row" state.
    fn history_message_with_flags(
        content_type: &str,
        sequence: i64,
        content: &str,
        plan: bool,
        payload_ref: Option<&str>,
    ) -> HistoryMessage {
        HistoryMessage {
            chat: ChatMessage::user(content),
            content_type: content_type.to_string(),
            sequence,
            plan,
            payload_ref: payload_ref.map(|s| s.to_string()),
            tool_name: None,
        }
    }

    /// A `tool_result` row carrying `tool_name`/`payload_ref` — the two
    /// fields `context::most_recent_read_path` reads to find the
    /// most-recently-`Read` file's source path in a summarized span
    /// (FR-3). Distinct from `history_message_with_flags` above (which
    /// always defaults `tool_name` to `None`) because that helper predates
    /// this field and every other existing test relies on it staying
    /// `None`.
    fn tool_result_message(
        sequence: i64,
        tool_name: &str,
        payload_ref: Option<&str>,
    ) -> HistoryMessage {
        HistoryMessage {
            chat: ChatMessage::tool_result("id", tool_name, "result"),
            content_type: "tool_result".to_string(),
            sequence,
            plan: false,
            payload_ref: payload_ref.map(|s| s.to_string()),
            tool_name: Some(tool_name.to_string()),
        }
    }

    // --- apply_lightweight_clearing ---

    #[test]
    fn no_tool_messages_clears_nothing() {
        let mut history = vec![
            history_message("text", 0, "hi"),
            history_message("text", 1, "hello"),
        ];
        assert!(apply_lightweight_clearing(&mut history, TOOL_KEEP_N, None).is_empty());
        assert_eq!(history[0].chat.text(), "hi");
        assert_eq!(history[1].chat.text(), "hello");
    }

    #[test]
    fn exactly_keep_n_tool_messages_clears_nothing() {
        let mut history: Vec<HistoryMessage> = (0..TOOL_KEEP_N as i64)
            .map(|i| history_message("tool_result", i, "result"))
            .collect();
        assert!(apply_lightweight_clearing(&mut history, TOOL_KEEP_N, None).is_empty());
        assert!(history.iter().all(|m| m.chat.text() == "result"));
    }

    #[test]
    fn keep_n_plus_three_tool_messages_clears_the_oldest_three() {
        let mut history: Vec<HistoryMessage> = (0..(TOOL_KEEP_N as i64 + 3))
            .map(|i| history_message("tool_result", i, &format!("result {i}")))
            .collect();

        let cleared = apply_lightweight_clearing(&mut history, TOOL_KEEP_N, None).len();
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

        let cleared = apply_lightweight_clearing(&mut history, TOOL_KEEP_N, None).len();
        assert_eq!(cleared, 5 - TOOL_KEEP_N);
        assert_eq!(history[0].chat.text(), "old text stays");
        for message in &history[1..=cleared] {
            assert_eq!(message.chat.text(), TOOL_CLEARED_PLACEHOLDER);
        }
        for message in &history[cleared + 1..] {
            assert_ne!(message.chat.text(), TOOL_CLEARED_PLACEHOLDER);
        }
    }

    #[test]
    fn cleared_row_with_payload_ref_gets_the_restorable_pointer_placeholder() {
        // TOOL_KEEP_N + 1 tool_result rows -- the oldest (index 0) is the
        // one that actually gets cleared. Its `payload_ref` is set,
        // mirroring what `storage::conversations::load_history_annotated`
        // parses out of a real row whose `detail.payloadRef` was stamped by
        // `commands::agent::handle_general_tool_call`
        // (`detail["payloadRef"] = json!(staged.payload_ref)`).
        let mut history: Vec<HistoryMessage> = (0..(TOOL_KEEP_N as i64 + 1))
            .map(|i| history_message("tool_result", i, &format!("result {i}")))
            .collect();
        history[0] =
            history_message_with_flags("tool_result", 0, "result 0", false, Some("/tmp/x.txt"));

        let cleared = apply_lightweight_clearing(&mut history, TOOL_KEEP_N, None).len();
        assert_eq!(cleared, 1);
        assert_eq!(
            history[0].chat.text(),
            limits::tool_cleared_placeholder_with_pointer("/tmp/x.txt"),
            "a staged row's placeholder must point back at the payload file, not just say it's gone"
        );
        for message in &history[1..] {
            assert_ne!(message.chat.text(), TOOL_CLEARED_PLACEHOLDER);
        }
    }

    #[test]
    fn cleared_row_without_payload_ref_gets_the_plain_placeholder() {
        let mut history: Vec<HistoryMessage> = (0..(TOOL_KEEP_N as i64 + 1))
            .map(|i| history_message("tool_result", i, &format!("result {i}")))
            .collect();

        let cleared = apply_lightweight_clearing(&mut history, TOOL_KEEP_N, None).len();
        assert_eq!(cleared, 1);
        assert_eq!(history[0].chat.text(), TOOL_CLEARED_PLACEHOLDER);
    }

    #[test]
    fn cleared_rows_without_payload_ref_cite_their_transcript_entry() {
        // 4 tool_result rows (seq 0-3), none staged (no payload_ref) --
        // with TOOL_KEEP_N=2 kept, rows 0 and 1 clear. When a transcript
        // path is available, each cleared row cites ITS OWN sequence in
        // the transcript rather than the generic "gone for good" wording.
        let mut history: Vec<HistoryMessage> = (0..4)
            .map(|i| history_message("tool_result", i, &format!("result {i}")))
            .collect();

        let cleared = apply_lightweight_clearing(&mut history, 2, Some("/t/c1.txt")).len();
        assert_eq!(cleared, 2);
        assert_eq!(
            history[0].chat.text(),
            limits::tool_cleared_placeholder_transcript("/t/c1.txt", 0)
        );
        assert_eq!(
            history[1].chat.text(),
            limits::tool_cleared_placeholder_transcript("/t/c1.txt", 1)
        );
        assert_eq!(history[2].chat.text(), "result 2");
        assert_eq!(history[3].chat.text(), "result 3");
    }

    #[test]
    fn cleared_rows_with_payload_ref_still_cite_the_payload_file() {
        // Same shape, but the cleared rows DO carry a payload_ref -- the
        // payload file stays the recovery route even when a transcript is
        // also available, since it's the more specific pointer (the exact
        // staged content, not the whole conversation).
        let mut history: Vec<HistoryMessage> = (0..4)
            .map(|i| {
                history_message_with_flags(
                    "tool_result",
                    i,
                    &format!("result {i}"),
                    false,
                    Some("/p/x.txt"),
                )
            })
            .collect();

        let cleared = apply_lightweight_clearing(&mut history, 2, Some("/t/c1.txt")).len();
        assert_eq!(cleared, 2);
        assert_eq!(
            history[0].chat.text(),
            limits::tool_cleared_placeholder_with_pointer("/p/x.txt")
        );
        assert_eq!(
            history[1].chat.text(),
            limits::tool_cleared_placeholder_with_pointer("/p/x.txt")
        );
    }

    #[test]
    fn plan_rows_clear_beyond_the_most_recent_two_even_when_keep_n_would_keep_them_all() {
        let mut history: Vec<HistoryMessage> = (0..5)
            .map(|i| {
                history_message_with_flags(
                    "tool_result",
                    i,
                    &format!("plan result {i}"),
                    true,
                    None,
                )
            })
            .collect();

        // keep_n is large enough that the ordinary TOOL_KEEP_N-style rule
        // would keep every one of these -- proving plan rows are cleared
        // by their own, stricter PLAN_KEEP_N cutoff instead.
        let cleared = apply_lightweight_clearing(&mut history, 10, None).len();
        assert_eq!(cleared, 5 - limits::PLAN_KEEP_N);
        for message in &history[0..cleared] {
            assert_eq!(message.chat.text(), TOOL_CLEARED_PLACEHOLDER);
        }
        for (i, message) in history.iter().enumerate().skip(cleared) {
            assert_eq!(message.chat.text(), format!("plan result {i}"));
        }
    }

    #[test]
    fn plan_rows_and_regular_tool_rows_are_cleared_independently() {
        let mut history = vec![
            history_message_with_flags("tool_result", 0, "plan 0", true, None),
            history_message_with_flags("tool_result", 1, "plan 1", true, None),
            history_message_with_flags("tool_result", 2, "plan 2", true, None),
            history_message("tool_result", 3, "regular 0"),
            history_message("tool_result", 4, "regular 1"),
        ];

        // TOOL_KEEP_N (2) regular rows exist -- none of them should clear.
        // Of the 3 plan rows, only the oldest (beyond PLAN_KEEP_N=2)
        // should clear.
        let cleared = apply_lightweight_clearing(&mut history, TOOL_KEEP_N, None).len();
        assert_eq!(cleared, 1);
        assert_eq!(history[0].chat.text(), TOOL_CLEARED_PLACEHOLDER);
        assert_eq!(history[1].chat.text(), "plan 1");
        assert_eq!(history[2].chat.text(), "plan 2");
        assert_eq!(history[3].chat.text(), "regular 0");
        assert_eq!(history[4].chat.text(), "regular 1");
    }

    #[test]
    fn plan_call_rows_are_plan_partitioned_and_never_displace_regular_tool_history() {
        // Regression for a review finding on baab3f3: a plan tool's CALL
        // row (`commands::agent::persist_tool_call`) used to persist with
        // no "plan" marker at all -- only its paired RESULT row carried
        // one -- so this row's `plan` field was silently always `false`,
        // miscounting every plan interaction's call row as an
        // ordinary/regular tool row in the partition below. With enough
        // plan activity interspersed among genuine tool history, that
        // miscount could push a genuine, recent tool result out of
        // TOOL_KEEP_N prematurely -- exactly what this reproduces: if
        // either `plan_call_*` row here were (incorrectly) constructed
        // with `plan: false`, "genuine result 1" would be cleared instead
        // of surviving.
        let mut history = vec![
            history_message("tool_result", 0, "genuine result 0"),
            history_message_with_flags("tool_call", 1, "plan call A", true, None),
            history_message_with_flags("tool_result", 2, "plan result A", true, None),
            history_message("tool_result", 3, "genuine result 1"),
            history_message_with_flags("tool_call", 4, "plan call B", true, None),
            history_message_with_flags("tool_result", 5, "plan result B", true, None),
            history_message("tool_result", 6, "genuine result 2"),
        ];

        let cleared = apply_lightweight_clearing(&mut history, TOOL_KEEP_N, None).len();

        // Regular population is exactly the 3 genuine results (TOOL_KEEP_N
        // = 2 survive, the oldest clears) -- the two plan call rows never
        // count toward it at all.
        assert_eq!(history[0].chat.text(), TOOL_CLEARED_PLACEHOLDER);
        assert_eq!(
            history[3].chat.text(),
            "genuine result 1",
            "a plan interaction's call row must never displace genuine tool history out of TOOL_KEEP_N"
        );
        assert_eq!(history[6].chat.text(), "genuine result 2");
        // Plan population is the 4 plan rows (2 pairs); PLAN_KEEP_N=2 keeps
        // only the most recent pair (call B/result B), clearing the oldest
        // (call A/result A).
        assert_eq!(history[1].chat.text(), TOOL_CLEARED_PLACEHOLDER);
        assert_eq!(history[2].chat.text(), TOOL_CLEARED_PLACEHOLDER);
        assert_eq!(history[4].chat.text(), "plan call B");
        assert_eq!(history[5].chat.text(), "plan result B");
        assert_eq!(cleared, 3);
    }

    // apply_lightweight_clearing_in_memory/compact_in_memory's own former
    // unit tests lived here -- both were removed in favor of
    // fit_turn_to_budget/fit_to_budget (see fit_to_budget's own tests
    // below; fit_turn_to_budget itself is exercised end-to-end by the
    // real-model agent task tests instead of a unit test here, per this
    // file's own testability note at the top).

    // --- ContextSettings::from_raw ---

    #[test]
    fn missing_settings_fall_back_to_defaults() {
        let raw = std::collections::HashMap::new();
        let settings = ContextSettings::from_raw(&raw);
        assert_eq!(
            settings.warn_threshold_pct,
            ContextSettings::DEFAULT_WARN_THRESHOLD_PCT
        );
        assert_eq!(
            settings.compact_threshold_pct,
            ContextSettings::DEFAULT_COMPACT_THRESHOLD_PCT
        );
        assert_eq!(
            settings.hard_limit_pct,
            ContextSettings::DEFAULT_HARD_LIMIT_PCT
        );
    }

    #[test]
    fn unparseable_settings_fall_back_to_defaults() {
        let mut raw = std::collections::HashMap::new();
        raw.insert(
            ContextSettings::KEY_WARN_THRESHOLD_PCT.to_string(),
            "not a number".to_string(),
        );
        let settings = ContextSettings::from_raw(&raw);
        assert_eq!(
            settings.warn_threshold_pct,
            ContextSettings::DEFAULT_WARN_THRESHOLD_PCT
        );
    }

    #[test]
    fn out_of_order_thresholds_are_clamped_up() {
        let mut raw = std::collections::HashMap::new();
        raw.insert(
            ContextSettings::KEY_WARN_THRESHOLD_PCT.to_string(),
            "0.9".to_string(),
        );
        raw.insert(
            ContextSettings::KEY_COMPACT_THRESHOLD_PCT.to_string(),
            "0.5".to_string(),
        );
        raw.insert(
            ContextSettings::KEY_HARD_LIMIT_PCT.to_string(),
            "0.6".to_string(),
        );
        let settings = ContextSettings::from_raw(&raw);
        assert_eq!(settings.warn_threshold_pct, 0.9);
        assert_eq!(
            settings.compact_threshold_pct, 0.9,
            "compact must be clamped up to at least warn"
        );
        assert_eq!(
            settings.hard_limit_pct, 0.9,
            "hard limit must be clamped up to at least compact"
        );
    }

    #[test]
    fn valid_settings_pass_through_unchanged() {
        let mut raw = std::collections::HashMap::new();
        raw.insert(
            ContextSettings::KEY_WARN_THRESHOLD_PCT.to_string(),
            "0.4".to_string(),
        );
        raw.insert(
            ContextSettings::KEY_COMPACT_THRESHOLD_PCT.to_string(),
            "0.6".to_string(),
        );
        raw.insert(
            ContextSettings::KEY_HARD_LIMIT_PCT.to_string(),
            "0.8".to_string(),
        );
        let settings = ContextSettings::from_raw(&raw);
        assert_eq!(settings.warn_threshold_pct, 0.4);
        assert_eq!(settings.compact_threshold_pct, 0.6);
        assert_eq!(settings.hard_limit_pct, 0.8);
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

    // --- fit_to_budget ---

    fn text_messages(n: usize) -> Vec<ChatMessage> {
        (0..n).map(|i| ChatMessage::user(format!("m{i}"))).collect()
    }

    #[test]
    fn everything_fits_when_under_budget() {
        let messages = text_messages(5);
        let costs = vec![10u32; 5];
        let result = fit_to_budget(&messages, &costs, 1000, 1);
        assert_eq!(result.len(), 5);
        for (i, m) in result.iter().enumerate() {
            assert_eq!(m.text(), format!("m{i}"));
        }
    }

    #[test]
    fn drops_the_oldest_non_pinned_messages_first_when_over_budget() {
        let messages = text_messages(5);
        let costs = vec![10u32; 5]; // pinned(1)=10, remaining 4 cost 10 each
                                    // budget 10 (pinned) + 25 leaves room for exactly 2 of the 4 remaining
        let result = fit_to_budget(&messages, &costs, 35, 1);
        assert_eq!(
            result.iter().map(|m| m.text()).collect::<Vec<_>>(),
            vec!["m0", "m3", "m4"],
            "keeps the pinned prefix plus the most recent messages that fit, dropping the oldest of the rest"
        );
    }

    #[test]
    fn pinned_prefix_is_always_kept_even_with_no_room_left_for_anything_else() {
        let messages = text_messages(3);
        let costs = vec![50u32, 10, 10];
        let result = fit_to_budget(&messages, &costs, 50, 1);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].text(), "m0");
    }

    #[test]
    fn a_single_message_costing_more_than_the_whole_remaining_budget_is_dropped_whole() {
        let messages = text_messages(3);
        let costs = vec![10u32, 1000, 5];
        // pinned(1)=10, remaining budget=15 -- m1 costs 1000 (dropped
        // entirely, never partially included), m2 costs 5 (fits, kept).
        let result = fit_to_budget(&messages, &costs, 25, 1);
        assert_eq!(
            result.iter().map(|m| m.text()).collect::<Vec<_>>(),
            vec!["m0", "m2"]
        );
    }

    #[test]
    fn an_oversized_message_is_skipped_not_treated_as_a_stopping_point() {
        // m2 is oversized and also the OLDER of the two non-pinned
        // messages that would fit -- m1 and m3 are both small and must
        // both survive despite m2 sitting chronologically between them,
        // proving the trim doesn't give up on everything once it hits one
        // message that doesn't fit.
        let messages = text_messages(4);
        let costs = vec![10u32, 5, 1000, 5]; // pinned=m0(10), then m1(5), m2(1000), m3(5)
        let result = fit_to_budget(&messages, &costs, 25, 1);
        assert_eq!(
            result.iter().map(|m| m.text()).collect::<Vec<_>>(),
            vec!["m0", "m1", "m3"],
            "an oversized message in the middle must not cost an older, smaller message its place"
        );
    }

    #[test]
    fn zero_pinned_prefix_trims_purely_by_recency() {
        let messages = text_messages(4);
        let costs = vec![10u32; 4];
        let result = fit_to_budget(&messages, &costs, 25, 0);
        assert_eq!(
            result.iter().map(|m| m.text()).collect::<Vec<_>>(),
            vec!["m2", "m3"]
        );
    }

    /// F1 (final whole-branch review): the per-turn state tail
    /// (`PlanState::state_tail`) is pushed AFTER measure/threshold/
    /// `fit_turn_to_budget` have run, so none of them ever see it. The fix
    /// reserves `STATE_TAIL_RESERVE_TOKENS` alongside the output reserve;
    /// this test proves the arithmetic envelope with the exact failure
    /// shape: a long history that compaction parks just under the
    /// threshold, plus a realistically large (~700-token) tail.
    #[test]
    fn state_tail_reserve_keeps_a_near_threshold_history_plus_tail_within_the_window() {
        use crate::context::limits::{
            AGENT_TURN_MAX_OUTPUT_TOKENS, CONTEXT_WINDOW_TOKENS, STATE_TAIL_RESERVE_TOKENS,
        };

        // Pinned system prompt + enough uniform turns to overflow any budget.
        let per_message_cost: u32 = 400;
        let system_cost: u32 = 600;
        let messages = text_messages(60);
        let mut costs = vec![per_message_cost; 60];
        costs[0] = system_cost;
        let large_tail_cost: u32 = 700; // a 20-step plan's mode banner + frame + checklist
        let fitted_total = |fitted: &[ChatMessage]| -> u32 {
            system_cost + (fitted.len() as u32 - 1) * per_message_cost
        };

        // Pre-fix shape (non-vacuity guard): reserving ONLY the output
        // tokens leaves a fitted history whose rendered prompt + tail +
        // output can exceed the window -- the silent late-task abort.
        let old_budget = CONTEXT_WINDOW_TOKENS - AGENT_TURN_MAX_OUTPUT_TOKENS;
        let old_fitted = fit_to_budget(&messages, &costs, old_budget, 1);
        assert!(
            fitted_total(&old_fitted) + large_tail_cost + AGENT_TURN_MAX_OUTPUT_TOKENS
                > CONTEXT_WINDOW_TOKENS,
            "without the tail reserve this history+tail+output must overflow, or this test proves nothing"
        );

        // Post-fix: `fit_turn_to_budget` (and the hosts' thresholds)
        // additionally reserve STATE_TAIL_RESERVE_TOKENS, so any tail up
        // to the reserve plus a full turn's output always fits.
        let new_budget =
            CONTEXT_WINDOW_TOKENS - AGENT_TURN_MAX_OUTPUT_TOKENS - STATE_TAIL_RESERVE_TOKENS;
        let fitted = fit_to_budget(&messages, &costs, new_budget, 1);
        assert!(large_tail_cost <= STATE_TAIL_RESERVE_TOKENS);
        assert!(
            fitted_total(&fitted) + large_tail_cost + AGENT_TURN_MAX_OUTPUT_TOKENS
                <= CONTEXT_WINDOW_TOKENS,
            "fitted history ({} tokens) + tail ({large_tail_cost}) + output ({AGENT_TURN_MAX_OUTPUT_TOKENS}) must stay within the {CONTEXT_WINDOW_TOKENS}-token window",
            fitted_total(&fitted)
        );
    }

    #[test]
    fn wants_token_count_is_true_only_for_the_four_size_variable_tools() {
        assert!(wants_token_count("Read"));
        assert!(wants_token_count("Bash"));
        assert!(wants_token_count("Grep"));
        assert!(wants_token_count("Glob"));
        assert!(!wants_token_count("Write"));
        assert!(!wants_token_count("Edit"));
        assert!(!wants_token_count("Task"));
        assert!(!wants_token_count("AskUserQuestion"));
    }

    #[test]
    fn merge_token_count_inserts_the_field_into_an_object_detail() {
        let detail = serde_json::json!({"toolName": "Read", "filePath": "/tmp/x.txt"});
        let merged = merge_token_count(detail, 312);
        assert_eq!(merged["tokenCount"], 312);
        assert_eq!(merged["filePath"], "/tmp/x.txt");
    }

    // --- messages_to_summarize (tier 2's summarization-input selection) ---
    //
    // `summarize_and_persist` itself needs a real model server (this
    // file's own testability note at the top), so per the
    // `fit_turn_to_budget`/`fit_to_budget` precedent, the pure
    // span-selection logic is pulled out into `messages_to_summarize` so
    // it's unit-testable on its own.

    #[test]
    fn messages_to_summarize_is_empty_at_or_under_the_protected_window() {
        let history = vec![history_message("text", 0, "task statement")];
        assert!(messages_to_summarize(&history, PROTECTED_RECENT_MESSAGES).is_empty());
    }

    #[test]
    fn messages_to_summarize_excludes_the_most_recent_protected_messages() {
        // All assistant-authored -- no genuine user message exists in this
        // history at all, so only the protected-recent-window exclusion is
        // under test here (the separate first-user-message pin has its own
        // dedicated test below).
        let history: Vec<HistoryMessage> = (0..5)
            .map(|i| HistoryMessage {
                chat: ChatMessage::assistant(format!("m{i}")),
                content_type: "text".to_string(),
                sequence: i,
                plan: false,
                payload_ref: None,
                tool_name: None,
            })
            .collect();
        let selected = messages_to_summarize(&history, 2);
        let texts: Vec<String> = selected.iter().map(|m| m.chat.text()).collect();
        assert_eq!(texts, vec!["m0", "m1", "m2"]);
    }

    #[test]
    fn messages_to_summarize_always_excludes_the_first_user_message() {
        // protected_recent=2 would otherwise summarize indices [0..4) --
        // "task statement", m1, m2 -- but the first user message (the
        // task statement) must never enter the summarized span: a
        // summarization pass is generative, lossy compression by a small
        // model, and losing the task statement to it would be
        // unrecoverable (OpenHands' "keep-first" behavior).
        let mut history = vec![history_message("text", 0, "task statement")];
        history.extend((1..6).map(|i| history_message("text", i, &format!("m{i}"))));

        let selected = messages_to_summarize(&history, 2);
        let texts: Vec<String> = selected.iter().map(|m| m.chat.text()).collect();
        assert_eq!(
            texts,
            vec!["m1", "m2", "m3"],
            "the first user message must survive outside the summarized span"
        );
    }

    #[test]
    fn messages_to_summarize_does_not_mistake_a_leading_tool_result_for_the_first_user_message() {
        // Defensive: `ChatMessage::tool_result` also reconstructs with
        // role "user" (see its own doc comment), but a tool_result row is
        // never "the task statement" and must not be pinned as if it were
        // -- the pin must instead land on "task statement", the real first
        // user message, excluding it (and only it) from the result.
        let history = vec![
            history_message("tool_result", 0, "tool output"),
            history_message("text", 1, "task statement"),
            history_message("text", 2, "m1"),
            history_message("text", 3, "m2"),
        ];
        let selected = messages_to_summarize(&history, 1);
        let texts: Vec<String> = selected.iter().map(|m| m.chat.text()).collect();
        assert_eq!(
            texts,
            vec!["tool output", "m1"],
            "only a genuine text/rich_text user row can be the pinned first user message"
        );
    }

    // --- the post-compaction history contract ---
    //
    // What each tier PERSISTS, and what the next turn is therefore seeded
    // with, driven through the real functions end to end: production's own
    // span selection (`messages_to_summarize`), its own notice payloads
    // (`summarized_notice_json`/`cleared_notice_json`), its own insert path
    // (`storage::conversations::persist_context_notice` ->
    // `storage::messages::insert`, which allocates the sequence), and its
    // own load (`load_history_annotated`, what `commands::agent`'s seed goes
    // through). Nothing here hand-builds a row shape or a JSON payload, so
    // no fixture can drift from what production writes.
    //
    // These exist because BOTH of the bugs the 2026-07-15 real-model pass
    // found lived exactly here, in the seam between deciding and persisting,
    // and neither was reachable by any of the pure unit tests above: tier 2
    // chose the right span and then spliced at the wrong point, and tier 1
    // computed the right clearing and then dropped it on the floor. A
    // ten-minute `#[ignore]`d real-model test is the wrong and only place to
    // have caught that.

    /// A real, migrated DB with conversation `c1` already in it — the
    /// `messages.conversation_id` foreign key is real, so these fixtures
    /// cannot cut the corner of inserting messages into a conversation that
    /// does not exist.
    fn conversation_conn() -> Connection {
        let conn = crate::storage::test_connection();
        conn.execute(
            "INSERT INTO conversations (id, workspace_id, title, created_at, updated_at) \
             VALUES ('c1', NULL, 'T', 0, 0)",
            [],
        )
        .unwrap();
        conn
    }

    /// Seeds a row through production's only insert path, which allocates
    /// `MAX(sequence) + 1`. Returns the allocated sequence.
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
                tool_call_id: Some("tc"),
                model_text: Some(content),
                created_at: 0,
                duration_ms: None,
                token_count: None,
            },
        )
        .unwrap()
    }

    fn load(conn: &Connection) -> Vec<HistoryMessage> {
        load_history_annotated(conn, "c1", Path::new("/nonexistent-skills")).unwrap()
    }

    /// The whole of tier 2's persistence contract, minus only the model:
    /// span in, notice persisted, history reloaded. Pins the shape the
    /// `#[ignore]`d `the_real_maybe_compact_condenses_an_over_threshold_conversation`
    /// spends ten minutes and a real Qwen3.5-4B to reach.
    #[test]
    fn a_persisted_summary_replaces_exactly_the_span_messages_to_summarize_chose() {
        let conn = conversation_conn();
        seed_row(&conn, "user", "text", "the task statement");
        seed_row(&conn, "assistant", "text", "summarized 1");
        seed_row(&conn, "tool", "tool_result", "summarized tool output");
        seed_row(&conn, "assistant", "text", "summarized 2");
        for i in 0..3 {
            seed_row(&conn, "user", "text", &format!("protected {i}"));
        }

        // Production's own span selection over production's own load, then
        // production's own notice, persisted through production's own path.
        let history = load(&conn);
        let to_summarize = messages_to_summarize(&history, 3);
        let summarized: Vec<String> = to_summarize.iter().map(|m| m.chat.text()).collect();
        assert_eq!(
            summarized,
            vec![
                "summarized 1",
                "<tool_response>summarized tool output</tool_response>",
                "summarized 2"
            ],
            "fixture guard: the span must exclude the task statement and the protected recent \
             turns, or this test proves nothing about what happens to them"
        );
        let through = to_summarize.last().unwrap().sequence;
        persist_context_notice(
            &conn,
            None,
            "c1",
            0,
            &summarized_notice_json("<state_snapshot>the gist</state_snapshot>", through),
        )
        .unwrap();

        let after: Vec<String> = load(&conn).iter().map(|m| m.chat.text()).collect();
        assert_eq!(
            after,
            vec![
                "the task statement",
                "<state_snapshot>the gist</state_snapshot>",
                "protected 0",
                "protected 1",
                "protected 2",
            ],
            "the summary must replace exactly the span and nothing else. Every message \
             `messages_to_summarize` refused to summarize is one the summary does not describe, \
             so dropping it destroys it outright: the task statement (keep-first) and every \
             protected recent turn have to survive verbatim."
        );
    }

    /// Tier 1's clearing has to reach the MODEL, which means surviving a
    /// reload -- `maybe_compact` mutates a local copy of the history and
    /// then drops it, so the notice is the only thing that carries the
    /// clearing forward to the next turn's prompt.
    #[test]
    fn a_persisted_clearing_reaches_the_next_load() {
        let conn = conversation_conn();
        seed_row(&conn, "user", "text", "the task statement");
        for i in 0..(TOOL_KEEP_N + 2) {
            seed_row(
                &conn,
                "tool",
                "tool_result",
                &format!("big tool output {i}"),
            );
        }

        let mut history = load(&conn);
        let cleared = apply_lightweight_clearing(&mut history, TOOL_KEEP_N, None);
        assert_eq!(cleared.len(), 2, "the two oldest of four tool rows clear");
        persist_context_notice(&conn, None, "c1", 0, &cleared_notice_json(&cleared)).unwrap();

        let reloaded = load(&conn);
        let tool_texts: Vec<String> = reloaded
            .iter()
            .filter(|m| m.content_type == "tool_result")
            .map(|m| m.chat.text())
            .collect();
        assert_eq!(
            tool_texts,
            vec![
                TOOL_CLEARED_PLACEHOLDER,
                TOOL_CLEARED_PLACEHOLDER,
                "<tool_response>big tool output 2</tool_response>",
                "<tool_response>big tool output 3</tool_response>",
            ],
            "the rows tier 1 cleared must come back cleared. If they come back whole, the \
             `cleared` notice and the usage drop maybe_compact reports for it are both fiction, \
             and usage is UNDER-reported -- the direction that lets an over-budget prompt reach \
             the server."
        );
    }

    /// Tier 1 runs on every over-threshold turn, over a history that already
    /// has the previous clearing replayed into it. If it re-cleared those
    /// rows it would report a fresh clearing, persist a fresh notice, and
    /// claim `state:"justCompacted"` every single turn, forever.
    #[test]
    fn re_clearing_an_already_cleared_history_is_a_no_op() {
        let conn = conversation_conn();
        for i in 0..(TOOL_KEEP_N + 2) {
            seed_row(
                &conn,
                "tool",
                "tool_result",
                &format!("big tool output {i}"),
            );
        }

        let mut history = load(&conn);
        let first = apply_lightweight_clearing(&mut history, TOOL_KEEP_N, None);
        persist_context_notice(&conn, None, "c1", 0, &cleared_notice_json(&first)).unwrap();

        // Exactly what the next turn does: load (replaying the notice above)
        // and run tier 1 again.
        let mut reloaded = load(&conn);
        let second = apply_lightweight_clearing(&mut reloaded, TOOL_KEEP_N, None);
        assert!(
            second.is_empty(),
            "tier 1 re-cleared rows a previous pass had already cleared: {second:?}"
        );
    }

    /// A conversation compacted twice: the second summary supersedes the
    /// first, and must still cover everything the first one did -- a row the
    /// user has been told is condensed must never come back.
    #[test]
    fn a_second_compaction_still_excludes_the_first_summarys_span() {
        let conn = conversation_conn();
        seed_row(&conn, "user", "text", "the task statement");
        seed_row(&conn, "assistant", "text", "ancient 1");
        seed_row(&conn, "assistant", "text", "ancient 2");
        for i in 0..3 {
            seed_row(&conn, "user", "text", &format!("first protected {i}"));
        }

        let history = load(&conn);
        let first_through = messages_to_summarize(&history, 3).last().unwrap().sequence;
        persist_context_notice(
            &conn,
            None,
            "c1",
            0,
            &summarized_notice_json("first summary", first_through),
        )
        .unwrap();

        // More turns, then compact again over the ALREADY-SPLICED history --
        // whose first rows are now the synthesized summary itself.
        seed_row(&conn, "assistant", "text", "newer 1");
        let history = load(&conn);
        let second_through = messages_to_summarize(&history, 3).last().unwrap().sequence;
        persist_context_notice(
            &conn,
            None,
            "c1",
            0,
            &summarized_notice_json("second summary", second_through),
        )
        .unwrap();

        let after: Vec<String> = load(&conn).iter().map(|m| m.chat.text()).collect();
        assert_eq!(
            after,
            vec![
                "the task statement",
                "second summary",
                "first protected 1",
                "first protected 2",
                "newer 1",
            ],
            "the second summary must cover the first summary's own span too -- including the \
             first summary itself, which is what keeps a twice-compacted conversation condensed \
             instead of resurrecting rows the user was told were gone"
        );
    }

    // --- ContextSettings ---

    #[test]
    fn tool_output_offload_tokens_parses_and_defaults() {
        use std::collections::HashMap;
        // Absent -> default.
        let s = ContextSettings::from_raw(&HashMap::new());
        assert_eq!(
            s.tool_output_offload_tokens,
            limits::DEFAULT_TOOL_OUTPUT_OFFLOAD_TOKENS
        );
        // Present and valid -> honored.
        let mut raw = HashMap::new();
        raw.insert(
            ContextSettings::KEY_TOOL_OUTPUT_OFFLOAD_TOKENS.to_string(),
            "1024".to_string(),
        );
        assert_eq!(
            ContextSettings::from_raw(&raw).tool_output_offload_tokens,
            1024
        );
        // Zero/garbage -> default (same clamp discipline as the other keys).
        raw.insert(
            ContextSettings::KEY_TOOL_OUTPUT_OFFLOAD_TOKENS.to_string(),
            "0".to_string(),
        );
        assert_eq!(
            ContextSettings::from_raw(&raw).tool_output_offload_tokens,
            limits::DEFAULT_TOOL_OUTPUT_OFFLOAD_TOKENS
        );
    }

    // --- evaluate_summary (compaction fail-safe guards) ---
    //
    // Mirrors qwen-code's COMPRESSION_FAILED_* guards: a candidate summary
    // from a small local model must never be applied blind. Precedence
    // matters -- empty beats truncated beats inflated -- so each rejection
    // case is tested both in isolation and, where two conditions could fire
    // together, for which one wins.

    #[test]
    fn evaluate_summary_rejects_an_empty_summary() {
        assert_eq!(
            evaluate_summary("", None, 100, 10),
            SummaryDecision::RejectEmpty
        );
    }

    #[test]
    fn evaluate_summary_rejects_a_whitespace_only_summary() {
        assert_eq!(
            evaluate_summary("   \n\t  ", None, 100, 10),
            SummaryDecision::RejectEmpty
        );
    }

    #[test]
    fn evaluate_summary_rejects_a_truncated_summary() {
        // Non-empty and shrinking, but finish_reason "length" means the
        // completion was cut off mid-summary -- unsafe to apply even though
        // it "looks" small.
        assert_eq!(
            evaluate_summary(
                "a partial summary that got cut off",
                Some("length"),
                100,
                10
            ),
            SummaryDecision::RejectTruncated
        );
    }

    #[test]
    fn evaluate_summary_rejects_an_inflated_summary() {
        // post_tokens >= pre_tokens: the "summary" is not actually smaller
        // than what it would replace.
        assert_eq!(
            evaluate_summary(
                "a summary that grew bigger than the original",
                Some("stop"),
                10,
                10
            ),
            SummaryDecision::RejectInflated
        );
        assert_eq!(
            evaluate_summary("an even bigger summary", Some("stop"), 10, 20),
            SummaryDecision::RejectInflated
        );
    }

    #[test]
    fn evaluate_summary_accepts_a_nonempty_nontruncated_shrinking_summary() {
        assert_eq!(
            evaluate_summary("a concise summary", Some("stop"), 100, 10),
            SummaryDecision::Accept
        );
    }

    #[test]
    fn evaluate_summary_accepts_when_finish_reason_is_absent() {
        // A missing finish_reason (e.g. an older/mocked server response)
        // must not be mistaken for "length" -- only an explicit
        // `Some("length")` triggers RejectTruncated.
        assert_eq!(
            evaluate_summary("a concise summary", None, 100, 10),
            SummaryDecision::Accept
        );
    }

    #[test]
    fn evaluate_summary_empty_beats_truncated() {
        // Both conditions fire (empty AND finish_reason "length") --
        // RejectEmpty must win, since it's the more fundamental failure.
        assert_eq!(
            evaluate_summary("   ", Some("length"), 100, 10),
            SummaryDecision::RejectEmpty
        );
    }

    #[test]
    fn evaluate_summary_truncated_beats_inflated() {
        // Both conditions fire (finish_reason "length" AND post >= pre) --
        // RejectTruncated must win over RejectInflated.
        assert_eq!(
            evaluate_summary("not empty", Some("length"), 10, 20),
            SummaryDecision::RejectTruncated
        );
    }

    // --- authoritative_prompt_tokens (FR-2: server-truth base + chars/4 delta) ---

    fn est(s: &str) -> u32 {
        (s.chars().count() / 4) as u32
    }

    fn msgs(n: usize) -> Vec<serde_json::Value> {
        (0..n)
            .map(|i| serde_json::json!({"role": "user", "content": format!("message {i}")}))
            .collect()
    }

    #[test]
    fn authoritative_prompt_tokens_falls_back_to_a_full_estimate_when_unobserved() {
        let messages = msgs(3);
        let expected = est(&serde_json::to_string(&messages).unwrap());
        assert_eq!(authoritative_prompt_tokens(None, &messages, est), expected);
    }

    #[test]
    fn authoritative_prompt_tokens_is_the_base_alone_when_nothing_was_appended_since() {
        let messages = msgs(3);
        let observed = ObservedUsage {
            prompt_tokens: 999,
            at_len: messages.len(),
        };
        assert_eq!(
            authoritative_prompt_tokens(Some(&observed), &messages, est),
            999,
            "at_len == len must add zero delta -- the base is the whole answer"
        );
    }

    #[test]
    fn authoritative_prompt_tokens_adds_the_estimated_delta_of_messages_appended_since() {
        let messages = msgs(5);
        let observed = ObservedUsage {
            prompt_tokens: 999,
            at_len: 3,
        };
        let expected_delta = est(&serde_json::to_string(&messages[3..]).unwrap());
        assert_eq!(
            authoritative_prompt_tokens(Some(&observed), &messages, est),
            999 + expected_delta
        );
        assert!(
            expected_delta > 0,
            "the delta must be nonzero, or this test proves nothing"
        );
    }

    #[test]
    fn authoritative_prompt_tokens_falls_back_to_a_full_estimate_when_the_base_is_stale_or_shrunk()
    {
        // `at_len` (5) exceeds the current message count (3) -- a
        // compaction/reload invalidated the base. Must fall back to a full
        // estimate, never underflow/panic on `all_openai_msgs[o.at_len..]`.
        let messages = msgs(3);
        let observed = ObservedUsage {
            prompt_tokens: 999,
            at_len: 5,
        };
        let expected = est(&serde_json::to_string(&messages).unwrap());
        assert_eq!(
            authoritative_prompt_tokens(Some(&observed), &messages, est),
            expected
        );
    }

    // --- breaker_open (consecutive-failure circuit breaker) ---

    #[test]
    fn breaker_open_is_false_under_the_threshold() {
        assert!(!breaker_open(0, false));
        assert!(!breaker_open(
            limits::MAX_CONSECUTIVE_COMPACTION_FAILURES - 1,
            false
        ));
    }

    #[test]
    fn breaker_open_is_true_at_or_above_the_threshold() {
        assert!(breaker_open(
            limits::MAX_CONSECUTIVE_COMPACTION_FAILURES,
            false
        ));
        assert!(breaker_open(
            limits::MAX_CONSECUTIVE_COMPACTION_FAILURES + 5,
            false
        ));
    }

    #[test]
    fn breaker_open_is_always_false_when_forced_regardless_of_failure_count() {
        // A forced "Compact now" always ignores the breaker -- that's the
        // recovery path a successful forced compaction resets the counter
        // through.
        assert!(!breaker_open(0, true));
        assert!(!breaker_open(
            limits::MAX_CONSECUTIVE_COMPACTION_FAILURES,
            true
        ));
        assert!(!breaker_open(
            limits::MAX_CONSECUTIVE_COMPACTION_FAILURES + 100,
            true
        ));
    }

    // --- most_recent_read_path / bounded_restore_body (FR-3: restore the
    // most-recent Read'd file's contents after a compaction) ---

    #[test]
    fn most_recent_read_path_returns_the_last_reads_source_path() {
        let read_a = tool_result_message(0, "Read", Some("/a.rs"));
        let bash = tool_result_message(1, "Bash", Some("/tmp/out.txt"));
        let read_b = tool_result_message(2, "Read", Some("/b.rs"));
        let span: Vec<&HistoryMessage> = vec![&read_a, &bash, &read_b];

        assert_eq!(
            most_recent_read_path(&span),
            Some("/b.rs".to_string()),
            "must return the LAST Read's path, not the first"
        );
    }

    #[test]
    fn most_recent_read_path_is_none_without_a_read() {
        let bash = tool_result_message(0, "Bash", Some("/tmp/out.txt"));
        let span: Vec<&HistoryMessage> = vec![&bash];

        assert_eq!(most_recent_read_path(&span), None);
    }

    #[test]
    fn bounded_restore_body_inlines_full_content_under_cap() {
        let est = |s: &str| (s.chars().count() / 4) as u32;
        let content = "fn main() {\n    println!(\"hi\");\n}\n";

        let body = bounded_restore_body("/a.rs", content, 1000, est);

        assert!(body.contains("/a.rs"), "must name the restored path");
        assert!(
            body.contains(content),
            "must inline the FULL content, not a reference"
        );
        assert!(
            !body.contains("Read \""),
            "must never be a \"Read ... to view\" reference line"
        );
        assert!(!body.contains("to view"));
    }

    #[test]
    fn bounded_restore_body_head_tail_windows_over_cap() {
        let est = |s: &str| (s.chars().count() / 4) as u32;
        let content = "x".repeat(10_000);
        let cap_tokens = 100;

        let body = bounded_restore_body("/big.rs", &content, cap_tokens, est);

        assert!(body.contains("/big.rs"), "must still name the path");
        assert!(
            body.contains("truncated"),
            "must note that content was truncated"
        );
        assert!(!body.contains("Read \""), "never a reference line");
        assert!(!body.contains("to view"));
        let body_tokens = est(&body);
        assert!(
            body_tokens <= cap_tokens as u32 + 50,
            "expected the windowed body to stay roughly within cap ({cap_tokens}) plus small \
             header/note slack, got {body_tokens} estimated tokens"
        );
    }

    // --- parse_extraction_output (SP4's extraction-output decision) ---
    //
    // Every guard standing between the model and the workspace's durable
    // memory set is a pure function of (text, finish_reason), so these tests
    // call it directly: no HTTP, no DB, no async.
    //
    // An earlier cut of this suite stood up a wiremock llama-server per case
    // just to shuttle a canned string into this parser. That was the wrong
    // layer twice over: it mocked an LLM call to test a pure function, and --
    // worse -- what it actually asserted was "IF the model emits a clean fact
    // list, we parse it", i.e. it tested an ASSUMPTION about the model, which
    // is precisely what these guards were written blind against. Whether the
    // real model OBEYS `MEMORY_EXTRACTION_PROMPT` is now covered where only it
    // can be: `tests/real_model_smoke.rs`'s real-llama-server extraction
    // smoke. The wiremock tests kept below are the ones that genuinely test
    // async wiring (transport failure, read-before-call ordering, the CAS
    // window) rather than the parser.

    /// A good response passes through untouched -- the shape check is for
    /// garbage only, and a false rejection silently costs a real fact. Covers
    /// a long fact and a non-ASCII one (the bounds count CHARS, not bytes).
    #[test]
    fn a_well_formed_extraction_passes_intact() {
        let long = "The agent must never let a bad extraction destroy good memories, because \
                    a condensed span is gone forever and cannot be re-extracted later on.";
        let text =
            format!("The user prefers oxfmt over prettier.\n{long}\nユーザーはoxfmtを好みます。");

        assert_eq!(
            parse_extraction_output(&text, Some("stop")),
            ExtractionOutcome::Facts(vec![
                "The user prefers oxfmt over prettier.".to_string(),
                long.to_string(),
                "ユーザーはoxfmtを好みます。".to_string(),
            ])
        );
    }

    /// FINDING A. The prompt asks for the FULL replacement set every time, so
    /// the expected output grows as memories accumulate -- a truncated
    /// response would silently drop every memory not yet re-emitted AND
    /// persist a half-finished sentence as a durable fact. Rejected whole,
    /// even though the text alone would have parsed fine.
    #[test]
    fn a_truncated_extraction_is_rejected_whole() {
        assert_eq!(
            parse_extraction_output(
                "The user prefers oxfmt over prettier.\nBenchmarks are gat",
                Some("length")
            ),
            ExtractionOutcome::Rejected(ExtractionRejection::Truncated)
        );
    }

    /// `ChatOutcome::finish_reason` is a plain `String` whose `""` means "the
    /// server never sent one" -- NOT a truncation. Reading the sentinel as
    /// truncation would reject every extraction from a server that omits the
    /// field, silently disabling memory entirely.
    #[test]
    fn the_empty_finish_reason_sentinel_is_not_a_truncation() {
        let text = "The user prefers oxfmt over prettier.";
        let expected = ExtractionOutcome::Facts(vec![text.to_string()]);

        assert_eq!(parse_extraction_output(text, Some("")), expected);
        assert_eq!(parse_extraction_output(text, None), expected);
    }

    /// THE GUARD, at its own layer: nothing fact-shaped came back, so there is
    /// nothing to persist. `replace_memories` would happily wipe a workspace
    /// with an empty set, so this rejection is what stands in front of it --
    /// see `empty_extraction_never_wipes_existing_memories` for the DB-level
    /// proof that it does.
    #[test]
    fn an_empty_extraction_is_rejected() {
        assert_eq!(
            parse_extraction_output("", Some("stop")),
            ExtractionOutcome::Rejected(ExtractionRejection::Empty)
        );
    }

    #[test]
    fn a_whitespace_only_extraction_is_rejected() {
        assert_eq!(
            parse_extraction_output("   \n  \n", Some("stop")),
            ExtractionOutcome::Rejected(ExtractionRejection::Empty)
        );
    }

    /// The observed 4B failure mode: the model disobeys "no commentary" and
    /// opens with a preamble. Persisted, it would be fed back into the next
    /// pass under "keep the existing ones that are still true", kept, and ride
    /// in `messages[0]` forever with no UI to remove it. The preamble is
    /// dropped; the real facts around it survive (one bad line of three is not
    /// a majority).
    #[test]
    fn a_preamble_line_is_dropped_and_the_real_facts_kept() {
        assert_eq!(
            parse_extraction_output(
                "Here is the updated set of memories:\nThe user prefers oxfmt over prettier.\n\
                 Benchmarks are gated on prompt changes.",
                Some("stop")
            ),
            ExtractionOutcome::Facts(vec![
                "The user prefers oxfmt over prettier.".to_string(),
                "Benchmarks are gated on prompt changes.".to_string(),
            ]),
            "the trailing-colon preamble must never become a durable fact"
        );
    }

    /// A majority of unshaped lines means the model did not do the task -- and
    /// the task is a FULL replacement set. Persisting the surviving minority
    /// would swap the whole workspace set for whatever fragments happened to
    /// pass a syntactic filter. Spec §4 step 4: a parse failure is "no
    /// change". Note the real fact here is NOT returned.
    #[test]
    fn a_majority_unshaped_extraction_is_rejected_whole() {
        // 4 unshaped (3 headers + 1 too-short) vs 1 real fact.
        assert_eq!(
            parse_extraction_output(
                "Here is the updated set of memories:\nUser preferences:\nok\n\
                 The user prefers oxfmt over prettier.\nProject constraints:",
                Some("stop")
            ),
            ExtractionOutcome::Rejected(ExtractionRejection::MajorityUnshaped {
                unshaped: 4,
                total: 5
            })
        );
    }

    /// All-garbage: nothing survives the shape check at all. Reported as a
    /// parse failure rather than `Empty` -- the model emitted plenty, it just
    /// emitted nothing usable.
    #[test]
    fn an_all_unshaped_extraction_is_rejected_as_a_parse_failure() {
        assert_eq!(
            parse_extraction_output("Memories:\nSure:\nok\n1.", Some("stop")),
            ExtractionOutcome::Rejected(ExtractionRejection::MajorityUnshaped {
                unshaped: 4,
                total: 4
            })
        );
    }

    /// The Task-1 review's carried-over MINOR: `replace_memories` does not
    /// dedup its `contents`, so a model that repeats a line verbatim would
    /// otherwise produce duplicate rows.
    #[test]
    fn repeated_facts_are_deduped_preserving_first_seen_order() {
        assert_eq!(
            parse_extraction_output(
                "The alpha fact.\nThe beta fact.\nThe alpha fact.",
                Some("stop")
            ),
            ExtractionOutcome::Facts(vec![
                "The alpha fact.".to_string(),
                "The beta fact.".to_string(),
            ])
        );
    }

    /// A model that disobeys the "no bullets" instruction must still yield the
    /// bare fact -- the defensive `trim_start_matches("- ")` strip.
    #[test]
    fn a_disobedient_bullet_prefix_is_stripped() {
        assert_eq!(
            parse_extraction_output("- The user prefers oxfmt", Some("stop")),
            ExtractionOutcome::Facts(vec!["The user prefers oxfmt".to_string()]),
            "a bulleted line should still yield the bare fact"
        );
    }

    // --- extract_and_persist_memories (SP4's out-of-band extraction pass) ---
    //
    // What is left below is the async/DB wiring the pure tests above cannot
    // reach: a transport failure being swallowed, the read-before-call
    // ordering, the compare-and-swap window, workspace routing, and the
    // DB-level proof that an empty extraction cannot wipe a good set. The stub
    // reuses `inference::http`'s own wiremock SSE harness verbatim, and the DB
    // reuses `storage::test_async_connection`, the same fully-migrated
    // in-memory connection `storage::memories`' tests use.

    /// An SSE body shaped exactly like the ones `inference::http`'s tests
    /// feed `LlamaServerClient::chat`: one content delta, a finish reason,
    /// then `[DONE]`.
    fn sse_text_body_with_finish(text: &str, finish_reason: &str) -> String {
        format!(
            "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
            serde_json::json!({"choices": [{"delta": {"content": text}, "index": 0}]}),
            serde_json::json!({"choices": [{"delta": {}, "finish_reason": finish_reason, "index": 0}]}),
        )
    }

    /// Mounts a 200/SSE stub returning `text` as a complete (`"stop"`)
    /// completion, mirroring `inference::http::tests`' mock setup.
    async fn stub_completion(text: &str) -> wiremock::MockServer {
        let finish_reason = "stop";
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/v1/chat/completions"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_raw(
                        sse_text_body_with_finish(text, finish_reason),
                        "text/event-stream",
                    ),
            )
            .mount(&server)
            .await;
        server
    }

    async fn seed_workspace(conn: &tokio_rusqlite::Connection, id: &str) {
        let id = id.to_string();
        conn.call(move |conn: &mut Connection| {
            conn.execute(
                "INSERT INTO workspaces (id, path, display_name, created_at, last_opened_at) \
                 VALUES (?1, ?1, 'Test workspace', 0, 0)",
                [&id],
            )
        })
        .await
        .unwrap();
    }

    async fn seed_conversation(
        conn: &tokio_rusqlite::Connection,
        id: &str,
        workspace_id: Option<&str>,
    ) {
        let id = id.to_string();
        let workspace_id = workspace_id.map(|s| s.to_string());
        conn.call(move |conn: &mut Connection| {
            conn.execute(
                "INSERT INTO conversations (id, workspace_id, spawned_by_conversation_id, title, created_at, updated_at) \
                 VALUES (?1, ?2, NULL, 'Test', 0, 0)",
                rusqlite::params![&id, &workspace_id],
            )
        })
        .await
        .unwrap();
    }

    async fn contents_of(
        conn: &tokio_rusqlite::Connection,
        workspace_id: Option<&str>,
    ) -> Vec<String> {
        crate::storage::memories::load_memories(conn, workspace_id)
            .await
            .unwrap()
            .into_iter()
            .map(|m| m.content)
            .collect()
    }

    /// The parsed set replaces the prior one wholesale (the write is a
    /// `replace_memories_if_unchanged` swap, not an append) -- the pure tests
    /// above stop at "which facts", this proves what the DB does with them.
    #[tokio::test]
    async fn extraction_replaces_the_prior_set() {
        let conn = crate::storage::test_async_connection().await;
        seed_workspace(&conn, "w1").await;
        seed_conversation(&conn, "c1", Some("w1")).await;
        crate::storage::memories::replace_memories(&conn, Some("w1"), &["old fact".to_string()], 5)
            .await
            .unwrap();
        let server = stub_completion("The new fact replaces it.").await;

        let span = vec![history_message("text", 0, "some work")];
        let span_refs: Vec<&HistoryMessage> = span.iter().collect();
        extract_and_persist_memories(&conn, &server.uri(), "c1", &span_refs, 10)
            .await
            .unwrap();

        assert_eq!(
            contents_of(&conn, Some("w1")).await,
            vec!["The new fact replaces it.".to_string()],
            "the emitted set replaces the prior one wholesale"
        );
    }

    /// THE GUARD, proven against the DB. `an_empty_extraction_is_rejected`
    /// covers the decision purely; this one covers the property that actually
    /// matters -- that the rejection reaches the database, i.e. the rows are
    /// still there afterwards. `replace_memories` with an empty set would
    /// happily wipe the workspace, so nothing but this early return stands
    /// between a degenerate extraction and permanent data loss; it is worth an
    /// end-to-end test even though the pure one exists. Deleting the
    /// `ExtractionRejection::Empty` arm must fail this test.
    #[tokio::test]
    async fn empty_extraction_never_wipes_existing_memories() {
        let conn = crate::storage::test_async_connection().await;
        seed_workspace(&conn, "w1").await;
        seed_conversation(&conn, "c1", Some("w1")).await;
        crate::storage::memories::replace_memories(&conn, Some("w1"), &["precious".to_string()], 5)
            .await
            .unwrap();
        let server = stub_completion("   \n  \n").await;

        let span = vec![history_message("text", 0, "some work")];
        let span_refs: Vec<&HistoryMessage> = span.iter().collect();
        extract_and_persist_memories(&conn, &server.uri(), "c1", &span_refs, 10)
            .await
            .unwrap();

        assert_eq!(
            contents_of(&conn, Some("w1")).await,
            vec!["precious".to_string()],
            "an empty extraction must never destroy existing memories"
        );
    }

    #[tokio::test]
    async fn extraction_error_is_swallowed_and_changes_nothing() {
        let conn = crate::storage::test_async_connection().await;
        seed_workspace(&conn, "w1").await;
        seed_conversation(&conn, "c1", Some("w1")).await;
        crate::storage::memories::replace_memories(&conn, Some("w1"), &["precious".to_string()], 5)
            .await
            .unwrap();
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/v1/chat/completions"))
            .respond_with(wiremock::ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let span = vec![history_message("text", 0, "some work")];
        let span_refs: Vec<&HistoryMessage> = span.iter().collect();
        let r = extract_and_persist_memories(&conn, &server.uri(), "c1", &span_refs, 10).await;

        assert!(r.is_ok(), "a server error must never fail the turn");
        assert_eq!(
            contents_of(&conn, Some("w1")).await,
            vec!["precious".to_string()],
            "a failed extraction must leave memories untouched"
        );
    }

    #[tokio::test]
    async fn extraction_writes_under_the_conversations_workspace() {
        let conn = crate::storage::test_async_connection().await;
        seed_workspace(&conn, "w1").await;
        seed_conversation(&conn, "c1", Some("w1")).await;
        let server = stub_completion("A durable fact.").await;

        let span = vec![history_message("text", 0, "some work")];
        let span_refs: Vec<&HistoryMessage> = span.iter().collect();
        extract_and_persist_memories(&conn, &server.uri(), "c1", &span_refs, 10)
            .await
            .unwrap();

        assert_eq!(
            contents_of(&conn, Some("w1")).await,
            vec!["A durable fact.".to_string()]
        );
        assert!(
            contents_of(&conn, None).await.is_empty(),
            "the NULL bucket must stay isolated from a workspace's memories"
        );
    }

    /// FINDING B. A transient failure reading the existing memory set must
    /// NOT be treated as "no existing memories" -- that would let a set built
    /// in ignorance of the real memories wipe them via `replace_memories`.
    /// Induced deterministically by dropping the `memories` table out from
    /// under an otherwise-healthy connection: `workspace_id_for_conversation`
    /// (reads `conversations`) still succeeds, but `load_memories` itself now
    /// returns `Err`. The assertion that the mock server received zero
    /// requests proves the function bailed out BEFORE ever calling the
    /// model -- it never got the chance to extract from ignorance of the
    /// real set.
    #[tokio::test]
    async fn load_failure_leaves_memories_untouched_and_never_calls_the_model() {
        let conn = crate::storage::test_async_connection().await;
        seed_workspace(&conn, "w1").await;
        seed_conversation(&conn, "c1", Some("w1")).await;
        let server = stub_completion("should never be requested.").await;

        conn.call(|conn: &mut Connection| conn.execute("DROP TABLE memories", []))
            .await
            .unwrap();

        let span = vec![history_message("text", 0, "some work")];
        let span_refs: Vec<&HistoryMessage> = span.iter().collect();
        let r = extract_and_persist_memories(&conn, &server.uri(), "c1", &span_refs, 10).await;

        assert!(r.is_ok(), "a read failure must never fail the turn");
        assert_eq!(
            server.received_requests().await.unwrap().len(),
            0,
            "a load_memories failure must bail out before ever calling the model"
        );
    }

    /// IMPORTANT 1, end to end: proves `extract_and_persist_memories` actually
    /// routes through the CAS with the set IT read as the expectation --
    /// `storage::memories`' own tests cover the CAS semantics, this covers the
    /// wiring.
    ///
    /// The real lost-update window is "read, spend seconds in the LLM
    /// round-trip, write", so the window is reproduced literally: the stub
    /// server delays its response, the extraction runs as a concurrent task,
    /// and a sibling conversation commits `[X, gated]` while that task is
    /// parked in the HTTP call -- provably after its `load_memories` (which
    /// precedes the request) and before its write (which follows the
    /// response). The 500ms/50ms margin is what makes the interleaving
    /// deterministic rather than lucky; without the CAS this pass would write
    /// `[X, oxfmt]` and "gated" would be gone forever.
    #[tokio::test]
    async fn extraction_skips_the_write_when_a_sibling_changed_the_set_mid_flight() {
        let conn = crate::storage::test_async_connection().await;
        seed_workspace(&conn, "w1").await;
        seed_conversation(&conn, "c1", Some("w1")).await;
        crate::storage::memories::replace_memories(&conn, Some("w1"), &["X".to_string()], 5)
            .await
            .unwrap();

        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/v1/chat/completions"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_raw(
                        sse_text_body_with_finish("X\nThe user prefers oxfmt.", "stop"),
                        "text/event-stream",
                    )
                    .set_delay(std::time::Duration::from_millis(500)),
            )
            .mount(&server)
            .await;

        let uri = server.uri();
        let extraction_conn = conn.clone();
        let extraction = tokio::spawn(async move {
            let span = vec![history_message("text", 0, "some work")];
            let span_refs: Vec<&HistoryMessage> = span.iter().collect();
            extract_and_persist_memories(&extraction_conn, &uri, "c1", &span_refs, 20).await
        });

        // The sibling compacts and commits while the extraction above is parked
        // in its (delayed) round-trip.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        crate::storage::memories::replace_memories(
            &conn,
            Some("w1"),
            &["X".to_string(), "gated".to_string()],
            10,
        )
        .await
        .unwrap();

        let r = extraction.await.unwrap();
        assert!(r.is_ok(), "a skipped write must never fail the turn");

        let mut got = contents_of(&conn, Some("w1")).await;
        got.sort();
        assert_eq!(
            got,
            vec!["X".to_string(), "gated".to_string()],
            "the sibling's 'gated' must survive; the stale pass must not have written 'oxfmt'"
        );
    }

    // --- The anti-prefill invariant, pinned on the REQUEST BODY ---
    //
    // Both compaction calls once built `[system(PROMPT)] + span` and appended
    // nothing. A span is an arbitrary slice of history and routinely ENDS ON AN
    // ASSISTANT MESSAGE, which llama-server's chat template treats as a prefill
    // to CONTINUE -- so the model closed out that sentence instead of
    // summarizing/extracting, and the echo (non-empty, un-truncated, and smaller
    // than the span it replaced) passed every guard `evaluate_summary` /
    // `parse_extraction_output` apply. Tier-2 compaction ACCEPTED garbage and
    // silently corrupted conversation state while reporting success.
    //
    // The fix is one line at each site (`messages.push(ChatMessage::user(
    // limits::SUMMARIZATION_FINAL_TURN))` and its `EXTRACTION_FINAL_TURN` twin),
    // and deleting BOTH left the whole lib suite green. The DB-outcome tests
    // above cannot see it: a stub's canned reply is independent of the request
    // that asked for it, so no assertion about which facts landed can observe
    // the shape that was sent. The REQUEST is the only place the hazard is
    // visible, so these two tests read it back off the wire (`received_requests`,
    // the same idiom `load_failure_leaves_memories_untouched_and_never_calls_the_model`
    // already uses) and assert the shape production actually builds.
    //
    // Every expected value is REFERENCED from the production const it pins,
    // never copied: a test carrying its own transcription of a prompt keeps
    // passing after production's prompt moves on, which is exactly how
    // `tests/real_model_smoke.rs` came to pin a prompt retired months earlier.

    /// Every request body the stub server received, in order.
    async fn request_bodies(server: &wiremock::MockServer) -> Vec<serde_json::Value> {
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .map(|r| serde_json::from_slice(&r.body).unwrap())
            .collect()
    }

    /// The invariants BOTH `Forbid`-mode compaction calls share, asserted
    /// against the serialized wire body rather than the `ChatRequest` struct --
    /// a field that never reaches the server is the silent no-op these switches
    /// cannot afford to be.
    fn assert_forbid_mode_compaction_shape(body: &serde_json::Value, what: &str) {
        assert_eq!(
            body["max_tokens"], SUMMARY_MAX_TOKENS,
            "{what} must cap output at the flat SUMMARY_MAX_TOKENS production sets"
        );
        assert_eq!(
            body["chat_template_kwargs"]["enable_thinking"], false,
            "{what} must disable thinking: the reasoning block was measured consuming \
             this call's ENTIRE budget, leaving empty content and finish_reason:\"length\""
        );
        assert!(
            body.get("tools").is_none(),
            "{what} is a Forbid-mode call: `tools` must be absent so a compaction \
             can never emit a tool call"
        );
        assert!(
            body.get("tool_choice").is_none(),
            "{what} is a Forbid-mode call: `tool_choice` must be absent"
        );
    }

    #[tokio::test]
    async fn the_summarization_request_ends_on_a_user_turn_and_carries_the_production_prompt() {
        let conn = crate::storage::test_async_connection().await;
        seed_workspace(&conn, "w1").await;
        seed_conversation(&conn, "c1", Some("w1")).await;
        let server = stub_completion("<state_snapshot>\nGOAL: fix login\n</state_snapshot>").await;

        // The hazard's exact shape. `messages_to_summarize` drops the first
        // genuine user message (index 0, keep-first) and the `protected_recent`
        // tail (index 3), leaving [assistant(1), assistant(2)].
        let history = vec![
            history_message("text", 0, "The login page throws a 500. Fix it."),
            assistant_history_message(1, "Found it: an unwrap() in the handler."),
            assistant_history_message(2, "Fixed -- all 128 tests pass."),
            history_message("text", 3, "Great. Now add a rate limiter."),
        ];
        let protected_recent = 1;
        // Guard the FIXTURE: without a span that ends on an assistant message
        // this test would still pass while no longer testing the bug it exists
        // for.
        assert_eq!(
            messages_to_summarize(&history, protected_recent)
                .last()
                .unwrap()
                .chat
                .role,
            "assistant",
            "fixture is wrong: this only pins the prefill hazard while the span \
             ENDS on an assistant message"
        );

        summarize_and_persist(&conn, None, &server.uri(), "c1", &history, protected_recent)
            .await
            .unwrap();

        // An ACCEPTED summary chains straight into the out-of-band memory
        // extraction (`summarize_and_persist`'s `Accept` arm awaits
        // `extract_and_persist_memories`), so the same stub sees two calls. The
        // summarization is the first; its twin below covers the second.
        let bodies = request_bodies(&server).await;
        assert_eq!(
            bodies.len(),
            2,
            "an accepted summary must make exactly two calls: the summarization, then \
             the chained extraction"
        );
        let body = &bodies[0];
        let messages = body["messages"].as_array().unwrap();

        // THE ANTI-PREFILL INVARIANT. Deleting `summarize_and_persist`'s
        // `messages.push(ChatMessage::user(limits::SUMMARIZATION_FINAL_TURN))`
        // must fail HERE -- the span's own last message is an assistant turn, so
        // without that push the request ends on one and the model continues it.
        let last = messages.last().unwrap();
        assert_eq!(
            last["role"], "user",
            "the summarization request must NEVER end on an assistant message -- the \
             chat template reads a trailing assistant turn as a prefill to CONTINUE, and \
             the resulting echo is accepted as a summary. Got: {last}"
        );
        assert_eq!(
            last["content"],
            limits::SUMMARIZATION_FINAL_TURN,
            "the closing user turn must be SUMMARIZATION_FINAL_TURN -- what makes the \
             system prompt the thing being answered"
        );

        assert_eq!(messages[0]["role"], "system");
        assert_eq!(
            messages[0]["content"], SUMMARIZATION_PROMPT,
            "the system message must be the production const itself, not a copy"
        );

        // The span sits between the system prompt and the closing user turn,
        // exactly as `messages_to_summarize` selected it.
        let span: Vec<&str> = messages[1..messages.len() - 1]
            .iter()
            .map(|m| m["content"].as_str().unwrap())
            .collect();
        assert_eq!(
            span,
            vec![
                "Found it: an unwrap() in the handler.",
                "Fixed -- all 128 tests pass."
            ],
            "the request must carry `messages_to_summarize`'s span: the first genuine \
             user message and the protected-recent tail are both excluded"
        );

        assert_forbid_mode_compaction_shape(body, "the summarization call");
    }

    #[tokio::test]
    async fn the_extraction_request_ends_on_a_user_turn_and_carries_the_production_prompt() {
        let conn = crate::storage::test_async_connection().await;
        seed_workspace(&conn, "w1").await;
        seed_conversation(&conn, "c1", Some("w1")).await;
        let server = stub_completion("The user prefers oxfmt.").await;

        // The hazard's exact shape: the span handed to the extraction ends on an
        // assistant message.
        let span = vec![
            history_message("text", 0, "Format the project."),
            assistant_history_message(1, "Done -- the repo formats with oxfmt, not prettier."),
        ];
        let span_refs: Vec<&HistoryMessage> = span.iter().collect();
        // Guard the FIXTURE, as the summarization twin above does.
        assert_eq!(
            span_refs.last().unwrap().chat.role,
            "assistant",
            "fixture is wrong: this only pins the prefill hazard while the span \
             ENDS on an assistant message"
        );

        extract_and_persist_memories(&conn, &server.uri(), "c1", &span_refs, 10)
            .await
            .unwrap();

        let bodies = request_bodies(&server).await;
        assert_eq!(
            bodies.len(),
            1,
            "the extraction is exactly one round-trip, never a retry loop"
        );
        let body = &bodies[0];
        let messages = body["messages"].as_array().unwrap();

        // THE ANTI-PREFILL INVARIANT, extraction side. Deleting
        // `extract_and_persist_memories`'s
        // `messages.push(ChatMessage::user(limits::EXTRACTION_FINAL_TURN))`
        // must fail HERE.
        let last = messages.last().unwrap();
        assert_eq!(
            last["role"], "user",
            "the extraction request must NEVER end on an assistant message -- that \
             trailing-assistant prefill had this call echoing the span's last message \
             back as a durable \"memory\". Got: {last}"
        );
        assert_eq!(
            last["content"],
            limits::EXTRACTION_FINAL_TURN,
            "the closing user turn must be EXTRACTION_FINAL_TURN"
        );

        assert_eq!(messages[0]["role"], "system");
        assert_eq!(
            messages[0]["content"],
            limits::MEMORY_EXTRACTION_PROMPT,
            "the system message must be the production const itself, not a copy"
        );
        // The existing-memory block is a user turn immediately after the system
        // prompt -- the model cannot update a set it was never shown.
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(
            messages[1]["content"],
            "Existing memories:\n(no existing memories)"
        );

        assert_forbid_mode_compaction_shape(body, "the extraction call");
    }
}
