//! Goal/plan state for the two-state agent loop: a single `run_loop` call
//! whose system prompt and available tools depend on an external state the
//! backend itself carries (`LoopState`) — not two separate loops. See
//! `tests/agent_benchmark.rs`'s state-driven backend for the actual
//! `AgentBackend` implementation; this module only holds the state shape
//! and the two system prompts, so both the benchmark and any future
//! production wiring build on the same definitions.
//!
//! `Planning` maintains the plan via ordinary tool calls (`CreatePlan`/
//! `AddStep`) and can independently verify results with read-only tools —
//! the same grammar-constrained `{"tool_call": ...}` mechanism every other
//! tool already uses, not a bespoke one-shot structured-JSON call (an
//! earlier design that got stuck: re-emitting the entire plan as JSON
//! every turn, judged from a hand-built evidence string, with no real way
//! to act on its own uncertainty). `Executing` is a single step, framed
//! with the overall goal plus that step's own description — it must end
//! with `StepDone` or `RefuseStep`, never a plain-text reply, since a
//! flat `run_loop` treats any plain-text reply as ending the whole call,
//! not just the current step. `RefuseStep`'s reason is carried into the
//! next Planning turn so a revision is informed, not blind.
//!
//! Both states share one continuous conversation (unlike an earlier
//! nested-`run_loop`-per-step design) — a step's own tool activity stays
//! visible for the rest of the task, which is also why `Executing`'s
//! prompt below tells the model to delegate bulky, self-contained work to
//! `Task` (a real subagent, isolated by construction) rather than doing it
//! all inline and flooding the shared history.
//!
//! Benchmark evidence for why this replaced the earlier two-backend/
//! recursive-`run_loop` design (`RunStep` delegating into a second,
//! isolated `run_loop`, judged only from a compressed summary string):
//! on the exact same 20-scattered-bugs task, the two-backend design
//! scored 2-4/20 (each delegated step capped at its own small turn
//! budget, so it could never finish more than a few files before being
//! cut off), while this single-loop, shared-context, shared-turn-budget
//! design scored 20/20 -- a step with imperfect (too-coarse) plan
//! granularity can simply keep working across many turns until it's
//! actually done, instead of being artificially forced to stop early.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanStep {
    pub description: String,
    pub done: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Plan {
    pub goal: String,
    pub steps: Vec<PlanStep>,
}

/// Which of the two states the loop is in right now. Carried by the
/// backend, not `run_loop` itself — `run_loop`'s own signature and control
/// flow are completely unaware this exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopState {
    Planning,
    Executing { step_index: usize },
}

/// System prompt for `LoopState::Planning`. Tools: `CreatePlan` (define
/// the plan; only valid once, since calling it again would discard
/// progress already recorded on other steps), `AddStep` (the only way to
/// extend/correct the plan after that), `ResumeExecution` (hand off to the
/// next undone step -- used both right after `CreatePlan` and again after
/// a refusal-driven revision), read-only verification tools, and
/// `AskUserQuestion`.
pub const PLANNING_SYSTEM_PROMPT: &str = r#"You are a planning supervisor. You maintain a plan and hand off each step's actual work to an execution mode with file/shell tools -- you do not personally edit files or run commands from here. To use a tool, respond with ONLY a JSON object in this exact shape, nothing else:
{"tool_call": {"name": "ToolName", "arguments": {...}}}

Available tools:
- CreatePlan: {"goal": string, "steps": [string]} -- define the plan. Valid only once, when no plan exists yet. Step granularity matters: each step is executed with its own limited number of turns, so a step must be small enough to actually finish within that. If the task repeats similar work across multiple items (e.g. multiple files), create ONE STEP PER ITEM, never a single step like "for each file, do X" that silently bundles many items together -- that kind of step cannot finish in a bounded number of turns and will only get partway done.
- AddStep: {"description": string} -- append a step. This is how you extend or correct the plan after the first CreatePlan call -- do not call CreatePlan again, that would discard progress already made on other steps.
- ResumeExecution: {} -- hand off to the next step that isn't done yet. Call this right after CreatePlan to begin, and again any time you've finished adding/correcting steps and are ready to continue.
- Read: {"file_path": string} / Grep: {"pattern": string, "path"?: string} / Glob: {"pattern": string, "path"?: string} -- read-only tools to independently verify a step's actual result yourself, instead of trusting its summary.
- AskUserQuestion: {"header": string, "question": string, "options": [{"label": string, "description"?: string}], "multiSelect"?: boolean} -- ask the user directly if the request is genuinely ambiguous.

You return here automatically once every step reports done, or when a step reports it could not be completed (its reason will be given to you). A step reporting done is a CLAIM, not proof -- before answering, independently verify with Read/Grep/Glob rather than trusting it. If verification shows something is genuinely still wrong, use AddStep and ResumeExecution rather than accepting the claim.

Once you have verified the task is genuinely, completely done, respond in plain text with your final answer -- do not wrap it in JSON."#;

/// System prompt for `LoopState::Executing { step_index }`, parameterized
/// with the overall goal and that one step's own description -- a step
/// description alone is not self-contained by the third or fourth item in
/// a decomposed plan (confirmed against the real model: given only its own
/// step text with no goal attached, it hallucinated a nonexistent file).
pub fn executing_system_prompt(goal: &str, step_description: &str) -> String {
    format!(
        r#"You are executing one step of a larger plan. To use a tool, respond with ONLY a JSON object in this exact shape, nothing else:
{{"tool_call": {{"name": "ToolName", "arguments": {{...}}}}}}

Overall goal: {goal}
Your current step: {step_description}

Available tools:
- Read / Write / Edit / Bash / Grep / Glob -- the usual file and shell tools.
- Task: {{"prompt": string}} -- delegate substantial, self-contained work (extensive exploration, a large search, a bulky sub-investigation) to an isolated subagent instead of doing it inline. This conversation is shared across the WHOLE task, not just this step -- everything you do here stays visible to every later step too, so keep it lean: reach for Task when a piece of work would otherwise flood this shared history with exploration detail nobody later needs, and only the outcome actually matters going forward.
- StepDone: {{"summary": string}} -- call this once you have actually completed the step, not when you believe you're close.
- RefuseStep: {{"reason": string}} -- call this if the step cannot be completed as described (unclear, blocked, or wrong). Explain why -- your reason is used to revise the plan.

You must end by calling StepDone or RefuseStep -- never answer in plain text here, that would end the WHOLE task, not just this step."#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_default_is_empty() {
        let plan = Plan::default();
        assert_eq!(plan.goal, "");
        assert!(plan.steps.is_empty());
    }

    #[test]
    fn executing_prompt_embeds_the_goal_and_step_so_it_stays_self_contained() {
        let prompt = executing_system_prompt("ship the feature", "write the tests");
        assert!(prompt.contains("ship the feature"));
        assert!(prompt.contains("write the tests"));
        assert!(prompt.contains("StepDone"));
        assert!(prompt.contains("RefuseStep"));
    }
}
