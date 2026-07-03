use crate::agent::tools::{bash, fs, search};
use crate::agent::ToolCall;
use std::path::PathBuf;

/// Executes a parsed `ToolCall` against the real built-in tools (FR-009),
/// returning a plain-text result to feed back into the loop's transcript.
/// Never restricted to any workspace folder (FR-009's explicit
/// requirement) — paths are taken as given, absolute or relative to the
/// process's own cwd.
pub fn execute(call: &ToolCall) -> String {
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
            match fs::read(&PathBuf::from(path), offset, limit) {
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
            match fs::write(&PathBuf::from(path), content) {
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
            match fs::edit(&PathBuf::from(path), old_string, new_string, replace_all) {
                Ok(()) => "Edit applied successfully".to_string(),
                Err(e) => format!("Error: {e}"),
            }
        }
        "Bash" => {
            let Some(command) = call.arguments.get("command").and_then(|v| v.as_str()) else {
                return "Error: Bash requires a command argument".to_string();
            };
            let timeout_ms = call.arguments.get("timeout").and_then(|v| v.as_u64());
            match bash::run(command, timeout_ms) {
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
            let base = call
                .arguments
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or(".");
            match search::glob_search(pattern, &PathBuf::from(base)) {
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
            let base = call
                .arguments
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or(".");
            let glob_filter = call.arguments.get("glob").and_then(|v| v.as_str());
            match search::grep(pattern, &PathBuf::from(base), glob_filter) {
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

        let result = execute(&call(
            "Read",
            serde_json::json!({"file_path": file.to_str().unwrap()}),
        ));
        assert!(result.contains("hello"));
    }

    #[test]
    fn dispatches_write_then_read() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("f.txt");

        let write_result = execute(&call(
            "Write",
            serde_json::json!({"file_path": file.to_str().unwrap(), "content": "new content"}),
        ));
        assert!(write_result.contains("successfully"));
        assert_eq!(stdfs::read_to_string(&file).unwrap(), "new content");
    }

    #[test]
    fn dispatches_bash_and_captures_denylist_rejection() {
        let result = execute(&call("Bash", serde_json::json!({"command": "rm -rf ~"})));
        assert!(result.contains("Error"));
        assert!(result.contains("catastrophic"));
    }

    #[test]
    fn unknown_tool_returns_a_clear_error_not_a_panic() {
        let result = execute(&call("NotARealTool", serde_json::json!({})));
        assert!(result.contains("unknown tool"));
    }

    #[test]
    fn missing_required_argument_returns_a_clear_error() {
        let result = execute(&call("Read", serde_json::json!({})));
        assert!(result.contains("Error"));
        assert!(result.contains("file_path"));
    }
}
