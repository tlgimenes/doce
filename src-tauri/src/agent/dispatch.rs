use crate::agent::tools::{bash, fs, search};
use crate::agent::ToolCall;
use std::path::{Path, PathBuf};

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

/// Executes a parsed `ToolCall` against the real built-in tools (FR-009).
/// `cwd` is the conversation's workspace path, if it has one
/// (007-workspace-cwd-resolution) — used only to resolve *relative* paths
/// and to give `Bash` a sensible starting directory. Absolute paths are
/// never restricted to any workspace folder (FR-009's explicit
/// requirement, unchanged by this feature) — an absolute path is always
/// taken exactly as given.
pub fn execute(call: &ToolCall, cwd: Option<&Path>) -> String {
    match call.name.as_str() {
        "Read" => {
            let Some(path) = call.arguments.get("file_path").and_then(|v| v.as_str()) else {
                return "Error: Read requires a file_path argument".to_string();
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
                Ok(content) => content,
                Err(e) => format!("Error: {e}"),
            }
        }
        "Write" => {
            let (Some(path), Some(content)) = (
                call.arguments.get("file_path").and_then(|v| v.as_str()),
                call.arguments.get("content").and_then(|v| v.as_str()),
            ) else {
                return "Error: Write requires file_path and content arguments".to_string();
            };
            let resolved = resolve_against(cwd, &PathBuf::from(path));
            match fs::write(&resolved, content) {
                Ok(()) => "File written successfully".to_string(),
                Err(e) => format!("Error: {e}"),
            }
        }
        "Edit" => {
            let (Some(path), Some(old_string), Some(new_string)) = (
                call.arguments.get("file_path").and_then(|v| v.as_str()),
                call.arguments.get("old_string").and_then(|v| v.as_str()),
                call.arguments.get("new_string").and_then(|v| v.as_str()),
            ) else {
                return "Error: Edit requires file_path, old_string, and new_string arguments"
                    .to_string();
            };
            let replace_all = call
                .arguments
                .get("replace_all")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let resolved = resolve_against(cwd, &PathBuf::from(path));
            match fs::edit(&resolved, old_string, new_string, replace_all) {
                Ok(()) => "Edit applied successfully".to_string(),
                Err(e) => format!("Error: {e}"),
            }
        }
        "Bash" => {
            let Some(command) = call.arguments.get("command").and_then(|v| v.as_str()) else {
                return "Error: Bash requires a command argument".to_string();
            };
            let timeout_ms = call.arguments.get("timeout").and_then(|v| v.as_u64());
            match bash::run(command, timeout_ms, cwd) {
                Ok(result) => {
                    format!(
                        "exit_code: {}\nstdout:\n{}\nstderr:\n{}",
                        result.exit_code, result.stdout, result.stderr
                    )
                }
                Err(e) => format!("Error: {e}"),
            }
        }
        "Glob" => {
            let Some(pattern) = call.arguments.get("pattern").and_then(|v| v.as_str()) else {
                return "Error: Glob requires a pattern argument".to_string();
            };
            let base =
                resolve_optional_base(cwd, call.arguments.get("path").and_then(|v| v.as_str()));
            match search::glob_search(pattern, &base) {
                Ok(paths) => {
                    if paths.is_empty() {
                        "No files matched".to_string()
                    } else {
                        paths
                            .iter()
                            .map(|p| p.display().to_string())
                            .collect::<Vec<_>>()
                            .join("\n")
                    }
                }
                Err(e) => format!("Error: {e}"),
            }
        }
        "Grep" => {
            let Some(pattern) = call.arguments.get("pattern").and_then(|v| v.as_str()) else {
                return "Error: Grep requires a pattern argument".to_string();
            };
            let base =
                resolve_optional_base(cwd, call.arguments.get("path").and_then(|v| v.as_str()));
            let glob_filter = call.arguments.get("glob").and_then(|v| v.as_str());
            match search::grep(pattern, &base, glob_filter) {
                Ok(matches) => {
                    if matches.is_empty() {
                        "No matches found".to_string()
                    } else {
                        matches
                            .iter()
                            .map(|m| format!("{}:{}:{}", m.path.display(), m.line_number, m.line))
                            .collect::<Vec<_>>()
                            .join("\n")
                    }
                }
                Err(e) => format!("Error: {e}"),
            }
        }
        other => format!("Error: unknown tool '{other}'"),
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
        assert!(result.contains("hello"));
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
        assert!(write_result.contains("successfully"));
        assert_eq!(stdfs::read_to_string(&file).unwrap(), "new content");
    }

    #[test]
    fn dispatches_bash_and_captures_denylist_rejection() {
        let result = execute(
            &call("Bash", serde_json::json!({"command": "rm -rf ~"})),
            None,
        );
        assert!(result.contains("Error"));
        assert!(result.contains("catastrophic"));
    }

    #[test]
    fn unknown_tool_returns_a_clear_error_not_a_panic() {
        let result = execute(&call("NotARealTool", serde_json::json!({})), None);
        assert!(result.contains("unknown tool"));
    }

    #[test]
    fn missing_required_argument_returns_a_clear_error() {
        let result = execute(&call("Read", serde_json::json!({})), None);
        assert!(result.contains("Error"));
        assert!(result.contains("file_path"));
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
        assert!(result.contains("marker.txt"));
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
        assert!(result.contains("successfully"));
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
        assert!(result.contains("successfully"));
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
        assert!(result.contains("a.rs"));
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
