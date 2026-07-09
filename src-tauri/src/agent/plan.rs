//! Goal/plan state for the two-state agent loop: a single `run_loop` call
//! whose available tools depend on an external state the backend itself
//! carries (`LoopState`) — not two separate loops. The state machine
//! itself lives here as `PlanState`: both production
//! (`commands::agent::RealBackend`) and the benchmark's `PlanExecBackend`
//! (`tests/agent_benchmark.rs`) embed this same struct as their
//! `AgentBackend`'s `plan_state` field, rather than each independently
//! reimplementing the state shape and prompts.
//!
//! Prompt architecture (stable prefix): `messages[0]` is ONE immutable
//! union prompt per host (`plan_system_prompt`) that never changes across
//! state transitions, so `inference::PromptSession`'s KV prefix survives
//! every Planning↔Executing and step→step boundary. Everything volatile —
//! which mode is active, the current step's goal/description framing, a
//! refusal being revised, the plan-recitation checklist — rides in ONE
//! per-turn tail message (`PlanState::state_tail`) appended after the
//! whole conversation. Per-state tool availability is enforced at the
//! sampler (`PlanState::allowed_tool_names` + the grammar name-enum gate
//! in `inference`), not by swapping prompts: a tool outside the current
//! state's set is unsamplable, which is strictly stronger than the old
//! prompt-level gating.
//!
//! `Planning` maintains the plan via ordinary tool calls (`CreatePlan`/
//! `AddStep`) and can independently verify results with read-only tools —
//! the same grammar-constrained `<tool_call>` mechanism every other
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
//! visible for the rest of the task, which is also why the `Task` tool's
//! description below tells the model to delegate bulky, self-contained
//! work to it (a real subagent, isolated by construction) rather than
//! doing it all inline and flooding the shared history.
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

/// The union `<tools>` JSON lines for `plan_system_prompt` -- EVERY tool
/// either state can use, so the system prompt never changes when the state
/// does. Each line's text is carried over verbatim from the per-state
/// prompts this union replaced (benchmark-validated wording -- see
/// docs/reports/2026-07-09-small-model-context-tooling.md §5); where the
/// same tool appeared in both states with different schemas (`Read`'s
/// offset/limit, `Grep`'s glob), the superset schema wins, and where only
/// descriptions differed (`Glob`'s "Omit path" sentence), the fuller
/// description wins.
const UNION_TOOL_LINES: &[&str] = &[
    r#"{"type": "function", "function": {"name": "CreatePlan", "description": "Define the plan as a goal and a list of steps. Valid only once, when no plan exists yet. Follow the plan granularity rules below.", "parameters": {"type": "object", "properties": {"goal": {"type": "string"}, "steps": {"type": "array", "items": {"type": "string"}}}, "required": ["goal", "steps"]}}}"#,
    r#"{"type": "function", "function": {"name": "AddStep", "description": "Append one step to the existing plan.", "parameters": {"type": "object", "properties": {"description": {"type": "string"}}, "required": ["description"]}}}"#,
    r#"{"type": "function", "function": {"name": "ResumeExecution", "description": "Hand off to the next step that isn't done yet. Takes no arguments.", "parameters": {"type": "object", "properties": {}}}}"#,
    r#"{"type": "function", "function": {"name": "Read", "description": "Read a file from disk.", "parameters": {"type": "object", "properties": {"file_path": {"type": "string"}, "offset": {"type": "number"}, "limit": {"type": "number"}}, "required": ["file_path"]}}}"#,
    r#"{"type": "function", "function": {"name": "Write", "description": "Create or overwrite a file.", "parameters": {"type": "object", "properties": {"file_path": {"type": "string"}, "content": {"type": "string"}}, "required": ["file_path", "content"]}}}"#,
    r#"{"type": "function", "function": {"name": "Edit", "description": "Targeted in-place edit: replace old_string with new_string inside the file.", "parameters": {"type": "object", "properties": {"file_path": {"type": "string"}, "old_string": {"type": "string"}, "new_string": {"type": "string"}, "replace_all": {"type": "boolean"}}, "required": ["file_path", "old_string", "new_string"]}}}"#,
    r#"{"type": "function", "function": {"name": "Bash", "description": "Run a shell command.", "parameters": {"type": "object", "properties": {"command": {"type": "string"}, "timeout": {"type": "number"}}, "required": ["command"]}}}"#,
    r#"{"type": "function", "function": {"name": "Grep", "description": "Search file contents with a regular expression. Omit path to search the current working directory.", "parameters": {"type": "object", "properties": {"pattern": {"type": "string"}, "path": {"type": "string"}, "glob": {"type": "string"}}, "required": ["pattern"]}}}"#,
    r#"{"type": "function", "function": {"name": "Glob", "description": "Find files by name pattern. The pattern is a single wildcard expression, e.g. \"bug_*.txt\" or \"*.rs\" -- never a space-separated list of literal filenames, that matches nothing. Omit path to search the current working directory.", "parameters": {"type": "object", "properties": {"pattern": {"type": "string"}, "path": {"type": "string"}}, "required": ["pattern"]}}}"#,
];

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

const UNION_TOOL_LINES_TAIL: &[&str] = &[
    r#"{"type": "function", "function": {"name": "StepDone", "description": "Call this once you have actually completed the step, not when you believe you're close.", "parameters": {"type": "object", "properties": {"summary": {"type": "string"}}, "required": ["summary"]}}}"#,
    r#"{"type": "function", "function": {"name": "RefuseStep", "description": "Call this if the step cannot be completed as described (unclear, blocked, or wrong). Explain why -- your reason is used to revise the plan.", "parameters": {"type": "object", "properties": {"reason": {"type": "string"}}, "required": ["reason"]}}}"#,
    r#"{"type": "function", "function": {"name": "FinishTask", "description": "End the task and deliver your final answer to the user. Only call this after you have verified the outcome yourself.", "parameters": {"type": "object", "properties": {"answer": {"type": "string"}}, "required": ["answer"]}}}"#,
];

/// Renders the ONE union system prompt (see `plan_system_prompt` for the
/// cached, byte-stable entry point). The prose is the merge of the two
/// per-state prompts this replaces: the Planning prompt's supervisor
/// framing, "# Plan granularity" and "# Verification" sections, and the
/// Executing prompt's StepDone/RefuseStep rule -- reorganized around a
/// two-mode narrative, not rewritten (the wording survived a seven-run
/// benchmark ladder; only mode-referencing glue changed, e.g. "You return
/// here automatically" became "You return to PLANNING mode automatically").
/// Everything volatile (which mode is active, the current step, refusals,
/// the plan checklist) lives in `PlanState::state_tail`, never here.
fn build_plan_system_prompt(allow_task: bool) -> String {
    let mut tools: Vec<&str> = UNION_TOOL_LINES.to_vec();
    if allow_task {
        // Top-level-only tools, both keyed on the same host flag: `Task`
        // because of FR-016's nesting cap, `AskUserQuestion` because only
        // the top-level host has an AskUserQuestion route to a real user
        // (see each line's own doc comment).
        tools.push(TASK_TOOL_LINE);
        tools.push(ASK_USER_QUESTION_TOOL_LINE);
    }
    tools.extend(UNION_TOOL_LINES_TAIL);
    let tools_block = tools.join("\n");
    let planning_names = if allow_task {
        "CreatePlan, AddStep, ResumeExecution, Read, Grep, Glob, AskUserQuestion, FinishTask"
    } else {
        "CreatePlan, AddStep, ResumeExecution, Read, Grep, Glob, FinishTask"
    };
    let executing_names = if allow_task {
        "Read, Write, Edit, Bash, Grep, Glob, Task, StepDone, RefuseStep"
    } else {
        "Read, Write, Edit, Bash, Grep, Glob, StepDone, RefuseStep"
    };

    format!(
        r#"You are a task agent that works in two modes, switched by tool calls. In PLANNING mode you are a planning supervisor: you maintain a plan and hand off each step's actual work to EXECUTING mode -- you do not personally edit files or run commands while planning. In EXECUTING mode you are executing one step of that larger plan with file/shell tools. The last message of the conversation always tells you which mode you are in right now.

# Tools

You may call one or more functions to assist with the user query.

You are provided with function signatures within <tools></tools> XML tags:
<tools>
{tools_block}
</tools>

For each function call, return a json object with function name and arguments within <tool_call></tool_call> XML tags:
<tool_call>
{{"name": <function-name>, "arguments": <args-json-object>}}
</tool_call>

# Modes

PLANNING mode tools: {planning_names}.
EXECUTING mode tools: {executing_names}.
Only the current mode's tools are available to you -- the last message of the conversation names the mode.

# Plan granularity

Each step is executed with its own limited number of turns, so a step must be small enough to actually finish within that. If the task repeats similar work across multiple items (e.g. multiple files), create ONE STEP PER ITEM -- a task covering 20 files needs 20 per-file steps. NEVER write a step like "repeat this process for the remaining files": a bundled step silently stops partway and the remaining items are lost. Use AddStep to extend or correct the plan afterward -- do not call CreatePlan again, that would discard progress already made. Plans you may see in earlier conversation history are finished -- each new user request starts with no plan.

# Executing a step

Each step runs in EXECUTING mode, framed with the overall goal plus that step's own description. You must end every step by calling StepDone or RefuseStep -- never answer in plain text there, that would end the WHOLE task, not just this step.

# Verification

You return to PLANNING mode automatically once every step reports done, or when a step reports it could not be completed (its reason will be given to you). A step reporting done is a CLAIM, not proof. Before giving your final answer you MUST independently verify the outcome with Read/Grep/Glob -- re-read the changed files or search for remaining problems yourself. If verification shows something is genuinely still wrong, use AddStep and ResumeExecution rather than accepting the claim.

Once you have verified the task is genuinely, completely done, call FinishTask with your final answer. Every response you give must be exactly one <tool_call>."#
    )
}

/// THE system prompt for the plan engine -- one immutable string per host
/// flavor, containing the union `<tools>` block (all Planning + Executing
/// tools) and both rules sections. Byte-stable by construction: repeated
/// calls return the SAME cached `&'static str`, so a host that seeds
/// `messages[0]` with this can never see it drift across plan-state
/// transitions -- which is what lets `PromptSession`'s KV prefix survive
/// every Planning<->Executing and step->step boundary instead of collapsing
/// on each per-state prompt swap. All per-turn state rides in ONE tail
/// message (`PlanState::state_tail`); per-state tool availability is
/// enforced at the sampler instead (`PlanState::allowed_tool_names` +
/// grammar name-enum gating), not by swapping prompts.
///
/// A builder rather than a single `const` because the union tool list
/// genuinely differs by host: `allow_task = false` (subagent hosts) omits
/// the `Task` tool line entirely (FR-016). What matters for KV reuse is
/// byte-stability WITHIN a host across turns, and each host always passes
/// the same flag.
pub fn plan_system_prompt(allow_task: bool) -> &'static str {
    use std::sync::OnceLock;
    static WITH_TASK: OnceLock<String> = OnceLock::new();
    static WITHOUT_TASK: OnceLock<String> = OnceLock::new();
    let cell = if allow_task { &WITH_TASK } else { &WITHOUT_TASK };
    cell.get_or_init(|| build_plan_system_prompt(allow_task))
}

/// The five tools owned by the plan state machine itself — used by the
/// frontend (via ipc.ts's mirror of this list) to keep plan activity
/// invisible in the transcript, and by hosts to route calls.
pub const PLAN_TOOL_NAMES: [&str; 6] = [
    "CreatePlan",
    "AddStep",
    "ResumeExecution",
    "StepDone",
    "RefuseStep",
    "FinishTask",
];

/// What handling a plan tool produced: an ordinary result string fed back
/// into the loop, or the task's final answer (`FinishTask`) — hosts map
/// `Finish` onto `agent::ToolExecution::Finish`, ending `run_loop`.
/// Putting "done" behind a tool call is what lets BOTH states run with
/// grammar-required tool calls: free-text replies (which a small model
/// degrades into after repetitive stretches — observed: a bare
/// "ResumeExecution" text answer ended a whole task) become unsamplable.
#[derive(Debug, Clone, PartialEq)]
pub enum PlanToolReply {
    Reply(String),
    Finish(String),
}

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
    /// `state_tail` renders a Planning tail — carries the refusal
    /// reason into that one revision turn without lingering after.
    refusal_context: Option<String>,
}

/// The tool names each state may call, mirroring EXACTLY what that state's
/// system prompt advertised before the prompts were unified -- now enforced
/// at the sampler (grammar name-enum gating) instead of by prompt swaps.
/// Static slices so hosts can pass them straight into a `generate` call
/// without allocating per turn.
const PLANNING_ALLOWED_TOOLS: &[&str] = &[
    "CreatePlan",
    "AddStep",
    "ResumeExecution",
    "Read",
    "Grep",
    "Glob",
    "AskUserQuestion",
    "FinishTask",
];
/// A subagent's Planning turns must not be able to sample
/// `AskUserQuestion` -- `SubagentBackend` has no route to a user, so the
/// call could only ever come back as "Error: unknown tool", a
/// guaranteed-wasted turn (the same reasoning as `Task`/FR-016 below).
const PLANNING_ALLOWED_TOOLS_NO_ASK: &[&str] = &[
    "CreatePlan",
    "AddStep",
    "ResumeExecution",
    "Read",
    "Grep",
    "Glob",
    "FinishTask",
];
const EXECUTING_ALLOWED_TOOLS: &[&str] = &[
    "Read", "Write", "Edit", "Bash", "Grep", "Glob", "Task", "StepDone", "RefuseStep",
];
/// FR-016: a subagent's Executing turns must not be able to sample `Task`.
const EXECUTING_ALLOWED_TOOLS_NO_TASK: &[&str] = &[
    "Read", "Write", "Edit", "Bash", "Grep", "Glob", "StepDone", "RefuseStep",
];

impl PlanState {
    /// The single per-turn tail message a host appends AFTER the whole
    /// conversation, every `generate` call -- all the state that used to be
    /// spread across per-state system-prompt swaps (the mode, the current
    /// step's framing, a refusal being revised) plus the recitation
    /// checklist Task 10 used to push as its own separate message, folded
    /// into ONE message. Keeping every volatile piece here is what lets
    /// `messages[0]` stay byte-stable (see `plan_system_prompt`), so a
    /// `PromptSession` re-decodes only this tail plus the newest tool
    /// exchange each turn instead of the entire history.
    ///
    /// `&mut` because rendering a Planning tail consumes the refusal
    /// context -- the reason appears in exactly one revision turn, same
    /// consumption semantic the old `system_prompt` had.
    ///
    /// The wording reuses the retired per-state prompts' validated prose
    /// verbatim where it was per-turn by nature: the Executing frame
    /// ("Overall goal:"/"Your current step:" -- confirmed against the real
    /// model that a step description alone is not self-contained), its
    /// closing StepDone/RefuseStep rule, and the refusal-revision paragraph.
    pub fn state_tail(&mut self) -> String {
        let mut sections: Vec<String> = Vec::new();
        match self.state {
            LoopState::Planning => {
                sections.push(
                    "You are in PLANNING mode -- maintain the plan and hand off each step's actual work to EXECUTING mode; you do not personally edit files or run commands from here.".to_string(),
                );
                if let Some(reason) = self.refusal_context.take() {
                    sections.push(format!(
                        "The previous step could not be completed. Reason given: {reason}\n\nRevise the plan accordingly (AddStep, then ResumeExecution)."
                    ));
                }
            }
            LoopState::Executing { step_index } => {
                sections.push(format!(
                    "You are in EXECUTING mode -- you are executing one step of a larger plan.\n\nOverall goal: {}\nYour current step: {}\n\nYou must end by calling StepDone or RefuseStep -- never answer in plain text here, that would end the WHOLE task, not just this step.",
                    self.plan.goal, self.plan.steps[step_index].description
                ));
            }
        }
        if let Some(recitation) = self.recitation_text() {
            sections.push(recitation);
        }
        sections.join("\n\n")
    }

    /// The tool names the model may call THIS turn -- passed by hosts into
    /// the sampler's grammar name-enum gate on every `generate` call, so a
    /// tool outside the current state's set is unsamplable rather than
    /// merely un-advertised (the union prompt lists everything). Mirrors
    /// exactly the per-state tool sets each host flavor's prompt
    /// advertises. `allow_task = false` (subagent hosts) drops the two
    /// top-level-only tools from their respective states: `Task` while
    /// Executing (FR-016) and `AskUserQuestion` while Planning (no
    /// subagent route to a user).
    pub fn allowed_tool_names(&self, allow_task: bool) -> &'static [&'static str] {
        match self.state {
            LoopState::Planning if allow_task => PLANNING_ALLOWED_TOOLS,
            LoopState::Planning => PLANNING_ALLOWED_TOOLS_NO_ASK,
            LoopState::Executing { .. } if allow_task => EXECUTING_ALLOWED_TOOLS,
            LoopState::Executing { .. } => EXECUTING_ALLOWED_TOOLS_NO_TASK,
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
    pub fn handle_plan_tool(&mut self, call: &crate::agent::ToolCall) -> Option<PlanToolReply> {
        if self.state == LoopState::Planning && call.name == "FinishTask" {
            return Some(match call.arguments.get("answer").and_then(|v| v.as_str()) {
                Some(answer) => PlanToolReply::Finish(answer.to_string()),
                None => PlanToolReply::Reply(
                    "Error: FinishTask requires an answer argument".to_string(),
                ),
            });
        }
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
                    // The granularity nudge rides the result itself — it
                    // lands in the freshest context at exactly the moment
                    // the model can still cheaply revise, which binds far
                    // more reliably on a small model than the same rule
                    // sitting only in the system prompt (observed: bundled
                    // plans slipped through prompt-only guidance).
                    format!(
                        "Plan created with {step_count} steps. Review it now: if any step bundles multiple items (e.g. \"repeat for the remaining files\"), fix that with AddStep -- one step per item -- BEFORE continuing. Then call ResumeExecution to begin."
                    )
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
                        // Same decision-moment placement as CreatePlan's
                        // nudge: the verification requirement is restated
                        // right when the model is about to write its final
                        // answer (observed: prompt-only placement let it
                        // answer with confident, unverified success claims).
                        format!(
                            "Step {step_index} done. All steps report done -- back to planning. Before giving your final answer you MUST VERIFY the outcome yourself with Read/Grep/Glob (a step's claim is not proof). If anything is still wrong or unfinished, AddStep then ResumeExecution."
                        )
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
        Some(PlanToolReply::Reply(result))
    }

    pub fn next_undone_step(&self) -> Option<usize> {
        self.plan.steps.iter().position(|s| !s.done)
    }

    pub fn has_plan(&self) -> bool {
        !self.plan.steps.is_empty()
    }

    /// The live plan restated for the context TAIL — Manus's recitation
    /// trick: on long tasks the system prompt drifts into the
    /// lost-in-the-middle zone; a compact checklist at the end of the
    /// context keeps the global plan inside the model's recent attention
    /// span. `None` when no plan exists (trivial turns pay nothing).
    /// Private: hosts get it folded into `state_tail`, never separately.
    fn recitation_text(&self) -> Option<String> {
        if !self.has_plan() {
            return None;
        }
        let done = self.plan.steps.iter().filter(|s| s.done).count();
        let current = match self.state {
            LoopState::Executing { step_index } => Some(step_index),
            LoopState::Planning => None,
        };
        let mut lines = vec![format!("Plan status -- goal: {}", self.plan.goal)];
        for (i, step) in self.plan.steps.iter().enumerate() {
            let mark = if step.done {
                "[x]"
            } else if current == Some(i) {
                "[>]"
            } else {
                "[ ]"
            };
            lines.push(format!("{mark} {}", step.description));
        }
        lines.push(format!("({done}/{} done)", self.plan.steps.len()));
        Some(lines.join("\n"))
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

    /// The union prompt must advertise EVERY tool from BOTH states (the
    /// whole point: messages[0] never changes when the state does), and
    /// carry both rules sections' validated prose intact.
    #[test]
    fn plan_system_prompt_contains_the_union_toolset_and_both_rules_sections() {
        let prompt = plan_system_prompt(true);
        for tool in [
            "CreatePlan",
            "AddStep",
            "ResumeExecution",
            "AskUserQuestion",
            "FinishTask",
            "Read",
            "Write",
            "Edit",
            "Bash",
            "Grep",
            "Glob",
            "Task",
            "StepDone",
            "RefuseStep",
        ] {
            assert!(
                prompt.contains(&format!("\"name\": \"{tool}\"")),
                "union prompt must list {tool}"
            );
        }
        assert!(prompt.contains("planning supervisor"));
        assert!(prompt.contains("# Plan granularity"));
        assert!(prompt.contains("ONE STEP PER ITEM"));
        assert!(prompt.contains("# Verification"));
        assert!(prompt.contains("A step reporting done is a CLAIM, not proof."));
        assert!(prompt.contains("Every response you give must be exactly one <tool_call>"));
    }

    /// FR-016's one-level nesting cap means `run_loop` rejects ANY `Task`
    /// call from a subagent -- so a subagent host's union prompt must not
    /// advertise `Task` at all. `AskUserQuestion` gets the identical
    /// treatment (F4, final whole-branch review): `SubagentBackend` has no
    /// route to a user, so advertising it (in the tools block OR the
    /// "# Modes" PLANNING list) invites a guaranteed "unknown tool" turn.
    /// Repeated calls per variant must also return the SAME cached string
    /// (pointer equality): byte-stability of messages[0] within a host is
    /// the entire point of this prompt.
    #[test]
    fn plan_system_prompt_omits_task_and_ask_user_for_subagents_and_is_cached_per_variant() {
        let sub = plan_system_prompt(false);
        assert!(!sub.contains("\"name\": \"Task\""));
        assert!(!sub.contains("AskUserQuestion"), "no tool line and no # Modes mention");
        assert!(sub.contains(
            "PLANNING mode tools: CreatePlan, AddStep, ResumeExecution, Read, Grep, Glob, FinishTask."
        ));
        assert!(sub.contains("\"name\": \"StepDone\""));
        assert!(sub.contains("\"name\": \"FinishTask\""));
        let top = plan_system_prompt(true);
        assert!(top.contains("\"name\": \"Task\""));
        assert!(top.contains("\"name\": \"AskUserQuestion\""));
        assert!(top.contains(
            "PLANNING mode tools: CreatePlan, AddStep, ResumeExecution, Read, Grep, Glob, AskUserQuestion, FinishTask."
        ));
        assert!(std::ptr::eq(plan_system_prompt(true), plan_system_prompt(true)));
        assert!(std::ptr::eq(plan_system_prompt(false), plan_system_prompt(false)));
    }

    #[test]
    fn state_tail_announces_planning_mode_for_a_fresh_state() {
        let mut ps = PlanState::default();
        let tail = ps.state_tail();
        assert!(tail.contains("PLANNING"));
        assert!(
            !tail.contains("Plan status"),
            "no plan exists yet -- the tail must not carry an empty checklist"
        );
    }

    #[test]
    fn state_tail_frames_the_executing_step_with_goal_step_and_checklist() {
        let mut ps = PlanState::default();
        ps.handle_plan_tool(&call(
            "CreatePlan",
            serde_json::json!({"goal": "ship it", "steps": ["write tests", "run tests"]}),
        ));
        ps.handle_plan_tool(&call("ResumeExecution", serde_json::json!({})));
        let tail = ps.state_tail();
        assert!(tail.contains("EXECUTING"));
        assert!(tail.contains("ship it"), "the overall goal must be in the tail");
        assert!(tail.contains("Your current step: write tests"));
        assert!(
            tail.contains("StepDone") && tail.contains("RefuseStep"),
            "the step-ending rule must ride the tail"
        );
        // Task 10's recitation checklist is folded into this ONE tail message.
        assert!(tail.contains("[>] write tests"));
        assert!(tail.contains("[ ] run tests"));
        assert!(tail.contains("(0/2 done)"));
    }

    #[test]
    fn state_tail_carries_a_refusal_reason_exactly_once() {
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

        let tail = ps.state_tail();
        assert!(
            tail.contains("the file does not exist"),
            "refusal reason must reach the revision turn's tail"
        );
        // Consumed: the next tail is clean again.
        let tail2 = ps.state_tail();
        assert!(!tail2.contains("the file does not exist"));
    }

    /// The grammar-level gate must mirror EXACTLY the tool set each host
    /// flavor's prompt advertises per state -- Planning's read-only+plan
    /// set (minus `AskUserQuestion` for subagents, which have no route to
    /// a user), Executing's file/shell set (minus `Task` for subagents,
    /// FR-016).
    #[test]
    fn allowed_tool_names_mirror_the_per_state_toolsets() {
        let mut ps = PlanState::default();
        assert_eq!(
            ps.allowed_tool_names(true),
            [
                "CreatePlan",
                "AddStep",
                "ResumeExecution",
                "Read",
                "Grep",
                "Glob",
                "AskUserQuestion",
                "FinishTask"
            ]
        );
        // A subagent's Planning turns must not be able to sample
        // AskUserQuestion -- SubagentBackend cannot service it (F4).
        assert_eq!(
            ps.allowed_tool_names(false),
            [
                "CreatePlan",
                "AddStep",
                "ResumeExecution",
                "Read",
                "Grep",
                "Glob",
                "FinishTask"
            ]
        );

        ps.handle_plan_tool(&call(
            "CreatePlan",
            serde_json::json!({"goal": "g", "steps": ["a"]}),
        ));
        ps.handle_plan_tool(&call("ResumeExecution", serde_json::json!({})));
        assert_eq!(
            ps.allowed_tool_names(true),
            ["Read", "Write", "Edit", "Bash", "Grep", "Glob", "Task", "StepDone", "RefuseStep"]
        );
        assert_eq!(
            ps.allowed_tool_names(false),
            ["Read", "Write", "Edit", "Bash", "Grep", "Glob", "StepDone", "RefuseStep"]
        );
    }

    use crate::agent::ToolCall;

    fn call(name: &str, arguments: serde_json::Value) -> ToolCall {
        ToolCall {
            name: name.to_string(),
            arguments,
        }
    }

    /// Unwraps the ordinary-reply variant — the shape every test below
    /// except the FinishTask ones expects.
    fn reply(outcome: Option<PlanToolReply>) -> String {
        match outcome.expect("expected a handled plan tool") {
            PlanToolReply::Reply(text) => text,
            PlanToolReply::Finish(answer) => panic!("expected Reply, got Finish({answer:?})"),
        }
    }

    #[test]
    fn create_plan_then_resume_moves_to_executing_the_first_step() {
        let mut ps = PlanState::default();
        assert_eq!(ps.state, LoopState::Planning);
        assert!(!ps.has_plan());

        let result = reply(ps.handle_plan_tool(&call(
            "CreatePlan",
            serde_json::json!({"goal": "fix bugs", "steps": ["fix a", "fix b"]}),
        )));
        assert!(result.contains("2 steps"));
        assert!(
            result.contains("one step per item"),
            "the CreatePlan result must carry the granularity nudge at the decision moment"
        );
        assert!(ps.has_plan());
        assert_eq!(ps.plan.goal, "fix bugs");
        assert_eq!(ps.state, LoopState::Planning, "CreatePlan alone does not start execution");

        let result = reply(ps.handle_plan_tool(&call("ResumeExecution", serde_json::json!({}))));
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
        let second = reply(ps.handle_plan_tool(&call(
            "CreatePlan",
            serde_json::json!({"goal": "other", "steps": ["x"]}),
        )));
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

        let result = reply(ps.handle_plan_tool(&call("StepDone", serde_json::json!({"summary": "did a"}))));
        assert!(ps.plan.steps[0].done);
        assert_eq!(ps.state, LoopState::Executing { step_index: 1 });
        assert!(result.contains("step 1"));

        let all_done = reply(ps.handle_plan_tool(&call("StepDone", serde_json::json!({"summary": "did b"}))));
        assert!(ps.plan.steps[1].done);
        assert_eq!(ps.state, LoopState::Planning, "all done returns to planning for review");
        assert!(
            all_done.contains("VERIFY"),
            "the all-steps-done result must instruct verification at the decision moment"
        );
    }

    #[test]
    fn add_step_rejects_when_no_plan_exists_yet() {
        let mut ps = PlanState::default();
        assert!(!ps.has_plan());

        let result = reply(ps.handle_plan_tool(&call(
            "AddStep",
            serde_json::json!({"description": "orphan step"}),
        )));
        assert!(result.starts_with("Error"));
        assert!(!ps.has_plan(), "AddStep must not mutate the plan when none exists");
        assert!(ps.plan.steps.is_empty());

        // A subsequent CreatePlan still works normally.
        let created = reply(ps.handle_plan_tool(&call(
            "CreatePlan",
            serde_json::json!({"goal": "g", "steps": ["a"]}),
        )));
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
        let rejected = reply(ps.handle_plan_tool(&call("Write", serde_json::json!({}))));
        assert!(rejected.starts_with("Error"));

        ps.handle_plan_tool(&call(
            "CreatePlan",
            serde_json::json!({"goal": "g", "steps": ["a"]}),
        ));
        ps.handle_plan_tool(&call("ResumeExecution", serde_json::json!({})));
        // Executing: file/shell/Task pass through, plan-editing is rejected.
        assert!(ps.handle_plan_tool(&call("Write", serde_json::json!({}))).is_none());
        assert!(ps.handle_plan_tool(&call("Task", serde_json::json!({}))).is_none());
        let rejected = reply(ps.handle_plan_tool(&call("AddStep", serde_json::json!({"description": "x"}))));
        assert!(rejected.starts_with("Error"));
    }

    #[test]
    fn finish_task_ends_from_planning_and_is_rejected_while_executing() {
        let mut ps = PlanState::default();
        match ps
            .handle_plan_tool(&call(
                "FinishTask",
                serde_json::json!({"answer": "done and verified"}),
            ))
            .unwrap()
        {
            PlanToolReply::Finish(answer) => assert_eq!(answer, "done and verified"),
            other => panic!("expected Finish, got {other:?}"),
        }

        // Missing answer degrades to an ordinary correctable error.
        let no_answer = reply(ps.handle_plan_tool(&call("FinishTask", serde_json::json!({}))));
        assert!(no_answer.starts_with("Error"));

        // While Executing, finishing requires returning to Planning first
        // (StepDone/RefuseStep) -- FinishTask is state-gated out.
        ps.handle_plan_tool(&call(
            "CreatePlan",
            serde_json::json!({"goal": "g", "steps": ["a"]}),
        ));
        ps.handle_plan_tool(&call("ResumeExecution", serde_json::json!({})));
        let rejected = reply(ps.handle_plan_tool(&call(
            "FinishTask",
            serde_json::json!({"answer": "nope"}),
        )));
        assert!(rejected.starts_with("Error"));
    }

    #[test]
    fn recitation_text_formats_the_live_plan_for_context_tail() {
        // Fresh state with no plan returns None
        let ps = PlanState::default();
        assert!(ps.recitation_text().is_none(), "fresh state should have no recitation");

        // Create a 3-step plan
        let mut ps = PlanState::default();
        ps.handle_plan_tool(&call(
            "CreatePlan",
            serde_json::json!({"goal": "fix bugs", "steps": ["fix a", "fix b", "fix c"]}),
        ));

        // Mark the first step done, then move to executing the second
        ps.handle_plan_tool(&call("ResumeExecution", serde_json::json!({})));
        ps.handle_plan_tool(&call("StepDone", serde_json::json!({"summary": "done"})));

        // Now in Executing{1}, with step 0 done
        assert_eq!(ps.state, LoopState::Executing { step_index: 1 });
        assert!(ps.plan.steps[0].done);

        let recitation = ps.recitation_text().expect("should have recitation with active plan");

        // Each step must carry ITS OWN state marker -- asserting the
        // marker and the description together (not `contains(desc) ||
        // contains(marked desc)`, which the description alone satisfied
        // regardless of marker placement).
        assert!(recitation.contains("fix bugs"), "should contain goal");
        assert!(
            recitation.contains("[x] fix a"),
            "done step must be marked [x], got: {recitation:?}"
        );
        assert!(
            recitation.contains("[>] fix b"),
            "current step must be marked [>], got: {recitation:?}"
        );
        assert!(
            recitation.contains("[ ] fix c"),
            "undone step must be marked [ ], got: {recitation:?}"
        );
        assert!(recitation.contains("1/3"), "should show progress counter");
    }
}
