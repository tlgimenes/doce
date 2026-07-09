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

/// Tier-1 placeholder for a cleared tool row whose result names a
/// `Read`-able path — either a payload file every staged result now writes
/// (`context::payload::stage_tool_result`, stamped into the row's persisted
/// `detail.payloadRef`) or, for a `Read` row, the source file itself (see
/// `commands::agent::handle_general_tool_call`'s `Read` carve-out). Unlike
/// `TOOL_CLEARED_PLACEHOLDER`, clearing here is restorable: the model can
/// `Read` the path back into context if it turns out to still need this
/// result, rather than the row's content being gone for good. Deliberately
/// says nothing about the file already containing "the full output" —
/// unlike the offload-era version of this placeholder, that promise no
/// longer holds for a `Read` row, whose `payloadRef` points at the
/// original (possibly much larger) source file, not a copy of what the
/// model saw.
pub fn tool_cleared_placeholder_with_pointer(payload_ref: &str) -> String {
    format!("[Old tool result cleared; recover with Read \"{payload_ref}\"]")
}

/// Tier-1 placeholder for a cleared row with no payload file of its own
/// (a `Task`/plan/`AskUserQuestion` row, or a legacy row persisted before
/// payload staging existed): none of those have a `payload_ref` to point
/// `Read` at, so the conversation's own materialized transcript
/// (`context::transcript`) is the recovery route instead -- every row,
/// staged or not, always has an entry there. `seq` is the row's own
/// `HistoryMessage.sequence`, matching the `[#{seq} ...]` header
/// `transcript::render_entry` gives that same row, so the model can find
/// the exact entry rather than having to search the whole file.
pub fn tool_cleared_placeholder_transcript(transcript_path: &str, seq: i64) -> String {
    format!(
        "[Old tool result cleared; see entry #{seq} in the transcript at \"{transcript_path}\" — Read it to recover]"
    )
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

/// 1/16 of `CONTEXT_WINDOW_TOKENS` (= 1024 at the 16K window). A tool result whose
/// model-facing text costs at most this many tokens is inlined whole;
/// anything larger becomes a status reference line pointing at its payload
/// file (2026-07-09 payload-files design, `context::payload::stage_tool_result`).
pub const DEFAULT_TOOL_OUTPUT_OFFLOAD_TOKENS: usize = (CONTEXT_WINDOW_TOKENS / 16) as usize;

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
/// rendered past `n_ctx`, `ctx.decode` failed mid-stream, and the whole
/// task silently ended with "Error: inference failed" as its final answer.
/// Since Task 14, the Executing tail (mode banner + goal/step frame +
/// optional refusal reason + the clamped recitation window) is bounded to
/// roughly six recitation lines plus the goal/current-step frame
/// regardless of how large the plan grows -- only Planning's recitation
/// still renders every step and so still scales with plan length, which
/// this reserve must continue to cover (realistically 400-700 tokens on a
/// 20-step plan). Sized above that observed worst case so the envelope
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
        assert_eq!(
            DEFAULT_TOOL_OUTPUT_OFFLOAD_TOKENS,
            (CONTEXT_WINDOW_TOKENS / 16) as usize
        );
        assert!(AGENT_TURN_MAX_OUTPUT_TOKENS >= CONTEXT_WINDOW_TOKENS / 16);
        // The tail reserve must comfortably cover a big plan's tail
        // (hundreds of tokens) without the combined per-turn reserve
        // eating a meaningful slice of the window.
        assert!(STATE_TAIL_RESERVE_TOKENS >= CONTEXT_WINDOW_TOKENS / 32);
        assert!(
            STATE_TAIL_RESERVE_TOKENS + AGENT_TURN_MAX_OUTPUT_TOKENS <= CONTEXT_WINDOW_TOKENS / 8
        );
    }
}
