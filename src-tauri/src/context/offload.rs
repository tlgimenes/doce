//! Tool-output offloading (010-context-window-management, User Story 3):
//! oversized agent-mode tool results are written to disk with only a short
//! preview + pointer kept in the model-facing conversation, mirroring the
//! research brief's "truncate + pointer, retrievable via Read" pattern —
//! the assistant re-reads the rest later via the existing, unmodified
//! `Read` tool, and this feature invents no new retrieval concept.

use std::path::{Path, PathBuf};

/// How much of an oversized result is kept inline as a preview before the
/// "[Use Read on ...]" pointer.
const PREVIEW_CHARS: usize = 500;

/// If `result` is at or under `threshold_chars`, returns it unchanged with
/// no file written. Otherwise writes the full `result` to
/// `<app_data_dir>/tool-outputs/<conversation_id>/<tool_call_id>.txt` and
/// returns a short preview + a pointer telling the model to `Read` that
/// path for the rest, plus `Some(path)` for the caller to record in the
/// persisted `tool_result` row's `detail.offloadedTo`. Takes the already-
/// resolved `app_data_dir` (rather than an `AppHandle`) so this stays a
/// plain, Tauri-independent function callers can unit-test without a live
/// app — the same reason `agent::dispatch::execute` takes `cwd: Option<&Path>`
/// rather than resolving it itself.
pub fn offload_if_oversized(
    app_data_dir: &Path,
    conversation_id: &str,
    tool_call_id: &str,
    result: &str,
    threshold_chars: usize,
) -> Result<(String, Option<String>), String> {
    if result.chars().count() <= threshold_chars {
        return Ok((result.to_string(), None));
    }

    let dir = app_data_dir.join("tool-outputs").join(conversation_id);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let path: PathBuf = dir.join(format!("{tool_call_id}.txt"));
    std::fs::write(&path, result).map_err(|e| e.to_string())?;

    let path_string = path.to_string_lossy().to_string();
    let preview: String = result.chars().take(PREVIEW_CHARS).collect();
    let total_chars = result.chars().count();
    // No "Tool result for {tool_name}" prefix here, and no <tool_response>
    // tags either -- ChatMessage::tool_result's own .text() rendering is
    // the one and only place that wraps a result for the model, applied
    // exactly once regardless of whether a result was offloaded. This
    // function previously included its own copy of a "Tool result for X:"
    // prefix, which agent::run_loop's own wrapping (at the time, the same
    // prefix) then duplicated on top of, producing a literal double
    // prefix in practice -- both have since been removed from here.
    let model_text = format!(
        "(truncated — {total_chars} chars total, full output saved): {preview}...\n[Use Read on \"{path_string}\" to view the rest]"
    );

    Ok((model_text, Some(path_string)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn under_threshold_passes_through_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let (text, path) =
            offload_if_oversized(dir.path(), "conv1", "call1", "short output", 2000)
                .unwrap();
        assert_eq!(text, "short output");
        assert!(path.is_none());
    }

    #[test]
    fn exactly_at_threshold_passes_through_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let result = "a".repeat(100);
        let (text, path) =
            offload_if_oversized(dir.path(), "conv1", "call1", &result, 100).unwrap();
        assert_eq!(text, result);
        assert!(path.is_none());
    }

    #[test]
    fn over_threshold_writes_the_full_content_and_returns_a_preview_and_pointer() {
        let dir = tempfile::tempdir().unwrap();
        let result = "x".repeat(3000);
        let (text, path) =
            offload_if_oversized(dir.path(), "conv1", "call1", &result, 2000).unwrap();

        let path = path.expect("expected a file path for an over-threshold result");
        assert!(text.contains("truncated"));
        assert!(text.contains(&"x".repeat(500)));
        assert!(!text.contains(&"x".repeat(501)), "preview must not exceed PREVIEW_CHARS");
        assert!(text.contains(&path));

        let written = std::fs::read_to_string(&path).unwrap();
        assert_eq!(written, result, "the file must contain the exact original result");
    }

    #[test]
    fn different_tool_calls_in_the_same_conversation_get_distinct_files() {
        let dir = tempfile::tempdir().unwrap();
        let result = "y".repeat(3000);
        let (_, path1) =
            offload_if_oversized(dir.path(), "conv1", "call1", &result, 2000).unwrap();
        let (_, path2) =
            offload_if_oversized(dir.path(), "conv1", "call2", &result, 2000).unwrap();
        assert_ne!(path1, path2);
    }
}
