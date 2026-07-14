//! Pure data-mapping layer for the llama-server (OpenAI-compatible)
//! `/v1/chat/completions` API — no networking here, just the shape
//! translation between doce's internal `ChatMessage`/`ToolCallMode` and the
//! JSON a `reqwest` client (a later task) will POST. Kept pure and
//! model-free so every mapping decision (role mapping, tool-call
//! arguments-as-string, sampling defaults) is unit-testable without a
//! running server.

use super::{ChatMessage, MessageContent, ToolCallMode};
use serde::Serialize;
use serde_json::Value;

/// Maps doce's internal transcript to the `messages` array
/// `/v1/chat/completions` expects. `Text` turns map straight through on
/// their stored role; `ToolUse`/`ToolResult` are re-keyed to OpenAI's own
/// tool-calling shape regardless of the in-memory role `ChatMessage`'s
/// constructors stamped on them (`tool_result` stores role `user` — see its
/// doc comment — but always maps to OpenAI role `tool` here).
pub fn to_openai_messages(msgs: &[ChatMessage]) -> Vec<Value> {
    msgs.iter().map(to_openai_message).collect()
}

fn to_openai_message(msg: &ChatMessage) -> Value {
    match &msg.content {
        MessageContent::Text(text) => serde_json::json!({
            "role": msg.role,
            "content": text,
        }),
        MessageContent::ToolUse { id, name, input } => serde_json::json!({
            "role": "assistant",
            "tool_calls": [{
                "id": id,
                "type": "function",
                "function": {
                    "name": name,
                    // OpenAI's `arguments` is a JSON-encoded STRING, not a
                    // nested object — `.to_string()` of the Value, not the
                    // Value itself.
                    "arguments": input.to_string(),
                }
            }],
        }),
        MessageContent::ToolResult {
            tool_use_id,
            content,
            ..
        } => serde_json::json!({
            "role": "tool",
            "tool_call_id": tool_use_id,
            "content": content,
        }),
    }
}

/// Maps doce's grammar-gating mode to OpenAI's `tool_choice` request field.
/// `Forbid` omits `tool_choice` entirely (`None`) rather than sending
/// `"none"` — task 4's caller is expected to also omit `tools` in that case,
/// so there is nothing for `tool_choice` to select among.
pub fn tool_choice_for(mode: ToolCallMode) -> Option<&'static str> {
    match mode {
        ToolCallMode::Require => Some("required"),
        ToolCallMode::Allow => Some("auto"),
        ToolCallMode::Forbid => None,
    }
}

/// Builds the OpenAI `tools` array for the given tool names, from
/// structured `serde_json::json!` schemas (never parsed from the prompt
/// text at runtime) — the authority for Read/Update/Bash/Grep/Glob's
/// argument names and required-ness is `agent::dispatch`'s
/// `REQUIRED_STRING_ARGS`/`LEGAL_TOOL_ARGS`; Todo/Task/AskUserQuestion/
/// FinishTask mirror the schemas embedded in `agent::plan`'s tool lines.
/// Unknown names are skipped, not a panic — a future tool-set drift should
/// degrade gracefully here, the same way `dispatch::execute` already
/// tolerates unrecognized names.
pub fn tools_array(names: &[&str]) -> Vec<Value> {
    names.iter().filter_map(|name| tool_def(name)).collect()
}

fn tool_def(name: &str) -> Option<Value> {
    let (description, parameters): (&str, Value) = match name {
        "Read" => (
            "Read a file from disk.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "file_path": {"type": "string"},
                    "offset": {"type": "number"},
                    "limit": {"type": "number"}
                },
                "required": ["file_path"]
            }),
        ),
        "Update" => (
            "Create or modify a file. Pass content to create or fully overwrite the file. Pass old_string and new_string (and no content) to replace one exact occurrence in place.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "file_path": {"type": "string"},
                    "content": {"type": "string"},
                    "old_string": {"type": "string"},
                    "new_string": {"type": "string"},
                    "replace_all": {"type": "boolean"}
                },
                "required": ["file_path"]
            }),
        ),
        "Bash" => (
            "Run a shell command.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {"type": "string"},
                    "timeout": {"type": "number"}
                },
                "required": ["command"]
            }),
        ),
        "Grep" => (
            "Search file contents with a regular expression. Omit path to search the current working directory. Results are capped at 100 matches -- for counting or exhaustive listings use a Bash pipeline instead.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": {"type": "string"},
                    "path": {"type": "string"},
                    "glob": {"type": "string"}
                },
                "required": ["pattern"]
            }),
        ),
        "Glob" => (
            "Find files by name pattern. The pattern is a single wildcard expression, e.g. \"bug_*.txt\" or \"*.rs\" -- never a space-separated list of literal filenames, that matches nothing. Omit path to search the current working directory. Results are capped at the 100 most recently modified matches -- for counting or exhaustive listings use a Bash pipeline instead.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": {"type": "string"},
                    "path": {"type": "string"}
                },
                "required": ["pattern"]
            }),
        ),
        "Todo" => (
            "Replace your todo list. Keep one for any multi-step task: one item per file or unit of work, done: true as you finish each. Calling this only records progress -- keep working afterwards.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "items": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "text": {"type": "string"},
                                "done": {"type": "boolean"}
                            },
                            "required": ["text", "done"]
                        }
                    }
                },
                "required": ["items"]
            }),
        ),
        "Task" => (
            "Delegate substantial, self-contained work (extensive exploration, a large search, a bulky sub-investigation) to an isolated subagent instead of doing it inline. This conversation is shared across the WHOLE task, not just this step -- everything you do here stays visible to every later step too, so keep it lean: reach for Task when a piece of work would otherwise flood this shared history with exploration detail nobody later needs, and only the outcome actually matters going forward.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "prompt": {"type": "string"}
                },
                "required": ["prompt"]
            }),
        ),
        "AskUserQuestion" => (
            "Ask the user directly if the request is genuinely ambiguous.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "header": {"type": "string"},
                    "question": {"type": "string"},
                    "options": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "label": {"type": "string"},
                                "description": {"type": "string"}
                            },
                            "required": ["label"]
                        }
                    },
                    "multiSelect": {"type": "boolean"}
                },
                "required": ["header", "question", "options"]
            }),
        ),
        "FinishTask" => (
            "End the task and deliver your final answer to the user. Only call this after you have verified the outcome yourself.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "answer": {"type": "string"}
                },
                "required": ["answer"]
            }),
        ),
        _ => return None,
    };
    Some(serde_json::json!({
        "type": "function",
        "function": {
            "name": name,
            "description": description,
            "parameters": parameters,
        }
    }))
}

/// The per-request body a later HTTP client POSTs to llama-server's
/// `/v1/chat/completions`. `ChatRequest::build` is the ONLY constructor —
/// it fills every sampling/behavior default from the Global-Constraint
/// design (stream, cache_prompt, parallel_tool_calls, enable_thinking) and
/// the coding sampling preset, so no caller re-derives those values by
/// hand.
#[derive(Debug, Clone, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<String>,
    pub parallel_tool_calls: bool,
    pub stream: bool,
    pub cache_prompt: bool,
    pub chat_template_kwargs: Value,
    pub temperature: f32,
    pub top_p: f32,
    pub top_k: u32,
    pub min_p: f32,
    pub presence_penalty: f32,
    /// Only meaningful (and only sent) alongside `stream: true` — llama-server
    /// echoes token-usage in the final SSE event when this is set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_options: Option<Value>,
}

impl ChatRequest {
    /// Builds a request with the Global-Constraint defaults: streaming on,
    /// prompt-cache reuse on, parallel tool calls off (the model's tool-call
    /// grammar only ever emits one call at a time), thinking enabled, and
    /// the coding sampling preset (`temperature=0.6, top_p=0.95, top_k=20,
    /// min_p=0.0, presence_penalty=0.0`). `tools`/`tool_choice` are passed
    /// straight through — `None` for both omits them from the serialized
    /// JSON entirely (matching `ToolCallMode::Forbid`'s mapping).
    pub fn build(
        model: impl Into<String>,
        messages: Vec<Value>,
        tools: Option<Vec<Value>>,
        tool_choice: Option<String>,
    ) -> Self {
        let stream = true;
        Self {
            model: model.into(),
            messages,
            tools,
            tool_choice,
            parallel_tool_calls: false,
            stream,
            cache_prompt: true,
            chat_template_kwargs: serde_json::json!({"enable_thinking": true}),
            temperature: 0.6,
            top_p: 0.95,
            top_k: 20,
            min_p: 0.0,
            presence_penalty: 0.0,
            stream_options: stream.then(|| serde_json::json!({"include_usage": true})),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_roles_and_tool_messages() {
        let msgs = vec![
            ChatMessage::system("sys"),
            ChatMessage::user("hi"),
            ChatMessage::tool_use("call_1", "Read", serde_json::json!({"file_path":"/x"})),
            ChatMessage::tool_result("call_1", "Read", "contents"),
        ];
        let out = to_openai_messages(&msgs);
        assert_eq!(out[0]["role"], "system");
        assert_eq!(out[1]["role"], "user");
        assert_eq!(out[2]["role"], "assistant");
        assert_eq!(out[2]["tool_calls"][0]["function"]["name"], "Read");
        assert_eq!(out[3]["role"], "tool");
        assert_eq!(out[3]["tool_call_id"], "call_1");
    }

    #[test]
    fn tool_choice_maps_modes() {
        assert_eq!(tool_choice_for(ToolCallMode::Require), Some("required"));
        assert_eq!(tool_choice_for(ToolCallMode::Allow), Some("auto"));
        assert_eq!(tool_choice_for(ToolCallMode::Forbid), None);
    }

    #[test]
    fn tools_array_emits_valid_openai_function() {
        let t = tools_array(&["Read"]);
        assert_eq!(t[0]["type"], "function");
        assert_eq!(t[0]["function"]["name"], "Read");
        assert!(t[0]["function"]["parameters"]["properties"]["file_path"].is_object());
    }

    #[test]
    fn tools_array_skips_unknown_names() {
        let t = tools_array(&["NotARealTool"]);
        assert!(t.is_empty());
    }

    #[test]
    fn tools_array_covers_all_nine_single_mode_tools() {
        let names = [
            "Read",
            "Update",
            "Bash",
            "Grep",
            "Glob",
            "Todo",
            "Task",
            "AskUserQuestion",
            "FinishTask",
        ];
        let t = tools_array(&names);
        assert_eq!(t.len(), names.len());
        for (def, name) in t.iter().zip(names.iter()) {
            assert_eq!(def["function"]["name"], *name);
        }
    }

    #[test]
    fn chat_request_serializes_with_sampling_defaults() {
        let req = ChatRequest::build("qwen", vec![], None, None);
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["parallel_tool_calls"], false);
        assert_eq!(v["stream"], true);
        assert_eq!(v["cache_prompt"], true);
        assert_eq!(v["chat_template_kwargs"]["enable_thinking"], true);
        // Compared as f32 (round-tripped through the JSON f64 the field's
        // f32 value widens to) rather than against an f64 literal directly
        // — 0.6f32 widened to f64 is 0.600000023841858, not bit-identical
        // to the f64 literal `0.6`, even though both are "0.6" at f32
        // precision.
        assert_eq!(v["temperature"].as_f64().unwrap() as f32, 0.6_f32);
        assert_eq!(v["top_p"].as_f64().unwrap() as f32, 0.95_f32);
        assert_eq!(v["top_k"], 20);
        assert_eq!(v["min_p"].as_f64().unwrap() as f32, 0.0_f32);
        assert_eq!(v["presence_penalty"].as_f64().unwrap() as f32, 0.0_f32);
        assert_eq!(v["stream_options"]["include_usage"], true);
        assert!(v.get("tools").is_none());
        assert!(v.get("tool_choice").is_none());
    }

    #[test]
    fn chat_request_omits_tools_and_tool_choice_when_none() {
        let req = ChatRequest::build("qwen", vec![], None, None);
        let s = serde_json::to_string(&req).unwrap();
        assert!(!s.contains("\"tools\""));
        assert!(!s.contains("\"tool_choice\""));
    }

    #[test]
    fn chat_request_includes_tools_and_tool_choice_when_set() {
        let req = ChatRequest::build(
            "qwen",
            vec![],
            Some(tools_array(&["Read"])),
            Some("auto".to_string()),
        );
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["tool_choice"], "auto");
        assert_eq!(v["tools"][0]["function"]["name"], "Read");
    }
}
