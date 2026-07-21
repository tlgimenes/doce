//! Observer-verified completion: pure pieces that adjudicate a completion claim
//! against the agent's own action log — file edits AND commands / external tool
//! calls (an ops or comms task completes by *doing*, not by mutating a file).
//! The network call lives in `request_verdict`; everything here is pure and
//! unit-tested.

use crate::agent::plan::{CompletionKind, MutationRecord, Plan};
use crate::inference::ChatMessage;

#[derive(Clone, Debug, PartialEq)]
pub struct Verdict {
    pub complete: bool,
    pub missing: String,
}

pub const OBSERVER_PROMPT: &str = "You are a strict verifier. An agent claims it has completed a unit of work. You are given the claim and a log of the ACTIONS the agent took (each: the tool, the subject it acted on — a file for edits, the command for Bash, otherwise just the tool name — and whether the call succeeded). Judge the evidence, never the agent's assertion. First decide what kind of claim it is:\n- A FILE claim (create/edit/fix/write a file's contents): approve ONLY if the log shows a successful Update/Write to that exact file. A command's success is NOT proof a file was fixed or a test passed — so never approve a file claim on Bash success alone.\n- An ACTION claim (run a command, upgrade packages, send/reply to a message, or any external operation the work completes by DOING rather than by changing a file): approve if the log shows a relevant, successful tool call for that action — e.g. a successful Bash `brew upgrade` for 'upgrade my packages', or a successful email/send tool call for 'reply to the email'.\nREJECT if no logged action is clearly relevant to the claim, or the one that is relevant failed. Do not approve on a line you cannot match to the claim, and do not approve on a different action's success. Respond by calling the Verdict tool: complete=true only if the evidence supports the claim, else complete=false with a short missing naming exactly what evidence is absent.";

/// Render the action log as compact evidence lines the observer can judge: the
/// tool, the subject it acted on (file for edits, command for Bash), and whether
/// it succeeded.
fn render_evidence(log: &[MutationRecord]) -> String {
    if log.is_empty() {
        return "(no actions recorded)".to_string();
    }
    log.iter()
        .map(|r| {
            let outcome = if r.ok { "ok" } else { "failed" };
            match (r.tool.as_str(), r.target.as_deref()) {
                ("Bash", Some(cmd)) => format!("- Bash: `{cmd}` -> {outcome}"),
                (tool, Some(t)) => format!("- {tool} {t} -> {outcome}"),
                (tool, None) => format!("- {tool} -> {outcome}"),
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
                    "The agent claims this todo item is DONE:\n  \"{}\"\n\nAgent's action log:\n{evidence}\n\nIs this item actually done, based ONLY on the evidence? Call Verdict.",
                    step.description
                ),
                None => format!(
                    "The agent claims todo item {i} is DONE, but that index is invalid (out of range for the current plan).\n\nAgent's action log:\n{evidence}\n\nIs this item actually done, based ONLY on the evidence? Call Verdict."
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
                "{goal_line}\n{answer_line}\n\nTodo list:\n{todos}\n\nAgent's action log:\n{evidence}\n\nIs the goal met and the task actually complete, based on the evidence? Call Verdict."
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
    cancel: &tokio_util::sync::CancellationToken,
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
    let outcome = crate::inference::http::LlamaServerClient::new(base_url.to_string())
        .chat(req, |_piece| {}, cancel)
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

    #[test]
    fn action_log_shows_the_bash_command_and_its_outcome() {
        // The observer must see WHICH command ran to judge relevance to an ops
        // claim like "upgrade my packages" — not a bare "ran".
        let plan = Plan {
            goal: "upgrade my packages".to_string(),
            steps: vec![],
        };
        let log = vec![MutationRecord {
            tool: "Bash".to_string(),
            target: Some("brew upgrade".to_string()),
            ok: true,
        }];
        let msgs = build_observer_messages(
            &CompletionKind::FinishTask,
            &plan,
            &log,
            Some("upgraded 3 formulae"),
            Some("upgrade my packages"),
        );
        let joined = msgs.iter().map(|m| m.text()).collect::<Vec<_>>().join("\n");
        assert!(joined.contains("brew upgrade") && joined.contains("-> ok"));
    }

    #[test]
    fn action_log_shows_an_external_tool_action() {
        // A comms action (send an email) shows up as evidence even with no file
        // or command subject — its tool name carries the claim's relevance.
        let plan = Plan {
            goal: "reply to the email".to_string(),
            steps: vec![],
        };
        let log = vec![MutationRecord {
            tool: "send_email".to_string(),
            target: None,
            ok: true,
        }];
        let msgs = build_observer_messages(
            &CompletionKind::FinishTask,
            &plan,
            &log,
            Some("replied"),
            Some("reply to the email"),
        );
        let joined = msgs.iter().map(|m| m.text()).collect::<Vec<_>>().join("\n");
        assert!(joined.contains("send_email") && joined.contains("-> ok"));
    }

    #[test]
    fn observer_prompt_admits_action_claims_but_keeps_file_claims_strict() {
        // Guards the reframe: an external action / command can satisfy an ACTION
        // claim...
        assert!(OBSERVER_PROMPT.contains("ACTION claim"));
        assert!(OBSERVER_PROMPT.contains("FILE claim"));
        // ...while a FILE claim still can't be waved through on Bash success.
        assert!(OBSERVER_PROMPT.contains("never approve a file claim on Bash success alone"));
    }
}
