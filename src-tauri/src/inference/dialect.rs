//! Tool-call dialects (2026-07-13 tool-dialects design): the per-model
//! output conventions still needed by the surviving prompt/render surface
//! after the llama-server cutover: which model family this is (`detect`),
//! how a past tool call/result replays into history in the model's own
//! trained shape (`render_tool_use`/`render_tool_result`, used by
//! `InferenceEngine::render_chat_prompt` for context measurement), and the
//! system prompt's call-format teaching (`call_format_instructions`). The
//! generation-side grammar and the text tool-call parser that once lived
//! here were deleted with the rest of the in-process inference stack.

/// Detected from the model's chat template, the same way llama.cpp's
/// `common/chat.cpp` fingerprints formats. `HermesJson` is the historical
/// assumption (Qwen-family: `<tool_call>{"name", "arguments"}</tool_call>`);
/// `MiniCpmXml` is MiniCPM5's
/// `<function name="X"><param name="k">v</param></function>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolDialect {
    #[default]
    HermesJson,
    MiniCpmXml,
}

impl ToolDialect {
    /// Fingerprints the template SOURCE (the Jinja text, not a render):
    /// each dialect's template literally contains its own call markup in
    /// the branch that replays `tool_calls`. Unknown templates fall back
    /// to Hermes — the historical behavior, correct for every model this
    /// app shipped with before dialects existed.
    pub fn detect(template_source: &str) -> Self {
        if template_source.contains("<function name=") {
            ToolDialect::MiniCpmXml
        } else {
            ToolDialect::HermesJson
        }
    }

    /// The system prompt's "how to call" teaching — each dialect's wording
    /// matches what its models' own chat templates emit as tool guidance,
    /// so the instruction text never fights the training (the exact
    /// failure that produced the 2026-07-13 degenerate MiniCPM turns).
    pub fn call_format_instructions(&self) -> &'static str {
        match self {
            ToolDialect::HermesJson => {
                r#"For each function call, return a json object with function name and arguments within <tool_call></tool_call> XML tags:
<tool_call>
{"name": <function-name>, "arguments": <args-json-object>}
</tool_call>"#
            }
            ToolDialect::MiniCpmXml => {
                r#"When calling a function, return an XML object within <function ... </function> using:
<function name="function-name"><param name="param-name">param-value</param></function>
param-value may be multi-line. If it contains <, & or newline characters, wrap it in a CDATA block: <param name="param-name"><![CDATA[...multi-line value...]]></param>. For a non-string parameter (array, number, boolean), write the value as JSON."#
            }
        }
    }

    /// A past tool call, replayed into history in the model's own trained
    /// shape (what each model's chat template would render for an
    /// assistant `tool_calls` entry).
    pub fn render_tool_use(&self, name: &str, input: &serde_json::Value) -> String {
        match self {
            ToolDialect::HermesJson => format!(
                "<tool_call>\n{}\n</tool_call>",
                serde_json::json!({ "name": name, "arguments": input })
            ),
            ToolDialect::MiniCpmXml => {
                let mut out = format!("<function name=\"{name}\">");
                if let Some(obj) = input.as_object() {
                    for (k, v) in obj {
                        let raw = match v {
                            serde_json::Value::String(s) => s.clone(),
                            other => other.to_string(),
                        };
                        let needs_cdata =
                            raw.contains('<') || raw.contains('&') || raw.contains('\n');
                        out.push_str(&format!("<param name=\"{k}\">"));
                        if needs_cdata {
                            out.push_str(&format!("<![CDATA[{raw}]]>"));
                        } else {
                            out.push_str(&raw);
                        }
                        out.push_str("</param>");
                    }
                }
                out.push_str("</function>");
                out
            }
        }
    }

    /// A tool result, replayed into history. Both current dialects' chat
    /// templates wrap results in the same `<tool_response>` tags.
    pub fn render_tool_result(&self, content: &str) -> String {
        format!("<tool_response>{content}</tool_response>")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const QWEN_TEMPLATE_SNIPPET: &str = r#"{%- for tool_call in message.tool_calls %}<tool_call>{"name": "..."}</tool_call>{%- endfor %}"#;
    const MINICPM_TEMPLATE_SNIPPET: &str = r#"{{- '<function name="' ~ tool_call.name ~ '">' }}"#;

    #[test]
    fn detects_dialect_from_template_source() {
        assert_eq!(
            ToolDialect::detect(QWEN_TEMPLATE_SNIPPET),
            ToolDialect::HermesJson
        );
        assert_eq!(
            ToolDialect::detect(MINICPM_TEMPLATE_SNIPPET),
            ToolDialect::MiniCpmXml
        );
        // Unknown templates keep the historical assumption.
        assert_eq!(
            ToolDialect::detect("{{ messages }}"),
            ToolDialect::HermesJson
        );
    }
}
