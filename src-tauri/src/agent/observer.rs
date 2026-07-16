//! Observer-verified completion: pure pieces that adjudicate a completion claim
//! against the agent's own file-mutation evidence. The network call lives in Task 4's
//! `request_verdict`; everything here is pure and unit-tested.

use crate::agent::plan::{CompletionKind, MutationRecord, Plan};
use crate::inference::ChatMessage;

#[derive(Clone, Debug, PartialEq)]
pub struct Verdict {
    pub complete: bool,
    pub missing: String,
}

pub const OBSERVER_PROMPT: &str = "You are a strict verifier. A coding agent claims it has completed a unit of work. You are given the claim and a log of the agent's file mutations (each: tool, target file, and whether the tool call succeeded). Decide whether the evidence actually supports the claim. Approve ONLY if a relevant, successful mutation shows the work was done; if the claimed file was never successfully edited, REJECT. Do not trust the agent's assertion -- judge the evidence. A successful Update to the relevant file is real evidence; a Bash command's success only means it ran, not that a test passed, so never treat Bash success alone as proof a fix is correct. To decide: identify the exact file the claim is about, then scan every line of the mutation log for that exact file as the target. If you find no such line, or its outcome is failed, you must REJECT -- do not approve based on a different file's success or on a line you are not sure matches. Respond by calling the Verdict tool: complete=true only if the evidence supports the claim, else complete=false with a short missing naming exactly what evidence is absent.";

/// Render the mutation log as compact evidence lines the observer can judge.
fn render_evidence(log: &[MutationRecord]) -> String {
    if log.is_empty() {
        return "(no file mutations recorded)".to_string();
    }
    log.iter()
        .map(|r| match r.tool.as_str() {
            "Bash" => "- Bash (no file) -> ran".to_string(),
            _ => {
                let target = r.target.as_deref().unwrap_or("(no target)");
                let outcome = if r.ok { "ok" } else { "failed" };
                format!("- {} {target} -> {outcome}", r.tool)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// PURE. Build the observer's chat messages for a completion claim. System =
/// OBSERVER_PROMPT; one user message stating the specific claim + the evidence.
pub fn build_observer_messages(
    kind: &CompletionKind,
    plan: &Plan,
    mutation_log: &[MutationRecord],
    answer: Option<&str>,
    goal: Option<&str>,
) -> Vec<ChatMessage> {
    let system = ChatMessage::system(OBSERVER_PROMPT);
    let user = match kind {
        CompletionKind::TodoItem(i) => {
            let evidence = render_evidence(mutation_log);
            match plan.steps.get(*i) {
                Some(step) => format!(
                    "The agent claims this todo item is DONE:\n  \"{}\"\n\nAgent's file-mutation log:\n{evidence}\n\nIs this item actually done, based ONLY on the evidence? Call Verdict.",
                    step.description
                ),
                None => format!(
                    "The agent claims todo item {i} is DONE, but that index is invalid (out of range for the current plan).\n\nAgent's file-mutation log:\n{evidence}\n\nIs this item actually done, based ONLY on the evidence? Call Verdict."
                ),
            }
        }
        CompletionKind::FinishTask => {
            let evidence = render_evidence(mutation_log);
            let goal_line = format!("Goal: {}", goal.unwrap_or("(none set)"));
            let answer_line = format!("Final answer: {}", answer.unwrap_or("(none)"));
            let todos = if plan.steps.is_empty() {
                "(no todo items)".to_string()
            } else {
                plan.steps
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
                    .join("\n")
            };
            format!(
                "{goal_line}\n{answer_line}\n\nTodo list:\n{todos}\n\nAgent's file-mutation log:\n{evidence}\n\nIs the goal met and the task actually complete, based on the evidence? Call Verdict."
            )
        }
    };
    vec![system, ChatMessage::user(user)]
}

/// The observer's output is a single Verdict tool call — tiny. Cap output so a
/// runaway can't stall the loop.
pub const OBSERVER_MAX_TOKENS: u32 = 256;

/// Adjudicate a completion claim against the evidence via one observer LLM call.
/// Returns Err on any transport/parse failure — callers FAIL OPEN (treat Err as
/// approve) so the observer can never trap the loop. Deterministic under
/// DOCE_GEN_SEED because `ChatRequest::build` seeds from `seed_from_env()`.
pub async fn request_verdict(
    base_url: &str,
    kind: &CompletionKind,
    plan: &Plan,
    mutation_log: &[MutationRecord],
    answer: Option<&str>,
    goal: Option<&str>,
) -> Result<Verdict, String> {
    let msgs = build_observer_messages(kind, plan, mutation_log, answer, goal);
    let mut req = crate::inference::http::ChatRequest::build(
        "doce",
        crate::inference::http::to_openai_messages(&msgs),
        Some(crate::inference::http::tools_array(&["Verdict"])),
        crate::inference::http::tool_choice_for(crate::inference::ToolCallMode::Require)
            .map(|s| s.to_string()),
    );
    req.max_tokens = Some(OBSERVER_MAX_TOKENS);
    req.disable_thinking(); // no reasoning block — mirrors summarize_and_persist
    let cancel = tokio_util::sync::CancellationToken::new();
    let outcome = crate::inference::http::LlamaServerClient::new(base_url.to_string())
        .chat(req, |_piece| {}, &cancel)
        .await
        .map_err(|e| format!("observer request failed: {e:?}"))?;
    match outcome.tool_call {
        Some((name, args)) if name == "Verdict" => Ok(parse_verdict(&args)),
        other => Err(format!(
            "observer returned no Verdict tool call (got {other:?}, finish={})",
            outcome.finish_reason
        )),
    }
}

/// PURE. Parse the observer's forced `Verdict` tool-call arguments.
pub fn parse_verdict(tool_args: &serde_json::Value) -> Verdict {
    Verdict {
        complete: tool_args
            .get("complete")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        missing: tool_args
            .get("missing")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::plan::PlanStep;

    #[test]
    fn verdict_parses_from_tool_args() {
        let v = parse_verdict(
            &serde_json::json!({"complete": false, "missing": "no edit to bug_04.txt"}),
        );
        assert!(!v.complete);
        assert_eq!(v.missing, "no edit to bug_04.txt");
    }

    #[test]
    fn verdict_defaults_to_incomplete_on_missing_fields() {
        let v = parse_verdict(&serde_json::json!({}));
        assert!(!v.complete); // fail-closed on parse gaps
    }

    #[test]
    fn todo_messages_include_the_item_and_its_failed_evidence() {
        let plan = Plan {
            goal: "fix bugs".to_string(),
            steps: vec![PlanStep {
                description: "Fix bug_04.txt".to_string(),
                done: false,
            }],
        };
        let log = vec![MutationRecord {
            tool: "Update".to_string(),
            target: Some("bug_04.txt".to_string()),
            ok: false,
        }];
        let msgs = build_observer_messages(&CompletionKind::TodoItem(0), &plan, &log, None, None);
        let joined = msgs.iter().map(|m| m.text()).collect::<Vec<_>>().join("\n");
        assert!(joined.contains("Fix bug_04.txt"));
        assert!(joined.contains("bug_04.txt") && joined.contains("failed"));
        assert_eq!(msgs[0].role, "system"); // first msg is the OBSERVER_PROMPT system msg
    }

    #[test]
    fn finish_messages_include_goal_answer_and_todos() {
        let plan_done = Plan {
            goal: "ship the fix".to_string(),
            steps: vec![PlanStep {
                description: "Fix bug_04.txt".to_string(),
                done: true,
            }],
        };
        let msgs = build_observer_messages(
            &CompletionKind::FinishTask,
            &plan_done,
            &[],
            Some("all fixed"),
            Some("ship the fix"),
        );
        let joined = msgs.iter().map(|m| m.text()).collect::<Vec<_>>().join("\n");
        assert!(joined.contains("ship the fix") && joined.contains("all fixed"));
    }
}
