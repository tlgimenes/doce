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
//! in a per-turn tail message (`PlanState::state_tail`) appended after the
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

/// Which completion a model just claimed: one `TodoItem` (0-based index) or
/// the whole `FinishTask`. The key an observer's (Task 4) or the always-
/// approve stub's (this task) verdict is filed under in `reject_counts`, so
/// a rejected `TodoItem(3)` and a rejected `FinishTask` accrue separately.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum CompletionKind {
    TodoItem(usize),
    FinishTask,
}

/// What handling `Todo`/`FinishTask` produced: an ordinary result string
/// fed back into the loop, the task's final answer (`Finish`, now
/// unproduced by `handle_todo_tool` -- kept so no caller needs a partial
/// match, and harmless since nothing constructs it anymore), or a
/// completion the model claimed but that has not yet been committed
/// (`ProposeComplete`). Hosts map `Finish` onto `agent::ToolExecution::
/// Finish`, ending `run_loop`; `ProposeComplete` instead routes through
/// `PlanState::apply_completion_verdict` before the host decides that.
/// Putting "done" behind a tool call is what lets `run_loop` run with
/// grammar-required tool calls: free-text replies (which a small model
/// degrades into after repetitive stretches) become unsamplable.
#[derive(Debug, Clone, PartialEq)]
pub enum PlanToolReply {
    Reply(String),
    Finish(String),
    /// A completion claim awaiting a verdict: `kind` identifies what was
    /// claimed done, `answer` carries `FinishTask`'s answer text (`None`
    /// for `TodoItem`, which has none). Every host runs this straight
    /// through `apply_completion_verdict` before it reaches the model.
    ProposeComplete {
        kind: CompletionKind,
        answer: Option<String>,
    },
}

/// One real (file-mutating) tool call's outcome, recorded as it happens —
/// the raw evidence a later observer LLM checks a `FinishTask` claim
/// against, rather than trusting the model's own account of what it did.
/// Append-only: nothing in this task ever clears or rewrites an entry.
#[derive(Clone, Debug, PartialEq)]
pub struct MutationRecord {
    pub tool: String, // "Update", "Write", "Bash", or an external/MCP tool name
    /// The SUBJECT the action acted on, for the observer to judge relevance:
    /// the file path for `Update`/`Write`, the command for `Bash`; `None` for
    /// any other action whose own tool name is the evidence (e.g. an MCP call).
    pub target: Option<String>,
    pub ok: bool, // did the call succeed
}

/// How many times a single `CompletionKind` may be rejected before the
/// model wins by default and the commit happens anyway (with an
/// unresolved-concern note appended). Unreached while nothing ever rejects
/// (this task's always-approve stub); becomes live once an observer (Task
/// 4) can return `approved: false`.
pub const OBSERVER_REJECT_CAP: u32 = 2;

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
    /// Append-only evidence log of every real ACTION this task's backend has
    /// executed — file edits (`Update`/`Write`), `Bash` commands, and external
    /// tool calls (e.g. MCP) — so an ops/comms task that completes by *doing*
    /// still leaves evidence. Only the read-only / meta tools
    /// (`Read`/`Grep`/`Glob`/`Task`/`AskUserQuestion`) never touch it. The
    /// observer reads this to verify a `FinishTask` completion claim against
    /// what actually happened, instead of trusting the claim on its own.
    pub mutation_log: Vec<MutationRecord>,
    /// How many times each `CompletionKind` has been rejected so far this
    /// task, keyed independently per kind (a rejected `TodoItem(3)` does
    /// not count against `FinishTask`'s cap). Read by `reject_cap_reached`
    /// to let the model win after `OBSERVER_REJECT_CAP` rejects.
    reject_counts: std::collections::HashMap<CompletionKind, u32>,
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

For any multi-step task, keep a todo list with Todo: one item per file or unit of work, never a bundled "handle the rest" item. Mark each item done with TodoDone as you finish it, and work the list in order. Your current todos are shown to you each turn.

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
    "TodoDone",
    "FinishTask",
];
const SINGLE_MODE_TOOLS_SUB: &[&str] = &[
    "Read",
    "Update",
    "Bash",
    "Grep",
    "Glob",
    "Todo",
    "TodoDone",
    "FinishTask",
];

impl PlanState {
    /// Appends one entry to the evidence log — never clears, never
    /// rewrites. Hosts call this right after a real (non-harness) tool has
    /// executed and its result string is known; see `mutation_log`'s doc
    /// comment for what belongs here.
    pub fn record_mutation(&mut self, tool: &str, target: Option<String>, ok: bool) {
        self.mutation_log.push(MutationRecord {
            tool: tool.to_string(),
            target,
            ok,
        });
    }

    /// The old `TodoDone` commit, extracted verbatim: flip `index`'s done
    /// flag and return the exact reply string `TodoDone` used to return
    /// directly. `index` must already be validated (`handle_todo_tool`'s
    /// `TodoDone` arm checks it before ever proposing this kind) --
    /// panics on an out-of-range index rather than silently doing nothing.
    pub fn commit_todo_done(&mut self, index: usize) -> String {
        let step = self
            .plan
            .steps
            .get_mut(index)
            .expect("commit_todo_done: index must be validated by the caller");
        let already = step.done;
        let desc = step.description.clone();
        step.done = true;
        let done = self.plan.steps.iter().filter(|s| s.done).count();
        let total = self.plan.steps.len();
        let note = if already { " (was already done)" } else { "" };
        format!("Marked done{note}: {desc}. {done}/{total} done.")
    }

    /// Records one rejection of `kind` and returns the new count. Filed
    /// independently per `CompletionKind`, so a rejected `TodoItem(3)`
    /// never counts against `FinishTask`'s own cap.
    pub fn note_reject(&mut self, kind: &CompletionKind) -> u32 {
        let c = self.reject_counts.entry(kind.clone()).or_insert(0);
        *c += 1;
        *c
    }

    /// Whether `kind` has already been rejected `OBSERVER_REJECT_CAP`
    /// times -- past this, the model wins by default rather than being
    /// rejected forever.
    pub fn reject_cap_reached(&self, kind: &CompletionKind) -> bool {
        self.reject_counts.get(kind).copied().unwrap_or(0) >= OBSERVER_REJECT_CAP
    }

    /// Apply an adjudicated completion. `approved`/`missing` come from the
    /// observer (Task 4) or the always-approve stub (Task 2). Returns
    /// `(reply_text, finish)`: `finish = Some(answer)` ends the task,
    /// `None` continues. This is the SINGLE place both backends apply a
    /// verdict, so they cannot drift.
    pub fn apply_completion_verdict(
        &mut self,
        kind: CompletionKind,
        answer: Option<String>,
        approved: bool,
        missing: &str,
    ) -> (String, Option<String>) {
        if approved {
            match kind {
                CompletionKind::TodoItem(i) => (self.commit_todo_done(i), None),
                CompletionKind::FinishTask => {
                    let a = answer.unwrap_or_default();
                    (a.clone(), Some(a))
                }
            }
        } else {
            let _n = self.note_reject(&kind);
            if self.reject_cap_reached(&kind) {
                // Model wins after the cap: commit anyway, note the unresolved concern.
                match kind {
                    CompletionKind::TodoItem(i) => {
                        let base = self.commit_todo_done(i);
                        (
                            format!("{base} (closed despite unresolved concern: {missing})"),
                            None,
                        )
                    }
                    CompletionKind::FinishTask => {
                        let a = answer.unwrap_or_default();
                        (a.clone(), Some(a))
                    }
                }
            } else {
                // Reject: do NOT commit. Post-pivot (FinishTask-only observation)
                // the FinishTask reject string is live in production; the TodoItem
                // reject string is unreachable there (TodoDone self-marks) but stays
                // covered by unit tests and the observer_smoke path.
                match kind {
                    CompletionKind::TodoItem(_) => (
                        format!("Not done: {missing}. Do the work, then mark it done again."),
                        None,
                    ),
                    CompletionKind::FinishTask => (
                        format!("Not finished: {missing}. Keep working, then FinishTask again."),
                        None,
                    ),
                }
            }
        }
    }

    /// The single-mode grammar enum: the full set, no per-state swapping —
    /// state legality was the two-mode machine's concern.
    pub fn single_mode_tool_names(&self, allow_task: bool) -> &'static [&'static str] {
        if allow_task {
            SINGLE_MODE_TOOLS_TOP
        } else {
            SINGLE_MODE_TOOLS_SUB
        }
    }

    /// The volatile recitation tail: a `Goal: <goal>` line (only when
    /// `plan.goal` is set) followed by the current todos as one compact
    /// line (unchanged from the old `todo_tail`, which this replaced —
    /// renamed because it now recites goal state too, not just todos).
    /// EMPTY when there is neither a goal nor any todos — hosts must skip
    /// pushing an empty tail.
    ///
    /// BYTE-INVARIANCE (benchmark gate): every tier runs with no goal set,
    /// so `self.plan.goal` is `""` for the whole gate. In that case this
    /// must produce EXACTLY what the old `todo_tail` produced — see
    /// `state_tail_with_empty_goal_is_byte_identical_to_old_todo_tail`
    /// below, which pins this. The goal line is kept to one short line
    /// (prompt bytes spent every turn) and only appears at all once a goal
    /// is actually set.
    pub fn state_tail(&self) -> String {
        let goal_line = if self.plan.goal.is_empty() {
            String::new()
        } else {
            format!("Goal: {}\n", self.plan.goal)
        };
        if self.plan.steps.is_empty() {
            // `trim_end` drops the goal line's own trailing newline when a
            // goal is set but there are no todos yet; with no goal either,
            // `goal_line` is already `""`, so this is a no-op -- the exact
            // empty string `todo_tail` returned.
            return goal_line.trim_end().to_string();
        }
        let items = self
            .plan
            .steps
            .iter()
            .enumerate()
            .map(|(i, s)| {
                format!(
                    "{i}. [{}] {}",
                    if s.done { "x" } else { " " },
                    s.description
                )
            })
            .collect::<Vec<_>>()
            .join("  ");
        let done = self.plan.steps.iter().filter(|s| s.done).count();
        format!(
            "{goal_line}Todos ({done}/{} done): {items}
Work the first undone item; add new items with Todo, mark one done with TodoDone {{\"index\": N}}.",
            self.plan.steps.len()
        )
    }

    /// Intercepts the harness tools (Todo, TodoDone, FinishTask) before
    /// dispatch. `Todo` is create-or-APPEND-ONLY-grow: on an active list it
    /// only adds new texts and can never remove, reorder, relabel, or
    /// un-done an existing item. `TodoDone {index}` is the ONLY path to
    /// completion (flip one item's done flag). Splitting completion out of
    /// the list-replacing `Todo` is what stops the compaction drift where a
    /// small model rewrote the whole list to "all done" with real work
    /// undone (tier6 seed 42). FinishTask with undone todos bounces ONCE per
    /// task, naming what's left; the second attempt is honored so a genuinely
    /// stuck task can still end.
    pub fn handle_todo_tool(&mut self, call: &crate::agent::ToolCall) -> Option<PlanToolReply> {
        match call.name.as_str() {
            "Todo" => {
                let Some(items) = call.arguments.get("items").and_then(|v| v.as_array()) else {
                    return Some(PlanToolReply::Reply(
                        r#"Error: Todo requires items: an array of {"text": string}."#.to_string(),
                    ));
                };
                // Read the texts first, rejecting a malformed shape before we
                // mutate anything. New items carry NO `done` on input -- any
                // `done` field the model sends is ignored; completion is
                // TodoDone-only.
                let mut texts = Vec::with_capacity(items.len());
                for item in items {
                    let Some(text) = item.get("text").and_then(|v| v.as_str()) else {
                        return Some(PlanToolReply::Reply(
                            r#"Error: every Todo item needs {"text": string}."#.to_string(),
                        ));
                    };
                    texts.push(text.to_string());
                }
                // APPEND-ONLY MERGE. This is the drift firewall: an already
                // active list's texts and done-flags are IMMUTABLE through
                // this tool. We only add items whose text is not already
                // present (created undone), and NEVER remove, reorder,
                // relabel, or un-done an existing item. That is what makes
                // "rewrite the whole list to all-done" structurally
                // impossible here -- the only way to complete an item is
                // TodoDone. (Empty list => this simply creates it.)
                let mut added = 0usize;
                for text in texts {
                    if !self.plan.steps.iter().any(|s| s.description == text) {
                        self.plan.steps.push(PlanStep {
                            description: text,
                            done: false,
                        });
                        added += 1;
                    }
                }
                let done = self.plan.steps.iter().filter(|s| s.done).count();
                let total = self.plan.steps.len();
                Some(PlanToolReply::Reply(format!(
                    "Todo updated: {added} added, {done}/{total} done. Mark an item done with TodoDone {{\"index\": N}}."
                )))
            }
            "TodoDone" => {
                // Self-mark: flip exactly ONE item's done flag by 0-based
                // index and reply directly -- NO observer here. Per-item
                // authoritative observation collapsed seed 11 (19->2/20) by
                // trapping the model in reject/retry loops on files it
                // struggled to edit (gate 2026-07-16). Completion is now
                // observer-verified ONLY at FinishTask, where one whole-thread
                // audit bounces a premature finish (catching a falsely-done
                // item like bug_04) without derailing per-item progress. A
                // bad/absent index names the valid undone items so the model
                // can self-correct.
                let Some(index) = call
                    .arguments
                    .get("index")
                    .and_then(|v| v.as_u64())
                    .map(|i| i as usize)
                else {
                    return Some(PlanToolReply::Reply(
                        self.todo_done_error("TodoDone needs an integer index (0-based)."),
                    ));
                };
                if self.plan.steps.get(index).is_none() {
                    return Some(PlanToolReply::Reply(
                        self.todo_done_error(&format!("No todo at index {index}.")),
                    ));
                }
                Some(PlanToolReply::Reply(self.commit_todo_done(index)))
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
                    // NAME the specific undone items, don't just count them: after a
                    // long, compacted run the model loses track of WHICH item is the
                    // gap (tier6 diagnosis 2026-07-16 -- the model re-read an
                    // already-done file, then marked everything done without doing the
                    // remaining one). A bare count also invited the escape the model
                    // took: the old "remove them with Todo if they no longer apply"
                    // clause let it resolve the bounce by marking work done it never
                    // did. List the actual work and forbid that. Cap the list so a
                    // large plan can't blow the reply size.
                    let listed: Vec<String> = self
                        .plan
                        .steps
                        .iter()
                        .filter(|s| !s.done)
                        .take(5)
                        .map(|s| format!("- {}", s.description))
                        .collect();
                    let more = undone.saturating_sub(listed.len());
                    let more_line = if more > 0 {
                        format!("\n- ...and {more} more")
                    } else {
                        String::new()
                    };
                    return Some(PlanToolReply::Reply(format!(
                        "{undone} todo(s) still undone:\n{}{more_line}\nComplete the actual work for each with your tools -- do NOT mark an item done unless you have really done it -- then FinishTask.",
                        listed.join("\n")
                    )));
                }
                // Clean list (or the honored second attempt): propose
                // rather than commit -- `apply_completion_verdict` decides
                // whether this actually ends the task.
                Some(PlanToolReply::ProposeComplete {
                    kind: CompletionKind::FinishTask,
                    answer: Some(answer),
                })
            }
            _ => None,
        }
    }

    /// Builds a TodoDone error reply that names the valid undone items (with
    /// their 0-based indices) so a model that passed a bad index is pointed
    /// straight at the ones it can still complete.
    fn todo_done_error(&self, msg: &str) -> String {
        let undone: Vec<String> = self
            .plan
            .steps
            .iter()
            .enumerate()
            .filter(|(_, s)| !s.done)
            .map(|(i, s)| format!("{i}. {}", s.description))
            .collect();
        if undone.is_empty() {
            format!("Error: {msg} No undone todos remain.")
        } else {
            format!("Error: {msg} Undone todos:\n{}", undone.join("\n"))
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

    /// Convenience: create/grow a list and return the resulting state.
    fn todo(state: &mut PlanState, texts: &[&str]) -> String {
        let items: Vec<_> = texts
            .iter()
            .map(|t| serde_json::json!({"text": t}))
            .collect();
        let PlanToolReply::Reply(text) = state
            .handle_todo_tool(&call("Todo", serde_json::json!({"items": items})))
            .unwrap()
        else {
            panic!("Todo must reply, not finish")
        };
        text
    }

    #[test]
    fn todo_creates_the_list_undone_and_the_tail_shows_indices() {
        let mut state = PlanState::default();
        // `done` is not part of the input contract; even if the model sends
        // one it is IGNORED -- new items are always created undone.
        let reply = state
            .handle_todo_tool(&call(
                "Todo",
                serde_json::json!({"items": [
                    {"text": "fix a", "done": true},
                    {"text": "fix b"},
                ]}),
            ))
            .unwrap();
        let PlanToolReply::Reply(text) = reply else {
            panic!("Todo must reply")
        };
        assert!(text.contains("2 added"), "{text}");
        // Nothing done: the "done: true" on input was ignored.
        assert_eq!(state.next_undone_step(), Some(0));
        assert_eq!(state.plan.steps.iter().filter(|s| s.done).count(), 0);

        // The tail recites the list with 0-based indices the model passes to
        // TodoDone; it is empty before any todos exist.
        assert!(
            state.state_tail().contains("0. [ ] fix a"),
            "{}",
            state.state_tail()
        );
        assert!(
            state.state_tail().contains("1. [ ] fix b"),
            "{}",
            state.state_tail()
        );
        assert!(
            state.state_tail().contains("TodoDone"),
            "{}",
            state.state_tail()
        );
        assert!(PlanState::default().state_tail().is_empty());
    }

    /// Reproduces the OLD `todo_tail` formula verbatim (the exact code this
    /// task's `state_tail` replaced) so the byte-invariance test below has
    /// something authoritative to compare against, independent of
    /// `state_tail`'s own implementation.
    fn old_todo_tail_format(state: &PlanState) -> String {
        if state.plan.steps.is_empty() {
            return String::new();
        }
        let items = state
            .plan
            .steps
            .iter()
            .enumerate()
            .map(|(i, s)| {
                format!(
                    "{i}. [{}] {}",
                    if s.done { "x" } else { " " },
                    s.description
                )
            })
            .collect::<Vec<_>>()
            .join("  ");
        let done = state.plan.steps.iter().filter(|s| s.done).count();
        format!(
            "Todos ({done}/{} done): {items}
Work the first undone item; add new items with Todo, mark one done with TodoDone {{\"index\": N}}.",
            state.plan.steps.len()
        )
    }

    /// BYTE-INVARIANCE (protects the just-passed benchmark gate): every
    /// benchmark tier runs with no goal set, so `state_tail`'s output for an
    /// empty-goal plan must be the EXACT SAME bytes the old `todo_tail`
    /// produced -- not just "equivalent," identical, since the gate's
    /// prompt-shape score is sensitive to this string. Covers both an empty
    /// plan (no steps either) and a plan with steps.
    #[test]
    fn state_tail_with_empty_goal_is_byte_identical_to_old_todo_tail() {
        let empty = PlanState::default();
        assert_eq!(empty.state_tail(), old_todo_tail_format(&empty));
        assert_eq!(empty.state_tail(), "");

        let mut with_steps = PlanState::default();
        todo(&mut with_steps, &["fix a", "fix b"]);
        todo_done_committed(&mut with_steps, 0);
        assert_eq!(
            with_steps.state_tail(),
            old_todo_tail_format(&with_steps),
            "a set goal must never change the empty-goal path's bytes"
        );
        assert!(with_steps.plan.goal.is_empty(), "sanity: goal is unset");
        assert!(
            !with_steps.state_tail().starts_with("Goal:"),
            "no goal line may appear when the goal is empty: {}",
            with_steps.state_tail()
        );
    }

    /// Once a goal IS set, `state_tail` prepends a short `Goal: <goal>`
    /// line -- rendered even with no todos yet (the goal is meaningful on
    /// its own, before any Todo call).
    #[test]
    fn state_tail_renders_the_goal_line_when_a_goal_is_set() {
        let mut no_steps = PlanState::default();
        no_steps.plan.goal = "ship the login page".to_string();
        assert_eq!(no_steps.state_tail(), "Goal: ship the login page");

        let mut with_steps = PlanState::default();
        with_steps.plan.goal = "ship the login page".to_string();
        todo(&mut with_steps, &["fix a"]);
        let tail = with_steps.state_tail();
        assert!(tail.starts_with("Goal: ship the login page\n"), "{tail}");
        assert!(tail.contains("Todos (0/1 done)"), "{tail}");
        // The goal line is the ONLY thing added on top of the old todo
        // format -- strip it and what's left must match the old format
        // exactly.
        let without_goal_line = tail.strip_prefix("Goal: ship the login page\n").unwrap();
        assert_eq!(without_goal_line, old_todo_tail_format(&with_steps));
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
        // An item missing its text is named too.
        let reply = state
            .handle_todo_tool(&call(
                "Todo",
                serde_json::json!({"items": [{"foo": "bar"}]}),
            ))
            .unwrap();
        let PlanToolReply::Reply(text) = reply else {
            panic!()
        };
        assert!(text.contains("text"), "{text}");
        // The rejected item did NOT mutate the list.
        assert!(state.plan.steps.is_empty());
    }

    #[test]
    fn append_merge_never_removes_reorders_relabels_or_undones_existing_items() {
        let mut state = PlanState::default();
        todo(&mut state, &["fix a", "fix b", "fix c"]);
        // Complete the middle item: TodoDone self-marks directly.
        todo_done_committed(&mut state, 1);
        let before = state.plan.steps.clone();

        // A second Todo call that tries to (a) drop items, (b) reorder them,
        // (c) relabel "fix a", and (d) re-send "fix b" as undone -- the exact
        // corruption shapes. Append-only merge ignores all of it and only
        // adds the genuinely new "fix d".
        let reply = todo(
            &mut state,
            &["fix c", "fix b", "totally different label", "fix d"],
        );

        // The first three existing items are byte-for-byte unchanged, in the
        // same order, with the same done-flags.
        assert_eq!(
            &state.plan.steps[..3],
            &before[..],
            "existing items are immutable"
        );
        // "fix b" (index 1) is STILL done -- the merge could not un-done it.
        assert!(
            state.plan.steps[1].done,
            "an existing done item can never be un-done via Todo"
        );
        // Only the new label "fix d" was appended; the relabel became a 4th
        // brand-new item (a relabel of an existing item is impossible -- it
        // can only ever add).
        assert_eq!(state.plan.steps.len(), 5);
        assert_eq!(state.plan.steps[3].description, "totally different label");
        assert!(!state.plan.steps[3].done);
        assert_eq!(state.plan.steps[4].description, "fix d");
        assert!(reply.contains("2 added"), "{reply}");
    }

    #[test]
    fn todo_on_an_active_list_cannot_set_anything_done() {
        let mut state = PlanState::default();
        todo(&mut state, &["a", "b"]);
        // Every hostile re-send the model might try: done flags, a shorter
        // list, an all-done list. None of them can flip a done bit.
        todo(&mut state, &["a", "b"]);
        state
            .handle_todo_tool(&call(
                "Todo",
                serde_json::json!({"items": [{"text": "a", "done": true}, {"text": "b", "done": true}]}),
            ))
            .unwrap();
        assert_eq!(
            state.plan.steps.iter().filter(|s| s.done).count(),
            0,
            "Todo is append-only; completion is TodoDone-only"
        );
    }

    /// Convenience: `TodoDone {index}` self-marks directly now (FinishTask-only
    /// observation -- per-item observation collapsed seed 11, gate 2026-07-16),
    /// returning the reply text after the flip commits.
    fn todo_done_committed(state: &mut PlanState, index: usize) -> String {
        match state
            .handle_todo_tool(&call("TodoDone", serde_json::json!({"index": index})))
            .unwrap()
        {
            PlanToolReply::Reply(text) => text,
            other => panic!("a valid TodoDone index must self-commit and reply, got {other:?}"),
        }
    }

    #[test]
    fn todo_done_commits_directly() {
        let mut state = PlanState::default();
        todo(&mut state, &["a", "b"]);
        let reply = state
            .handle_todo_tool(&call("TodoDone", serde_json::json!({"index": 0})))
            .unwrap();
        let PlanToolReply::Reply(text) = reply else {
            panic!("TodoDone self-marks: expected Reply, not a proposal")
        };
        assert!(
            text.contains("Marked done") && text.contains("1/2 done"),
            "{text}"
        );
        // Self-mark: the done flag IS set immediately (no observer at TodoDone).
        assert!(state.plan.steps[0].done);
    }

    #[test]
    fn commit_todo_done_flips_and_returns_exact_reply() {
        let mut state = PlanState::default();
        todo(&mut state, &["a", "b"]);
        let text = state.commit_todo_done(0);
        assert!(text.contains("Marked done"), "{text}");
        assert!(text.contains("1/2 done"), "{text}");
        assert!(state.plan.steps[0].done);
    }

    #[test]
    fn apply_verdict_approved_todo_is_byte_identical_to_old_commit() {
        let mut direct = PlanState::default();
        todo(&mut direct, &["x", "y"]);
        let direct_reply = direct.commit_todo_done(0);

        let mut via_verdict = PlanState::default();
        todo(&mut via_verdict, &["x", "y"]);
        let (verdict_reply, finish) =
            via_verdict.apply_completion_verdict(CompletionKind::TodoItem(0), None, true, "");
        assert!(finish.is_none());
        assert_eq!(
            verdict_reply, direct_reply,
            "the approved path must be byte-identical to the old direct commit"
        );
    }

    #[test]
    fn apply_verdict_rejected_todo_does_not_commit_under_cap() {
        let mut state = PlanState::default();
        todo(&mut state, &["a"]);
        let (text, finish) = state.apply_completion_verdict(
            CompletionKind::TodoItem(0),
            None,
            false,
            "tests still fail",
        );
        assert!(finish.is_none());
        assert!(text.starts_with("Not done"), "{text}");
        assert!(!state.plan.steps[0].done, "a reject must not commit");
    }

    #[test]
    fn reject_cap_lets_the_model_win_after_two_rejects() {
        let mut state = PlanState::default();
        todo(&mut state, &["a"]);
        let kind = CompletionKind::TodoItem(0);
        state.note_reject(&kind);
        state.note_reject(&kind);
        assert!(state.reject_cap_reached(&kind));
        let (text, finish) =
            state.apply_completion_verdict(kind, None, false, "still missing something");
        assert!(finish.is_none());
        assert!(text.contains("closed despite unresolved concern"), "{text}");
        assert!(
            state.plan.steps[0].done,
            "past the cap the model wins and the item commits"
        );
    }

    #[test]
    fn finish_task_still_bounces_undone_before_proposing() {
        let mut state = PlanState::default();
        todo(&mut state, &["fix a"]);
        let reply = state
            .handle_todo_tool(&call("FinishTask", serde_json::json!({"answer": "done!"})))
            .unwrap();
        let PlanToolReply::Reply(text) = reply else {
            panic!("undone todos must still bounce, not propose")
        };
        assert!(text.contains("still undone"), "{text}");
    }

    #[test]
    fn finish_task_clean_proposes_finish() {
        let mut state = PlanState::default();
        let reply = state
            .handle_todo_tool(&call("FinishTask", serde_json::json!({"answer": "42"})))
            .unwrap();
        assert_eq!(
            reply,
            PlanToolReply::ProposeComplete {
                kind: CompletionKind::FinishTask,
                answer: Some("42".to_string()),
            }
        );
    }

    #[test]
    fn todo_done_flips_exactly_one_item() {
        let mut state = PlanState::default();
        todo(&mut state, &["a", "b", "c"]);
        let text = todo_done_committed(&mut state, 1);
        assert!(text.contains("Marked done"), "{text}");
        assert_eq!(
            state.plan.steps.iter().map(|s| s.done).collect::<Vec<_>>(),
            vec![false, true, false],
            "exactly the addressed item flips"
        );
        // Marking an already-done item is a harmless no-op that stays done.
        let text = todo_done_committed(&mut state, 1);
        assert!(text.contains("already done"), "{text}");
        assert!(state.plan.steps[1].done);
    }

    #[test]
    fn todo_done_with_a_bad_index_names_the_valid_undone_items() {
        let mut state = PlanState::default();
        todo(&mut state, &["write data_1", "write data_2"]);
        todo_done_committed(&mut state, 0);
        // Out-of-range index: helpful error naming the remaining undone item
        // with its index, and NO completion happens.
        let PlanToolReply::Reply(text) = state
            .handle_todo_tool(&call("TodoDone", serde_json::json!({"index": 9})))
            .unwrap()
        else {
            panic!()
        };
        assert!(text.starts_with("Error"), "{text}");
        assert!(
            text.contains("1. write data_2"),
            "must name the valid undone item: {text}"
        );
        assert!(
            !text.contains("write data_1"),
            "already-done items are not offered: {text}"
        );
        // A non-integer / missing index is rejected the same helpful way.
        let PlanToolReply::Reply(text) = state
            .handle_todo_tool(&call("TodoDone", serde_json::json!({})))
            .unwrap()
        else {
            panic!()
        };
        assert!(text.contains("index"), "{text}");
        assert!(
            !state.plan.steps[1].done,
            "a bad TodoDone completes nothing"
        );
    }

    #[test]
    fn no_tool_sequence_rewrites_an_active_list_to_all_done_without_doing_the_work() {
        // The tier6 seed-42 failure mode: 14 items, one (data_13) never
        // actually written, yet the run reported 14/14 done because a single
        // Todo call replaced the whole list with everything flagged done.
        // With append-only Todo + TodoDone-per-item, the ONLY way to reach
        // all-done is one TodoDone per item -- there is no bulk shortcut.
        let mut state = PlanState::default();
        let texts: Vec<String> = (0..14).map(|i| format!("write data_{i}")).collect();
        let refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
        todo(&mut state, &refs);

        // Simulate the model doing the work for every item EXCEPT data_13,
        // then trying every corruption trick to close the list:
        for i in 0..14 {
            if i == 13 {
                continue;
            }
            todo_done_committed(&mut state, i);
        }
        // Trick 1: re-Todo the whole list with done flags -> ignored.
        let all_done: Vec<_> = texts
            .iter()
            .map(|t| serde_json::json!({"text": t, "done": true}))
            .collect();
        state
            .handle_todo_tool(&call("Todo", serde_json::json!({"items": all_done})))
            .unwrap();
        // Trick 2: TodoDone a bogus index -> error, no effect.
        state
            .handle_todo_tool(&call("TodoDone", serde_json::json!({"index": 99})))
            .unwrap();

        // data_13 is STILL undone; the list cannot be silently completed.
        assert_eq!(state.next_undone_step(), Some(13));
        assert_eq!(state.plan.steps.iter().filter(|s| !s.done).count(), 1);
        assert_eq!(state.plan.steps.len(), 14, "no phantom items were added");

        // And FinishTask still bounces, naming the one real gap.
        let PlanToolReply::Reply(text) = state
            .handle_todo_tool(&call(
                "FinishTask",
                serde_json::json!({"answer": "all done"}),
            ))
            .unwrap()
        else {
            panic!("expected a bounce")
        };
        assert!(
            text.contains("write data_13"),
            "the bounce names the real gap: {text}"
        );
    }

    #[test]
    fn finish_task_bounces_once_on_undone_todos_then_honors_the_second_attempt() {
        let mut state = PlanState::default();
        todo(&mut state, &["fix a"]);
        // First attempt with an undone todo: bounced, task continues.
        let first = state
            .handle_todo_tool(&call("FinishTask", serde_json::json!({"answer": "done!"})))
            .unwrap();
        // The bounce NAMES the specific undone item (not just a count) so a model
        // that has lost track of which item is the gap is pointed straight at it,
        // and does NOT offer "remove it if it no longer applies" (which a model
        // exploited by marking undone work done -- tier6 diagnosis 2026-07-16).
        let PlanToolReply::Reply(text) = &first else {
            panic!("expected a bounce reply")
        };
        assert!(
            text.contains("fix a"),
            "bounce must name the undone item: {text}"
        );
        assert!(text.contains("still undone"), "{text}");
        assert!(
            !text.contains("no longer apply"),
            "the escape-hatch clause must be gone: {text}"
        );
        // Second attempt is honored — a stuck task can still propose finishing.
        let second = state
            .handle_todo_tool(&call("FinishTask", serde_json::json!({"answer": "done!"})))
            .unwrap();
        assert_eq!(
            second,
            PlanToolReply::ProposeComplete {
                kind: CompletionKind::FinishTask,
                answer: Some("done!".to_string()),
            }
        );
    }

    #[test]
    fn finish_bounce_lists_up_to_five_undone_items_then_summarizes_the_rest() {
        let mut state = PlanState::default();
        let texts: Vec<String> = (0..8).map(|i| format!("task {i}")).collect();
        let refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
        todo(&mut state, &refs);
        let PlanToolReply::Reply(text) = state
            .handle_todo_tool(&call("FinishTask", serde_json::json!({"answer": "x"})))
            .unwrap()
        else {
            panic!("expected a bounce reply")
        };
        // First five named, the remaining three summarized -- bounded reply size.
        assert!(text.contains("task 0") && text.contains("task 4"), "{text}");
        assert!(!text.contains("task 5"), "should cap at 5 named: {text}");
        assert!(text.contains("and 3 more"), "{text}");
    }

    #[test]
    fn finish_task_with_a_clean_list_ends_immediately() {
        let mut state = PlanState::default();
        let reply = state
            .handle_todo_tool(&call("FinishTask", serde_json::json!({"answer": "42"})))
            .unwrap();
        assert_eq!(
            reply,
            PlanToolReply::ProposeComplete {
                kind: CompletionKind::FinishTask,
                answer: Some("42".to_string()),
            }
        );
        // Approving the proposal ends the task exactly as the old direct
        // `Finish` reply did.
        let PlanToolReply::ProposeComplete { kind, answer } = reply else {
            unreachable!()
        };
        let (text, finish) = state.apply_completion_verdict(kind, answer, true, "");
        assert_eq!(finish, Some("42".to_string()));
        assert_eq!(text, "42");
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
        // TodoDone -- the completion tool -- is advertised EVERY turn in both
        // host flavors (no state gating of the tool SET).
        assert!(state.single_mode_tool_names(true).contains(&"TodoDone"));
        assert!(state.single_mode_tool_names(false).contains(&"TodoDone"));
    }

    #[test]
    fn record_mutation_appends_evidence_and_never_clears() {
        let mut st = PlanState::default();
        st.record_mutation("Update", Some("/x/bug_04.txt".into()), false);
        st.record_mutation("Update", Some("/x/bug_04.txt".into()), true);
        st.record_mutation("Bash", None, true);
        assert_eq!(st.mutation_log.len(), 3);
        assert_eq!(
            st.mutation_log[0],
            MutationRecord {
                tool: "Update".into(),
                target: Some("/x/bug_04.txt".into()),
                ok: false
            }
        );
        assert!(st.mutation_log[2].target.is_none());
    }
}
