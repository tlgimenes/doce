//! Goal/plan state for the two-state agent loop: a single `run_loop` call
//! whose available tools depend on an external state the backend itself
//! carries (`LoopState`) — not two separate loops. The state machine
//! itself lives here as `PlanState`: both production
//! (`commands::agent::RealBackend`) and the task tests' `PlanExecBackend`
//! (`tests/agent_tasks.rs`) embed this same struct as their
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
    r#"{"type": "function", "function": {"name": "Grep", "description": "Search file contents with a regular expression. Omit path to search the current working directory. Results are capped at 100 matches -- for counting or exhaustive listings use a Bash pipeline instead.", "parameters": {"type": "object", "properties": {"pattern": {"type": "string"}, "path": {"type": "string"}, "glob": {"type": "string"}}, "required": ["pattern"]}}}"#,
    r#"{"type": "function", "function": {"name": "Glob", "description": "Find files by name pattern. The pattern is a single wildcard expression, e.g. \"bug_*.txt\" or \"*.rs\" -- never a space-separated list of literal filenames, that matches nothing. Omit path to search the current working directory. Results are capped at the 100 most recently modified matches -- for counting or exhaustive listings use a Bash pipeline instead.", "parameters": {"type": "object", "properties": {"pattern": {"type": "string"}, "path": {"type": "string"}}, "required": ["pattern"]}}}"#,
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
fn build_plan_system_prompt(allow_task: bool, dialect: crate::inference::ToolDialect) -> String {
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

    // The "unclear" triage bullet is the one host-variant sentence beyond
    // the tool lists: a subagent has no route to a user, so its variant
    // must not mention asking one (the invariant
    // plan_system_prompt_omits_task_and_ask_user_for_subagents pins).
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

# Counting and sampling

Glob and Grep results are capped at 100, so never answer "how many" or "list all" by counting their output -- a capped result undercounts silently. For counts, sizes, samples, or statistics over files, run one Bash command that computes the answer directly, e.g. `find . -name "*.ts" | wc -l` for "how many .ts files?", `du -sh */ | sort -h` for "which folder is biggest?". One command, one number -- not a listing you count yourself.

# Size up the request first

Not every message is a task. Decide before anything else:
- A greeting, small talk, or a question you can already answer: call FinishTask with your answer right away -- no plan. Never invent work the user did not ask for.
- A request that is unclear, or that names files or things you cannot find: {unclear_action}. Never guess what the task might be.
- A clear task: call CreatePlan, then ResumeExecution.

# Modes

You work in two modes, switched by tool calls. The last message of the conversation always names your current mode, and only that mode's tools are available to you.

PLANNING mode tools: {planning_names}. You maintain the plan and hand each step's work to EXECUTING mode; you do not personally edit files or run commands here.
EXECUTING mode tools: {executing_names}. You do one step's work, framed with the overall goal plus that step's own description. You must end every step by calling StepDone or RefuseStep -- never answer in plain text there, that would end the WHOLE task, not just this step.

# Plans

Each step runs with its own limited number of turns, so keep steps small: one step per item -- a task covering 20 files needs 20 per-file steps, never "repeat this process for the remaining files": a bundled step silently stops partway and the remaining items are lost. Extend or correct the plan with AddStep; never call CreatePlan again, that would discard progress already made. Plans in earlier conversation history are finished -- each new user request starts with no plan.

Once every step reports done you are back in PLANNING mode. A step reporting done is a CLAIM, not proof: before giving your final answer you MUST verify the outcome yourself with Read/Grep/Glob -- re-read the changed files or search for remaining problems. If verification shows anything still wrong, AddStep and ResumeExecution rather than accepting the claim. Only once verified, call FinishTask with your final answer.

Every response you give must be exactly one tool call."#
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
pub fn plan_system_prompt(
    allow_task: bool,
    dialect: crate::inference::ToolDialect,
) -> &'static str {
    use std::sync::OnceLock;
    // One cache cell per (host flavor, dialect) — byte-stability WITHIN a
    // host+model pairing is what KV reuse needs, and both inputs are
    // stable for an engine's lifetime.
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
    cell.get_or_init(|| build_plan_system_prompt(allow_task, dialect))
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
    /// Total `RefuseStep` calls this task. Past `REFUSAL_WRAP_UP_THRESHOLD`
    /// every Planning tail tells the model to stop replanning around the
    /// failure and FinishTask honestly — the prompt-level half of the
    /// doom-loop fix (`PlanStep::refused` is the machine-level half).
    refusal_count: u32,
    /// Single-mode harness: FinishTask with undone todos was already
    /// bounced once this task (`handle_todo_tool`) — the second attempt
    /// is honored.
    finish_bounced: bool,
}

// ===================================================================
// Single-mode harness (2026-07-13 design doc): Todo replaces the
// two-state machine. The functions below are the new engine; the machine
// above is dead code kept through the transition (deleted once the
// ladder accepts the rewrite). Hosts call exactly these four:
// `single_mode_system_prompt`, `single_mode_tool_names`, `todo_tail`,
// `handle_todo_tool`.
// ===================================================================

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
    /// dispatch — the single-mode counterpart of `handle_plan_tool`.
    /// FinishTask with undone todos bounces ONCE per task ("finish or
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
}

/// After this many refusals in one task, Planning tails push the model to
/// stop verify-spirals and either do the remaining work or wrap up.
const REFUSAL_WRAP_UP_THRESHOLD: u32 = 3;

/// At this many refusals the machine ends the task itself: `RefuseStep`
/// returns `Finish` with an honest failure report instead of another
/// planning round. A task refusing this often is going nowhere -- the
/// 2026-07-12 doom loop never terminated on its own (it died of KV-cache
/// exhaustion at turn ~135, well before the 200-turn cap), so the
/// terminator must be mechanical, not another nudge.
const REFUSAL_HARD_LIMIT: u32 = 8;

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
    "Read",
    "Write",
    "Edit",
    "Bash",
    "Grep",
    "Glob",
    "Task",
    "StepDone",
    "RefuseStep",
];
/// FR-016: a subagent's Executing turns must not be able to sample `Task`.
const EXECUTING_ALLOWED_TOOLS_NO_TASK: &[&str] = &[
    "Read",
    "Write",
    "Edit",
    "Bash",
    "Grep",
    "Glob",
    "StepDone",
    "RefuseStep",
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
                if self.has_plan() {
                    sections.push(
                        "You are in PLANNING mode -- maintain the plan and hand off each step's actual work to EXECUTING mode; you do not personally edit files or run commands from here.".to_string(),
                    );
                } else {
                    // The triage rule restated at the decision moment: the
                    // last message is what a small model actually attends
                    // to, and the 2026-07-12 "ola" doom loop showed the
                    // system-prompt copy alone does not bind.
                    sections.push(
                        "You are in PLANNING mode and no plan exists yet. Size up the request first: if it is a greeting or something you can already answer, call FinishTask with your answer now -- no plan. If it is unclear or names things you cannot find, ask before planning. Only a clear task gets CreatePlan.".to_string(),
                    );
                }
                if let Some(reason) = self.refusal_context.take() {
                    sections.push(format!(
                        "The previous step could not be completed and was retired. Reason given: {reason}\n\nRevise the plan: AddStep steps that DO the remaining work (one per item -- never a step that only re-checks or re-verifies), then ResumeExecution. Only if the task itself is impossible -- what it names does not exist or cannot be done -- call FinishTask and say so honestly."
                    ));
                }
                if self.refusal_count >= REFUSAL_WRAP_UP_THRESHOLD {
                    sections.push(format!(
                        "Steps have been refused {} times in this task. Stop adding steps that only verify: AddStep concrete steps that do the remaining work, one per item. If the task truly cannot proceed, call FinishTask now and report honestly what was done and what could not be.",
                        self.refusal_count
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
            return Some(
                match call.arguments.get("answer").and_then(|v| v.as_str()) {
                    Some(answer) => PlanToolReply::Finish(answer.to_string()),
                    None => PlanToolReply::Reply(
                        "Error: FinishTask requires an answer argument".to_string(),
                    ),
                },
            );
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
                                    refused: false,
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
                    // Duplicate guard: the 2026-07-12 doom loop appended
                    // the byte-identical step 20+ times -- each duplicate
                    // in history makes the next more likely (repetition
                    // collapse), so the dedup must be mechanical, not a
                    // nudge.
                    if self.plan.steps.iter().any(|s| s.description == description) {
                        return Some(PlanToolReply::Reply(
                            "Error: that exact step is already in the plan -- not added. If the work cannot proceed, stop planning: call FinishTask and tell the user honestly what you found.".to_string(),
                        ));
                    }
                    self.plan.steps.push(PlanStep {
                        description,
                        done: false,
                        refused: false,
                    });
                    let n = self.plan.steps.len();
                    if n > 12 {
                        // Plan-shape guard (Task 14): a bloated plan is the
                        // OTHER half of the diagnosed benchmark regression
                        // (degenerate 25-step plans with duplicated
                        // per-file steps), so nudge away from growing it
                        // further right at the moment a step was just
                        // added -- the same decision-moment placement as
                        // CreatePlan's and StepDone's own nudges above.
                        format!(
                            "Step added. Plan now has {n} steps -- that is a lot: long plans burn turns. Only add more if NO existing step already covers the work; duplicate or per-item steps that restate a broader step should not be added."
                        )
                    } else {
                        format!("Step added. Plan now has {n} steps.")
                    }
                }
            }
            (LoopState::Planning, "ResumeExecution") => match self.next_undone_step() {
                Some(idx) => {
                    self.state = LoopState::Executing { step_index: idx };
                    format!(
                        "Resuming at step {idx}: {}",
                        self.plan.steps[idx].description
                    )
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
                self.plan.steps[step_index].refused = true;
                self.refusal_count += 1;
                self.state = LoopState::Planning;
                if self.refusal_count >= REFUSAL_HARD_LIMIT {
                    return Some(PlanToolReply::Finish(format!(
                        "I could not complete this task: steps were refused {} times without finding a way forward. Last failure -- {reason}",
                        self.refusal_count
                    )));
                }
                "Step refused and retired -- it will not run again as written. Back to planning."
                    .to_string()
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
        self.plan.steps.iter().position(|s| !s.done && !s.refused)
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
    ///
    /// Planning renders the full checklist, unclamped, regardless of plan
    /// size -- the model is the plan editor there and needs every step to
    /// revise it, and Planning turns are comparatively few. Executing
    /// clamps to a small window (Task 14): a benchmark regression traced
    /// the full pending checklist re-advertising every far-off step every
    /// turn as a distractor that kept a small model servicing salient
    /// pending "Read"-shaped steps instead of finishing the current one.
    /// Executing turns are many, so this is the render that actually needs
    /// to stay bounded regardless of how large the plan has grown.
    fn recitation_text(&self) -> Option<String> {
        if !self.has_plan() {
            return None;
        }
        let done = self.plan.steps.iter().filter(|s| s.done).count();
        let total = self.plan.steps.len();
        let mut lines = vec![format!("Plan status -- goal: {}", self.plan.goal)];
        match self.state {
            LoopState::Planning => {
                for step in &self.plan.steps {
                    let mark = if step.done {
                        "[x]"
                    } else if step.refused {
                        "[!]"
                    } else {
                        "[ ]"
                    };
                    lines.push(format!("{mark} {}", step.description));
                }
            }
            LoopState::Executing { step_index } => {
                if done > 0 {
                    lines.push(format!("[x] {done} steps done"));
                }
                lines.push(format!("[>] {}", self.plan.steps[step_index].description));
                let upcoming: Vec<usize> = self
                    .plan
                    .steps
                    .iter()
                    .enumerate()
                    .skip(step_index + 1)
                    .filter(|(_, s)| !s.done && !s.refused)
                    .map(|(i, _)| i)
                    .collect();
                let shown = upcoming.len().min(2);
                for &i in upcoming.iter().take(2) {
                    lines.push(format!("[ ] next: {}", self.plan.steps[i].description));
                }
                let remaining = upcoming.len() - shown;
                if remaining > 0 {
                    lines.push(format!("({remaining} more steps pending)"));
                }
            }
        }
        lines.push(format!("({done}/{total} done)"));
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
        let prompt = plan_system_prompt(true, crate::inference::ToolDialect::HermesJson);
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
        assert!(prompt.contains("You are doce, a local coding agent."));
        assert!(prompt.contains("# Size up the request first"));
        assert!(
            prompt.contains("# Counting and sampling"),
            "the counting-via-Bash steering section must be in the prompt"
        );
        // Dialect-specific call teaching (tool-dialects design): the
        // Hermes flavor teaches <tool_call> JSON; the MiniCPM flavor
        // teaches its <function>/<param> XML — never each other's.
        assert!(prompt.contains("<tool_call></tool_call> XML tags"));
        let minicpm = plan_system_prompt(true, crate::inference::ToolDialect::MiniCpmXml);
        assert!(minicpm.contains("<function name=\"function-name\">"));
        assert!(!minicpm.contains("<tool_call></tool_call> XML tags"));
        assert!(minicpm.contains("Every response you give must be exactly one tool call"));
        assert!(
            prompt.contains("call FinishTask with your answer right away"),
            "the greeting/direct-answer triage rule must be in the prompt"
        );
        assert!(
            prompt.contains("Never invent work the user did not ask for."),
            "the anti-confabulation rule must be in the prompt"
        );
        assert!(prompt.contains("# Plans"));
        assert!(prompt.contains("one step per item"));
        assert!(
            prompt.contains("a bundled step silently stops partway"),
            "the benchmark-diagnosed bundled-step rationale must stay in the prompt (dropping it cost 19/20 on tier4_planned, 2026-07-12)"
        );
        assert!(prompt.contains("A step reporting done is a CLAIM, not proof"));
        assert!(prompt.contains("Every response you give must be exactly one tool call"));
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
        let sub = plan_system_prompt(false, crate::inference::ToolDialect::HermesJson);
        assert!(!sub.contains("\"name\": \"Task\""));
        assert!(
            !sub.contains("AskUserQuestion"),
            "no tool line and no # Modes mention"
        );
        assert!(sub.contains(
            "PLANNING mode tools: CreatePlan, AddStep, ResumeExecution, Read, Grep, Glob, FinishTask."
        ));
        assert!(sub.contains("\"name\": \"StepDone\""));
        assert!(sub.contains("\"name\": \"FinishTask\""));
        assert!(
            sub.contains("call FinishTask explaining exactly what is missing"),
            "the subagent triage bullet must route 'unclear' to FinishTask, not to a user it cannot reach"
        );
        let top = plan_system_prompt(true, crate::inference::ToolDialect::HermesJson);
        assert!(top.contains("\"name\": \"Task\""));
        assert!(top.contains("\"name\": \"AskUserQuestion\""));
        assert!(top.contains("call AskUserQuestion, and keep asking until the task is clear"));
        assert!(top.contains(
            "PLANNING mode tools: CreatePlan, AddStep, ResumeExecution, Read, Grep, Glob, AskUserQuestion, FinishTask."
        ));
        assert!(std::ptr::eq(
            plan_system_prompt(true, crate::inference::ToolDialect::HermesJson),
            plan_system_prompt(true, crate::inference::ToolDialect::HermesJson)
        ));
        assert!(std::ptr::eq(
            plan_system_prompt(false, crate::inference::ToolDialect::HermesJson),
            plan_system_prompt(false, crate::inference::ToolDialect::HermesJson)
        ));
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
        assert!(
            tail.contains("ship it"),
            "the overall goal must be in the tail"
        );
        assert!(tail.contains("Your current step: write tests"));
        assert!(
            tail.contains("StepDone") && tail.contains("RefuseStep"),
            "the step-ending rule must ride the tail"
        );
        // Task 10's recitation checklist is folded into this ONE tail message.
        assert!(tail.contains("[>] write tests"));
        assert!(
            tail.contains("[ ] next: run tests"),
            "upcoming undone steps carry the 'next:' label (Task 14 clamp), got: {tail:?}"
        );
        assert!(tail.contains("(0/2 done)"));
    }

    /// The no-plan Planning tail must restate the triage rule (answer
    /// directly / ask / plan) at the decision moment -- the 2026-07-12
    /// "ola" doom loop showed the system-prompt copy alone does not bind
    /// on a small model.
    #[test]
    fn state_tail_offers_direct_answer_when_no_plan_exists() {
        let mut ps = PlanState::default();
        let tail = ps.state_tail();
        assert!(
            tail.contains("call FinishTask with your answer now"),
            "the fresh-state tail must offer the direct-answer path, got: {tail:?}"
        );

        // Once a plan exists the triage nudge must NOT linger.
        ps.handle_plan_tool(&call(
            "CreatePlan",
            serde_json::json!({"goal": "g", "steps": ["s"]}),
        ));
        let tail = ps.state_tail();
        assert!(!tail.contains("call FinishTask with your answer now"));
    }

    /// The doom-loop engine fix: a refused step is retired, so
    /// ResumeExecution moves PAST it instead of re-entering it verbatim
    /// forever ("Resuming at step 0" x84 on 2026-07-12).
    #[test]
    fn refused_step_is_retired_and_resume_moves_past_it() {
        let mut ps = PlanState::default();
        ps.handle_plan_tool(&call(
            "CreatePlan",
            serde_json::json!({"goal": "g", "steps": ["impossible", "possible"]}),
        ));
        ps.handle_plan_tool(&call("ResumeExecution", serde_json::json!({})));
        assert_eq!(ps.state, LoopState::Executing { step_index: 0 });

        let reply = ps.handle_plan_tool(&call(
            "RefuseStep",
            serde_json::json!({"reason": "file does not exist"}),
        ));
        assert_eq!(
            reply,
            Some(PlanToolReply::Reply(
                "Step refused and retired -- it will not run again as written. Back to planning."
                    .to_string()
            ))
        );

        let reply = ps.handle_plan_tool(&call("ResumeExecution", serde_json::json!({})));
        assert_eq!(ps.state, LoopState::Executing { step_index: 1 });
        assert_eq!(
            reply,
            Some(PlanToolReply::Reply(
                "Resuming at step 1: possible".to_string()
            ))
        );
    }

    /// With every step refused there is nothing to resume -- the model is
    /// pushed toward AddStep (a corrected step) or FinishTask instead of
    /// cycling.
    #[test]
    fn resume_with_only_refused_steps_reports_no_undone_steps() {
        let mut ps = PlanState::default();
        ps.handle_plan_tool(&call(
            "CreatePlan",
            serde_json::json!({"goal": "g", "steps": ["impossible"]}),
        ));
        ps.handle_plan_tool(&call("ResumeExecution", serde_json::json!({})));
        ps.handle_plan_tool(&call("RefuseStep", serde_json::json!({"reason": "nope"})));

        let reply = ps.handle_plan_tool(&call("ResumeExecution", serde_json::json!({})));
        assert_eq!(
            reply,
            Some(PlanToolReply::Reply(
                "Error: no undone steps -- create or add a step first".to_string()
            ))
        );
        assert_eq!(ps.state, LoopState::Planning);
    }

    /// The other half of the doom loop: the byte-identical AddStep spammed
    /// 20+ times. The dedup is mechanical -- an error reply, no growth.
    #[test]
    fn add_step_rejects_an_exact_duplicate() {
        let mut ps = PlanState::default();
        ps.handle_plan_tool(&call(
            "CreatePlan",
            serde_json::json!({"goal": "g", "steps": ["search for main.py"]}),
        ));
        let reply = ps.handle_plan_tool(&call(
            "AddStep",
            serde_json::json!({"description": "search for main.py"}),
        ));
        match reply {
            Some(PlanToolReply::Reply(text)) => {
                assert!(
                    text.starts_with("Error: that exact step is already in the plan"),
                    "duplicate AddStep must be rejected, got: {text:?}"
                );
            }
            other => panic!("expected a Reply, got {other:?}"),
        }
        assert_eq!(ps.plan.steps.len(), 1, "the duplicate must not be added");
    }

    /// Repeated refusals must escalate to a wrap-up demand in the Planning
    /// tail -- the backstop that ends a confabulated task well before the
    /// 200-turn cap.
    #[test]
    fn state_tail_urges_finish_after_repeated_refusals() {
        let mut ps = PlanState::default();
        ps.handle_plan_tool(&call(
            "CreatePlan",
            serde_json::json!({"goal": "g", "steps": ["a", "b", "c"]}),
        ));
        for _ in 0..3 {
            ps.handle_plan_tool(&call("ResumeExecution", serde_json::json!({})));
            ps.handle_plan_tool(&call(
                "RefuseStep",
                serde_json::json!({"reason": "blocked"}),
            ));
        }
        let tail = ps.state_tail();
        assert!(
            tail.contains("refused 3 times") && tail.contains("call FinishTask now"),
            "after {REFUSAL_WRAP_UP_THRESHOLD} refusals the tail must demand a wrap-up, got: {tail:?}"
        );
    }

    /// The deterministic doom-loop terminator: at REFUSAL_HARD_LIMIT the
    /// machine itself ends the task with an honest failure -- the
    /// 2026-07-12 loop never self-terminated (KV-cache death at ~135
    /// turns), so this cannot be a nudge.
    #[test]
    fn refuse_step_forces_finish_at_the_hard_limit() {
        let mut ps = PlanState::default();
        let steps: Vec<String> = (0..8).map(|i| format!("step {i}")).collect();
        ps.handle_plan_tool(&call(
            "CreatePlan",
            serde_json::json!({"goal": "g", "steps": steps}),
        ));
        for i in 0..7 {
            ps.handle_plan_tool(&call("ResumeExecution", serde_json::json!({})));
            let reply = ps.handle_plan_tool(&call(
                "RefuseStep",
                serde_json::json!({"reason": format!("blocked {i}")}),
            ));
            assert!(
                matches!(reply, Some(PlanToolReply::Reply(_))),
                "refusal {i} must stay a Reply, got: {reply:?}"
            );
        }
        ps.handle_plan_tool(&call("ResumeExecution", serde_json::json!({})));
        let reply = ps.handle_plan_tool(&call(
            "RefuseStep",
            serde_json::json!({"reason": "blocked 7"}),
        ));
        match reply {
            Some(PlanToolReply::Finish(answer)) => {
                assert!(
                    answer.contains("refused 8 times") && answer.contains("blocked 7"),
                    "the forced finish must report the count and last reason, got: {answer:?}"
                );
            }
            other => panic!("the 8th refusal must force Finish, got {other:?}"),
        }
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
            [
                "Read",
                "Write",
                "Edit",
                "Bash",
                "Grep",
                "Glob",
                "Task",
                "StepDone",
                "RefuseStep"
            ]
        );
        assert_eq!(
            ps.allowed_tool_names(false),
            [
                "Read",
                "Write",
                "Edit",
                "Bash",
                "Grep",
                "Glob",
                "StepDone",
                "RefuseStep"
            ]
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
        assert_eq!(
            ps.state,
            LoopState::Planning,
            "CreatePlan alone does not start execution"
        );

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

        let result =
            reply(ps.handle_plan_tool(&call("StepDone", serde_json::json!({"summary": "did a"}))));
        assert!(ps.plan.steps[0].done);
        assert_eq!(ps.state, LoopState::Executing { step_index: 1 });
        assert!(result.contains("step 1"));

        let all_done =
            reply(ps.handle_plan_tool(&call("StepDone", serde_json::json!({"summary": "did b"}))));
        assert!(ps.plan.steps[1].done);
        assert_eq!(
            ps.state,
            LoopState::Planning,
            "all done returns to planning for review"
        );
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
        assert!(
            !ps.has_plan(),
            "AddStep must not mutate the plan when none exists"
        );
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
        assert!(ps
            .handle_plan_tool(&call("Read", serde_json::json!({})))
            .is_none());
        assert!(ps
            .handle_plan_tool(&call("AskUserQuestion", serde_json::json!({})))
            .is_none());
        // Planning: write tools are rejected.
        let rejected = reply(ps.handle_plan_tool(&call("Write", serde_json::json!({}))));
        assert!(rejected.starts_with("Error"));

        ps.handle_plan_tool(&call(
            "CreatePlan",
            serde_json::json!({"goal": "g", "steps": ["a"]}),
        ));
        ps.handle_plan_tool(&call("ResumeExecution", serde_json::json!({})));
        // Executing: file/shell/Task pass through, plan-editing is rejected.
        assert!(ps
            .handle_plan_tool(&call("Write", serde_json::json!({})))
            .is_none());
        assert!(ps
            .handle_plan_tool(&call("Task", serde_json::json!({})))
            .is_none());
        let rejected =
            reply(ps.handle_plan_tool(&call("AddStep", serde_json::json!({"description": "x"}))));
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
        let rejected =
            reply(ps.handle_plan_tool(&call("FinishTask", serde_json::json!({"answer": "nope"}))));
        assert!(rejected.starts_with("Error"));
    }

    #[test]
    fn recitation_text_returns_none_without_a_plan() {
        let ps = PlanState::default();
        assert!(
            ps.recitation_text().is_none(),
            "fresh state should have no recitation"
        );
    }

    /// Task 14 test (b): a small Executing plan (3 steps, 1 done) still
    /// shows everything -- nothing lands in the "(N more steps pending)"
    /// bucket, even though the one done step now collapses into a count
    /// line rather than showing its own description (that collapse is
    /// unconditional on `done_count > 0`, not just a large-plan behavior).
    #[test]
    fn recitation_text_in_executing_mode_shows_everything_when_a_plan_is_small() {
        let mut ps = PlanState::default();
        ps.handle_plan_tool(&call(
            "CreatePlan",
            serde_json::json!({"goal": "fix bugs", "steps": ["fix a", "fix b", "fix c"]}),
        ));

        // Mark the first step done, then move to executing the second.
        ps.handle_plan_tool(&call("ResumeExecution", serde_json::json!({})));
        ps.handle_plan_tool(&call("StepDone", serde_json::json!({"summary": "done"})));

        // Now in Executing{1}, with step 0 done.
        assert_eq!(ps.state, LoopState::Executing { step_index: 1 });
        assert!(ps.plan.steps[0].done);

        let recitation = ps
            .recitation_text()
            .expect("should have recitation with active plan");

        assert!(recitation.contains("fix bugs"), "should contain goal");
        assert!(
            recitation.contains("[x] 1 steps done"),
            "done steps collapse into a single count line, got: {recitation:?}"
        );
        assert!(
            !recitation.contains("fix a"),
            "the collapsed done step's own description must not appear, got: {recitation:?}"
        );
        assert!(
            recitation.contains("[>] fix b"),
            "current step must be marked [>] with full text, got: {recitation:?}"
        );
        assert!(
            recitation.contains("[ ] next: fix c"),
            "the upcoming undone step must be shown with a next: label, got: {recitation:?}"
        );
        assert!(
            !recitation.contains("more steps pending"),
            "a small plan must not hide anything behind the remaining-count line, got: {recitation:?}"
        );
        assert!(
            recitation.contains("(1/3 done)"),
            "should show progress counter"
        );
    }

    /// Task 14 test (a): a large Executing plan (25 steps, 0-6 done,
    /// current 7) clamps the recitation to a window -- the diagnosed
    /// benchmark distractor was the full pending checklist re-advertising
    /// every far-off step every turn.
    #[test]
    fn recitation_text_in_executing_mode_clamps_a_large_plan_to_a_window() {
        let mut ps = PlanState::default();
        let steps: Vec<PlanStep> = (0..25)
            .map(|i| PlanStep {
                description: format!("step {i}"),
                done: i < 7,
                refused: false,
            })
            .collect();
        ps.plan = Plan {
            goal: "big plan".to_string(),
            steps,
        };
        ps.state = LoopState::Executing { step_index: 7 };

        let recitation = ps
            .recitation_text()
            .expect("should have recitation with active plan");
        assert!(
            recitation.contains("[x] 7 steps done"),
            "got: {recitation:?}"
        );
        assert!(recitation.contains("[>] step 7"), "got: {recitation:?}");
        assert!(
            recitation.contains("[ ] next: step 8"),
            "got: {recitation:?}"
        );
        assert!(
            recitation.contains("[ ] next: step 9"),
            "got: {recitation:?}"
        );
        assert!(
            recitation.contains("(15 more steps pending)"),
            "17 undone steps after current minus the 2 shown = 15, got: {recitation:?}"
        );
        assert!(recitation.contains("(7/25 done)"), "got: {recitation:?}");
        assert!(
            !recitation.contains("step 20"),
            "far-off pending steps must not appear in a clamped recitation, got: {recitation:?}"
        );
    }

    /// Task 14 test (c): Planning mode's recitation is the full render,
    /// unclamped, regardless of plan size -- the model is the plan editor
    /// there and needs to see every step to revise it.
    #[test]
    fn recitation_text_in_planning_mode_renders_every_step_regardless_of_plan_size() {
        let mut ps = PlanState::default();
        let steps: Vec<String> = (0..25).map(|i| format!("step {i}")).collect();
        ps.handle_plan_tool(&call(
            "CreatePlan",
            serde_json::json!({"goal": "g", "steps": steps}),
        ));
        assert_eq!(
            ps.state,
            LoopState::Planning,
            "CreatePlan alone stays in Planning"
        );

        let recitation = ps
            .recitation_text()
            .expect("a created plan always has a recitation");
        for i in 0..25 {
            assert!(
                recitation.contains(&format!("step {i}")),
                "planning mode must render every step's description in full, missing step {i}, got: {recitation:?}"
            );
        }
        assert!(
            !recitation.contains("more steps pending"),
            "planning mode is never clamped, got: {recitation:?}"
        );
    }

    /// Task 14 test (d): the AddStep result nudge warns once the plan
    /// crosses 12 steps (long plans burn turns servicing a bloated
    /// checklist) but keeps the classic text at or below that ceiling.
    #[test]
    fn add_step_warns_once_the_plan_exceeds_twelve_steps() {
        let mut ps = PlanState::default();
        let steps: Vec<String> = (0..11).map(|i| format!("step {i}")).collect();
        ps.handle_plan_tool(&call(
            "CreatePlan",
            serde_json::json!({"goal": "g", "steps": steps}),
        ));
        assert_eq!(ps.plan.steps.len(), 11);

        // The 12th step lands exactly at the ceiling -- classic text.
        let at_twelve = reply(ps.handle_plan_tool(&call(
            "AddStep",
            serde_json::json!({"description": "step 11"}),
        )));
        assert_eq!(at_twelve, "Step added. Plan now has 12 steps.");

        // The 13th step crosses the ceiling -- the plan-bloat nudge.
        let beyond = reply(ps.handle_plan_tool(&call(
            "AddStep",
            serde_json::json!({"description": "step 12"}),
        )));
        assert_eq!(
            beyond,
            "Step added. Plan now has 13 steps -- that is a lot: long plans burn turns. Only add more if NO existing step already covers the work; duplicate or per-item steps that restate a broader step should not be added."
        );
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
