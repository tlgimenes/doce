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
//! pass against the real app instead. Where a real function needs a pure
//! core unit-tested on its own, that core is split out (`fit_turn_to_budget`
//! /`fit_to_budget`; `summarize_and_persist`/`messages_to_summarize`), the
//! same way each time.

pub mod limits;
pub mod payload;
pub mod transcript;

use crate::agent::dispatch::ToolOutcome;
use crate::inference::{ChatMessage, InferenceEngine, MessageContent};
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

    let rendered = engine
        .render_chat_prompt(&messages)
        .map_err(|e| e.to_string())?;
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
    // (not just inside maybe_compact) is what makes this
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
pub fn apply_lightweight_clearing(history: &mut [HistoryMessage], keep_n: usize) -> usize {
    let tool_rows: Vec<(usize, bool, Option<String>)> = history
        .iter()
        .enumerate()
        .filter(|(_, m)| m.content_type == "tool_call" || m.content_type == "tool_result")
        .map(|(i, m)| (i, m.plan, m.payload_ref.clone()))
        .collect();

    let plan_indices: Vec<usize> = tool_rows
        .iter()
        .filter(|(_, plan, _)| *plan)
        .map(|(i, _, _)| *i)
        .collect();
    let plan_to_clear: &[usize] = if plan_indices.len() > limits::PLAN_KEEP_N {
        &plan_indices[..plan_indices.len() - limits::PLAN_KEEP_N]
    } else {
        &[]
    };

    let regular_indices: Vec<usize> = tool_rows
        .iter()
        .filter(|(_, plan, _)| !*plan)
        .map(|(i, _, _)| *i)
        .collect();
    let regular_to_clear: &[usize] = if regular_indices.len() > keep_n {
        &regular_indices[..regular_indices.len() - keep_n]
    } else {
        &[]
    };

    let mut cleared = 0;
    for (i, _, payload_ref) in &tool_rows {
        if plan_to_clear.contains(i) || regular_to_clear.contains(i) {
            let placeholder = match payload_ref {
                Some(path) => limits::tool_cleared_placeholder_with_pointer(path),
                None => TOOL_CLEARED_PLACEHOLDER.to_string(),
            };
            history[*i].chat.content = MessageContent::Text(placeholder);
            cleared += 1;
        }
    }
    cleared
}

/// True for a `HistoryMessage` that is a genuine user-authored turn (a
/// `text`/`rich_text` row with role `"user"`) — deliberately distinct from
/// a `tool_result` row, which also reconstructs with `chat.role == "user"`
/// (see `ChatMessage::tool_result`'s own doc comment) but is never "the
/// task statement".
fn is_genuine_user_message(message: &HistoryMessage) -> bool {
    message.chat.role == "user"
        && (message.content_type == "text" || message.content_type == "rich_text")
}

/// The pure span-selection logic behind tier 2: everything in `history`
/// except the most recent `protected_recent` messages, *and* except the
/// first genuine user message (the task statement) — OpenHands' "keep-
/// first" behavior, applied here because a summarization pass is
/// generative, lossy compression by a small model, the riskiest single
/// step in the whole compaction pipeline; it must never be the thing that
/// makes the model forget what it was asked to do. Split out from
/// `summarize_and_persist` (which needs a real `InferenceEngine` — this
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

/// Tier 2: summarizes everything except the most recent `protected_recent`
/// messages and the first user message (`messages_to_summarize`) via a real
/// `generate()` call against the same loaded model, and persists the result
/// as a `context_notice` row (`kind:"summarized"`) that
/// `load_history_annotated` will splice in on every subsequent load.
/// Returns `Ok(None)` (no-op) when there's nothing eligible to summarize.
pub async fn summarize_and_persist(
    conn: &tokio_rusqlite::Connection,
    transcript_dir: Option<std::path::PathBuf>,
    engine: &InferenceEngine,
    conversation_id: &str,
    history: &[HistoryMessage],
    protected_recent: usize,
) -> Result<Option<String>, String> {
    let to_summarize = messages_to_summarize(history, protected_recent);
    if to_summarize.is_empty() {
        return Ok(None);
    }

    let mut messages = vec![ChatMessage::system(SUMMARIZATION_PROMPT)];
    messages.extend(to_summarize.iter().map(|m| m.chat.clone()));

    let rendered = engine
        .render_chat_prompt(&messages)
        .map_err(|e| e.to_string())?;
    let summary = engine
        .generate(
            &rendered,
            SUMMARY_MAX_TOKENS,
            crate::inference::ToolCallMode::Forbid,
            None,
            |_| {},
            || false,
        )
        .map_err(|e| e.to_string())?;

    let notice_json = serde_json::json!({
        "kind": "summarized",
        "summary": summary,
        "notice": "Conversation condensed to save space",
    })
    .to_string();
    persist_notice(conn, transcript_dir, conversation_id, notice_json).await?;

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
    transcript_dir: Option<std::path::PathBuf>,
    engine: &InferenceEngine,
    conversation_id: &str,
    skills_dir: &Path,
    system_prompt: &str,
    force: bool,
) -> Result<ContextUsage, String> {
    let settings = ContextSettings::load(conn).await?;
    let mut history = load_history_via_conn(conn, conversation_id, skills_dir).await?;
    let mut usage =
        usage_from_history(engine, conversation_id, &history, system_prompt, &settings).await?;

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
        persist_notice(conn, transcript_dir.clone(), conversation_id, notice_json).await?;
        usage =
            usage_from_history(engine, conversation_id, &history, system_prompt, &settings).await?;
    }

    if over_compact_threshold(&usage) {
        let summarized = summarize_and_persist(
            conn,
            transcript_dir.clone(),
            engine,
            conversation_id,
            &history,
            PROTECTED_RECENT_MESSAGES,
        )
        .await?;
        if summarized.is_some() {
            changed = true;
            // summarize_and_persist just persisted a new context_notice row
            // that changes load_history_annotated's splice point -- reload
            // rather than trying to reconstruct the spliced view in memory.
            history = load_history_via_conn(conn, conversation_id, skills_dir).await?;
            usage = usage_from_history(engine, conversation_id, &history, system_prompt, &settings)
                .await?;
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
/// from the real-model agent benchmark (`tests/agent_benchmark.rs`)
/// instead of the benchmark reimplementing its own version of this step.
pub fn fit_turn_to_budget(
    engine: &InferenceEngine,
    messages: &[ChatMessage],
) -> Result<Vec<ChatMessage>, String> {
    // The reserve covers BOTH per-turn costs `messages` doesn't contain
    // yet: the output tokens generation may still produce, and the plan
    // hosts' state-tail message (`PlanState::state_tail`), which every
    // plan backend pushes AFTER this fit has already run -- see
    // `limits::STATE_TAIL_RESERVE_TOKENS` for the overflow this closes.
    engine
        .fit_to_context(
            messages,
            1,
            limits::AGENT_TURN_MAX_OUTPUT_TOKENS + limits::STATE_TAIL_RESERVE_TOKENS,
        )
        .map_err(|e| e.to_string())
}

/// `ContextUsage` for an already-fully-assembled message list (system
/// prompt included as the first element, e.g. `fit_turn_to_budget`'s own
/// output) — unlike `usage_from_chat_messages`/`usage_from_history`, does
/// not prepend a second system message, since this one's already there.
pub fn usage_from_fitted_messages(
    engine: &InferenceEngine,
    conversation_id: &str,
    messages: &[ChatMessage],
    settings: &ContextSettings,
) -> Result<ContextUsage, String> {
    let rendered = engine
        .render_chat_prompt(messages)
        .map_err(|e| e.to_string())?;
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

/// Annotates a tool result with its real token cost — the same tokenizer
/// `fit_to_budget`/the context usage gauge already use, not a client-side
/// estimate, since the whole point is that this number has to match the
/// real budget math. Applied only to the four tool results whose size
/// varies enough to matter (`wants_token_count`); every other tool's
/// `detail` passes through unchanged. Called right after
/// `dispatch::execute()` returns, before persistence, from every call site
/// that already holds an `&InferenceEngine` for this exact reason
/// (`context::fit_turn_to_budget`). A tokenization failure leaves `detail`
/// unannotated rather than failing the whole tool result over a
/// UI-only concern.
pub fn annotate_with_token_count(engine: &InferenceEngine, outcome: ToolOutcome) -> ToolOutcome {
    let tool_name = outcome
        .detail
        .get("toolName")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !wants_token_count(tool_name) {
        return outcome;
    }
    let Ok(token_count) = engine.count_tokens(&outcome.model_text) else {
        return outcome;
    };
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

        let cleared = apply_lightweight_clearing(&mut history, TOOL_KEEP_N);
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

        let cleared = apply_lightweight_clearing(&mut history, TOOL_KEEP_N);
        assert_eq!(cleared, 1);
        assert_eq!(history[0].chat.text(), TOOL_CLEARED_PLACEHOLDER);
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
        let cleared = apply_lightweight_clearing(&mut history, 10);
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
        let cleared = apply_lightweight_clearing(&mut history, TOOL_KEEP_N);
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

        let cleared = apply_lightweight_clearing(&mut history, TOOL_KEEP_N);

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
    // below; fit_turn_to_budget itself needs a real InferenceEngine, so
    // per this file's own testability note at the top, it's exercised by
    // the real-model agent benchmark instead, not a unit test here).

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
    // `summarize_and_persist` itself needs a real `InferenceEngine` (this
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
}
