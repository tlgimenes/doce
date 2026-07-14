//! Pure data-mapping layer for the llama-server (OpenAI-compatible)
//! `/v1/chat/completions` API — no networking here, just the shape
//! translation between doce's internal `ChatMessage`/`ToolCallMode` and the
//! JSON a `reqwest` client (a later task) will POST. Kept pure and
//! model-free so every mapping decision (role mapping, tool-call
//! arguments-as-string, sampling defaults) is unit-testable without a
//! running server.

use super::{ChatMessage, InferenceError, MessageContent, ToolCallMode};
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

/// One decoded piece of a streaming `/v1/chat/completions` response —
/// `parse_sse_line`'s output unit. `ToolCallFragment::args` is always a
/// STRING fragment: llama-server normally streams `function.arguments` as
/// string chunks to be concatenated by `index` (see `ToolCallAccum`), but
/// some server builds send a fully-parsed JSON OBJECT instead — that
/// tolerance is resolved in `parse_sse_line` itself, by re-serializing the
/// object back to a string, so every downstream consumer of `ChatChunk`
/// only ever has one shape to handle.
#[derive(Debug, Clone, PartialEq)]
pub enum ChatChunk {
    Content(String),
    Reasoning(String),
    ToolCallFragment {
        index: u32,
        id: Option<String>,
        name: Option<String>,
        args: String,
    },
    Usage {
        prompt: u32,
        completion: u32,
    },
    /// `choices[0].finish_reason`, once the server sets it to a non-null
    /// string (e.g. `"stop"`, `"tool_calls"`) — a sibling of `delta` on the
    /// same choice object, not nested inside it, so it is surfaced as its
    /// own chunk rather than folded into the content/reasoning/tool-call
    /// handling above.
    FinishReason(String),
    Done,
}

/// Parses one line of an SSE stream from llama-server's
/// `/v1/chat/completions` endpoint into zero or more `ChatChunk`s.
/// Tolerant by design — this is fed straight from the wire, one line per
/// call, so it never panics: a line that isn't an `data:` event (blank
/// lines, SSE comments/keepalives, any other non-`data:` line) or whose
/// JSON payload fails to parse both return `None`, and the caller's job is
/// simply to skip the line and keep reading, not to treat it as a stream
/// error.
///
/// `data: [DONE]` — llama-server's (and every OpenAI-compatible server's)
/// sentinel for stream end — maps to `Some(vec![ChatChunk::Done])` rather
/// than going through JSON parsing at all, since `[DONE]` is deliberately
/// not valid JSON.
///
/// A single line CAN legitimately map to more than one chunk (e.g. a
/// content delta alongside a reasoning delta, or several `tool_calls[]`
/// entries in one event), hence `Vec` rather than a single `ChatChunk`; an
/// empty `Vec` never escapes this function — a chunk that decodes to
/// nothing collapses to `None`, keeping "skip this line" a single check at
/// every call site instead of two.
pub fn parse_sse_line(line: &str) -> Option<Vec<ChatChunk>> {
    let data = line
        .strip_prefix("data: ")
        .or_else(|| line.strip_prefix("data:"))?
        .trim();
    if data.is_empty() {
        return None;
    }
    if data == "[DONE]" {
        return Some(vec![ChatChunk::Done]);
    }

    let v: Value = serde_json::from_str(data).ok()?;
    let mut chunks = Vec::new();

    if let Some(choice) = v
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first())
    {
        if let Some(delta) = choice.get("delta") {
            if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                chunks.push(ChatChunk::Content(content.to_string()));
            }
            if let Some(reasoning) = delta.get("reasoning_content").and_then(|c| c.as_str()) {
                chunks.push(ChatChunk::Reasoning(reasoning.to_string()));
            }
            if let Some(tool_calls) = delta.get("tool_calls").and_then(|t| t.as_array()) {
                for tc in tool_calls {
                    let index = tc.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as u32;
                    let id = tc.get("id").and_then(|i| i.as_str()).map(|s| s.to_string());
                    let function = tc.get("function");
                    let name = function
                        .and_then(|f| f.get("name"))
                        .and_then(|n| n.as_str())
                        .map(|s| s.to_string());
                    // Tolerance rule: `arguments` is normally a JSON-encoded
                    // string fragment, but some server builds send a
                    // fully-parsed object instead — re-serialize it to a
                    // string here so `ChatChunk::ToolCallFragment::args` is
                    // always the same shape downstream.
                    let args = function
                        .and_then(|f| f.get("arguments"))
                        .map(|a| match a {
                            Value::String(s) => s.clone(),
                            other => other.to_string(),
                        })
                        .unwrap_or_default();
                    chunks.push(ChatChunk::ToolCallFragment {
                        index,
                        id,
                        name,
                        args,
                    });
                }
            }
        }
        // `finish_reason` sits alongside `delta` on the choice object, not
        // nested inside it — checked independently so a line that carries
        // only a finish reason (an empty `delta: {}`) still surfaces it.
        if let Some(finish_reason) = choice.get("finish_reason").and_then(|f| f.as_str()) {
            chunks.push(ChatChunk::FinishReason(finish_reason.to_string()));
        }
    }

    // The final usage chunk has empty `choices:[]`, so this is checked
    // independently of the `choices[0].delta` block above, not as an
    // `else`.
    if let Some(usage) = v.get("usage").filter(|u| !u.is_null()) {
        if let (Some(prompt), Some(completion)) = (
            usage.get("prompt_tokens").and_then(|p| p.as_u64()),
            usage.get("completion_tokens").and_then(|c| c.as_u64()),
        ) {
            chunks.push(ChatChunk::Usage {
                prompt: prompt as u32,
                completion: completion as u32,
            });
        }
    }

    if chunks.is_empty() {
        None
    } else {
        Some(chunks)
    }
}

/// Accumulates `ChatChunk::ToolCallFragment`s across a streamed response
/// into one complete tool call, keyed by `index` — the wire sends `name`
/// once and `arguments` as string fragments spread across many SSE events,
/// so nothing is a usable tool call until the stream (or at least that
/// `index`) finishes. `doce`'s requests always set `parallel_tool_calls:
/// false` (see `ChatRequest::build`), so in practice there is only ever one
/// index in play — `index: 0` — but fragments are still bucketed by index
/// rather than assumed single-stream, in case a server ignores that
/// request flag.
#[derive(Default)]
pub struct ToolCallAccum {
    /// `index -> (id, name, concatenated arguments)`. `BTreeMap` (not
    /// `HashMap`) so `finish` can deterministically pick the
    /// lowest/first/primary index without a separate insertion-order
    /// tracker.
    calls: std::collections::BTreeMap<u32, (Option<String>, Option<String>, String)>,
}

impl ToolCallAccum {
    /// Folds one chunk in. Non-`ToolCallFragment` chunks (`Content`,
    /// `Reasoning`, `Usage`, `Done`) are silently ignored — this accumulator
    /// only ever cares about tool-call fragments, so callers can feed it
    /// every chunk from `parse_sse_line` unfiltered. `id`/`name` are set
    /// once (first non-`None` wins — the wire only ever sends each once,
    /// on the fragment that opens that index) and `args` is concatenated in
    /// arrival order.
    pub fn push_fragment(&mut self, chunk: ChatChunk) {
        let ChatChunk::ToolCallFragment {
            index,
            id,
            name,
            args,
        } = chunk
        else {
            return;
        };
        let entry = self.calls.entry(index).or_default();
        if entry.0.is_none() {
            entry.0 = id;
        }
        if entry.1.is_none() {
            entry.1 = name;
        }
        entry.2.push_str(&args);
    }

    /// Resolves the first/primary tool call (the lowest accumulated index —
    /// always `0` in practice, see the struct doc comment) into its final
    /// `(name, arguments)` shape. The accumulated `args` string is parsed
    /// two ways, most-expected-shape first: first as a JSON object (the
    /// normal case — `arguments` is a JSON-encoded object), then, if that
    /// doesn't fit (e.g. a buggy build streamed a bare scalar), as any
    /// JSON value at all. Only if BOTH fail — a syntactically broken
    /// string, e.g. a stream that got cut off mid-argument — is `None`
    /// returned, so the caller treats it as a malformed tool call needing a
    /// correction turn rather than a half-formed value. `None` also covers
    /// the "nothing was ever accumulated" and "a name never arrived" cases
    /// — both leave no usable tool call to return.
    pub fn finish(self) -> Option<(String, Value)> {
        let (_, name, args) = self.calls.into_iter().next()?.1;
        let name = name?;
        let value = serde_json::from_str::<serde_json::Map<String, Value>>(&args)
            .map(Value::Object)
            .or_else(|_| serde_json::from_str::<Value>(&args))
            .ok()?;
        Some((name, value))
    }
}

/// The result of one complete `LlamaServerClient::chat` call — everything
/// the caller needs once the stream ends (or, in a later task, once it's
/// ready to fold into a persisted `ChatMessage`): the model's own text,
/// its reasoning (stripped `<think>`-equivalent, but here it's the
/// server-native `reasoning_content` delta rather than a tag to strip),
/// a resolved tool call if one was made, why the stream stopped, and the
/// token accounting for the turn.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatOutcome {
    /// `ToolCallAccum::finish`'s result — `None` unless the model actually
    /// emitted `tool_calls` deltas this turn.
    pub tool_call: Option<(String, Value)>,
    /// Concatenated `ChatChunk::Content` deltas, in arrival order.
    pub text: String,
    /// Concatenated `ChatChunk::Reasoning` deltas, in arrival order.
    pub reasoning: String,
    /// The last `ChatChunk::FinishReason` seen before the stream ended
    /// (`"stop"`, `"tool_calls"`, etc.) — empty if the server never sent
    /// one, which callers should treat the same as an unknown reason.
    pub finish_reason: String,
    /// `(prompt_tokens, completion_tokens)` from the trailing usage event
    /// llama-server sends because `ChatRequest::build` always sets
    /// `stream_options.include_usage`. `None` only if the server dropped
    /// the connection before that event arrived (e.g. mid-stream error).
    pub usage: Option<(u32, u32)>,
}

/// Talks to one llama-server instance's OpenAI-compatible
/// `/v1/chat/completions` endpoint over HTTP + SSE. Holds a `reqwest::Client`
/// (not recreated per call) so connection pooling/keep-alive work across
/// turns, matching the one-worker-per-app model `InferenceEngine` already
/// uses for the in-process llama.cpp path — this is the equivalent front
/// door for the llama-server cutover, deliberately just data-in/data-out
/// with no state of its own beyond the base URL and HTTP client.
pub struct LlamaServerClient {
    base_url: String,
    http: reqwest::Client,
}

impl LlamaServerClient {
    /// `base_url` is the sidecar's own root (e.g. `http://127.0.0.1:PORT`),
    /// with no trailing slash assumed either way — `chat` always joins it
    /// with a leading `/v1/chat/completions`.
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            http: reqwest::Client::new(),
        }
    }

    /// POSTs `req` to `{base_url}/v1/chat/completions` and drives the SSE
    /// response to a `ChatOutcome`, calling `on_piece` with each
    /// content/reasoning fragment as it arrives so a caller can stream
    /// progress to the UI the same way `InferenceEngine::generate`'s
    /// `on_token` already does for the in-process path.
    ///
    /// `cancel` is checked before the request is sent at all (an
    /// already-cancelled token never touches the network) and raced against
    /// every await point after that (the initial `send()` and every body
    /// chunk read) via `tokio::select!`, so a cancellation lands promptly
    /// instead of only once the stream naturally ends — the same "checked
    /// between steps, not just before starting" discipline
    /// `PromptSession::generate`'s `should_cancel` uses for the in-process
    /// decode loop.
    ///
    /// Reads the response body via `Response::chunk()` (an async pull, one
    /// `Bytes` frame at a time) rather than `bytes_stream()` — behaviorally
    /// identical, but avoids pulling in `futures_util::StreamExt` as a new
    /// direct dependency just to call `.next()` on the `Stream` `bytes_stream`
    /// returns. Frames are appended to a line buffer and split on `\n`, since
    /// SSE frames from the wire don't reliably land on line boundaries.
    pub async fn chat(
        &self,
        req: ChatRequest,
        mut on_piece: impl FnMut(&str),
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<ChatOutcome, InferenceError> {
        if cancel.is_cancelled() {
            return Err(InferenceError::Cancelled);
        }

        let url = format!("{}/v1/chat/completions", self.base_url);
        let mut response = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(InferenceError::Cancelled),
            result = self
                .http
                .post(&url)
                .header("content-type", "application/json")
                .json(&req)
                .send() => result.map_err(|e| InferenceError::Backend(e.to_string()))?,
        };

        let mut buf = String::new();
        let mut accum = ToolCallAccum::default();
        let mut text = String::new();
        let mut reasoning = String::new();
        let mut finish_reason = String::new();
        let mut usage = None;

        loop {
            let body_chunk = tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(InferenceError::Cancelled),
                result = response.chunk() => result.map_err(|e| InferenceError::Backend(e.to_string()))?,
            };
            let Some(bytes) = body_chunk else {
                // The connection closed without an explicit `[DONE]` line —
                // treat whatever was accumulated as final rather than
                // erroring, the same tolerance `parse_sse_line` already
                // extends to individual malformed lines.
                break;
            };
            buf.push_str(&String::from_utf8_lossy(&bytes));

            while let Some(pos) = buf.find('\n') {
                let line = buf[..pos].trim_end_matches('\r').to_string();
                buf.drain(..=pos);
                let Some(sse_chunks) = parse_sse_line(&line) else {
                    continue;
                };
                for sse_chunk in sse_chunks {
                    match sse_chunk {
                        ChatChunk::Content(s) => {
                            on_piece(&s);
                            text.push_str(&s);
                        }
                        ChatChunk::Reasoning(s) => {
                            on_piece(&s);
                            reasoning.push_str(&s);
                        }
                        ChatChunk::ToolCallFragment { .. } => accum.push_fragment(sse_chunk),
                        ChatChunk::Usage { prompt, completion } => {
                            usage = Some((prompt, completion));
                        }
                        ChatChunk::FinishReason(s) => finish_reason = s,
                        ChatChunk::Done => {
                            return Ok(ChatOutcome {
                                tool_call: accum.finish(),
                                text,
                                reasoning,
                                finish_reason,
                                usage,
                            });
                        }
                    }
                }
            }
        }

        Ok(ChatOutcome {
            tool_call: accum.finish(),
            text,
            reasoning,
            finish_reason,
            usage,
        })
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

    // --- SSE stream parser (task 2.2) ---

    #[test]
    fn parses_content_and_reasoning_deltas() {
        let l = r#"data: {"choices":[{"delta":{"content":"hel"},"index":0}]}"#;
        assert!(matches!(&parse_sse_line(l).unwrap()[0], ChatChunk::Content(s) if s=="hel"));
        let r = r#"data: {"choices":[{"delta":{"reasoning_content":"think"},"index":0}]}"#;
        assert!(matches!(&parse_sse_line(r).unwrap()[0], ChatChunk::Reasoning(s) if s=="think"));
    }

    #[test]
    fn accumulates_tool_call_fragments_by_index() {
        let mut acc = ToolCallAccum::default();
        acc.push_fragment(ChatChunk::ToolCallFragment {
            index: 0,
            id: Some("c1".into()),
            name: Some("Read".into()),
            args: String::new(),
        });
        acc.push_fragment(ChatChunk::ToolCallFragment {
            index: 0,
            id: None,
            name: None,
            args: "{\"file_path\":".into(),
        });
        acc.push_fragment(ChatChunk::ToolCallFragment {
            index: 0,
            id: None,
            name: None,
            args: "\"/x\"}".into(),
        });
        let (name, args) = acc.finish().unwrap();
        assert_eq!(name, "Read");
        assert_eq!(args["file_path"], "/x");
    }

    #[test]
    fn tolerates_arguments_as_object() {
        let l = r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"name":"Read","arguments":{"file_path":"/x"}}}]}}]}"#;
        let chunks = parse_sse_line(l).unwrap();
        let mut acc = ToolCallAccum::default();
        for c in chunks {
            acc.push_fragment(c);
        }
        assert_eq!(acc.finish().unwrap().1["file_path"], "/x");
    }

    #[test]
    fn finish_falls_back_to_any_json_value_for_a_non_object() {
        let mut acc = ToolCallAccum::default();
        acc.push_fragment(ChatChunk::ToolCallFragment {
            index: 0,
            id: Some("c1".into()),
            name: Some("Read".into()),
            args: "[\"a\",".into(),
        });
        acc.push_fragment(ChatChunk::ToolCallFragment {
            index: 0,
            id: None,
            name: None,
            args: "\"b\"]".into(),
        });
        let (name, value) = acc.finish().unwrap();
        assert_eq!(name, "Read");
        assert_eq!(value, serde_json::json!(["a", "b"]));
    }

    #[test]
    fn finish_returns_none_for_syntactically_invalid_json() {
        let mut acc = ToolCallAccum::default();
        acc.push_fragment(ChatChunk::ToolCallFragment {
            index: 0,
            id: Some("c1".into()),
            name: Some("Read".into()),
            args: "{\"file_path\":".into(),
        });
        assert!(acc.finish().is_none());
    }

    #[test]
    fn parses_usage_tail_and_done() {
        let u = r#"data: {"choices":[],"usage":{"prompt_tokens":12,"completion_tokens":5}}"#;
        assert!(matches!(
            parse_sse_line(u).unwrap()[0],
            ChatChunk::Usage {
                prompt: 12,
                completion: 5
            }
        ));
        assert!(matches!(
            parse_sse_line("data: [DONE]").unwrap()[0],
            ChatChunk::Done
        ));
    }

    #[test]
    fn parse_sse_line_returns_none_for_a_blank_line() {
        assert!(parse_sse_line("").is_none());
        assert!(parse_sse_line("   ").is_none());
    }

    #[test]
    fn parse_sse_line_returns_none_for_malformed_json() {
        assert!(parse_sse_line("data: not json").is_none());
    }

    #[test]
    fn parses_finish_reason_from_a_choice() {
        let l = r#"data: {"choices":[{"delta":{},"finish_reason":"stop","index":0}]}"#;
        assert!(matches!(
            &parse_sse_line(l).unwrap()[0],
            ChatChunk::FinishReason(s) if s == "stop"
        ));
    }

    // --- streaming HTTP client (task 2.3) ---

    fn sample_request() -> ChatRequest {
        ChatRequest::build(
            "qwen",
            vec![serde_json::json!({"role":"user","content":"hi"})],
            None,
            None,
        )
    }

    #[tokio::test]
    async fn chat_returns_tool_call_from_sse() {
        let server = wiremock::MockServer::start().await;
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"hmm\"},\"index\":0}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",\"function\":{\"name\":\"Read\",\"arguments\":\"{\\\"file_path\\\":\\\"/x\\\"}\"}}]},\"index\":0}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\",\"index\":0}]}\n\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":9,\"completion_tokens\":4}}\n\n",
            "data: [DONE]\n\n"
        );
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/v1/chat/completions"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_raw(body, "text/event-stream"),
            )
            .mount(&server)
            .await;
        let client = LlamaServerClient::new(server.uri());
        let out = client
            .chat(
                sample_request(),
                |_p| {},
                &tokio_util::sync::CancellationToken::new(),
            )
            .await
            .unwrap();
        let (name, args) = out.tool_call.unwrap();
        assert_eq!(name, "Read");
        assert_eq!(args["file_path"], "/x");
        assert_eq!(out.reasoning, "hmm");
        assert_eq!(out.usage, Some((9, 4)));
        assert_eq!(out.finish_reason, "tool_calls");
    }

    #[tokio::test]
    async fn chat_aborts_on_cancel() {
        let token = tokio_util::sync::CancellationToken::new();
        token.cancel();
        let client = LlamaServerClient::new("http://127.0.0.1:1"); // unreachable; cancel wins
        let r = client.chat(sample_request(), |_p| {}, &token).await;
        assert!(matches!(r, Err(InferenceError::Cancelled)));
    }

    #[tokio::test]
    async fn chat_streams_text_only_response() {
        let server = wiremock::MockServer::start().await;
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"hel\"},\"index\":0}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"lo\"},\"index\":0}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\",\"index\":0}]}\n\n",
            "data: [DONE]\n\n"
        );
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/v1/chat/completions"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_raw(body, "text/event-stream"),
            )
            .mount(&server)
            .await;
        let client = LlamaServerClient::new(server.uri());
        let mut pieces: Vec<String> = Vec::new();
        let out = client
            .chat(
                sample_request(),
                |p| pieces.push(p.to_string()),
                &tokio_util::sync::CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(out.tool_call.is_none());
        assert_eq!(out.text, "hello");
        assert_eq!(out.finish_reason, "stop");
        assert_eq!(pieces, vec!["hel".to_string(), "lo".to_string()]);
    }
}
