//! Payload-file staging (2026-07-09 payload-files design): every
//! data-tool result is written to
//! `<app_data_dir>/tool-outputs/<conversation_id>/<tool_call_id>.txt` —
//! always, inline or not — and the token threshold only decides whether
//! `model_text` is the full result or a status reference line. Successor
//! to `offload_if_oversized`, which wrote a file only when oversized and
//! left the bulk duplicated in `detail`.

use std::path::Path;

/// How much of an oversized result survives inline when the payload file
/// could not be written (the bounded failure fallback).
const PREVIEW_CHARS: usize = 500;

/// Widget-facing preview length for slimmed `detail` fields.
const DETAIL_PREVIEW_CHARS: usize = 2000;

pub struct StagedResult {
    pub model_text: String,
    pub payload_ref: Option<String>,
    pub detail: serde_json::Value,
}

// `stage_tool_result` below sources the payload it writes from
// `ToolOutcome::offload_text()` (agent/dispatch.rs) rather than duplicating
// that reconstruction here: for Bash, `model_text` is already tail-biased
// capped by the time it reaches this module, and only `detail.outcome`
// still carries every byte, so `offload_text()` is the one place that
// rebuilds the full rendition from `detail`. Every other tool's
// `offload_text()` is just a borrow of `model_text`, since none of them
// cap it.

/// The status line an over-threshold result is replaced with: cheap
/// metadata that answers "did it work / how big" without a Read round-trip.
fn reference_line(detail: &serde_json::Value, payload_bytes: usize, path: &str) -> String {
    let tool = detail["toolName"].as_str().unwrap_or("Tool");
    let stats = match tool {
        "Bash" => {
            let exit = &detail["outcome"]["exitCode"];
            let stdout_b = detail["outcome"]["stdoutBytes"].as_u64().unwrap_or(0);
            let stderr_b = detail["outcome"]["stderrBytes"].as_u64().unwrap_or(0);
            format!("exit {exit} — {stdout_b} bytes stdout, {stderr_b} bytes stderr")
        }
        "Grep" | "Glob" => {
            let n = detail["matches"].as_array().map(|a| a.len()).unwrap_or(0);
            format!("{n} matches")
        }
        _ => format!("{payload_bytes} bytes of output"),
    };
    format!("{tool}: {stats} → Read \"{path}\" to view")
}

/// Replaces bulk text fields in `detail` with bounded previews + byte
/// counts (the spec's "detail becomes pure metadata"). Only Bash carries
/// bulk in `detail` today; other tools' detail passes through unchanged.
fn slim_detail(mut detail: serde_json::Value) -> serde_json::Value {
    if detail["toolName"] == "Bash" {
        if let Some(outcome) = detail["outcome"].as_object_mut() {
            for (bulk, preview_key, bytes_key) in [
                ("stdout", "stdoutPreview", "stdoutBytes"),
                ("stderr", "stderrPreview", "stderrBytes"),
            ] {
                if let Some(text) = outcome
                    .remove(bulk)
                    .and_then(|v| v.as_str().map(String::from))
                {
                    outcome.insert(bytes_key.to_string(), serde_json::json!(text.len()));
                    outcome.insert(
                        preview_key.to_string(),
                        serde_json::json!(text
                            .chars()
                            .take(DETAIL_PREVIEW_CHARS)
                            .collect::<String>()),
                    );
                }
            }
        }
    }
    detail
}

pub fn stage_tool_result(
    app_data_dir: &Path,
    conversation_id: &str,
    tool_call_id: &str,
    outcome: &crate::agent::dispatch::ToolOutcome,
    threshold_tokens: usize,
    count_tokens: impl Fn(&str) -> usize,
) -> StagedResult {
    let payload = outcome.offload_text().into_owned();
    let model_text = &outcome.model_text;
    let detail = slim_detail(outcome.detail.clone());

    let dir = app_data_dir.join("tool-outputs").join(conversation_id);
    let write_result = std::fs::create_dir_all(&dir).and_then(|()| {
        let path = dir.join(format!("{tool_call_id}.txt"));
        std::fs::write(&path, &payload).map(|()| path)
    });

    match write_result {
        Ok(path) => {
            let path_string = path.to_string_lossy().to_string();
            let model_text = if count_tokens(model_text) <= threshold_tokens {
                model_text.to_string()
            } else {
                reference_line(&detail, payload.len(), &path_string)
            };
            StagedResult {
                model_text,
                payload_ref: Some(path_string),
                detail,
            }
        }
        Err(e) => {
            // Invariant: unbounded text never enters the window, even here.
            let model_text = if count_tokens(model_text) <= threshold_tokens {
                model_text.to_string()
            } else {
                let preview: String = model_text.chars().take(PREVIEW_CHARS).collect();
                format!("{preview}…\n[full output could not be saved: {e}]")
            };
            StagedResult {
                model_text,
                payload_ref: None,
                detail,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// chars/4 — deterministic stand-in for the real tokenizer.
    fn fake_count(text: &str) -> usize {
        text.chars().count().div_ceil(4)
    }

    fn bash_detail(stdout: &str, stderr: &str) -> serde_json::Value {
        json!({
            "toolName": "Bash", "command": "x", "timeoutMs": null,
            "outcome": {"ok": true, "exitCode": 0, "stdout": stdout, "stderr": stderr},
        })
    }

    fn outcome(model_text: &str, detail: serde_json::Value) -> crate::agent::dispatch::ToolOutcome {
        crate::agent::dispatch::ToolOutcome {
            model_text: model_text.to_string(),
            detail,
        }
    }

    #[test]
    fn small_result_inlines_but_still_writes_the_payload_file() {
        let dir = tempfile::tempdir().unwrap();
        let staged = stage_tool_result(
            dir.path(),
            "conv1",
            "call1",
            &outcome(
                "short output",
                json!({"toolName": "Grep", "matches": [], "outcome": {"ok": true}}),
            ),
            512,
            fake_count,
        );
        assert_eq!(staged.model_text, "short output");
        let path = staged.payload_ref.expect("payload file must always exist");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "short output");
        assert!(path.contains("conv1") && path.contains("call1"));
    }

    #[test]
    fn oversized_result_becomes_a_status_reference_line() {
        let dir = tempfile::tempdir().unwrap();
        let big = "line of output\n".repeat(500); // ~1875 fake tokens
        let staged = stage_tool_result(
            dir.path(),
            "conv1",
            "call2",
            &outcome(
                &big,
                json!({"toolName": "Grep", "matches": ["a", "b"], "outcome": {"ok": true}}),
            ),
            512,
            fake_count,
        );
        let path = staged.payload_ref.clone().unwrap();
        assert!(staged.model_text.starts_with("Grep: 2 matches"));
        assert!(staged.model_text.contains(&path));
        assert!(staged.model_text.contains("Read"));
        assert!(
            !staged.model_text.contains("line of output"),
            "no content leaks into a reference line"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), big);
    }

    #[test]
    fn bash_payload_is_full_stdout_and_stderr_from_detail_and_detail_is_slimmed() {
        let dir = tempfile::tempdir().unwrap();
        let stdout = "s".repeat(10_000);
        let stderr = "e".repeat(3_000);
        let staged = stage_tool_result(
            dir.path(),
            "conv1",
            "call3",
            &outcome(
                "tail-biased preview the model would have seen",
                bash_detail(&stdout, &stderr),
            ),
            512,
            fake_count,
        );
        // offload_text()'s full rendition, not the tail-biased preview.
        let written = std::fs::read_to_string(staged.payload_ref.as_ref().unwrap()).unwrap();
        assert!(written.contains(&stdout) && written.contains(&stderr));
        // Slimmed detail: previews + byte counts, bulk gone.
        let out = &staged.detail["outcome"];
        assert!(out.get("stdout").is_none() && out.get("stderr").is_none());
        assert_eq!(out["stdoutBytes"], 10_000);
        assert_eq!(out["stderrBytes"], 3_000);
        assert_eq!(out["stdoutPreview"].as_str().unwrap().chars().count(), 2000);
        // Small model_text -> inlined even though the payload is big.
        assert_eq!(
            staged.model_text,
            "tail-biased preview the model would have seen"
        );
    }

    #[test]
    fn oversized_bash_reference_line_carries_exit_code_and_sizes() {
        let dir = tempfile::tempdir().unwrap();
        let big_preview = "x".repeat(4_000); // ~1000 fake tokens > 512
        let staged = stage_tool_result(
            dir.path(),
            "conv1",
            "call4",
            &outcome(&big_preview, bash_detail(&"s".repeat(10_000), "")),
            512,
            fake_count,
        );
        assert!(staged
            .model_text
            .starts_with("Bash: exit 0 — 10000 bytes stdout, 0 bytes stderr"));
        assert!(staged.model_text.contains("Read"));
    }

    #[test]
    fn write_failure_falls_back_to_a_bounded_preview_with_no_payload_ref() {
        // A file path in place of a directory forces create_dir_all to fail.
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("tool-outputs");
        std::fs::write(&blocker, "not a dir").unwrap();
        let big = "y".repeat(10_000);
        let staged = stage_tool_result(
            dir.path(),
            "conv1",
            "call5",
            &outcome(
                &big,
                json!({"toolName": "Grep", "matches": [], "outcome": {"ok": true}}),
            ),
            512,
            fake_count,
        );
        assert!(staged.payload_ref.is_none());
        assert!(staged.model_text.contains("could not be saved"));
        assert!(
            staged.model_text.chars().count() < 700,
            "fallback must stay bounded"
        );
    }
}
