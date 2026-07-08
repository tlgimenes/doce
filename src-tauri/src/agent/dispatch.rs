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
fn wrong_key_hint(arguments: &serde_json::Value, expected: &str, common_mistakes: &[&str]) -> String {
    common_mistakes
        .iter()
        .find(|candidate| arguments.get(**candidate).is_some())
        .map(|candidate| format!(" (you passed \"{candidate}\", the correct key is \"{expected}\")"))
        .unwrap_or_default()
}

/// Executes a parsed `ToolCall` against the real built-in tools (FR-009).
/// `cwd` is the conversation's workspace path, if it has one
/// (007-workspace-cwd-resolution) — used only to resolve *relative* paths
/// and to give `Bash` a sensible starting directory. Absolute paths are
/// never restricted to any workspace folder (FR-009's explicit
/// requirement, unchanged by this feature) — an absolute path is always
/// taken exactly as given.
pub fn execute(call: &ToolCall, cwd: Option<&Path>) -> ToolOutcome {
    match call.name.as_str() {
        "Read" => {
            let Some(path) = call.arguments.get("file_path").and_then(|v| v.as_str()) else {
                let hint = wrong_key_hint(&call.arguments, "file_path", &["file", "path", "filepath", "filename"]);
                return ToolOutcome {
                    model_text: format!("Error: Read requires a file_path argument{hint}"),
                    detail: json!({"toolName": "Read", "filePath": null, "outcome": {"ok": false, "error": format!("missing file_path argument{hint}")}}),
                };
            };
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
            let (Some(path), Some(content)) = (
                call.arguments.get("file_path").and_then(|v| v.as_str()),
                call.arguments.get("content").and_then(|v| v.as_str()),
            ) else {
                let hint = wrong_key_hint(&call.arguments, "file_path", &["file", "path", "filepath", "filename"]);
                return ToolOutcome {
                    model_text: format!("Error: Write requires file_path and content arguments{hint}"),
                    detail: json!({"toolName": "Write", "filePath": null, "outcome": {"ok": false, "error": format!("missing file_path or content argument{hint}")}}),
                };
            };
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
            let (Some(path), Some(old_string), Some(new_string)) = (
                call.arguments.get("file_path").and_then(|v| v.as_str()),
                call.arguments.get("old_string").and_then(|v| v.as_str()),
                call.arguments.get("new_string").and_then(|v| v.as_str()),
            ) else {
                let hint = wrong_key_hint(&call.arguments, "file_path", &["file", "path", "filepath", "filename"]);
                return ToolOutcome {
                    model_text: format!(
                        "Error: Edit requires file_path, old_string, and new_string arguments{hint}"
                    ),
                    detail: json!({"toolName": "Edit", "filePath": null, "outcome": {"ok": false, "error": format!("missing required argument{hint}")}}),
                };
            };
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
            let Some(command) = call.arguments.get("command").and_then(|v| v.as_str()) else {
                return ToolOutcome {
                    model_text: "Error: Bash requires a command argument".to_string(),
                    detail: json!({"toolName": "Bash", "command": null, "outcome": {"ok": false, "error": "missing command argument"}}),
                };
            };
            let timeout_ms = call.arguments.get("timeout").and_then(|v| v.as_u64());
            match bash::run(command, timeout_ms, cwd) {
                Ok(result) => ToolOutcome {
                    model_text: format!(
                        "exit_code: {}\nstdout:\n{}\nstderr:\n{}",
                        result.exit_code, result.stdout, result.stderr
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
            let Some(pattern) = call.arguments.get("pattern").and_then(|v| v.as_str()) else {
                return ToolOutcome {
                    model_text: "Error: Glob requires a pattern argument".to_string(),
                    detail: json!({"toolName": "Glob", "pattern": null, "path": null, "matches": []}),
                };
            };
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
            let Some(pattern) = call.arguments.get("pattern").and_then(|v| v.as_str()) else {
                return ToolOutcome {
                    model_text: "Error: Grep requires a pattern argument".to_string(),
                    detail: json!({"toolName": "Grep", "pattern": null, "path": null, "glob": null, "matches": []}),
                };
            };
            let base =
                resolve_optional_base(cwd, call.arguments.get("path").and_then(|v| v.as_str()));
            let glob_filter = call.arguments.get("glob").and_then(|v| v.as_str());
            match search::grep(pattern, &base, glob_filter) {
                Ok(matches) => {
                    let match_values: Vec<serde_json::Value> = matches
                        .iter()
                        .map(|m| {
                            json!({
                                "path": m.path.display().to_string(),
                                "lineNumber": m.line_number,
                                "line": m.line,
                            })
                        })
                        .collect();
                    ToolOutcome {
                        model_text: if matches.is_empty() {
                            "No matches found".to_string()
                        } else {
                            matches
                                .iter()
                                .map(|m| {
                                    format!("{}:{}:{}", m.path.display(), m.line_number, m.line)
                                })
                                .collect::<Vec<_>>()
                                .join("\n")
                        },
                        detail: json!({
                            "toolName": "Grep", "pattern": pattern,
                            "path": base.display().to_string(), "glob": glob_filter,
                            "matches": match_values,
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

    #[test]
    fn unknown_tool_returns_a_clear_error_not_a_panic() {
        let result = execute(&call("NotARealTool", serde_json::json!({})), None);
        assert!(result.model_text.contains("unknown tool"));
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
        assert_eq!(no_matches.model_text, "No matches found");
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
