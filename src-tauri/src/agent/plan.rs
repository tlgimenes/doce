//! Goal/plan state for the two-stage agent loop: a "planning" `run_loop`
//! (system prompt `PLANNING_SYSTEM_PROMPT` below, tools = `CreatePlan`/
//! `AddStep`/`MarkStepDone`/`RunStep`/read-only verification/
//! `AskUserQuestion`) maintains this state via ordinary tool calls, the
//! same grammar-constrained `{"tool_call": ...}` mechanism every other
//! tool already uses — not a bespoke one-shot structured-JSON call.
//! `RunStep`'s own implementation recursively calls `run_loop` again (the
//! same pattern the existing `Task` tool already uses to delegate to a
//! subagent), scoped to one step with the normal broad coding tool set.
//!
//! This replaced an earlier one-shot `check_in` design (ask the model to
//! re-emit the entire plan as one JSON blob every turn, judged from a
//! hand-built evidence string) that got stuck in practice: repeating the
//! same judgment on unchanged evidence with no way to act on its own
//! uncertainty. Giving the supervisor real tools — most importantly the
//! ability to independently verify a claim itself (`Read`/`Grep`/`Glob`)
//! rather than only ever judging secondhand evidence — is the fix; see
//! `tests/agent_benchmark.rs`'s `PlanningBackend` for the implementation.

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

/// The system prompt for the outer "planning" loop — deliberately mirrors
/// `agent::SYSTEM_PROMPT`'s own shape (tool list + the exact `{"tool_call":
/// ...}` convention + a worked example) rather than inventing a different
/// calling convention for this loop specifically.
pub const PLANNING_SYSTEM_PROMPT: &str = r#"You are a planning supervisor. You do not personally edit files or run arbitrary commands -- you maintain a plan and delegate the actual work to a step-execution loop, and you can independently verify results yourself using read-only tools. To use a tool, respond with ONLY a JSON object in this exact shape, nothing else:
{"tool_call": {"name": "ToolName", "arguments": {...}}}

Available tools:
- CreatePlan: {"goal": string, "steps": [string]} -- create the initial plan for this task. Call this once, at the start, before anything else. Step granularity matters: each step is delegated to a bounded execution loop with its own limited number of turns, so a step must be small enough to actually finish within that budget. If the task involves doing the same kind of work across multiple similar items (e.g. multiple files), create ONE STEP PER ITEM (one step per file), never a single step like "for each file, do X" that silently bundles many items together -- that kind of step cannot finish in a bounded number of turns and will only get partway done.
- AddStep: {"description": string} -- append a new step to the plan. Same granularity rule as CreatePlan: one step per item, not one step per phase, when the task repeats across similar items.
- MarkStepDone: {"step_index": number} -- mark a step complete. Only do this if you have real evidence it was actually, correctly done -- never mark a step done just because it claims success.
- RunStep: {"step_index": number} -- delegate a plan step to the execution loop (it has file/shell tools and will actually attempt the work). Returns exactly what it did -- its real tool calls and results -- plus its own closing summary, which may not be reliable on its own.
- Read: {"file_path": string} / Grep: {"pattern": string, "path"?: string} / Glob: {"pattern": string, "path"?: string} -- read-only tools to check a step's actual result yourself, instead of only trusting its summary.
- AskUserQuestion: {"header": string, "question": string, "options": [{"label": string, "description"?: string}], "multiSelect"?: boolean} -- ask the user directly if the request is genuinely ambiguous.

Example tool call:
{"tool_call": {"name": "CreatePlan", "arguments": {"goal": "Fix the reported bug", "steps": ["Reproduce the bug", "Fix it", "Verify the fix"]}}}

Once every step is genuinely, verifiably complete, respond in plain text with your final answer summarizing what was actually done -- do not wrap the final answer in JSON."#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_default_is_empty() {
        let plan = Plan::default();
        assert_eq!(plan.goal, "");
        assert!(plan.steps.is_empty());
    }
}
