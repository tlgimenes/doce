//! Goal/plan state for the two-state agent loop: a single `run_loop` call
//! whose system prompt and available tools depend on an external state the
//! backend itself carries (`LoopState`) — not two separate loops. The state
//! machine itself lives here as `PlanState`: both production
//! (`commands::agent::RealBackend`) and the benchmark's `PlanExecBackend`
//! (`tests/agent_benchmark.rs`) embed this same struct as their
//! `AgentBackend`'s `plan_state` field, rather than each independently
//! reimplementing the state shape and system prompts.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LoopState {
    #[default]
    Planning,
    Executing {
        step_index: usize,
    },
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
- AddStep: {"description": string} -- append a step. This is how you extend or correct the plan after the first CreatePlan call -- do not call CreatePlan again, that would discard progress already made on other steps. Plans you may see in earlier conversation history are finished -- each new user request starts with no plan.
- ResumeExecution: {} -- hand off to the next step that isn't done yet. Call this right after CreatePlan to begin, and again any time you've finished adding/correcting steps and are ready to continue.
- Read: {"file_path": string} / Grep: {"pattern": string, "path"?: string} / Glob: {"pattern": string, "path"?: string} -- read-only tools to independently verify a step's actual result yourself, instead of trusting its summary. Glob's pattern is a single wildcard expression, e.g. "bug_*.txt" or "*.rs" -- never a space-separated list of literal filenames, that matches nothing.
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
- Read / Write / Edit / Bash / Grep / Glob -- the usual file and shell tools. Glob's pattern is a single wildcard expression, e.g. "bug_*.txt" or "*.rs" -- never a space-separated list of literal filenames, that matches nothing.
- Task: {{"prompt": string}} -- delegate substantial, self-contained work (extensive exploration, a large search, a bulky sub-investigation) to an isolated subagent instead of doing it inline. This conversation is shared across the WHOLE task, not just this step -- everything you do here stays visible to every later step too, so keep it lean: reach for Task when a piece of work would otherwise flood this shared history with exploration detail nobody later needs, and only the outcome actually matters going forward.
- StepDone: {{"summary": string}} -- call this once you have actually completed the step, not when you believe you're close.
- RefuseStep: {{"reason": string}} -- call this if the step cannot be completed as described (unclear, blocked, or wrong). Explain why -- your reason is used to revise the plan.

You must end by calling StepDone or RefuseStep -- never answer in plain text here, that would end the WHOLE task, not just this step."#
    )
}

/// The five tools owned by the plan state machine itself — used by the
/// frontend (via ipc.ts's mirror of this list) to keep plan activity
/// invisible in the transcript, and by hosts to route calls.
pub const PLAN_TOOL_NAMES: [&str; 5] = [
    "CreatePlan",
    "AddStep",
    "ResumeExecution",
    "StepDone",
    "RefuseStep",
];

/// The two-state Planning/Executing machine, promoted from the benchmark's
/// `PlanExecBackend` so production (`commands::agent::RealBackend`) and the
/// benchmark embed the SAME engine — one implementation, two thin hosts.
/// Owns the plan, the current state, and the refusal context; hosts own
/// everything else (inference, persistence, events, real tool dispatch).
#[derive(Debug, Default)]
pub struct PlanState {
    pub plan: Plan,
    pub state: LoopState,
    /// Set by `RefuseStep`, consumed (and cleared) the next time
    /// `system_prompt` renders the Planning prompt — carries the refusal
    /// reason into that one revision turn without lingering after.
    refusal_context: Option<String>,
}

impl PlanState {
    /// The system prompt for the current state: Planning (refusal-annotated
    /// when a step was just refused) or the per-step Executing prompt.
    /// `&mut` because rendering the Planning prompt consumes the refusal
    /// context. The caller appends its own cwd line.
    pub fn system_prompt(&mut self) -> String {
        match self.state {
            LoopState::Planning => match self.refusal_context.take() {
                Some(reason) => format!(
                    "{PLANNING_SYSTEM_PROMPT}\n\nThe previous step could not be completed. Reason given: {reason}\n\nRevise the plan accordingly (AddStep, then ResumeExecution)."
                ),
                None => PLANNING_SYSTEM_PROMPT.to_string(),
            },
            LoopState::Executing { step_index } => {
                let step_desc = self.plan.steps[step_index].description.clone();
                executing_system_prompt(&self.plan.goal, &step_desc)
            }
        }
    }

    /// Handles a tool call that belongs to the plan machine: the five plan
    /// tools mutate state and return their result; regular tools that are
    /// NOT available in the current state return a rejection. `None` means
    /// "this is an ordinary tool the host should dispatch itself" —
    /// read-only tools + AskUserQuestion while Planning, file/shell/Task
    /// while Executing (the per-state tool-name sets the benchmark
    /// validated 20/20; actually dispatching whatever passes through is the
    /// host's job, not this function's).
    pub fn handle_plan_tool(&mut self, call: &crate::agent::ToolCall) -> Option<String> {
        let result = match (self.state, call.name.as_str()) {
            (LoopState::Planning, "CreatePlan") => {
                if !self.plan.steps.is_empty() {
                    "Error: a plan already exists -- use AddStep to extend or correct it, CreatePlan is only valid once".to_string()
                } else {
                    let goal = call
                        .arguments
                        .get("goal")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let steps: Vec<PlanStep> = call
                        .arguments
                        .get("steps")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|s| s.as_str())
                                .map(|d| PlanStep {
                                    description: d.to_string(),
                                    done: false,
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    let step_count = steps.len();
                    self.plan = Plan { goal, steps };
                    format!("Plan created with {step_count} steps. Call ResumeExecution to begin.")
                }
            }
            (LoopState::Planning, "AddStep") => {
                if !self.has_plan() {
                    // On a follow-up user turn, the previous turn's finished
                    // plan rows replay in model history and can invite the
                    // model to call AddStep against the fresh, empty
                    // PlanState this turn actually started with -- without
                    // this guard that produces a goal-less plan and an
                    // Executing prompt with an empty goal.
                    "Error: no plan exists yet -- call CreatePlan first".to_string()
                } else {
                    let description = call
                        .arguments
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    self.plan.steps.push(PlanStep {
                        description,
                        done: false,
                    });
                    format!("Step added. Plan now has {} steps.", self.plan.steps.len())
                }
            }
            (LoopState::Planning, "ResumeExecution") => match self.next_undone_step() {
                Some(idx) => {
                    self.state = LoopState::Executing { step_index: idx };
                    format!("Resuming at step {idx}: {}", self.plan.steps[idx].description)
                }
                None => "Error: no undone steps -- create or add a step first".to_string(),
            },
            (LoopState::Planning, "Read" | "Grep" | "Glob" | "AskUserQuestion") => return None,
            (LoopState::Executing { step_index }, "StepDone") => {
                self.plan.steps[step_index].done = true;
                match self.next_undone_step() {
                    Some(next) => {
                        self.state = LoopState::Executing { step_index: next };
                        format!("Step {step_index} done. Moving to step {next}.")
                    }
                    None => {
                        self.state = LoopState::Planning;
                        format!("Step {step_index} done. All steps report done -- back to planning for final review.")
                    }
                }
            }
            (LoopState::Executing { step_index }, "RefuseStep") => {
                let reason = call
                    .arguments
                    .get("reason")
                    .and_then(|v| v.as_str())
                    .unwrap_or("(no reason given)");
                self.refusal_context = Some(format!(
                    "step {step_index} (\"{}\"): {reason}",
                    self.plan.steps[step_index].description
                ));
                self.state = LoopState::Planning;
                "Step refused. Back to planning.".to_string()
            }
            (
                LoopState::Executing { .. },
                "Read" | "Write" | "Edit" | "Bash" | "Grep" | "Glob" | "Task",
            ) => return None,
            (_, other) => format!("Error: {other} is not available in the current phase"),
        };
        Some(result)
    }

    pub fn next_undone_step(&self) -> Option<usize> {
        self.plan.steps.iter().position(|s| !s.done)
    }

    pub fn has_plan(&self) -> bool {
        !self.plan.steps.is_empty()
    }
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

    use crate::agent::ToolCall;

    fn call(name: &str, arguments: serde_json::Value) -> ToolCall {
        ToolCall {
            name: name.to_string(),
            arguments,
        }
    }

    #[test]
    fn create_plan_then_resume_moves_to_executing_the_first_step() {
        let mut ps = PlanState::default();
        assert_eq!(ps.state, LoopState::Planning);
        assert!(!ps.has_plan());

        let result = ps
            .handle_plan_tool(&call(
                "CreatePlan",
                serde_json::json!({"goal": "fix bugs", "steps": ["fix a", "fix b"]}),
            ))
            .expect("CreatePlan is a plan tool");
        assert!(result.contains("2 steps"));
        assert!(ps.has_plan());
        assert_eq!(ps.plan.goal, "fix bugs");
        assert_eq!(ps.state, LoopState::Planning, "CreatePlan alone does not start execution");

        let result = ps
            .handle_plan_tool(&call("ResumeExecution", serde_json::json!({})))
            .expect("ResumeExecution is a plan tool");
        assert!(result.contains("fix a"));
        assert_eq!(ps.state, LoopState::Executing { step_index: 0 });
    }

    #[test]
    fn create_plan_is_only_valid_once() {
        let mut ps = PlanState::default();
        ps.handle_plan_tool(&call(
            "CreatePlan",
            serde_json::json!({"goal": "g", "steps": ["a"]}),
        ));
        let second = ps
            .handle_plan_tool(&call(
                "CreatePlan",
                serde_json::json!({"goal": "other", "steps": ["x"]}),
            ))
            .unwrap();
        assert!(second.starts_with("Error"));
        assert_eq!(ps.plan.goal, "g", "the existing plan must be untouched");
    }

    #[test]
    fn step_done_advances_to_next_undone_step_and_returns_to_planning_when_finished() {
        let mut ps = PlanState::default();
        ps.handle_plan_tool(&call(
            "CreatePlan",
            serde_json::json!({"goal": "g", "steps": ["a", "b"]}),
        ));
        ps.handle_plan_tool(&call("ResumeExecution", serde_json::json!({})));

        let result = ps
            .handle_plan_tool(&call("StepDone", serde_json::json!({"summary": "did a"})))
            .unwrap();
        assert!(ps.plan.steps[0].done);
        assert_eq!(ps.state, LoopState::Executing { step_index: 1 });
        assert!(result.contains("step 1"));

        ps.handle_plan_tool(&call("StepDone", serde_json::json!({"summary": "did b"})));
        assert!(ps.plan.steps[1].done);
        assert_eq!(ps.state, LoopState::Planning, "all done returns to planning for review");
    }

    #[test]
    fn refuse_step_returns_to_planning_and_threads_the_reason_into_the_next_prompt() {
        let mut ps = PlanState::default();
        ps.handle_plan_tool(&call(
            "CreatePlan",
            serde_json::json!({"goal": "g", "steps": ["impossible"]}),
        ));
        ps.handle_plan_tool(&call("ResumeExecution", serde_json::json!({})));

        ps.handle_plan_tool(&call(
            "RefuseStep",
            serde_json::json!({"reason": "the file does not exist"}),
        ));
        assert_eq!(ps.state, LoopState::Planning);

        let prompt = ps.system_prompt();
        assert!(prompt.contains("the file does not exist"), "refusal reason must reach the revision prompt");
        // Consumed: the next planning prompt is clean again.
        let prompt2 = ps.system_prompt();
        assert!(!prompt2.contains("the file does not exist"));
    }

    #[test]
    fn add_step_rejects_when_no_plan_exists_yet() {
        let mut ps = PlanState::default();
        assert!(!ps.has_plan());

        let result = ps
            .handle_plan_tool(&call("AddStep", serde_json::json!({"description": "orphan step"})))
            .unwrap();
        assert!(result.starts_with("Error"));
        assert!(!ps.has_plan(), "AddStep must not mutate the plan when none exists");
        assert!(ps.plan.steps.is_empty());

        // A subsequent CreatePlan still works normally.
        let created = ps
            .handle_plan_tool(&call(
                "CreatePlan",
                serde_json::json!({"goal": "g", "steps": ["a"]}),
            ))
            .unwrap();
        assert!(created.contains("1 steps"));
        assert!(ps.has_plan());
        assert_eq!(ps.plan.goal, "g");
    }

    #[test]
    fn add_step_appends_and_resume_picks_the_first_undone_step() {
        let mut ps = PlanState::default();
        ps.handle_plan_tool(&call(
            "CreatePlan",
            serde_json::json!({"goal": "g", "steps": ["a"]}),
        ));
        ps.handle_plan_tool(&call("ResumeExecution", serde_json::json!({})));
        ps.handle_plan_tool(&call("StepDone", serde_json::json!({})));
        assert_eq!(ps.state, LoopState::Planning);

        ps.handle_plan_tool(&call("AddStep", serde_json::json!({"description": "b"})));
        assert_eq!(ps.plan.steps.len(), 2);
        ps.handle_plan_tool(&call("ResumeExecution", serde_json::json!({})));
        assert_eq!(ps.state, LoopState::Executing { step_index: 1 });
    }

    #[test]
    fn regular_tools_are_state_gated() {
        let mut ps = PlanState::default();
        // Planning: read-only + AskUserQuestion pass through (None = host dispatches).
        assert!(ps.handle_plan_tool(&call("Read", serde_json::json!({}))).is_none());
        assert!(ps.handle_plan_tool(&call("AskUserQuestion", serde_json::json!({}))).is_none());
        // Planning: write tools are rejected.
        let rejected = ps.handle_plan_tool(&call("Write", serde_json::json!({}))).unwrap();
        assert!(rejected.starts_with("Error"));

        ps.handle_plan_tool(&call(
            "CreatePlan",
            serde_json::json!({"goal": "g", "steps": ["a"]}),
        ));
        ps.handle_plan_tool(&call("ResumeExecution", serde_json::json!({})));
        // Executing: file/shell/Task pass through, plan-editing is rejected.
        assert!(ps.handle_plan_tool(&call("Write", serde_json::json!({}))).is_none());
        assert!(ps.handle_plan_tool(&call("Task", serde_json::json!({}))).is_none());
        let rejected = ps.handle_plan_tool(&call("AddStep", serde_json::json!({"description": "x"}))).unwrap();
        assert!(rejected.starts_with("Error"));
    }

    #[test]
    fn system_prompt_matches_the_state() {
        let mut ps = PlanState::default();
        assert!(ps.system_prompt().contains("planning supervisor"));

        ps.handle_plan_tool(&call(
            "CreatePlan",
            serde_json::json!({"goal": "ship it", "steps": ["write tests"]}),
        ));
        ps.handle_plan_tool(&call("ResumeExecution", serde_json::json!({})));
        let prompt = ps.system_prompt();
        assert!(prompt.contains("ship it"));
        assert!(prompt.contains("write tests"));
        assert!(prompt.contains("StepDone"));
    }
}
