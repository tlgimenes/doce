//! Goal/plan state for the single-mode agent harness: one flat `run_loop`
//! call, no tool-availability state machine of any kind. `PlanState`
//! carries the live todo list (`plan`) plus a one-shot `FinishTask` bounce
//! flag (`finish_bounced`); both production (`commands::agent::RealBackend`)
//! and the task benchmark's backends (`tests/agent_tasks.rs`) embed this
//! same struct as their `AgentBackend`'s `plan_state` field, rather than
//! each independently reimplementing the todo shape.
//!
//! Prompt architecture (stable prefix): `messages[0]` is ONE immutable
//! system prompt per host (`single_mode_system_prompt`) that never changes
//! within a turn, so `inference::PromptSession`'s KV prefix survives every
//! turn boundary. The one volatile piece — the current todo list — rides
//! in a per-turn tail message (`PlanState::todo_tail`) appended after the
//! whole conversation; the full tool set is advertised and samplable every
//! turn (`PlanState::single_mode_tool_names`), so there is no per-state
//! gating left to enforce.
//!
//! `Task` gets its own line in the tool set because it's a union tool a
//! subagent host must never advertise (FR-016's one-level nesting cap:
//! `run_loop` rejects any `Task` call from a subagent, so listing it would
//! just spend a guaranteed-failing turn). `AskUserQuestion` gets the
//! identical treatment for its own reason (`SubagentBackend` has no route
//! to a user).
//!
//! This replaced an earlier two-state Planning/Executing machine (deleted
//! 2026-07-14, self-declared dead through the transition since 2026-07-13)
//! that gated tool availability at the sampler per state via a dedicated
//! `LoopState`; the single-mode harness relies on the model's own todo
//! list instead of a state machine, converged from a benchmark score of
//! 20/20 on the same 20-scattered-bugs task the two-state design scored
//! 2-4/20 on.

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

/// What handling `Todo`/`FinishTask` produced: an ordinary result string
/// fed back into the loop, or the task's final answer (`FinishTask`) —
/// hosts map `Finish` onto `agent::ToolExecution::Finish`, ending
/// `run_loop`. Putting "done" behind a tool call is what lets `run_loop`
/// run with grammar-required tool calls: free-text replies (which a small
/// model degrades into after repetitive stretches) become unsamplable.
#[derive(Debug, Clone, PartialEq)]
pub enum PlanToolReply {
    Reply(String),
    Finish(String),
}

/// The single-mode harness's live state: the todo list, and whether
/// `FinishTask` has already been bounced once this task. Owns the plan;
/// hosts own everything else (inference, persistence, events, real tool
/// dispatch).
#[derive(Debug, Default)]
pub struct PlanState {
    pub plan: Plan,
    /// Single-mode harness: FinishTask with undone todos was already
    /// bounced once this task (`handle_todo_tool`) — the second attempt
    /// is honored.
    finish_bounced: bool,
}

fn build_single_mode_system_prompt(allow_task: bool) -> String {
    let unclear_action = if allow_task {
        "call AskUserQuestion, and keep asking until the task is clear"
    } else {
        "call FinishTask explaining exactly what is missing"
    };

    format!(
        r#"You are doce, a local coding agent.

# Tools

You have tools to read, search, and change files and to run shell commands. Their signatures are provided to you. Call exactly one tool per response.

# Size up the request first

Not every message is a task. Decide before anything else:
- A greeting, small talk, or a question you can already answer: call FinishTask with your answer right away. Never invent work the user did not ask for.
- A request that is unclear, or that names files or things you cannot find: {unclear_action}. Never guess what the task might be.
- A clear task: do the work with your tools, then FinishTask with your answer.

# Todos

For any multi-step task, keep a todo list with Todo: one item per file or unit of work, never a bundled "handle the rest" item. Flip done: true as you finish each item, and work the list in order. Your current todos are shown to you each turn.

# Counting and sampling

Glob and Grep results are capped at 100, so never answer "how many" or "list all" by counting their output -- a capped result undercounts silently. For counts, sizes, samples, or statistics over files, run one Bash command that computes the answer directly, e.g. `find . -name "*.ts" | wc -l` for "how many .ts files?", `du -sh */ | sort -h` for "which folder is biggest?". One command, one number -- not a listing you count yourself.

# Finishing

A belief that something is done is not proof: before FinishTask, verify your own work with Read or Grep -- re-read what you changed or search for remaining problems. FinishTask delivers your final answer to the user.

Every response you give must be exactly one tool call."#
    )
}

/// THE single-mode system prompt — cached per host flavor, byte-stable
/// within a flavor (the KV-prefix invariant, unchanged from the union
/// prompt this replaces).
pub fn single_mode_system_prompt(allow_task: bool) -> &'static str {
    use std::sync::OnceLock;
    static WITH_TASK: OnceLock<String> = OnceLock::new();
    static WITHOUT_TASK: OnceLock<String> = OnceLock::new();
    let cell = if allow_task {
        &WITH_TASK
    } else {
        &WITHOUT_TASK
    };
    cell.get_or_init(|| build_single_mode_system_prompt(allow_task))
}

const SINGLE_MODE_TOOLS_TOP: &[&str] = &[
    "Read",
    "Update",
    "Bash",
    "Grep",
    "Glob",
    "Task",
    "AskUserQuestion",
    "Todo",
    "FinishTask",
];
const SINGLE_MODE_TOOLS_SUB: &[&str] = &[
    "Read",
    "Update",
    "Bash",
    "Grep",
    "Glob",
    "Todo",
    "FinishTask",
];

impl PlanState {
    /// The single-mode grammar enum: the full set, no per-state swapping —
    /// state legality was the two-mode machine's concern.
    pub fn single_mode_tool_names(&self, allow_task: bool) -> &'static [&'static str] {
        if allow_task {
            SINGLE_MODE_TOOLS_TOP
        } else {
            SINGLE_MODE_TOOLS_SUB
        }
    }

    /// The volatile recitation tail: current todos as one compact line.
    /// EMPTY when no todos exist — hosts must skip pushing an empty tail.
    pub fn todo_tail(&self) -> String {
        if self.plan.steps.is_empty() {
            return String::new();
        }
        let items = self
            .plan
            .steps
            .iter()
            .map(|s| format!("[{}] {}", if s.done { "x" } else { " " }, s.description))
            .collect::<Vec<_>>()
            .join("  ");
        let done = self.plan.steps.iter().filter(|s| s.done).count();
        format!(
            "Todos ({done}/{} done): {items}
Work the first undone item; update with Todo as you finish each.",
            self.plan.steps.len()
        )
    }

    /// Intercepts the two harness tools (Todo, FinishTask) before
    /// dispatch. FinishTask with undone todos bounces ONCE per task ("finish or
    /// remove them"), closing the bundled-work/stops-partway failure the
    /// step machine used to close; the second attempt is honored so a
    /// genuinely stuck task can still end.
    pub fn handle_todo_tool(&mut self, call: &crate::agent::ToolCall) -> Option<PlanToolReply> {
        match call.name.as_str() {
            "Todo" => {
                let Some(items) = call.arguments.get("items").and_then(|v| v.as_array()) else {
                    return Some(PlanToolReply::Reply(
                        r#"Error: Todo requires items: an array of {"text": string, "done": boolean}."#
                            .to_string(),
                    ));
                };
                let mut steps = Vec::with_capacity(items.len());
                for item in items {
                    let (Some(text), Some(done)) = (
                        item.get("text").and_then(|v| v.as_str()),
                        item.get("done").and_then(|v| v.as_bool()),
                    ) else {
                        return Some(PlanToolReply::Reply(
                            r#"Error: every Todo item needs {"text": string, "done": boolean}."#
                                .to_string(),
                        ));
                    };
                    steps.push(PlanStep {
                        description: text.to_string(),
                        done,
                    });
                }
                let done = steps.iter().filter(|s| s.done).count();
                let total = steps.len();
                self.plan.steps = steps;
                Some(PlanToolReply::Reply(format!(
                    "Todo updated: {done}/{total} done."
                )))
            }
            "FinishTask" => {
                let answer = call
                    .arguments
                    .get("answer")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let undone = self.plan.steps.iter().filter(|s| !s.done).count();
                if undone > 0 && !self.finish_bounced {
                    self.finish_bounced = true;
                    return Some(PlanToolReply::Reply(format!(
                        "{undone} todo(s) remain undone. Finish them, or remove them with Todo if they no longer apply -- then FinishTask."
                    )));
                }
                Some(PlanToolReply::Finish(answer))
            }
            _ => None,
        }
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
}

#[cfg(test)]
mod single_mode_tests {
    use super::*;

    fn call(name: &str, arguments: serde_json::Value) -> crate::agent::ToolCall {
        crate::agent::ToolCall {
            name: name.to_string(),
            arguments,
        }
    }

    #[test]
    fn todo_replaces_the_list_and_acks_progress() {
        let mut state = PlanState::default();
        let reply = state
            .handle_todo_tool(&call(
                "Todo",
                serde_json::json!({"items": [
                    {"text": "fix a", "done": true},
                    {"text": "fix b", "done": false},
                ]}),
            ))
            .unwrap();
        assert_eq!(
            reply,
            PlanToolReply::Reply("Todo updated: 1/2 done.".to_string())
        );
        assert_eq!(state.next_undone_step(), Some(1));

        // The tail recites the list; it is empty before any todos exist.
        assert!(state.todo_tail().contains("[x] fix a"));
        assert!(state.todo_tail().contains("[ ] fix b"));
        assert!(PlanState::default().todo_tail().is_empty());
    }

    #[test]
    fn todo_names_a_bad_shape_instead_of_guessing() {
        let mut state = PlanState::default();
        let reply = state
            .handle_todo_tool(&call("Todo", serde_json::json!({"items": "not an array"})))
            .unwrap();
        let PlanToolReply::Reply(text) = reply else {
            panic!("bad shape must not finish the task");
        };
        assert!(text.contains("array"));
        // A malformed item inside the array is named too.
        let reply = state
            .handle_todo_tool(&call(
                "Todo",
                serde_json::json!({"items": [{"text": "ok"}]}),
            ))
            .unwrap();
        let PlanToolReply::Reply(text) = reply else {
            panic!()
        };
        assert!(text.contains("done"));
    }

    #[test]
    fn finish_task_bounces_once_on_undone_todos_then_honors_the_second_attempt() {
        let mut state = PlanState::default();
        state
            .handle_todo_tool(&call(
                "Todo",
                serde_json::json!({"items": [{"text": "fix a", "done": false}]}),
            ))
            .unwrap();
        // First attempt with an undone todo: bounced, task continues.
        let first = state
            .handle_todo_tool(&call("FinishTask", serde_json::json!({"answer": "done!"})))
            .unwrap();
        assert!(matches!(&first, PlanToolReply::Reply(t) if t.contains("remain undone")));
        // Second attempt is honored — a stuck task can still end.
        let second = state
            .handle_todo_tool(&call("FinishTask", serde_json::json!({"answer": "done!"})))
            .unwrap();
        assert_eq!(second, PlanToolReply::Finish("done!".to_string()));
    }

    #[test]
    fn finish_task_with_a_clean_list_ends_immediately() {
        let mut state = PlanState::default();
        let reply = state
            .handle_todo_tool(&call("FinishTask", serde_json::json!({"answer": "42"})))
            .unwrap();
        assert_eq!(reply, PlanToolReply::Finish("42".to_string()));
        // Ordinary tools pass through to dispatch untouched.
        assert!(state
            .handle_todo_tool(&call("Read", serde_json::json!({"file_path": "a"})))
            .is_none());
    }

    #[test]
    fn single_mode_prompt_and_tool_names_carry_the_converged_set() {
        let prompt = single_mode_system_prompt(true);
        // The tool schemas now come from the llama-server chat template (the
        // `--jinja` tools array), NOT from a hand-listed `<tools>` block, and
        // the Hermes call format is no longer hand-taught in the prompt --
        // both were a redundant second copy of what the template injects.
        assert!(
            !prompt.contains("<tools>"),
            "the redundant <tools> block must be gone"
        );
        assert!(
            !prompt.contains("tool_call></tool_call> XML tags"),
            "the redundant call-format teaching must be gone"
        );
        // The retired machine's tools and modes are GONE from the prompt.
        for gone in [
            "CreatePlan",
            "StepDone",
            "RefuseStep",
            "ResumeExecution",
            "PLANNING mode",
        ] {
            assert!(!prompt.contains(gone), "{gone} must not appear");
        }
        assert!(prompt.contains("# Todos"));
        assert!(prompt.contains("exactly one tool call"));

        let state = PlanState::default();
        assert!(state.single_mode_tool_names(true).contains(&"Task"));
        assert!(!state.single_mode_tool_names(false).contains(&"Task"));
        assert!(state.single_mode_tool_names(false).contains(&"Todo"));
    }
}
