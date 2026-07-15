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

/// Max output tokens for the two `Forbid`-mode compaction calls --
/// `context::summarize_and_persist` and `context::extract_and_persist_memories`
/// -- both of which set this as their request's literal `max_tokens`. A flat
/// cap, not `clamp_output_tokens`: these are `Forbid`-mode calls over prompts
/// that carry their own explicit self-caps (~800 tokens each), not agent turns
/// sized against the live window.
///
/// RAISED 1024 -> 2048 (1/16 -> 1/8 of `CONTEXT_WINDOW_TOKENS`) by the
/// 2026-07-15 real-model pass, the first time either call was ever run against
/// a real Qwen3.5-4B rather than an HTTP stub. Qwen3.5 is a thinking model:
/// with `enable_thinking` on it emits a reasoning block BEFORE any content, and
/// the block is both large and highly variable (688 chars on a summarization
/// span, 3789 on an extraction span, ~8180 observed on another). At 1024 that
/// reasoning consumed the ENTIRE budget on extraction -- measured: empty
/// content, `finish_reason:"length"` -- so the truncation guard correctly
/// rejected everything and nothing ever persisted.
///
/// The real fix for that is `ChatRequest::disable_thinking` (these two calls
/// suppress reasoning outright rather than trying to out-budget it -- see its
/// doc comment), which drops `reasoning_len` to 0 and leaves the whole budget
/// for content. This cap is therefore sized for CONTENT ALONE: both prompts
/// self-cap at ~800 tokens, and the observed no-think outputs are ~45-93
/// tokens, so 2048 is ~2.5x the prompts' own ceiling -- generous headroom
/// without ever letting one of these calls crowd the window.
///
/// Bounded against the server, not just the window: `CONTEXT_WINDOW_TOKENS`
/// (16384, the largest prompt either call can present) + 2048 = 18432, which
/// still fits `server::SERVER_CTX_SIZE` (20480) with 2048 to spare. The
/// `compaction_output_budget_fits_the_server_context` test below pins that.
/// Note this is now EQUAL to `MEMORIES_MAX_TOKENS` -- see that constant's doc
/// comment, which this raise makes newly relevant.
pub const SUMMARY_MAX_TOKENS: i32 = (CONTEXT_WINDOW_TOKENS / 8) as i32;

/// The final USER turn `context::summarize_and_persist` appends after the span
/// it is condensing, and the reason it must: llama-server's chat template
/// treats a TRAILING ASSISTANT MESSAGE as a prefill to continue, not as
/// context to act on. `messages_to_summarize` returns an arbitrary slice of
/// history that routinely ENDS WITH an assistant message, so before the
/// 2026-07-15 real-model pass the request was `[system(SUMMARIZATION_PROMPT)]
/// + span` and nothing else -- and the model simply closed out the span's last
/// assistant sentence (measured: `reasoning_len=0`, and the "summary" came back
/// as that message echoed verbatim plus a tacked-on clause). That echo is
/// non-empty, not truncated, and smaller than the span it replaces, so
/// `evaluate_summary` ACCEPTED it: tier-2 compaction did not fail loudly, it
/// silently replaced the conversation's state with a continuation of its last
/// sentence. Ending the request on a user turn is what makes the model answer
/// the system prompt instead of autocompleting the transcript.
///
/// Deliberately a SEPARATE const rather than an edit to `SUMMARIZATION_PROMPT`:
/// this is a request-shape fix, and the prompt's bytes are benchmark-gated.
/// The wording restates that prompt's existing contract ("produce the
/// snapshot") and must not introduce new requirements of its own.
pub const SUMMARIZATION_FINAL_TURN: &str = "Now produce the summary as specified.";

/// The extraction-side twin of `SUMMARIZATION_FINAL_TURN` -- same
/// trailing-assistant prefill hazard, same fix, same measured echo failure
/// (`extract_and_persist_memories` persisted the span's last assistant message
/// verbatim as a durable memory, deterministically). Restates
/// `MEMORY_EXTRACTION_PROMPT`'s existing "output the COMPLETE updated set"
/// contract without adding to it.
pub const EXTRACTION_FINAL_TURN: &str = "Now output the updated memory set as specified.";

/// A structured `<state_snapshot>` compaction prompt (SP3 component b):
/// replaces the old one-sentence summary with named sections so a resumed
/// turn recovers goal/task/files/decisions/pending/next-step, not just prose.
/// Kept LEANER than claude-code's 9-section format — tuned for the local 4B
/// model and the `SUMMARY_MAX_TOKENS` budget: terse fragments, an
/// explicit ~800-token cap (headroom below `SUMMARY_MAX_TOKENS` so the call
/// never hits `finish_reason:"length"` and gets rejected by `evaluate_summary`),
/// and omit-empty-sections so a short conversation yields a short snapshot
/// (never an inflated one).
pub const SUMMARIZATION_PROMPT: &str = "You are compacting a coding-agent conversation to free up context space. Produce a state snapshot that lets the agent resume seamlessly with no loss of continuity.

Use EXACTLY this structure. Write terse fragments, not prose. Omit any section that is empty. One line per file, decision, or pending item.

<state_snapshot>
GOAL: the user's overall objective
CURRENT TASK: what is in progress right now
FILES TOUCHED: path — what changed
DECISIONS: choices made and why
PENDING: unresolved steps, in order
NEXT ACTION: the single immediate next step
</state_snapshot>

Keep the whole snapshot under about 800 tokens. Output ONLY the <state_snapshot> block — nothing before or after it.";

/// SP4: the out-of-band memory-extraction prompt. A separate `Forbid`-mode
/// call (never part of an agent turn), so this text cannot affect the
/// tier4_planned benchmark. Asks for the FULL replacement set, one fact per
/// line, because `replace_memories` swaps the workspace's whole set -- which
/// is exactly why this prompt carries its own explicit self-cap (30 facts,
/// ~20 words each), the same way `SUMMARIZATION_PROMPT`'s ~800-token cap
/// exists: the replacement set grows every time memories accumulate, so
/// without a cap the call eventually outgrows `SUMMARY_MAX_TOKENS` and
/// hits `finish_reason:"length"` -- which `extract_and_persist_memories`
/// rejects outright, since a truncated "full replacement set" would silently
/// drop every memory not yet re-emitted while persisting a half-finished
/// sentence as a durable fact. 30 facts * ~20 words (~26 tokens) stays
/// comfortably under 800 tokens, the same headroom below `SUMMARY_MAX_TOKENS`
/// that `SUMMARIZATION_PROMPT` keeps.
pub const MEMORY_EXTRACTION_PROMPT: &str = "\
You maintain a durable memory of a software project workspace.

You will be given the existing memories (possibly empty) and a transcript of \
work that is about to be condensed away. Output the COMPLETE updated set of \
memories: keep the existing ones that are still true, update ones that changed, \
drop ones that are now wrong or obsolete, and add anything newly learned that \
will still matter weeks from now.

Remember only durable facts: the user's stated preferences and working style, \
project constraints and conventions, architectural decisions and the reasoning \
behind them, and hard-won gotchas that cost real time to discover.

Never remember: transient task state, what you are doing right now, file \
contents, anything trivially re-derivable by reading the code, or anything you \
are not confident is true.

Output AT MOST 30 facts, one per line, each a single self-contained sentence of \
no more than 20 words. Keep the whole output under about 800 tokens. If more \
than 30 facts are worth keeping, keep only the most important and durable ones \
-- staying under the cap matters more than completeness. No bullets, no \
numbering, no commentary, no headers. If there is nothing worth remembering, \
output nothing at all.";

/// Auto-compaction gives up retrying a summarization that keeps getting
/// rejected by `context::evaluate_summary` (empty/truncated/inflated) after
/// this many CONSECUTIVE failures for the same conversation --
/// `context::breaker_open` -- until a FORCED "Compact now" run succeeds and
/// resets the count (`context::CompactionFailures`). Mirrors qwen-code's
/// `MAX_CONSECUTIVE_FAILURES`.
pub const MAX_CONSECUTIVE_COMPACTION_FAILURES: u32 = 3;

pub const DEFAULT_WARN_THRESHOLD_PCT: f64 = 0.5;
pub const DEFAULT_COMPACT_THRESHOLD_PCT: f64 = 0.75;
pub const DEFAULT_HARD_LIMIT_PCT: f64 = 0.9;

/// 1/16 of `CONTEXT_WINDOW_TOKENS` (= 1024 at the 16K window). A tool result whose
/// model-facing text costs at most this many tokens is inlined whole;
/// anything larger becomes a status reference line pointing at its payload
/// file (2026-07-09 payload-files design, `context::payload::stage_tool_result`).
pub const DEFAULT_TOOL_OUTPUT_OFFLOAD_TOKENS: usize = (CONTEXT_WINDOW_TOKENS / 16) as usize;

/// The budgeting RESERVE (not the wire `max_tokens` ceiling -- see
/// `AGENT_TURN_OUTPUT_CEILING`) subtracted by the plan-host `threshold`
/// computations and the `STATE_TAIL_RESERVE_TOKENS` envelope, and by
/// `agent::run_loop`'s own `fit_turn_to_budget` reserve. Since the
/// always-max-output task, this is no longer what `clamp_output_tokens`
/// uses as `ceiling` for agent turns (that's `AGENT_TURN_OUTPUT_CEILING`
/// now, the window itself) -- this constant lives on purely as the
/// conservative slack those budgets still reserve ahead of a turn, before
/// the turn's own request ever asks the clamp for its actual `max_tokens`.
///
/// Raised from 256 (~3.1% of the 8192 window) to 1024 (~6.2% of
/// `CONTEXT_WINDOW_TOKENS`) after a real benchmark failure: a well-granulated
/// 20-step `CreatePlan` call needs more than 256 output tokens, so generation
/// was cut off mid-JSON before the closing `</tool_call>` tag and the truncated
/// call silently became the turn's "final answer" (tier4_planned scored 0/20 at
/// turn 2). The grammar guarantees a tool call ends at its closing tag and EOG
/// ends short answers early, so the extra headroom costs nothing on turns that
/// don't need it.
/// Raised 1024 -> 2048 for thinking models (2026-07-13): the reasoning
/// block spends output budget BEFORE the tool call, and 1024 was observed
/// exhausted mid-think (the whole response then strips to empty). EOG
/// still ends short turns early, so the extra headroom costs nothing on
/// turns that don't think long.
pub const AGENT_TURN_MAX_OUTPUT_TOKENS: u32 = 2048;

/// The output-token CEILING for agent turns under the always-max-output policy:
/// agent turns request as much output as fits the window, so the ceiling IS the
/// window and `clamp_output_tokens` returns `window - prompt_est - margin`,
/// shrinking only when the prompt is large. `OUTPUT_RESERVE_TOKENS`
/// (`SERVER_CTX_SIZE - CONTEXT_WINDOW_TOKENS` = 4096) stays as slack beyond the
/// clamp's `window`, so `prompt + max_tokens <= CONTEXT_WINDOW_TOKENS <
/// SERVER_CTX_SIZE` holds even if `prompt_est` slightly under-counts. Distinct
/// from `AGENT_TURN_MAX_OUTPUT_TOKENS`, which remains the conservative RESERVE
/// the plan-host `threshold`/`STATE_TAIL_RESERVE` budgets subtract.
pub const AGENT_TURN_OUTPUT_CEILING: u32 = CONTEXT_WINDOW_TOKENS;

/// The floor `clamp_output_tokens` never sizes a request's `max_tokens`
/// below, even once headroom (`window - prompt_estimate - margin`) is fully
/// exhausted -- a request with less than this is more likely to fail the
/// turn outright (cut off before a tool call's closing tag, or before any
/// usable text at all) than to succeed short, so 512 is the practical
/// minimum a generation needs to have a chance at finishing something
/// coherent.
pub const MIN_OUTPUT_TOKENS: u32 = 512;

/// Sizes a request's `max_tokens` so `prompt + max_tokens <= window` is
/// structurally guaranteed rather than merely conventional (qwen-code's
/// `clampOutputTokensToWindow`). `ceiling` is the caller's preferred cap
/// (e.g. `AGENT_TURN_OUTPUT_CEILING`); `window` is the model's context
/// window; `prompt_estimate` is this turn's estimated prompt token count
/// (`inference::token_estimate`). `margin` reserves headroom beyond the
/// prompt itself -- the larger of a flat 1024 tokens or 1/20th of the
/// window -- for the chat-template overhead `prompt_estimate` doesn't
/// account for (role tags, etc.) plus a little slack, the same kind of
/// reserve `STATE_TAIL_RESERVE_TOKENS` covers for the per-turn state tail.
/// Never returns less than `MIN_OUTPUT_TOKENS`, even once headroom is fully
/// exhausted -- a starved request is better than un-generatable one.
pub fn clamp_output_tokens(ceiling: u32, window: u32, prompt_estimate: u32) -> u32 {
    let margin = 1024.max(window / 20);
    ceiling.min(MIN_OUTPUT_TOKENS.max(window.saturating_sub(prompt_estimate + margin)))
}

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

/// Max tokens of an `AGENTS.md` project-instructions file folded into the
/// cwd-aware system message (SP3). A file over this is truncated (tail
/// dropped) with a marker, so a huge instructions file can't crowd out the
/// conversation. ~12.5% of `CONTEXT_WINDOW_TOKENS`.
pub const PROJECT_INSTRUCTIONS_MAX_TOKENS: usize = (CONTEXT_WINDOW_TOKENS / 8) as usize; // = 2048

/// Defence-in-depth cap on the recalled `# Memories` block, sized at the same
/// 1/8-of-window share as `PROJECT_INSTRUCTIONS_MAX_TOKENS`. Injected once into
/// `messages[0]`, structurally outside the compaction window.
///
/// This cap STILL does not bind in production, but the margin that kept it
/// slack is gone and the old "half of this cap" framing no longer holds. The
/// real bound on a persisted memory set comes from the WRITE side:
/// `extract_and_persist_memories` caps its request at `SUMMARY_MAX_TOKENS` and
/// refuses to persist anything at all when the completion comes back
/// `finish_reason:"length"`. That write cap was 1024 -- comfortably half of
/// this one -- until the 2026-07-15 real-model pass raised it to 2048, which is
/// now EXACTLY this value. The predicted-here consequence has therefore
/// arrived: a maximal write (2048 tokens) now exactly meets this cap rather
/// than sitting far below it, so `render_memories_section`'s shrink loop is one
/// token of drift away from iterating on a production-written set for the first
/// time. It does not fire today (the check is `<=`, and real no-think
/// extractions measure ~45-93 tokens against a prompt that self-caps at ~800),
/// but the two constants are now COUPLED: raise `SUMMARY_MAX_TOKENS` again
/// without raising this one and the shrink loop starts silently dropping
/// trailing facts off recalled sets. Raise both together, or not at all.
///
/// It is kept for the reason it always was: it is the only thing standing
/// between recall and an over-large set that arrived by some other route
/// (hand-edited sqlite, an imported set, a future writer with a different cap).
pub const MEMORIES_MAX_TOKENS: usize = (CONTEXT_WINDOW_TOKENS / 8) as usize;

/// Shape bounds for a single extracted memory line (see
/// `context::is_plausible_fact`). Not a share of anything -- these are "is this
/// sentence-shaped at all" bounds on model output, deliberately far looser than
/// `MEMORY_EXTRACTION_PROMPT`'s own ~20-words-per-fact instruction so that only
/// clear garbage trips them.
pub const MEMORY_FACT_MIN_CHARS: usize = 10;
pub const MEMORY_FACT_MAX_CHARS: usize = 300;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn budget_constants_stay_proportional_to_the_window() {
        assert_eq!(SUMMARY_MAX_TOKENS, (CONTEXT_WINDOW_TOKENS / 8) as i32);
        assert_eq!(
            DEFAULT_TOOL_OUTPUT_OFFLOAD_TOKENS,
            (CONTEXT_WINDOW_TOKENS / 16) as usize
        );
        assert!(AGENT_TURN_MAX_OUTPUT_TOKENS >= CONTEXT_WINDOW_TOKENS / 16);
        // The tail reserve must comfortably cover a big plan's tail
        // (hundreds of tokens). The combined per-turn reserve widened
        // /8 -> /4 for thinking models (2026-07-13): reasoning spends
        // output budget by design, so up to a quarter of the window per
        // turn is the accepted envelope now.
        assert!(STATE_TAIL_RESERVE_TOKENS >= CONTEXT_WINDOW_TOKENS / 32);
        assert!(
            STATE_TAIL_RESERVE_TOKENS + AGENT_TURN_MAX_OUTPUT_TOKENS <= CONTEXT_WINDOW_TOKENS / 4
        );
        assert!(MIN_OUTPUT_TOKENS < AGENT_TURN_MAX_OUTPUT_TOKENS);
        assert_eq!(
            PROJECT_INSTRUCTIONS_MAX_TOKENS,
            (CONTEXT_WINDOW_TOKENS / 8) as usize
        );
    }

    /// The raise to `SUMMARY_MAX_TOKENS` must not let a compaction call ask the
    /// server for more than it can hold. The largest prompt either `Forbid`-mode
    /// call can present is a full window's worth of history, so
    /// `CONTEXT_WINDOW_TOKENS + SUMMARY_MAX_TOKENS` is the worst case and it has
    /// to fit the sidecar's actual `--ctx-size`, not merely the in-process input
    /// budget derived from it.
    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn compaction_output_budget_fits_the_server_context() {
        let worst_case = CONTEXT_WINDOW_TOKENS + SUMMARY_MAX_TOKENS as u32;
        assert!(
            worst_case <= crate::inference::server::SERVER_CTX_SIZE,
            "a compaction call's prompt+output ({worst_case}) must fit the server's ctx-size ({})",
            crate::inference::server::SERVER_CTX_SIZE
        );
    }

    /// `MEMORIES_MAX_TOKENS`'s doc comment: the write cap and the recall cap are
    /// coupled, and a write cap ABOVE the recall cap silently starts dropping
    /// trailing facts off recalled sets in `render_memories_section`'s shrink
    /// loop. Pin the direction of that inequality so the next raise of either
    /// constant has to confront it.
    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn the_memory_write_cap_never_exceeds_the_recall_cap() {
        assert!(
            SUMMARY_MAX_TOKENS as usize <= MEMORIES_MAX_TOKENS,
            "extraction can write up to {SUMMARY_MAX_TOKENS} tokens but recall only renders \
             {MEMORIES_MAX_TOKENS} -- raise MEMORIES_MAX_TOKENS too, or recall silently \
             truncates persisted facts"
        );
    }

    // --- clamp_output_tokens (restore-output-cap task) ---

    #[test]
    fn clamp_output_tokens_ceiling_wins_with_ample_headroom() {
        assert_eq!(clamp_output_tokens(2048, 16384, 4000), 2048);
    }

    #[test]
    fn clamp_output_tokens_floor_wins_when_headroom_is_exhausted() {
        assert_eq!(clamp_output_tokens(2048, 16384, 15000), MIN_OUTPUT_TOKENS);
        assert_eq!(clamp_output_tokens(2048, 16384, 15000), 512);
    }

    #[test]
    fn clamp_output_tokens_headroom_binds_below_the_ceiling() {
        // margin = max(1024, 16384/20=819) = 1024;
        // window - prompt_estimate - margin = 16384 - 13500 - 1024 = 1860.
        assert_eq!(clamp_output_tokens(2048, 16384, 13500), 1860);
    }

    // --- AGENT_TURN_OUTPUT_CEILING (always-max-output policy) ---

    #[test]
    fn agent_output_ceiling_lets_output_fill_the_free_window() {
        let window = CONTEXT_WINDOW_TOKENS;
        let out = clamp_output_tokens(AGENT_TURN_OUTPUT_CEILING, window, 1000);
        assert!(
            out > AGENT_TURN_MAX_OUTPUT_TOKENS,
            "expected max-fit output, got {out}"
        );
        assert_eq!(out, window - (1000 + 1024.max(window / 20)));
        assert!(1000 + out <= window); // structural guarantee still holds
    }

    #[test]
    fn agent_output_ceiling_floors_at_min_when_prompt_nearly_fills_window() {
        let window = CONTEXT_WINDOW_TOKENS;
        let out = clamp_output_tokens(AGENT_TURN_OUTPUT_CEILING, window, window - 100);
        assert_eq!(out, MIN_OUTPUT_TOKENS);
    }
}
