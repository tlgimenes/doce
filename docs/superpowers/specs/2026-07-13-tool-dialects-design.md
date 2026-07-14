# Tool dialects: real template execution + per-dialect grammar/parse

**Date:** 2026-07-13
**Status:** approved in conversation ("let's do b"). Benchmark-gated with
the same-day thinking-models work.

## Problem

doce is hardwired to Qwen's Hermes tool convention (`<tool_call>{json}`)
in four places: the system prompt's tools block, `ChatMessage::text`'s
history replay, the Require/Allow grammars, and the loop's
`first_tool_call_tag` parser. Worse, `render_chat_prompt` never executes
the GGUF's Jinja template — llama.cpp's core API only pattern-matches
known template shapes — so a model like MiniCPM5-1B (native dialect:
`<function name="X"><param name="k">v</param></function>`, template-driven
`<think>` pre-open) received a Hermes-shaped prompt it was never trained
on, and the Require grammar then forced Hermes tokens onto a distribution
with no mass there. Result: the observed degenerate turns.

llama.cpp DOES solve all of this — in `common/chat.cpp` + minja, which is
C++ without a C API; `llama-cpp-2` binds only the core. Decision (option
B over a llama-server sidecar): stay in-process, adopt the two missing
pieces at the right altitude.

## Design

### 1. Prompt rendering — REVISED during implementation

Full Jinja execution (hf-chat-template) was scoped out of v1: passing
tools through the template makes the rendered prompt diverge from what
`context::fit_turn_to_budget`/`measure` render unless tool definitions
thread through every context call site — a budget-coherence ripple out of
proportion to v1's two ChatML-framed models. Instead the DIALECT renders
the prompt-side pieces (tool-usage instructions in the system prompt,
ToolUse/ToolResult history replay), composed on the ChatML framing
llama.cpp's matcher already produces correctly for both Qwen and MiniCPM5.
`hf-chat-template` remains in Cargo.toml for golden tests asserting our
dialect rendering matches each model's real template output, and full
template execution stays the intended v2 once budgets learn about tools.

### 1b. Original v2 target: execute the model's own template

Adopt `hf-chat-template` (minijinja + transformers-compat). At model load,
`InferenceEngine` reads `tokenizer.chat_template` via `meta_val_str`,
compiles it once, and resolves the special tokens the template references
(`bos_token` etc.) from GGUF metadata. `render_chat_prompt` becomes a real
template render over structured input:

- `MessageContent::ToolUse` → assistant `Message` with
  `tool_calls: [{"type": "function", "function": {"name", "arguments"}}]`
  — the TEMPLATE formats it into the model's own dialect (this is the
  conversion code inside every model's template; we stop duplicating it).
- `MessageContent::ToolResult` → role `tool` with plain content. The old
  "role tool doesn't fire correctly" workaround was an artifact of
  llama.cpp's matcher; real Jinja executes the branch.
- `extra: {"enable_thinking": true}` — thinking models' templates
  pre-open `<think>` in the generation prompt themselves.
- Tools pass as structured JSON schemas (`tools` param), so each model's
  template renders its OWN tools block and usage guidance.

Fallback: a GGUF with no template, or a template minijinja can't render,
falls back to the current llama.cpp pattern-matching path (and therefore
to Hermes assumptions) with a logged warning.

### 2. System prompt: the tools block moves out of the prose

`plan.rs`'s hardcoded `# Tools … <tools>{json lines}</tools> … return a
json object within <tool_call>` section is DELETED from the union prompt:
that text is Hermes-specific teaching that fights any other model's
training. The tool definitions become `serde_json::Value`s passed via the
template's `tools` param; the remaining prose keeps only dialect-neutral
wording ("exactly one tool call", never "<tool_call>"). KV-prefix
stability is preserved: per host the tool list is turn-stable, so the
rendered prompt is byte-stable exactly as before.

### 3. Output side: `ToolDialect` (grammar + parse)

New `inference::dialect` module:

```rust
pub enum ToolDialect { HermesJson, MiniCpmXml }
```

- `detect(template_source) -> ToolDialect` — like llama.cpp: template
  containing `<tool_call>` → HermesJson; `<function name=` → MiniCpmXml;
  unknown → HermesJson (the historical assumption, harmless for the
  historical models).
- `grammar(&self, allowed_names, allow_think_prefix)` — HermesJson keeps
  today's `tool_call_grammar`; MiniCpmXml constrains
  `<function name="{enum}">` + `<param name="…">…</param>*` +
  `</function>`, with param VALUES free-form (until-literal ladder for
  `</param>`; CDATA accepted). Param names/values are not
  schema-constrained in v1 — the name enum is the load-bearing gate, and
  dispatch already validates arguments.
- `lazy_trigger(&self)` — `<tool_call>` / `<function` for Allow mode.
- `parse_first(&self, text) -> Option<(name, arguments)>` — moves
  `first_tool_call_tag`+serde for Hermes; XML extraction for MiniCPM with
  value coercion: a param value that parses as non-string JSON (array,
  number, bool, object) becomes that JSON when it LOOKS like JSON
  (leading `[ { digit t f`), else stays a string — CreatePlan's
  `steps: string[]` is the case that forces this.
- `has_unclosed_call(&self, text)` — the loop's mid-call detection.

The engine owns the detected dialect (`engine.dialect()`); the agent loop
and both backends route parsing/grammar through it. The think-prefix
wrapper composes around either dialect's grammar unchanged.

### 4. Explicitly out of scope

Per-model sampling parameters (still Qwen's recommended chain), the
llama-server sidecar (rejected for now), dialects beyond these two, and
schema-typed grammar constraints on MiniCPM param values.

## Verification

- Unit: dialect detection from both real templates; MiniCPM grammar
  string shape; MiniCPM parse round-trips (plain, CDATA, typed coercion,
  multiple params); Hermes tests keep passing behind the trait; template
  rendering golden tests against the two models' real template strings
  (Qwen: Hermes replay + <think> pre-open; MiniCPM: <function> replay +
  tools block + pre-open).
- Benchmark: the ladder against Qwen3-Thinking (regression vs today's
  hand-rolled rendering) and MiniCPM5-1B (the new capability).
