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
    /// Set by `RefuseStep`: the step is retired -- `next_undone_step`
    /// skips it, so `ResumeExecution` can never re-enter a step already
    /// refused as written (the 2026-07-12 doom loop: an impossible step 0
    /// was resumed verbatim for 200 turns). A revision arrives as a NEW
    /// step via `AddStep`, informed by the refusal reason.
    #[serde(default)]
    pub refused: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Plan {
    pub goal: String,
    pub steps: Vec<PlanStep>,
}

/// `Task` gets its own line because it's a union tool a subagent host
/// must never advertise (FR-016's one-level nesting cap: `run_loop` rejects
/// any `Task` call from a subagent, so listing it would just spend a
/// guaranteed-failing turn). `AskUserQuestion` below gets the identical
/// treatment for its own reason.
const TASK_TOOL_LINE: &str = r#"{"type": "function", "function": {"name": "Task", "description": "Delegate substantial, self-contained work (extensive exploration, a large search, a bulky sub-investigation) to an isolated subagent instead of doing it inline. This conversation is shared across the WHOLE task, not just this step -- everything you do here stays visible to every later step too, so keep it lean: reach for Task when a piece of work would otherwise flood this shared history with exploration detail nobody later needs, and only the outcome actually matters going forward.", "parameters": {"type": "object", "properties": {"prompt": {"type": "string"}}, "required": ["prompt"]}}}"#;

/// `AskUserQuestion` gets its own line for the same reason `Task` does:
/// only the top-level host can service it (`SubagentBackend::execute_tool`
/// has no AskUserQuestion route -- a subagent's questions have no user to
/// reach), so the subagent flavor must not advertise it, and Planning's
/// grammar allowed-set must not make it samplable there either.
const ASK_USER_QUESTION_TOOL_LINE: &str = r#"{"type": "function", "function": {"name": "AskUserQuestion", "description": "Ask the user directly if the request is genuinely ambiguous.", "parameters": {"type": "object", "properties": {"header": {"type": "string"}, "question": {"type": "string"}, "options": {"type": "array", "items": {"type": "object", "properties": {"label": {"type": "string"}, "description": {"type": "string"}}, "required": ["label"]}}, "multiSelect": {"type": "boolean"}}, "required": ["header", "question", "options"]}}}"#;

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

/// The single-mode `<tools>` lines: 9 tools, down from the union's 15.
/// `Update` absorbs Write+Edit (argument shape selects create/overwrite
/// vs surgical replace); `Todo` absorbs the five plan tools.
const SINGLE_MODE_TOOL_LINES: &[&str] = &[
    r#"{"type": "function", "function": {"name": "Read", "description": "Read a file from disk.", "parameters": {"type": "object", "properties": {"file_path": {"type": "string"}, "offset": {"type": "number"}, "limit": {"type": "number"}}, "required": ["file_path"]}}}"#,
    r#"{"type": "function", "function": {"name": "Update", "description": "Create or modify a file. Pass content to create or fully overwrite the file. Pass old_string and new_string (and no content) to replace one exact occurrence in place.", "parameters": {"type": "object", "properties": {"file_path": {"type": "string"}, "content": {"type": "string"}, "old_string": {"type": "string"}, "new_string": {"type": "string"}, "replace_all": {"type": "boolean"}}, "required": ["file_path"]}}}"#,
    r#"{"type": "function", "function": {"name": "Bash", "description": "Run a shell command.", "parameters": {"type": "object", "properties": {"command": {"type": "string"}, "timeout": {"type": "number"}}, "required": ["command"]}}}"#,
    r#"{"type": "function", "function": {"name": "Grep", "description": "Search file contents with a regular expression. Omit path to search the current working directory. Results are capped at 100 matches -- for counting or exhaustive listings use a Bash pipeline instead.", "parameters": {"type": "object", "properties": {"pattern": {"type": "string"}, "path": {"type": "string"}, "glob": {"type": "string"}}, "required": ["pattern"]}}}"#,
    r#"{"type": "function", "function": {"name": "Glob", "description": "Find files by name pattern. The pattern is a single wildcard expression, e.g. "bug_*.txt" or "*.rs" -- never a space-separated list of literal filenames, that matches nothing. Omit path to search the current working directory. Results are capped at the 100 most recently modified matches -- for counting or exhaustive listings use a Bash pipeline instead.", "parameters": {"type": "object", "properties": {"pattern": {"type": "string"}, "path": {"type": "string"}}, "required": ["pattern"]}}}"#,
    r#"{"type": "function", "function": {"name": "Todo", "description": "Replace your todo list. Keep one for any multi-step task: one item per file or unit of work, done: true as you finish each. Calling this only records progress -- keep working afterwards.", "parameters": {"type": "object", "properties": {"items": {"type": "array", "items": {"type": "object", "properties": {"text": {"type": "string"}, "done": {"type": "boolean"}}, "required": ["text", "done"]}}}, "required": ["items"]}}}"#,
];

const SINGLE_MODE_FINISH_LINE: &str = r#"{"type": "function", "function": {"name": "FinishTask", "description": "End the task and deliver your final answer to the user. Only call this after you have verified the outcome yourself.", "parameters": {"type": "object", "properties": {"answer": {"type": "string"}}, "required": ["answer"]}}}"#;

fn build_single_mode_system_prompt(
    allow_task: bool,
    dialect: crate::inference::ToolDialect,
) -> String {
    let mut tools: Vec<&str> = SINGLE_MODE_TOOL_LINES.to_vec();
    if allow_task {
        tools.push(TASK_TOOL_LINE);
        tools.push(ASK_USER_QUESTION_TOOL_LINE);
    }
    tools.push(SINGLE_MODE_FINISH_LINE);
    let tools_block = tools.join(
        "
",
    );
    let call_instructions = dialect.call_format_instructions();
    let unclear_action = if allow_task {
        "call AskUserQuestion, and keep asking until the task is clear"
    } else {
        "call FinishTask explaining exactly what is missing"
    };

    format!(
        r#"You are doce, a local coding agent.

# Tools

You may call one or more functions to assist with the user query.

You are provided with function signatures within <tools></tools> XML tags:
<tools>
{tools_block}
</tools>

{call_instructions}

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

/// THE single-mode system prompt — cached per (host flavor, dialect),
/// byte-stable within a pairing (the KV-prefix invariant, unchanged from
/// the union prompt this replaces).
pub fn single_mode_system_prompt(
    allow_task: bool,
    dialect: crate::inference::ToolDialect,
) -> &'static str {
    use std::sync::OnceLock;
    static WITH_TASK_HERMES: OnceLock<String> = OnceLock::new();
    static WITHOUT_TASK_HERMES: OnceLock<String> = OnceLock::new();
    static WITH_TASK_MINICPM: OnceLock<String> = OnceLock::new();
    static WITHOUT_TASK_MINICPM: OnceLock<String> = OnceLock::new();
    let cell = match (allow_task, dialect) {
        (true, crate::inference::ToolDialect::HermesJson) => &WITH_TASK_HERMES,
        (false, crate::inference::ToolDialect::HermesJson) => &WITHOUT_TASK_HERMES,
        (true, crate::inference::ToolDialect::MiniCpmXml) => &WITH_TASK_MINICPM,
        (false, crate::inference::ToolDialect::MiniCpmXml) => &WITHOUT_TASK_MINICPM,
    };
    cell.get_or_init(|| build_single_mode_system_prompt(allow_task, dialect))
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
                        refused: false,
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
        self.plan.steps.iter().position(|s| !s.done && !s.refused)
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
    use crate::inference::ToolDialect;

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
        let prompt = single_mode_system_prompt(true, ToolDialect::HermesJson);
        for tool in [
            "Read",
            "Update",
            "Bash",
            "Grep",
            "Glob",
            "Todo",
            "FinishTask",
            "Task",
        ] {
            assert!(
                prompt.contains(&format!("\"name\": \"{tool}\"")),
                "prompt must list {tool}"
            );
        }
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
