//! Tool-call dialects (2026-07-13 tool-dialects design): the per-model
//! output conventions — what grammar constrains a forced call, what
//! trigger arms the lazy grammar, and how a call parses back to
//! `(name, arguments)`. Prompt-side dialect conversion is NOT here: the
//! model's own chat template renders history/tools into its dialect
//! (`InferenceEngine::render_chat_prompt`); this module owns the output
//! side llama.cpp keeps in `common/chat.cpp`, which has no C API to bind.

use super::InferenceError;

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

    /// The byte sequence that arms the lazy (Allow-mode) grammar.
    pub fn lazy_trigger(&self) -> &'static [u8] {
        match self {
            ToolDialect::HermesJson => b"<tool_call>",
            ToolDialect::MiniCpmXml => b"<function",
        }
    }

    /// True when `text` STARTED a call but never closed it — the
    /// token-budget-truncation case the loop feeds a correction for.
    pub fn has_unclosed_call(&self, text: &str) -> bool {
        match self {
            ToolDialect::HermesJson => {
                text.contains("<tool_call>") && !text.contains("</tool_call>")
            }
            ToolDialect::MiniCpmXml => text.contains("<function") && !text.contains("</function>"),
        }
    }

    /// The dialect's closing tag, for the loop's cut-off correction text.
    pub fn closing_tag(&self) -> &'static str {
        match self {
            ToolDialect::HermesJson => "</tool_call>",
            ToolDialect::MiniCpmXml => "</function>",
        }
    }

    /// The complete GBNF for this dialect's forced call.
    /// `json_grammar` is `json_schema_to_grammar(tool_call_schema())`
    /// output — used by Hermes only (MiniCPM's shape is XML, built here
    /// directly). `allowed_names` gates the name enum in BOTH dialects;
    /// `allow_think_prefix` composes the thinking-models scratchpad in
    /// front (see `think_prefix_rules`).
    pub fn grammar(
        &self,
        json_grammar: &str,
        allowed_names: Option<&[&str]>,
        allow_think_prefix: bool,
    ) -> Result<String, InferenceError> {
        match self {
            ToolDialect::HermesJson => {
                super::hermes_tool_call_grammar(json_grammar, allowed_names, allow_think_prefix)
            }
            ToolDialect::MiniCpmXml => minicpm_grammar(allowed_names, allow_think_prefix),
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

    /// The first complete call in `text`, parsed to `(name, arguments)`.
    /// `None` means "no call here" — the caller decides whether that makes
    /// the text a final answer.
    pub fn parse_first(&self, text: &str) -> Option<(String, serde_json::Value)> {
        match self {
            ToolDialect::HermesJson => parse_hermes(text),
            ToolDialect::MiniCpmXml => parse_minicpm(text),
        }
    }
}

/// The shared optional-reasoning prefix (thinking-models design): an
/// optional `<think>` open (thinking templates pre-open the block in the
/// generation prompt, so the model may only emit the CLOSE), free
/// content, the mandatory close. The `think-char` ladder is the standard
/// GBNF until-literal construction for `</think>`.
pub(super) const THINK_PREFIX_RULES: &str = "think-prefix ::= \"<think>\"? think-char* \"</think>\" [ \\t\\r\\n]*\n\
     think-char ::= [^<] | \"<\" [^/] | \"</\" [^t] | \"</t\" [^h] | \"</th\" [^i] | \"</thi\" [^n] | \"</thin\" [^k] | \"</think\" [^>]";

/// MiniCPM5's forced-call grammar. The function-name enum is the
/// load-bearing gate (mirroring Hermes' name-kv enum); param names and
/// values stay free-form — `dispatch::execute` already validates
/// arguments, and schema-typing CDATA-able values in GBNF isn't worth its
/// complexity (tool-dialects design § 3). The `pvalue` ladder is
/// until-literal for `</param>`.
fn minicpm_grammar(
    allowed_names: Option<&[&str]>,
    allow_think_prefix: bool,
) -> Result<String, InferenceError> {
    let fname_rule = match allowed_names {
        None => "fname ::= fchar+\nfchar ::= [A-Za-z0-9_-]".to_string(),
        Some([]) => {
            return Err(InferenceError::Backend(
                "minicpm_grammar: allowed_names must not be empty".to_string(),
            ));
        }
        Some(names) => {
            let alternation = names
                .iter()
                .map(|n| format!("\"{n}\""))
                .collect::<Vec<_>>()
                .join(" | ");
            format!("fname ::= {alternation}")
        }
    };
    let root = if allow_think_prefix {
        "root ::= think-prefix? func"
    } else {
        "root ::= func"
    };
    let think = if allow_think_prefix {
        format!("\n{THINK_PREFIX_RULES}")
    } else {
        String::new()
    };
    Ok(format!(
        "{root}\n\
         func ::= \"<function name=\\\"\" fname \"\\\">\" param* \"</function>\"\n\
         {fname_rule}\n\
         param ::= \"<param name=\\\"\" pname \"\\\">\" pvalue \"</param>\"\n\
         pname ::= [A-Za-z0-9_]+\n\
         pvalue ::= pchar*\n\
         pchar ::= [^<] | \"<\" [^/] | \"</\" [^p] | \"</p\" [^a] | \"</pa\" [^r] | \"</par\" [^a] | \"</para\" [^m] | \"</param\" [^>]{think}"
    ))
}

/// Hermes: the first complete `<tool_call>…</tool_call>` pair anywhere in
/// the text, its JSON parsed to `(name, arguments)`.
fn parse_hermes(text: &str) -> Option<(String, serde_json::Value)> {
    let start = text.find("<tool_call>")? + "<tool_call>".len();
    let end = text[start..].find("</tool_call>")? + start;
    let value = serde_json::from_str::<serde_json::Value>(text[start..end].trim()).ok()?;
    let name = value.get("name")?.as_str()?.to_string();
    let arguments = value.get("arguments")?.clone();
    Some((name, arguments))
}

/// MiniCPM: the first complete `<function name="…">…</function>` block.
/// Param values accept the template's CDATA escape, and coerce to typed
/// JSON when they LOOK like JSON (leading `[ { digit - t f n`) and parse
/// as non-string — `CreatePlan { steps: [...] }` is the case that forces
/// this; everything else stays a plain string.
fn parse_minicpm(text: &str) -> Option<(String, serde_json::Value)> {
    let fn_open = text.find("<function name=\"")?;
    let name_start = fn_open + "<function name=\"".len();
    let name_end = text[name_start..].find('"')? + name_start;
    let name = text[name_start..name_end].to_string();
    let body_start = text[name_end..].find('>')? + name_end + 1;
    let body_end = text[body_start..].find("</function>")? + body_start;
    let body = &text[body_start..body_end];

    let mut arguments = serde_json::Map::new();
    let mut rest = body;
    while let Some(p_open) = rest.find("<param name=\"") {
        let pn_start = p_open + "<param name=\"".len();
        let Some(pn_rel) = rest[pn_start..].find('"') else {
            break;
        };
        let pn_end = pn_rel + pn_start;
        let pname = rest[pn_start..pn_end].to_string();
        let Some(v_rel) = rest[pn_end..].find('>') else {
            break;
        };
        let v_start = pn_end + v_rel + 1;
        let Some(v_end_rel) = rest[v_start..].find("</param>") else {
            break;
        };
        let v_end = v_start + v_end_rel;
        let raw = rest[v_start..v_end].trim();
        let value = raw
            .strip_prefix("<![CDATA[")
            .and_then(|s| s.strip_suffix("]]>"))
            .unwrap_or(raw);
        arguments.insert(pname, coerce_param_value(value));
        rest = &rest[v_end + "</param>".len()..];
    }
    Some((name, serde_json::Value::Object(arguments)))
}

/// See `parse_minicpm`: JSON-looking values become typed JSON, everything
/// else is a string. A value that LOOKS like JSON but fails to parse
/// stays a string — dispatch's own validation gives the model a real
/// error message either way.
fn coerce_param_value(raw: &str) -> serde_json::Value {
    let looks_like_json = matches!(
        raw.chars().next(),
        Some('[' | '{' | '-' | '0'..='9') // arrays, objects, numbers
    ) || matches!(raw, "true" | "false" | "null");
    if looks_like_json {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) {
            if !v.is_string() {
                return v;
            }
        }
    }
    serde_json::Value::String(raw.to_string())
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

    #[test]
    fn minicpm_grammar_gates_the_function_name_enum() {
        let g = ToolDialect::MiniCpmXml
            .grammar("", Some(&["Read", "FinishTask"]), false)
            .unwrap();
        assert!(g.contains(r#"fname ::= "Read" | "FinishTask""#));
        assert!(g.contains("root ::= func"));
        assert!(g.contains(r#"func ::= "<function name=\"" fname "\">" param* "</function>""#));
    }

    #[test]
    fn minicpm_grammar_composes_the_think_prefix() {
        let g = ToolDialect::MiniCpmXml
            .grammar("", Some(&["Read"]), true)
            .unwrap();
        assert!(g.contains("root ::= think-prefix? func"));
        assert!(g.contains("think-prefix ::="));
    }

    #[test]
    fn minicpm_grammar_rejects_an_empty_enum() {
        assert!(ToolDialect::MiniCpmXml
            .grammar("", Some(&[]), false)
            .is_err());
    }

    #[test]
    fn parses_a_minicpm_call_with_plain_params() {
        let (name, args) = ToolDialect::MiniCpmXml
            .parse_first(
                r#"<function name="Read"><param name="file_path">src/main.rs</param></function>"#,
            )
            .unwrap();
        assert_eq!(name, "Read");
        assert_eq!(args["file_path"], "src/main.rs");
    }

    #[test]
    fn parses_minicpm_cdata_and_typed_params() {
        let (name, args) = ToolDialect::MiniCpmXml
            .parse_first(concat!(
                r#"<function name="CreatePlan">"#,
                r#"<param name="goal"><![CDATA[fix <all> the & bugs]]></param>"#,
                r#"<param name="steps">["read the file", "fix it"]</param>"#,
                r#"</function>"#,
            ))
            .unwrap();
        assert_eq!(name, "CreatePlan");
        assert_eq!(args["goal"], "fix <all> the & bugs");
        assert_eq!(
            args["steps"],
            serde_json::json!(["read the file", "fix it"])
        );
    }

    #[test]
    fn minicpm_numeric_strings_coerce_but_paths_stay_strings() {
        let (_, args) = ToolDialect::MiniCpmXml
            .parse_first(concat!(
                r#"<function name="Read"><param name="limit">200</param>"#,
                r#"<param name="file_path">/tmp/2.txt</param></function>"#,
            ))
            .unwrap();
        assert_eq!(args["limit"], 200);
        assert_eq!(args["file_path"], "/tmp/2.txt");
    }

    #[test]
    fn parses_the_hermes_shape_unchanged() {
        let (name, args) = ToolDialect::HermesJson
            .parse_first("<think>hm</think><tool_call>\n{\"name\": \"Read\", \"arguments\": {\"file_path\": \"a\"}}\n</tool_call>")
            .unwrap();
        assert_eq!(name, "Read");
        assert_eq!(args["file_path"], "a");
    }

    #[test]
    fn unclosed_call_detection_is_per_dialect() {
        assert!(ToolDialect::HermesJson.has_unclosed_call("<tool_call>{\"name\""));
        assert!(!ToolDialect::HermesJson.has_unclosed_call("plain answer"));
        assert!(ToolDialect::MiniCpmXml.has_unclosed_call("<function name=\"Read\">"));
        assert!(!ToolDialect::MiniCpmXml.has_unclosed_call("<function name=\"Read\"></function>"));
    }
}
