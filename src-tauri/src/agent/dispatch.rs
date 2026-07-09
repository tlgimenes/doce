use crate::agent::tools::{bash, fs, search};
use crate::agent::ToolCall;
use serde_json::json;
use std::path::{Path, PathBuf};

/// 004-tool-call-widgets: the model-facing text (unchanged from before this
/// feature — exactly what used to be `execute`'s whole return value) plus a
/// tool-shaped, serializable `detail` for the UI to render a real widget
/// from (see `data-model.md`) — the two have different needs (the model
/// wants natural-language-ish prose, a widget wants raw structured fields
/// it doesn't have to re-parse out of that prose), so both are produced
/// together here, where the raw tool result is already at hand, rather
/// than the frontend reverse-engineering `model_text`.
#[derive(Debug, Clone)]
pub struct ToolOutcome {
    pub model_text: String,
    pub detail: serde_json::Value,
}

impl ToolOutcome {
    /// The text an offload (`context::offload::offload_if_oversized`)
    /// should write to disk for this outcome. For every tool but Bash,
    /// `model_text` IS the full result. Bash is the one tool whose
    /// `model_text` streams are already tail-biased CAPPED
    /// (`bash::truncate_tail_biased`) while `detail.outcome` still carries
    /// every byte -- offloading the capped copy would break the promise in
    /// tier 1's restorable clearing pointer ("full output saved at
    /// {path}") and in the offload pointer itself ("full output saved"),
    /// so the full rendition is reconstructed from `detail` here.
    pub fn offload_text(&self) -> std::borrow::Cow<'_, str> {
        if self.detail["toolName"] == "Bash" {
            let outcome = &self.detail["outcome"];
            if let (Some(stdout), Some(stderr)) =
                (outcome["stdout"].as_str(), outcome["stderr"].as_str())
            {
                let exit_code = outcome["exitCode"].as_i64().unwrap_or(-1);
                return std::borrow::Cow::Owned(bash_result_model_text(exit_code, stdout, stderr));
            }
        }
        std::borrow::Cow::Borrowed(&self.model_text)
    }
}

/// The one rendition of a completed Bash run the model (and the offload
/// file) ever sees -- shared by the Bash arm below (capped streams) and
/// `ToolOutcome::offload_text` (full streams), so the two can never drift
/// in shape.
fn bash_result_model_text(exit_code: i64, stdout: &str, stderr: &str) -> String {
    format!("exit_code: {exit_code}\nstdout:\n{stdout}\nstderr:\n{stderr}")
}

/// 007-workspace-cwd-resolution: resolves a tool-supplied path against the
/// conversation's working directory. A relative `given` is joined onto
/// `cwd` when one is known; an absolute `given`, or no known `cwd`, passes
/// through unchanged. This is the *only* new logic this feature adds —
/// deliberately not a validation or containment check (FR-004): an
/// absolute path always passes through untouched, regardless of `cwd`.
fn resolve_against(cwd: Option<&Path>, given: &Path) -> PathBuf {
    match cwd {
        Some(base) if given.is_relative() => base.join(given),
        _ => given.to_path_buf(),
    }
}

/// Resolves `Glob`/`Grep`'s optional `path` argument: an explicit value
/// goes through `resolve_against` like any other tool-supplied path; when
/// the model omits it entirely, the default becomes the known `cwd` when
/// there is one, or `"."` — today's existing default — when there isn't.
fn resolve_optional_base(cwd: Option<&Path>, given: Option<&str>) -> PathBuf {
    match given {
        Some(explicit) => resolve_against(cwd, &PathBuf::from(explicit)),
        None => cwd
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(".")),
    }
}

/// When a required argument is missing, checks whether `arguments` has any
/// of the given near-miss key names instead and, if so, names the mistake
/// directly rather than leaving the model to guess why a call it "should"
/// have gotten right failed. Confirmed against the real model: it called
/// Read with `{"file": ...}` instead of `{"file_path": ...}` six times in
/// a row without ever self-correcting, and eventually gave up blaming "the
/// environment" rather than its own wrong key name.
///
/// `legal_args` names every key that is a LEGAL argument of the tool being
/// called: a candidate on that list is never flagged, however the call
/// otherwise failed. Without this, `Grep {"path": ...}` missing `pattern`
/// yielded "(you passed \"path\", the correct key is \"pattern\")" --
/// inviting the model to RENAME a perfectly valid optional argument
/// instead of adding the missing one.
fn wrong_key_hint(
    arguments: &serde_json::Value,
    expected: &str,
    common_mistakes: &[&str],
    legal_args: &[&str],
) -> String {
    common_mistakes
        .iter()
        .filter(|candidate| !legal_args.contains(*candidate))
        .find(|candidate| arguments.get(**candidate).is_some())
        .map(|candidate| format!(" (you passed \"{candidate}\", the correct key is \"{expected}\")"))
        .unwrap_or_default()
}

/// A zero-match Grep whose pattern contains unescaped regex
/// metacharacters is ambiguous between "nothing matches" and "the
/// pattern doesn't mean what you think" — name the suspicion instead of
/// letting an empty result read as verification.
fn regex_literalness_hint(pattern: &str) -> Option<String> {
    let mut prev_backslash = false;
    for c in pattern.chars() {
        if prev_backslash {
            prev_backslash = false;
            continue;
        }
        if c == '\\' {
            prev_backslash = true;
            continue;
        }
        if "+*?()[]{}|".contains(c) {
            return Some(format!(
                " Note: your pattern contains '{c}', a regex metacharacter — if you meant the literal character, escape it as '\\{c}' and search again."
            ));
        }
    }
    None
}

/// (tool, required string-typed args). The NVIDIA SLM-agents "simple
/// format checks" applied at the boundary: a malformed call becomes a
/// one-turn correction naming exactly what's missing, instead of each
/// tool arm improvising (the model was observed repeating a wrong key
/// six times without self-correcting when the error didn't name it).
const REQUIRED_STRING_ARGS: &[(&str, &[&str])] = &[
    ("Read", &["file_path"]),
    ("Write", &["file_path", "content"]),
    ("Edit", &["file_path", "old_string", "new_string"]),
    ("Bash", &["command"]),
    ("Glob", &["pattern"]),
    ("Grep", &["pattern"]),
];

/// (tool, every legal argument key -- required AND optional). Fed to
/// `wrong_key_hint` so a key that is a legitimate argument of the SAME
/// tool (e.g. Glob/Grep's optional `path`) is never flagged as a
/// near-miss for a missing required one. Kept next to
/// `REQUIRED_STRING_ARGS` so a schema change updates both together.
const LEGAL_TOOL_ARGS: &[(&str, &[&str])] = &[
    ("Read", &["file_path", "offset", "limit"]),
    ("Write", &["file_path", "content"]),
    ("Edit", &["file_path", "old_string", "new_string", "replace_all"]),
    ("Bash", &["command", "timeout"]),
    ("Glob", &["pattern", "path"]),
    ("Grep", &["pattern", "path", "glob"]),
];

/// Checked as the first thing `execute()` does, ahead of every per-tool
/// arm: generalizes `wrong_key_hint` from the 3 tools that used to call it
/// by hand to all 6 built-in tools with required string arguments, and
/// additionally catches a required argument present under the right key
/// but the wrong JSON type (a bare `None` from `.as_str()` couldn't tell
/// "missing" apart from "wrong type" — this can).
fn validate_required_args(call: &ToolCall) -> Option<String> {
    let (_, required) = REQUIRED_STRING_ARGS
        .iter()
        .find(|(name, _)| *name == call.name)?;
    let legal_args: &[&str] = LEGAL_TOOL_ARGS
        .iter()
        .find(|(name, _)| *name == call.name)
        .map(|(_, args)| *args)
        .unwrap_or(&[]);
    let problems: Vec<String> = required
        .iter()
        .filter_map(|key| match call.arguments.get(*key) {
            None => {
                let hint = wrong_key_hint(
                    &call.arguments,
                    key,
                    &["file", "path", "filepath", "filename", "text", "cmd"],
                    legal_args,
                );
                Some(format!("missing required \"{key}\" (a string){hint}"))
            }
            Some(v) if !v.is_string() => Some(format!("\"{key}\" must be a string")),
            Some(_) => None,
        })
        .collect();
    if problems.is_empty() {
        None
    } else {
        Some(format!(
            "Error: invalid {} arguments: {}. Re-issue the call with the corrected arguments.",
            call.name,
            problems.join("; ")
        ))
    }
}

/// Executes a parsed `ToolCall` against the real built-in tools (FR-009).
/// `cwd` is the conversation's workspace path, if it has one
/// (007-workspace-cwd-resolution) — used only to resolve *relative* paths
/// and to give `Bash` a sensible starting directory. Absolute paths are
/// never restricted to any workspace folder (FR-009's explicit
/// requirement, unchanged by this feature) — an absolute path is always
/// taken exactly as given.
pub fn execute(call: &ToolCall, cwd: Option<&Path>) -> ToolOutcome {
    if let Some(error) = validate_required_args(call) {
        let a = |key: &str| {
            call.arguments
                .get(key)
                .filter(|v| v.is_string())
                .cloned()
                .unwrap_or(serde_json::Value::Null)
        };
        // Widget-safe minimal shapes: each known tool's detail must satisfy
        // its typed widget's required fields (SearchResultsWidget reads
        // detail.matches.length unconditionally — an absent `matches` is a
        // frontend crash, not a cosmetic gap).
        let detail = match call.name.as_str() {
            "Glob" => json!({"toolName": "Glob", "pattern": a("pattern"), "path": a("path"), "matches": [], "outcome": {"ok": false, "error": error}}),
            "Grep" => json!({"toolName": "Grep", "pattern": a("pattern"), "path": a("path"), "glob": a("glob"), "matches": [], "truncated": false, "skippedOversized": 0, "outcome": {"ok": false, "error": error}}),
            "Read" => json!({"toolName": "Read", "filePath": a("file_path"), "outcome": {"ok": false, "error": error}}),
            "Write" => json!({"toolName": "Write", "filePath": a("file_path"), "outcome": {"ok": false, "error": error}}),
            "Edit" => json!({"toolName": "Edit", "filePath": a("file_path"), "oldString": a("old_string"), "newString": a("new_string"), "replaceAll": false, "outcome": {"ok": false, "error": error}}),
            "Bash" => json!({"toolName": "Bash", "command": a("command"), "timeoutMs": null, "outcome": {"ok": false, "error": error}}),
            other => json!({"toolName": other, "arguments": call.arguments, "outcome": {"ok": false, "error": error}}),
        };
        return ToolOutcome { detail, model_text: error };
    }
    match call.name.as_str() {
        "Read" => {
            // validate_required_args already guaranteed file_path is present
            // and a string.
            let path = call
                .arguments
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let offset = call
                .arguments
                .get("offset")
                .and_then(|v| v.as_u64())
                .map(|n| n as usize);
            let limit = call
                .arguments
                .get("limit")
                .and_then(|v| v.as_u64())
                .map(|n| n as usize);
            let resolved = resolve_against(cwd, &PathBuf::from(path));
            match fs::read(&resolved, offset, limit) {
                Ok(content) => {
                    // fs::read caps at `limit` (default 2000) lines — the
                    // returned content hitting that count exactly is the
                    // only signal available here that more lines existed
                    // past the cap, without a second, wasteful read.
                    let cap = limit.unwrap_or(2000);
                    let truncated = content.lines().count() >= cap;
                    ToolOutcome {
                        model_text: content.clone(),
                        detail: json!({
                            "toolName": "Read", "filePath": path, "offset": offset, "limit": limit,
                            "outcome": {"ok": true, "content": content, "truncated": truncated},
                        }),
                    }
                }
                Err(e) => {
                    let text = format!("Error: {e}");
                    ToolOutcome {
                        detail: json!({
                            "toolName": "Read", "filePath": path, "offset": offset, "limit": limit,
                            "outcome": {"ok": false, "error": text},
                        }),
                        model_text: text,
                    }
                }
            }
        }
        "Write" => {
            // validate_required_args already guaranteed file_path and
            // content are present and strings.
            let path = call
                .arguments
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let content = call
                .arguments
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let resolved = resolve_against(cwd, &PathBuf::from(path));
            match fs::write(&resolved, content) {
                Ok(()) => ToolOutcome {
                    model_text: "File written successfully".to_string(),
                    detail: json!({
                        "toolName": "Write", "filePath": path,
                        "contentPreview": content.chars().take(500).collect::<String>(),
                        "byteCount": content.len(),
                        "outcome": {"ok": true},
                    }),
                },
                Err(e) => {
                    let text = format!("Error: {e}");
                    ToolOutcome {
                        detail: json!({
                            "toolName": "Write", "filePath": path,
                            "contentPreview": content.chars().take(500).collect::<String>(),
                            "byteCount": content.len(),
                            "outcome": {"ok": false, "error": text},
                        }),
                        model_text: text,
                    }
                }
            }
        }
        "Edit" => {
            // validate_required_args already guaranteed file_path,
            // old_string, and new_string are present and strings.
            let path = call
                .arguments
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let old_string = call
                .arguments
                .get("old_string")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let new_string = call
                .arguments
                .get("new_string")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let replace_all = call
                .arguments
                .get("replace_all")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let resolved = resolve_against(cwd, &PathBuf::from(path));
            let base_detail = json!({
                "toolName": "Edit", "filePath": path, "oldString": old_string,
                "newString": new_string, "replaceAll": replace_all,
            });
            match fs::edit(&resolved, old_string, new_string, replace_all) {
                Ok(()) => {
                    let mut detail = base_detail;
                    detail["outcome"] = json!({"ok": true});
                    ToolOutcome {
                        model_text: "Edit applied successfully".to_string(),
                        detail,
                    }
                }
                Err(e) => {
                    let text = format!("Error: {e}");
                    let mut detail = base_detail;
                    detail["outcome"] = json!({"ok": false, "error": text});
                    ToolOutcome {
                        model_text: text,
                        detail,
                    }
                }
            }
        }
        "Bash" => {
            // validate_required_args already guaranteed command is present
            // and a string.
            let command = call
                .arguments
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let timeout_ms = call.arguments.get("timeout").and_then(|v| v.as_u64());
            match bash::run(command, timeout_ms, cwd) {
                // Restorable-compression: `model_text` (what the model
                // reads, and what `offload_if_oversized` sees downstream)
                // gets the tail-biased cap; `detail.outcome.stdout`/
                // `stderr` keep `result`'s FULL, untruncated text so the
                // transcript widget (and anything reading `detail` back
                // later) never loses data the model itself didn't see.
                Ok(result) => ToolOutcome {
                    model_text: bash_result_model_text(
                        i64::from(result.exit_code),
                        &bash::truncate_tail_biased(&result.stdout),
                        &bash::truncate_tail_biased(&result.stderr),
                    ),
                    detail: json!({
                        "toolName": "Bash", "command": command, "timeoutMs": timeout_ms,
                        "outcome": {
                            "ok": true, "exitCode": result.exit_code,
                            "stdout": result.stdout, "stderr": result.stderr,
                        },
                    }),
                },
                Err(e) => {
                    let text = format!("Error: {e}");
                    ToolOutcome {
                        detail: json!({
                            "toolName": "Bash", "command": command, "timeoutMs": timeout_ms,
                            "outcome": {"ok": false, "error": text},
                        }),
                        model_text: text,
                    }
                }
            }
        }
        "Glob" => {
            // validate_required_args already guaranteed pattern is present
            // and a string.
            let pattern = call
                .arguments
                .get("pattern")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let base =
                resolve_optional_base(cwd, call.arguments.get("path").and_then(|v| v.as_str()));
            match search::glob_search(pattern, &base) {
                Ok(paths) => {
                    let matches: Vec<String> =
                        paths.iter().map(|p| p.display().to_string()).collect();
                    ToolOutcome {
                        model_text: if !matches.is_empty() {
                            matches.join("\n")
                        } else if pattern.contains(char::is_whitespace) {
                            // A real glob pattern is a single wildcard
                            // expression and never contains whitespace --
                            // this is the exact shape of mistake seen
                            // against the real model (a space-separated
                            // list of literal filenames passed as
                            // "pattern"), which silently matches nothing
                            // and reads to the model as "these files
                            // don't exist" rather than "I used the tool
                            // wrong" (confirmed against the real model: it
                            // trusted its own malformed call and gave up
                            // on a task whose files were there all along).
                            format!(
                                "No files matched \"{pattern}\". This pattern contains spaces, which usually means it isn't a valid glob pattern -- glob patterns are a single wildcard expression, e.g. \"bug_*.txt\" or \"*.rs\", not a space-separated list of literal filenames."
                            )
                        } else {
                            "No files matched".to_string()
                        },
                        detail: json!({
                            "toolName": "Glob", "pattern": pattern,
                            "path": base.display().to_string(), "matches": matches,
                        }),
                    }
                }
                Err(e) => {
                    let text = format!("Error: {e}");
                    ToolOutcome {
                        detail: json!({
                            "toolName": "Glob", "pattern": pattern,
                            "path": base.display().to_string(), "matches": [],
                        }),
                        model_text: text,
                    }
                }
            }
        }
        "Grep" => {
            // validate_required_args already guaranteed pattern is present
            // and a string.
            let pattern = call
                .arguments
                .get("pattern")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let base =
                resolve_optional_base(cwd, call.arguments.get("path").and_then(|v| v.as_str()));
            let glob_filter = call.arguments.get("glob").and_then(|v| v.as_str());
            match search::grep(pattern, &base, glob_filter) {
                Ok(outcome) => {
                    let match_values: Vec<serde_json::Value> = outcome
                        .matches
                        .iter()
                        .map(|m| {
                            json!({
                                "path": m.path.display().to_string(),
                                "lineNumber": m.line_number,
                                "line": m.line,
                            })
                        })
                        .collect();
                    let mut model_text = if outcome.matches.is_empty() {
                        let mut text = "No matches found".to_string();
                        text.push_str(&regex_literalness_hint(pattern).unwrap_or_default());
                        text
                    } else {
                        outcome
                            .matches
                            .iter()
                            .map(|m| format!("{}:{}:{}", m.path.display(), m.line_number, m.line))
                            .collect::<Vec<_>>()
                            .join("\n")
                    };
                    // Truncation/skip disclosure: without these lines the
                    // model can't tell "exactly N matches, complete" from
                    // "capped, arbitrarily walk-order-selected", and a
                    // match inside a skipped oversized file reads as a
                    // plain (false) "No matches found".
                    if outcome.truncated {
                        model_text.push_str(&format!(
                            "\n(Results capped at {} matches — narrow the pattern, path, or glob to see the rest.)",
                            search::GREP_RESULT_CAP
                        ));
                    }
                    if outcome.skipped_oversized > 0 {
                        model_text.push_str(&format!(
                            "\n({} file(s) larger than {}MB were skipped without being searched.)",
                            outcome.skipped_oversized,
                            search::GREP_MAX_FILE_LEN / (1024 * 1024)
                        ));
                    }
                    ToolOutcome {
                        model_text,
                        detail: json!({
                            "toolName": "Grep", "pattern": pattern,
                            "path": base.display().to_string(), "glob": glob_filter,
                            "matches": match_values,
                            "truncated": outcome.truncated,
                            "skippedOversized": outcome.skipped_oversized,
                        }),
                    }
                }
                Err(e) => {
                    let text = format!("Error: {e}");
                    ToolOutcome {
                        detail: json!({
                            "toolName": "Grep", "pattern": pattern,
                            "path": base.display().to_string(), "glob": glob_filter, "matches": [],
                        }),
                        model_text: text,
                    }
                }
            }
        }
        other => ToolOutcome {
            model_text: format!("Error: unknown tool '{other}'"),
            detail: json!({
                "toolName": other, "arguments": call.arguments,
                "outcome": {"ok": false, "text": format!("unknown tool '{other}'")},
            }),
        },
    }
}

/// `execute`, moved off the async executor. Every built-in tool is
/// synchronous, blocking work (file reads, directory walks, child
/// processes) — running it inline in an async context wedges the entire
/// runtime for the duration of the call, which in production froze the
/// whole app for as long as one slow Grep took (the tool loop, and
/// everything else sharing the runtime, lives on these threads). Owned
/// parameters because `spawn_blocking` requires `'static`.
pub async fn execute_async(call: ToolCall, cwd: Option<PathBuf>) -> ToolOutcome {
    let name = call.name.clone();
    let arguments = call.arguments.clone();
    match tokio::task::spawn_blocking(move || execute(&call, cwd.as_deref())).await {
        Ok(outcome) => outcome,
        // A panic inside a tool becomes an ordinary tool-error result the
        // model can react to, not a crashed agent turn.
        Err(e) => {
            let text = format!("Error: tool execution failed: {e}");
            ToolOutcome {
                detail: json!({
                    "toolName": name, "arguments": arguments,
                    "outcome": {"ok": false, "error": text},
                }),
                model_text: text,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs as stdfs;
    use tempfile::tempdir;

    fn call(name: &str, arguments: serde_json::Value) -> ToolCall {
        ToolCall {
            name: name.to_string(),
            arguments,
        }
    }

    #[test]
    fn dispatches_read() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("f.txt");
        stdfs::write(&file, "hello\n").unwrap();

        let result = execute(
            &call(
                "Read",
                serde_json::json!({"file_path": file.to_str().unwrap()}),
            ),
            None,
        );
        assert!(result.model_text.contains("hello"));
    }

    #[test]
    fn dispatches_write_then_read() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("f.txt");

        let write_result = execute(
            &call(
                "Write",
                serde_json::json!({"file_path": file.to_str().unwrap(), "content": "new content"}),
            ),
            None,
        );
        assert!(write_result.model_text.contains("successfully"));
        assert_eq!(stdfs::read_to_string(&file).unwrap(), "new content");
    }

    // --- 004-tool-call-widgets: US1 (Edit -> diff widget) ---

    #[test]
    fn us1_edit_success_produces_the_documented_detail_shape() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("f.txt");
        stdfs::write(&file, "hello world\n").unwrap();

        let result = execute(
            &call(
                "Edit",
                serde_json::json!({
                    "file_path": file.to_str().unwrap(),
                    "old_string": "world",
                    "new_string": "there",
                }),
            ),
            None,
        );

        assert_eq!(result.detail["toolName"], "Edit");
        assert_eq!(result.detail["filePath"], file.to_str().unwrap());
        assert_eq!(result.detail["oldString"], "world");
        assert_eq!(result.detail["newString"], "there");
        assert_eq!(result.detail["replaceAll"], false);
        assert_eq!(result.detail["outcome"]["ok"], true);
        assert_eq!(stdfs::read_to_string(&file).unwrap(), "hello there\n");
    }

    #[test]
    fn us1_edit_failure_produces_ok_false_with_a_non_empty_error() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("f.txt");
        stdfs::write(&file, "hello world\n").unwrap();

        let result = execute(
            &call(
                "Edit",
                serde_json::json!({
                    "file_path": file.to_str().unwrap(),
                    "old_string": "not present in the file",
                    "new_string": "there",
                }),
            ),
            None,
        );

        assert_eq!(result.detail["outcome"]["ok"], false);
        let error = result.detail["outcome"]["error"].as_str().unwrap();
        assert!(!error.is_empty());
        // The file must be untouched by a failed edit.
        assert_eq!(stdfs::read_to_string(&file).unwrap(), "hello world\n");
    }

    #[test]
    fn dispatches_bash_and_captures_denylist_rejection() {
        let result = execute(
            &call("Bash", serde_json::json!({"command": "rm -rf ~"})),
            None,
        );
        assert!(result.model_text.contains("Error"));
        assert!(result.model_text.contains("catastrophic"));
    }

    // --- 004-tool-call-widgets: US2 (Bash -> terminal widget) ---

    #[test]
    fn us2_bash_success_produces_the_documented_detail_shape() {
        let result = execute(
            &call("Bash", serde_json::json!({"command": "echo hi"})),
            None,
        );

        assert_eq!(result.detail["toolName"], "Bash");
        assert_eq!(result.detail["command"], "echo hi");
        assert_eq!(result.detail["outcome"]["ok"], true);
        assert_eq!(result.detail["outcome"]["exitCode"], 0);
        assert!(result.detail["outcome"]["stdout"]
            .as_str()
            .unwrap()
            .contains("hi"));
    }

    #[test]
    fn us2_bash_non_zero_exit_is_a_completed_run_not_a_dispatch_failure() {
        // A completed-but-failed run (contracts/tool-widgets.md's Failure
        // handling) — outcome.ok stays true, exitCode carries the failure.
        let result = execute(
            &call("Bash", serde_json::json!({"command": "exit 7"})),
            None,
        );

        assert_eq!(result.detail["outcome"]["ok"], true);
        assert_eq!(result.detail["outcome"]["exitCode"], 7);
    }

    #[test]
    fn us2_bash_denylisted_command_produces_ok_false() {
        let result = execute(
            &call("Bash", serde_json::json!({"command": "rm -rf ~"})),
            None,
        );

        assert_eq!(result.detail["toolName"], "Bash");
        assert_eq!(result.detail["outcome"]["ok"], false);
    }

    // --- Bash output cap (tail-biased, at the tool) ---

    #[test]
    fn us2_bash_oversized_stdout_is_tail_biased_in_model_text_but_full_in_detail() {
        // Portable POSIX sh (bash.rs's `run()` always shells out via
        // `/bin/sh -c`) loop producing 20k short lines -- well over
        // BASH_OUTPUT_MAX_BYTES once joined.
        let result = execute(
            &call(
                "Bash",
                serde_json::json!({
                    "command": "i=0; while [ $i -lt 20000 ]; do echo line-$i; i=$((i+1)); done"
                }),
            ),
            None,
        );

        // The stdout section of model_text must be capped, head- and
        // tail-preserved, with the omission disclosed.
        let stdout_section = result
            .model_text
            .split("stdout:\n")
            .nth(1)
            .and_then(|rest| rest.split("\nstderr:\n").next())
            .expect("model_text must have a stdout: section");
        assert!(
            stdout_section.len() <= bash::BASH_OUTPUT_MAX_BYTES + 1024,
            "stdout section of model_text must be tail-biased truncated, got {} bytes",
            stdout_section.len()
        );
        assert!(stdout_section.contains("line-0\n"), "head preserved");
        assert!(stdout_section.contains("line-19999"), "tail preserved");
        assert!(stdout_section.contains("bytes omitted"));

        // detail.outcome.stdout, by contrast, must keep every byte -- the
        // restorable-compression rule: the transcript widget (and any
        // later re-read) must never lose data the model itself didn't
        // see.
        let full_stdout = result.detail["outcome"]["stdout"].as_str().unwrap();
        assert!(
            full_stdout.len() > bash::BASH_OUTPUT_MAX_BYTES,
            "detail.outcome.stdout must retain the FULL untruncated output, got only {} bytes",
            full_stdout.len()
        );
        assert!(full_stdout.contains("line-0\n"));
        assert!(full_stdout.contains("line-19999"));
    }

    // --- F2 (final whole-branch review): offload sees the ORIGINAL text ---

    #[test]
    fn bash_offload_text_reconstructs_the_full_untruncated_rendition() {
        // Same >64KB shape as the tail-biased test above: model_text is
        // capped (middle lines gone), but what the offload writes to disk
        // must be the FULL rendition -- the tier-1 clearing pointer and the
        // offload pointer both promise "full output saved at {path}".
        let result = execute(
            &call(
                "Bash",
                serde_json::json!({
                    "command": "i=0; while [ $i -lt 20000 ]; do echo line-$i; i=$((i+1)); done"
                }),
            ),
            None,
        );

        // A middle line the tail-biased cap definitely dropped from
        // model_text.
        assert!(!result.model_text.contains("line-10000\n"));
        let offload_text = result.offload_text();
        assert!(offload_text.starts_with("exit_code: 0\nstdout:\n"));
        assert!(offload_text.contains("line-0\n"));
        assert!(offload_text.contains("line-10000\n"), "the offload text must keep every byte");
        assert!(offload_text.contains("line-19999"));
        assert!(!offload_text.contains("bytes omitted"));
    }

    #[test]
    fn offload_text_is_model_text_for_small_bash_and_for_every_other_tool() {
        // Under the cap, Bash's reconstruction and model_text are
        // byte-identical.
        let small = execute(&call("Bash", serde_json::json!({"command": "echo hi"})), None);
        assert_eq!(small.offload_text(), small.model_text);

        // A failed Bash dispatch has no streams in detail.outcome --
        // model_text passes through.
        let failed = execute(&call("Bash", serde_json::json!({"command": "rm -rf ~"})), None);
        assert_eq!(failed.offload_text(), failed.model_text);

        // Non-Bash tools never reconstruct.
        let dir = tempdir().unwrap();
        stdfs::write(dir.path().join("f.txt"), "hello\n").unwrap();
        let read = execute(
            &call(
                "Read",
                serde_json::json!({"file_path": dir.path().join("f.txt").to_str().unwrap()}),
            ),
            None,
        );
        assert_eq!(read.offload_text(), read.model_text);
    }

    #[test]
    fn bash_output_under_the_cap_is_unaffected() {
        // Existing exact-stdout tests (us2_bash_success_produces_the_...,
        // dispatches_bash_and_captures_denylist_rejection, etc.) already
        // guard the ordinary path; this test names the invariant directly:
        // small output must round-trip completely unchanged in both
        // model_text and detail, with no omission marker at all.
        let result = execute(
            &call("Bash", serde_json::json!({"command": "echo hello"})),
            None,
        );
        assert!(result.model_text.contains("hello"));
        assert!(!result.model_text.contains("bytes omitted"));
        assert_eq!(result.detail["outcome"]["stdout"], "hello\n");
    }

    #[test]
    fn unknown_tool_returns_a_clear_error_not_a_panic() {
        let result = execute(&call("NotARealTool", serde_json::json!({})), None);
        assert!(result.model_text.contains("unknown tool"));
    }

    #[tokio::test]
    async fn execute_async_does_not_block_the_async_executor() {
        // #[tokio::test]'s default runtime is single-threaded: if tool
        // execution ran synchronously on the executor thread (the exact
        // production bug — a wedged Grep froze every other task sharing
        // the runtime), the concurrent 50ms timer below couldn't fire
        // until the whole `sleep 0.5` shell command finished.
        let started = std::time::Instant::now();
        let timer = async {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            started.elapsed()
        };
        let (outcome, timer_elapsed) = tokio::join!(
            execute_async(
                call("Bash", serde_json::json!({"command": "sleep 0.5"})),
                None
            ),
            timer
        );
        assert_eq!(outcome.detail["outcome"]["ok"], true);
        assert!(
            timer_elapsed < std::time::Duration::from_millis(400),
            "the 50ms timer only fired after {timer_elapsed:?} — tool execution starved the executor"
        );
    }

    #[test]
    fn missing_required_argument_returns_a_clear_error() {
        let result = execute(&call("Read", serde_json::json!({})), None);
        assert!(result.model_text.contains("Error"));
        assert!(result.model_text.contains("file_path"));
    }

    #[test]
    fn read_with_the_wrong_key_name_gets_a_hint_not_a_bare_missing_argument_error() {
        // Confirmed against the real model: it called Read with
        // {"file": "..."} instead of {"file_path": "..."} six times in a
        // row without ever self-correcting, eventually blaming "the
        // environment" rather than its own wrong key name.
        let result = execute(
            &call("Read", serde_json::json!({"file": "/tmp/example.txt"})),
            None,
        );
        assert!(
            result.model_text.contains("\"file\"") && result.model_text.contains("\"file_path\""),
            "expected a hint naming both the wrong key and the correct one, got: {:?}",
            result.model_text
        );
    }

    #[test]
    fn a_legal_optional_arg_is_never_flagged_as_a_wrong_key() {
        // Task 5 ledger item (final whole-branch review): `path` IS a
        // legitimate optional argument of Grep/Glob -- a call carrying a
        // valid `path` but missing `pattern` must name the missing key
        // WITHOUT the near-miss hint, which read as an instruction to
        // rename (destroy) the valid argument.
        for tool in ["Grep", "Glob"] {
            let result = execute(&call(tool, serde_json::json!({"path": "/tmp"})), None);
            assert!(result.model_text.contains("pattern"), "must still name the missing key");
            assert!(
                !result.model_text.contains("you passed"),
                "{tool}'s legal optional `path` must not be flagged, got: {:?}",
                result.model_text
            );
        }
        // But the same key stays a genuine near-miss where it ISN'T legal:
        // Read has no `path` argument at all.
        let result = execute(&call("Read", serde_json::json!({"path": "/tmp/x"})), None);
        assert!(
            result.model_text.contains("\"path\"") && result.model_text.contains("\"file_path\""),
            "Read must keep hinting path -> file_path, got: {:?}",
            result.model_text
        );
    }

    #[test]
    fn missing_required_arguments_get_a_schema_shaped_error_before_dispatch() {
        let result = execute(&call("Grep", serde_json::json!({})), None);
        assert!(result.model_text.starts_with("Error:"));
        assert!(result.model_text.contains("pattern"), "must name the missing key");

        let result = execute(&call("Edit", serde_json::json!({"file_path": "/a"})), None);
        assert!(result.model_text.contains("old_string"));
        assert!(result.model_text.contains("new_string"));
    }

    #[test]
    fn wrong_type_arguments_get_named() {
        let result = execute(&call("Read", serde_json::json!({"file_path": 42})), None);
        assert!(result.model_text.starts_with("Error:"));
        assert!(result.model_text.contains("file_path"));
        assert!(result.model_text.contains("string"));
    }

    #[test]
    fn validation_failure_details_stay_widget_safe() {
        // SearchResultsWidget reads detail.matches.length unconditionally —
        // a validation-failure detail for Glob/Grep must carry matches: [].
        for tool in ["Glob", "Grep"] {
            let result = execute(&call(tool, serde_json::json!({})), None);
            assert!(result.model_text.starts_with("Error:"));
            assert!(
                result.detail["matches"].is_array(),
                "{tool} validation-failure detail must include matches: [], got {}",
                result.detail
            );
        }
        let result = execute(&call("Read", serde_json::json!({})), None);
        assert!(result.detail["filePath"].is_null());

        // Wrong-TYPE values must not be echoed raw: an object-valued arg
        // rendered as a JSX child crashes React ("Objects are not valid
        // as a React child").
        let result = execute(
            &call("Glob", serde_json::json!({"pattern": {"nested": "*.rs"}})),
            None,
        );
        assert!(result.model_text.starts_with("Error:"));
        assert!(
            result.detail["pattern"].is_null(),
            "non-string args must echo as null, got {}",
            result.detail
        );
    }

    // --- 004-tool-call-widgets: US4 (Read/Write/Glob/Grep widgets) ---

    #[test]
    fn us4_read_success_and_failure_produce_the_documented_detail_shape() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("f.txt");
        stdfs::write(&file, "hello\n").unwrap();

        let ok = execute(
            &call(
                "Read",
                serde_json::json!({"file_path": file.to_str().unwrap()}),
            ),
            None,
        );
        assert_eq!(ok.detail["toolName"], "Read");
        assert_eq!(ok.detail["filePath"], file.to_str().unwrap());
        assert_eq!(ok.detail["outcome"]["ok"], true);
        assert_eq!(ok.detail["outcome"]["truncated"], false);

        let missing = dir.path().join("does-not-exist.txt");
        let failed = execute(
            &call(
                "Read",
                serde_json::json!({"file_path": missing.to_str().unwrap()}),
            ),
            None,
        );
        assert_eq!(failed.detail["outcome"]["ok"], false);
        assert!(!failed.detail["outcome"]["error"]
            .as_str()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn us4_write_success_and_failure_produce_the_documented_detail_shape() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("f.txt");

        let ok = execute(
            &call(
                "Write",
                serde_json::json!({"file_path": file.to_str().unwrap(), "content": "hi there"}),
            ),
            None,
        );
        assert_eq!(ok.detail["toolName"], "Write");
        assert_eq!(ok.detail["filePath"], file.to_str().unwrap());
        assert_eq!(ok.detail["byteCount"], 8);
        assert_eq!(ok.detail["outcome"]["ok"], true);

        // A parent directory that can't exist (its own parent is a file,
        // not a directory) forces a real io error.
        let bogus_parent = dir.path().join("f.txt").join("nested.txt");
        let failed = execute(
            &call(
                "Write",
                serde_json::json!({"file_path": bogus_parent.to_str().unwrap(), "content": "x"}),
            ),
            None,
        );
        assert_eq!(failed.detail["outcome"]["ok"], false);
    }

    #[test]
    fn us4_glob_with_and_without_matches_produces_the_documented_detail_shape() {
        let dir = tempdir().unwrap();
        stdfs::write(dir.path().join("a.rs"), "").unwrap();

        let with_matches = execute(
            &call(
                "Glob",
                serde_json::json!({"pattern": "*.rs", "path": dir.path().to_str().unwrap()}),
            ),
            None,
        );
        assert_eq!(with_matches.detail["toolName"], "Glob");
        assert_eq!(with_matches.detail["matches"].as_array().unwrap().len(), 1);

        let no_matches = execute(
            &call(
                "Glob",
                serde_json::json!({"pattern": "*.nonexistent", "path": dir.path().to_str().unwrap()}),
            ),
            None,
        );
        assert_eq!(no_matches.detail["matches"].as_array().unwrap().len(), 0);
        assert_eq!(no_matches.model_text, "No files matched");
    }

    #[test]
    fn glob_with_a_whitespace_containing_pattern_hints_at_the_mistake_instead_of_a_bare_no_match() {
        // Confirmed against the real model: a space-separated list of
        // literal filenames passed as "pattern" (not a wildcard
        // expression) matches nothing, and a bare "No files matched" read
        // to the model as "these files don't exist" rather than "I used
        // the tool wrong" -- it trusted its own malformed call and gave up
        // on a task whose files were there all along.
        let dir = tempdir().unwrap();
        stdfs::write(dir.path().join("bug_00.txt"), "").unwrap();

        let result = execute(
            &call(
                "Glob",
                serde_json::json!({"pattern": "bug_00.txt bug_01.txt", "path": dir.path().to_str().unwrap()}),
            ),
            None,
        );
        assert!(
            result.model_text.contains("glob pattern"),
            "expected a hint about valid glob syntax, got: {:?}",
            result.model_text
        );
        assert!(result.model_text.contains("bug_*.txt") || result.model_text.contains("*.rs"));
    }

    #[test]
    fn us4_grep_with_and_without_matches_produces_the_documented_detail_shape() {
        let dir = tempdir().unwrap();
        stdfs::write(dir.path().join("f.txt"), "hello world\n").unwrap();

        let with_matches = execute(
            &call(
                "Grep",
                serde_json::json!({"pattern": "hello", "path": dir.path().to_str().unwrap()}),
            ),
            None,
        );
        assert_eq!(with_matches.detail["toolName"], "Grep");
        let matches = with_matches.detail["matches"].as_array().unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0]["lineNumber"], 1);

        let no_matches = execute(
            &call(
                "Grep",
                serde_json::json!({"pattern": "nonexistent-pattern", "path": dir.path().to_str().unwrap()}),
            ),
            None,
        );
        assert_eq!(no_matches.detail["matches"].as_array().unwrap().len(), 0);
        assert!(no_matches.model_text.contains("No matches found"));
    }

    #[test]
    fn us4_grep_cap_and_size_skips_are_signaled_not_silent() {
        // Cap signal: the model must be able to tell "capped at 100" apart
        // from "exactly 100 matches, complete".
        let dir = tempdir().unwrap();
        stdfs::write(dir.path().join("many.txt"), "needle here\n".repeat(150)).unwrap();
        let capped = execute(
            &call(
                "Grep",
                serde_json::json!({"pattern": "needle", "path": dir.path().to_str().unwrap()}),
            ),
            None,
        );
        assert_eq!(capped.detail["truncated"], true);
        assert_eq!(capped.detail["matches"].as_array().unwrap().len(), 100);
        assert!(
            capped.model_text.contains("capped at 100"),
            "model_text must carry the truncation signal, got: {:?}",
            capped.model_text.lines().last()
        );

        // Size-skip signal: an oversized file whose content was never
        // searched must not read as a bare "No matches found".
        let dir = tempdir().unwrap();
        stdfs::write(
            dir.path().join("huge.txt"),
            format!("needle\n{}", "a".repeat(10 * 1024 * 1024)),
        )
        .unwrap();
        let skipped = execute(
            &call(
                "Grep",
                serde_json::json!({"pattern": "needle", "path": dir.path().to_str().unwrap()}),
            ),
            None,
        );
        assert_eq!(skipped.detail["matches"].as_array().unwrap().len(), 0);
        assert_eq!(skipped.detail["skippedOversized"], 1);
        assert!(skipped.model_text.contains("No matches found"));
        assert!(
            skipped.model_text.contains("skipped"),
            "model_text must disclose the unsearched oversized file, got: {:?}",
            skipped.model_text
        );
    }

    #[test]
    fn zero_match_grep_with_unescaped_metachars_hints_at_escaping() {
        // Observed for real: the model verified its work by grepping for
        // "compute a + b" — `+` quantifies the space, matches nothing —
        // and trusted the empty result, reporting false success (0/20).
        let dir = tempdir().unwrap();
        stdfs::write(dir.path().join("f.txt"), "compute a + b now\n").unwrap();

        let result = execute(
            &call(
                "Grep",
                serde_json::json!({"pattern": "compute a + b", "path": dir.path().to_str().unwrap()}),
            ),
            None,
        );
        assert!(result.model_text.contains("No matches found"));
        assert!(
            result.model_text.contains("\\+"),
            "must show the escaped form, got: {:?}",
            result.model_text
        );
    }

    // --- 007-workspace-cwd-resolution ---

    #[test]
    fn us1_bash_ls_reflects_the_given_cwd() {
        let dir = tempdir().unwrap();
        stdfs::write(dir.path().join("marker.txt"), "").unwrap();
        let canonical = stdfs::canonicalize(dir.path()).unwrap();

        let result = execute(
            &call("Bash", serde_json::json!({"command": "ls ."})),
            Some(&canonical),
        );
        assert!(result.model_text.contains("marker.txt"));
    }

    #[test]
    fn us2_relative_write_lands_inside_the_given_cwd() {
        let dir = tempdir().unwrap();
        let canonical = stdfs::canonicalize(dir.path()).unwrap();

        let result = execute(
            &call(
                "Write",
                serde_json::json!({"file_path": "notes.md", "content": "hello"}),
            ),
            Some(&canonical),
        );
        assert!(result.model_text.contains("successfully"));
        assert_eq!(
            stdfs::read_to_string(canonical.join("notes.md")).unwrap(),
            "hello"
        );
    }

    #[test]
    fn us2_absolute_path_is_unaffected_by_cwd() {
        // FR-004 regression guard — an absolute file_path must be used
        // exactly as given, even when a cwd is known.
        let dir = tempdir().unwrap();
        let unrelated_dir = tempdir().unwrap();
        let file = unrelated_dir.path().join("f.txt");

        let result = execute(
            &call(
                "Write",
                serde_json::json!({"file_path": file.to_str().unwrap(), "content": "hi"}),
            ),
            Some(dir.path()),
        );
        assert!(result.model_text.contains("successfully"));
        assert_eq!(stdfs::read_to_string(&file).unwrap(), "hi");
    }

    #[test]
    fn us3_glob_with_no_path_defaults_to_the_given_cwd() {
        let dir = tempdir().unwrap();
        stdfs::write(dir.path().join("a.rs"), "").unwrap();

        let result = execute(
            &call("Glob", serde_json::json!({"pattern": "*.rs"})),
            Some(dir.path()),
        );
        assert!(result.model_text.contains("a.rs"));
    }

    // --- resolve_against() direct unit tests ---
    //
    // The "no cwd" / "None" regression guard (FR-005) is verified here,
    // directly against the pure resolution function, rather than by
    // mutating the test process's real working directory end-to-end
    // (std::env::set_current_dir is process-global and would race against
    // every other test running concurrently in the same `cargo test`
    // process — a real bug, not just a style preference, caught while
    // writing these tests).

    #[test]
    fn resolve_against_joins_a_relative_path_onto_a_known_cwd() {
        let cwd = PathBuf::from("/Users/alex/code/widget-app");
        let resolved = resolve_against(Some(&cwd), &PathBuf::from("notes.md"));
        assert_eq!(resolved, cwd.join("notes.md"));
    }

    #[test]
    fn resolve_against_leaves_an_absolute_path_unchanged_even_with_a_known_cwd() {
        let cwd = PathBuf::from("/Users/alex/code/widget-app");
        let absolute = PathBuf::from("/tmp/scratch.md");
        let resolved = resolve_against(Some(&cwd), &absolute);
        assert_eq!(resolved, absolute);
    }

    #[test]
    fn resolve_against_leaves_a_relative_path_unchanged_with_no_known_cwd() {
        // FR-005: this is exactly today's existing behavior — a relative
        // path with no cwd known resolves against the process's own
        // ambient directory, whatever it is, unchanged by this feature.
        let relative = PathBuf::from("notes.md");
        let resolved = resolve_against(None, &relative);
        assert_eq!(resolved, relative);
    }

    #[test]
    fn resolve_against_leaves_an_absolute_path_unchanged_with_no_known_cwd() {
        let absolute = PathBuf::from("/tmp/scratch.md");
        let resolved = resolve_against(None, &absolute);
        assert_eq!(resolved, absolute);
    }

    #[test]
    fn resolve_optional_base_defaults_to_the_known_cwd_when_no_path_is_given() {
        let cwd = PathBuf::from("/Users/alex/code/widget-app");
        assert_eq!(resolve_optional_base(Some(&cwd), None), cwd);
    }

    #[test]
    fn resolve_optional_base_defaults_to_dot_with_no_path_and_no_known_cwd() {
        // FR-005 regression guard — today's exact existing default.
        assert_eq!(resolve_optional_base(None, None), PathBuf::from("."));
    }

    #[test]
    fn resolve_optional_base_resolves_an_explicit_relative_path_against_cwd() {
        let cwd = PathBuf::from("/Users/alex/code/widget-app");
        assert_eq!(
            resolve_optional_base(Some(&cwd), Some("src")),
            cwd.join("src")
        );
    }

    #[test]
    fn resolve_optional_base_leaves_an_explicit_absolute_path_unchanged() {
        let cwd = PathBuf::from("/Users/alex/code/widget-app");
        assert_eq!(
            resolve_optional_base(Some(&cwd), Some("/tmp/elsewhere")),
            PathBuf::from("/tmp/elsewhere")
        );
    }
}
