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

/// Beyond this many plan-marked tool rows (`detail.plan == true` — the
/// five plan-machine tools plus state-gated rejections, see
/// `commands::agent::persist_plan_tool`), the oldest are cleared by tier 1
/// -- independent of, and far stricter than, `TOOL_KEEP_N`. A plan row
/// only ever echoes state the always-regenerated system/state prompt
/// already carries in full, so keeping more than a couple of them around
/// just spends window on nothing new.
pub const PLAN_KEEP_N: usize = 2;

/// Tier-1 placeholder for a cleared tool row whose full output was
/// previously offloaded to disk (`context::offload::offload_if_oversized`
/// stamps the resulting path into the row's persisted `detail.offloadedTo`
/// — see `commands::agent::execute_tool`). Unlike `TOOL_CLEARED_PLACEHOLDER`,
/// clearing here is restorable: the model can `Read` the path back into
/// context if it turns out to still need this result, rather than the
/// row's content being gone for good.
pub fn tool_cleared_placeholder_with_pointer(offload_path: &str) -> String {
    format!("[Old tool result cleared; full output saved at {offload_path} — Read it to recover]")
}

/// Messages tier 2 never summarizes away, regardless of how far back it
/// would otherwise reach (research.md).
pub const PROTECTED_RECENT_MESSAGES: usize = 10;

/// Max output tokens for the tier-2 summarization completion itself --
/// 1/16 of `CONTEXT_WINDOW_TOKENS`.
pub const SUMMARY_MAX_TOKENS: i32 = (CONTEXT_WINDOW_TOKENS / 16) as i32;

pub const SUMMARIZATION_PROMPT: &str =
    "Summarize the conversation so far concisely, preserving key facts, decisions, and unresolved tasks. Respond with only the summary text, nothing else.";

pub const DEFAULT_WARN_THRESHOLD_PCT: f64 = 0.5;
pub const DEFAULT_COMPACT_THRESHOLD_PCT: f64 = 0.75;
pub const DEFAULT_HARD_LIMIT_PCT: f64 = 0.9;

/// ~3% of `CONTEXT_WINDOW_TOKENS` (2000 chars ~= 500 tokens). A single tool
/// result over this threshold gets offloaded to disk with only a preview +
/// pointer left inline.
pub const DEFAULT_TOOL_OUTPUT_OFFLOAD_CHARS: usize = 2000;

/// `reserve` for `InferenceEngine::fit_to_context`'s per-turn call inside
/// `agent::run_loop` (`context::fit_turn_to_budget`) -- and now also the
/// literal `max_tokens` the agent `generate()` call sites pass (they
/// reference this constant rather than duplicating the number).
///
/// Raised from 256 (~3.1% of the 8192 window) to 1024 (~6.2% of
/// `CONTEXT_WINDOW_TOKENS`) after a real benchmark failure: a well-granulated
/// 20-step `CreatePlan` call needs more than 256 output tokens, so generation
/// was cut off mid-JSON before the closing `</tool_call>` tag and the truncated
/// call silently became the turn's "final answer" (tier4_planned scored 0/20 at
/// turn 2). The grammar guarantees a tool call ends at its closing tag and EOG
/// ends short answers early, so the extra headroom costs nothing on turns that
/// don't need it.
pub const AGENT_TURN_MAX_OUTPUT_TOKENS: u32 = 1024;

/// Headroom for the per-turn state tail (`agent::plan::PlanState::state_tail`)
/// -- ~4.7% (3/64) of `CONTEXT_WINDOW_TOKENS`. Every plan host pushes the
/// tail AFTER `run_loop`'s measure/threshold check and after
/// `fit_turn_to_budget` have already run, so neither ever sees it; without
/// this reserve a history parked just under the threshold plus a big tail
/// (mode banner + Executing goal/step frame + optional refusal reason +
/// the recitation checklist -- realistically 400-700 tokens on a 20-step
/// plan) rendered past `n_ctx`, `ctx.decode` failed mid-stream, and the
/// whole task silently ended with "Error: inference failed" as its final
/// answer. Sized above the observed worst case so the envelope
/// `fitted history + tail + AGENT_TURN_MAX_OUTPUT_TOKENS <=
/// CONTEXT_WINDOW_TOKENS` holds. Subtracted wherever a turn budget is
/// derived: the plan hosts' `threshold` computations and
/// `fit_turn_to_budget`'s reserve.
pub const STATE_TAIL_RESERVE_TOKENS: u32 = 768;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn budget_constants_stay_proportional_to_the_window() {
        assert_eq!(SUMMARY_MAX_TOKENS, (CONTEXT_WINDOW_TOKENS / 16) as i32);
        assert!(DEFAULT_TOOL_OUTPUT_OFFLOAD_CHARS >= 1500);
        assert!(AGENT_TURN_MAX_OUTPUT_TOKENS >= CONTEXT_WINDOW_TOKENS / 16);
        // The tail reserve must comfortably cover a big plan's tail
        // (hundreds of tokens) without the combined per-turn reserve
        // eating a meaningful slice of the window.
        assert!(STATE_TAIL_RESERVE_TOKENS >= CONTEXT_WINDOW_TOKENS / 32);
        assert!(STATE_TAIL_RESERVE_TOKENS + AGENT_TURN_MAX_OUTPUT_TOKENS <= CONTEXT_WINDOW_TOKENS / 8);
    }
}
