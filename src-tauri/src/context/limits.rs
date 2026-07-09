//! Every tunable number that governs context-window behavior, gathered in
//! one place instead of scattered across `inference::mod` and
//! `context::mod` (found worth doing after repeatedly having to hunt down
//! and re-derive which constants were sized relative to which budget while
//! diagnosing why usage climbed past 100% -- see this module's own
//! `mod.rs` history). `CONTEXT_WINDOW_TOKENS` is the one true anchor;
//! every other constant here is commented with what fraction of it that
//! constant represents, so raising the window makes it obvious which of
//! these are worth reconsidering too.

/// The model's real, authoritative context window -- defined in
/// `inference::mod` (where `with_n_ctx` actually consumes it) and
/// re-exported here so every other budget-relative constant sits
/// alongside the number it's relative to.
pub use crate::inference::CONTEXT_WINDOW_TOKENS;

/// Beyond this many tool_call/tool_result messages, the oldest are cleared
/// first by tier 1 (research.md's threshold-defaults decision).
pub const TOOL_KEEP_N: usize = 2;

pub const TOOL_CLEARED_PLACEHOLDER: &str = "[Old tool result cleared to save context space]";

/// Messages tier 2 never summarizes away, regardless of how far back it
/// would otherwise reach (research.md).
pub const PROTECTED_RECENT_MESSAGES: usize = 10;

/// Max output tokens for the tier-2 summarization completion itself --
/// ~12.5% of `CONTEXT_WINDOW_TOKENS` at the original 2048-token sizing.
pub const SUMMARY_MAX_TOKENS: i32 = 256;

pub const SUMMARIZATION_PROMPT: &str =
    "Summarize the conversation so far concisely, preserving key facts, decisions, and unresolved tasks. Respond with only the summary text, nothing else.";

pub const DEFAULT_WARN_THRESHOLD_PCT: f64 = 0.5;
pub const DEFAULT_COMPACT_THRESHOLD_PCT: f64 = 0.75;
pub const DEFAULT_HARD_LIMIT_PCT: f64 = 0.9;

/// ~24% of `CONTEXT_WINDOW_TOKENS` at the original 2048-token sizing (500
/// chars ~= 125 tokens). A single tool result over this threshold gets
/// offloaded to disk with only a preview + pointer left inline.
pub const DEFAULT_TOOL_OUTPUT_OFFLOAD_CHARS: usize = 500;

/// `reserve` for `InferenceEngine::fit_to_context`'s per-turn call inside
/// `agent::run_loop` (`context::fit_turn_to_budget`) -- and now also the
/// literal `max_tokens` the agent `generate()` call sites pass (they
/// reference this constant rather than duplicating the number).
///
/// Raised from 256 (~3% of the 8192 window) to 1024 (~12.5%) after a real
/// benchmark failure: a well-granulated 20-step `CreatePlan` call needs
/// more than 256 output tokens, so generation was cut off mid-JSON before
/// the closing `</tool_call>` tag and the truncated call silently became
/// the turn's "final answer" (tier4_planned scored 0/20 at turn 2). The
/// grammar guarantees a tool call ends at its closing tag and EOG ends
/// short answers early, so the extra headroom costs nothing on turns that
/// don't need it.
pub const AGENT_TURN_MAX_OUTPUT_TOKENS: u32 = 1024;
