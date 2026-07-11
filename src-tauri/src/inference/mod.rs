use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::json_schema_to_grammar;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaChatMessage, LlamaChatTemplate, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;
use std::num::NonZeroU32;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, thiserror::Error)]
pub enum InferenceError {
    #[error("llama.cpp backend error: {0}")]
    Backend(String),
    #[error("model load failed: {0}")]
    ModelLoad(String),
}

/// The client-side `LlamaBatch`'s fixed capacity — llama.cpp can't decode
/// more tokens than this in a single call, so any prompt longer than this
/// (system prompt + tool list + growing conversation history routinely
/// exceeds it in agent mode) must be prefilled across multiple `decode()`
/// calls via `prefill_chunks_from`, not one `batch.add()` loop over every
/// token.
const BATCH_CAPACITY: usize = 512;

/// The model's total context window, in tokens (010-context-window-management).
/// Named and public rather than a bare literal inside `generate()` so both
/// the budget/compaction calculations in `crate::context` and any future
/// IPC surface can read the same value `generate()` actually decodes
/// against, instead of each guessing at (or duplicating) the number.
///
/// This is the one anchor every constant in `context::limits` is sized
/// relative to -- see that module for the rest of the context-budget
/// knobs (tiered-compaction thresholds, tool-output offload size, etc.),
/// gathered there specifically so they're easy to reconsider together
/// whenever this value changes. Not a hardware limit: the model itself
/// was trained on sequences up to 262144 tokens (`n_ctx_train` in the
/// llama.cpp startup log) -- this is a deliberately chosen budget, raised
/// from the original 2048 once real use showed the tiered-compaction
/// pipeline had too little headroom to work with at that size.
pub const CONTEXT_WINDOW_TOKENS: u32 = 16384;

/// Splits the half-open range `[start, n_tokens)` into `<= batch_capacity`
/// chunks, in order — the sequence of chunks a prompt (or, for a
/// `PromptSession` that reused a KV prefix of length `start`, its divergent
/// suffix) is prefilled in. Positions are *absolute* (they continue from
/// `start`, not re-based to 0), so the suffix batch's `n_past` is exactly
/// `start`, which keeps the reused prefix's KV entries at their original
/// positions. `start == 0` is the whole-prompt case. Pure and independent of
/// llama.cpp so the off-by-one-prone boundary math (the exact bug this
/// fixes: a prompt of precisely `batch_capacity + 1` tokens) can be
/// unit-tested without a real model.
fn prefill_chunks_from(
    start: usize,
    n_tokens: usize,
    batch_capacity: usize,
) -> Vec<std::ops::Range<usize>> {
    (start..n_tokens)
        .step_by(batch_capacity)
        .map(|s| s..(s + batch_capacity).min(n_tokens))
        .collect()
}

/// Length of the longest shared prefix of two token slices — how many
/// leading tokens a new prompt has in common with what a `PromptSession`
/// already holds materialized in its KV cache, and therefore how much of
/// the prompt can be reused rather than re-decoded. Pure and llama.cpp-free
/// so the reuse boundary is unit-testable without a real model.
fn common_prefix_len(a: &[LlamaToken], b: &[LlamaToken]) -> usize {
    a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count()
}

/// A tool call's or tool result's structured payload — the content-block
/// shape frontier-lab APIs use (Anthropic's `tool_use`/`tool_result`
/// blocks, OpenAI's `tool_calls` + `tool_call_id`), adopted here now that
/// `generate`'s grammar-constrained decoding makes it safe to trust the
/// model's tool-call JSON is well-formed, rather than parsing free text
/// and hoping. Mirrors what gets persisted to the `messages` table
/// (storage/conversations.rs) closely on purpose — reconstructing this
/// from a DB row, or rendering it back to the flat string the model's
/// chat template needs (`ChatMessage::text`), are both small, pure
/// transforms rather than lossy reshaping.
#[derive(Debug, Clone)]
pub enum MessageContent {
    Text(String),
    /// The model's decision to call a tool. `id` is assigned by the
    /// harness (`agent::run_loop`), not the model — the model only ever
    /// decides `name`/`input`; stamping an id on the decision is the
    /// platform's job, the same convention OpenAI/Anthropic use.
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    /// A tool's result, linked back to its call via `tool_use_id` instead
    /// of sequence-adjacency plus a magic string prefix.
    ToolResult {
        tool_use_id: String,
        tool_name: String,
        content: String,
    },
}

/// How a `generate()` call treats tool calls. `Forbid`: no grammar at all
/// (summarization). `Allow`: lazy grammar — constrains output
/// only once the model starts a `<tool_call>`, so plain-text final answers
/// stay completely free. `Require`: non-lazy grammar — the response MUST
/// be one well-formed tool call, used while the plan engine is Executing a
/// step, where a plain-text reply would end the whole task (observed for
/// real: the model emitted `StepDone(...)` as prose mid-task and ended a
/// 20-file job at file 1 — making plain text unsamplable in that state
/// closes the failure at the sampler, not the prompt).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCallMode {
    Forbid,
    Allow,
    Require,
}

/// A single role-tagged conversation turn. Chat-tuned models like Qwen are
/// trained on turns wrapped in special tokens (e.g. ChatML's
/// `<|im_start|>role\n...<|im_end|>`), not on raw concatenated text — see
/// `ChatMessage::text`/`InferenceEngine::render_chat_prompt`, which is what
/// actually turns these into that flat per-turn string.
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: MessageContent,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: MessageContent::Text(content.into()),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: MessageContent::Text(content.into()),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: MessageContent::Text(content.into()),
        }
    }

    /// The model's own tool-call decision — always role `assistant`,
    /// matching this codebase's existing persistence convention
    /// (`commands::agent::persist_tool_call`).
    pub fn tool_use(
        id: impl Into<String>,
        name: impl Into<String>,
        input: serde_json::Value,
    ) -> Self {
        Self {
            role: "assistant".to_string(),
            content: MessageContent::ToolUse {
                id: id.into(),
                name: name.into(),
                input,
            },
        }
    }

    /// A tool's result fed back into the transcript — role `user`.
    ///
    /// This was tried as role `tool` first, on the theory that it would
    /// hit Qwen's own chat template branch for `role == "tool"` (its real,
    /// embedded Jinja template does have one, confirmed by reading the
    /// GGUF's own metadata) and render as `<tool_response>...
    /// </tool_response>` -- the format Qwen actually trained on. Verified
    /// against the real model that this doesn't happen: llama.cpp's
    /// template engine renders an unrecognized role generically as a bare
    /// `<|im_start|>tool\n...` block instead, which the model was never
    /// trained on -- worse than the plain-`user` approximation. So `role`
    /// stays `user` (reliably handled), and `ChatMessage::text` below
    /// wraps the content in the literal `<tool_response>` tags itself --
    /// same text Qwen expects, produced without depending on template
    /// branching that doesn't actually work in this runtime.
    pub fn tool_result(
        tool_use_id: impl Into<String>,
        tool_name: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            role: "user".to_string(),
            content: MessageContent::ToolResult {
                tool_use_id: tool_use_id.into(),
                tool_name: tool_name.into(),
                content: content.into(),
            },
        }
    }

    /// The plain string this message renders to for the model's own
    /// prompt — the one pure transform between the structured shape above
    /// and what `render_chat_prompt`/llama.cpp's chat template needs (a
    /// flat string per turn). A `ToolUse` renders in Qwen's own trained
    /// (Hermes-style) format — `{"name": ..., "arguments": ...}` JSON
    /// inside `<tool_call></tool_call>` tags with the same newline
    /// placement Qwen's embedded Jinja template produces — so the model's
    /// past actions replay in exactly the shape it was trained to emit. A
    /// `ToolResult`'s text is wrapped in the literal `<tool_response>...
    /// </tool_response>` tags Qwen's own chat template uses (confirmed
    /// against its real, embedded Jinja template) -- reproduced here as
    /// plain text rather than relying on template role-branching, since
    /// that branch doesn't actually fire correctly against this model at
    /// runtime (see `ChatMessage::tool_result`'s doc comment).
    pub fn text(&self) -> String {
        match &self.content {
            MessageContent::Text(s) => s.clone(),
            MessageContent::ToolUse { name, input, .. } => {
                format!(
                    "<tool_call>\n{}\n</tool_call>",
                    serde_json::json!({ "name": name, "arguments": input })
                )
            }
            // No "Tool result for {tool_name}:" framing -- Qwen's own
            // convention (per its chat template) is just the raw content
            // inside the tags, relying on tool_call/tool_result ordering
            // (never more than one pending at a time in this loop) to
            // establish which tool it came from, not a repeated name in
            // the text itself. Qwen's own template actually wraps with
            // newlines (`\n<tool_response>\n` + content + `\n</tool_response>`)
            // but that's not preserved here -- a single-line wrap instead.
            MessageContent::ToolResult { content, .. } => {
                format!("<tool_response>{content}</tool_response>")
            }
        }
    }
}

/// The sampler seed for one `generate()` call: `DOCE_GEN_SEED` (set by the
/// benchmark protocol for reproducible runs — single-run agent benchmarks
/// were observed swinging 0/20..20/20 on seed alone) or per-call entropy.
fn generation_seed() -> u32 {
    std::env::var("DOCE_GEN_SEED")
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0)
        })
}

/// The JSON schema every tool call must satisfy — Qwen's own trained
/// (Hermes-style) `{"name": string, "arguments": object}` shape. Kept as
/// its own function so `tool_call_grammar`'s unit tests exercise the exact
/// schema the sampler builds from.
fn tool_call_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" },
            "arguments": { "type": "object" }
        },
        "required": ["name", "arguments"]
    })
}

/// The `name-kv` rule `json_schema_to_grammar` emits for `tool_call_schema`
/// (an unconstrained JSON string), and the gated replacement that points it
/// at the `name-value` enum rule instead. Written out as literals so the
/// substitution in `tool_call_grammar` is exact — and so a llama.cpp
/// upgrade that changes the emitted shape fails loudly there rather than
/// silently leaving names unconstrained.
const NAME_KV_STRING_RULE: &str = r#"name-kv ::= "\"name\"" space ":" space string"#;
const NAME_KV_ENUM_RULE: &str = r#"name-kv ::= "\"name\"" space ":" space name-value"#;

/// Assembles the complete GBNF for one tool-call response from
/// `json_schema_to_grammar`'s output: demotes its `root` to a sub-rule,
/// wraps it in the literal `<tool_call>` tags (matching Qwen's template
/// newlines) as the real root, and — when `allowed_names` is set — rewires
/// the `name` field from "any JSON string" to a generated enum alternation
/// (`name-value ::= ("\"CreatePlan\"" | "\"AddStep\"" | ...) space`), which
/// is how the plan engine's per-state tool gating is enforced at the
/// sampler. Pure string assembly, unit-tested without a model.
fn tool_call_grammar(
    json_grammar: &str,
    allowed_names: Option<&[&str]>,
) -> Result<String, InferenceError> {
    let json_grammar = json_grammar.replacen("root ::=", "tool-json ::=", 1);
    let json_grammar = match allowed_names {
        None => json_grammar,
        Some([]) => {
            // An empty enum is an unsatisfiable grammar — a host bug,
            // surfaced here rather than compiled into a sampler that can
            // never produce a token.
            return Err(InferenceError::Backend(
                "tool_call_grammar: allowed_names must not be empty".to_string(),
            ));
        }
        Some(names) => {
            if !json_grammar.contains(NAME_KV_STRING_RULE) {
                return Err(InferenceError::Backend(format!(
                    "tool_call_grammar: expected name rule {NAME_KV_STRING_RULE:?} not found — json_schema_to_grammar output shape changed"
                )));
            }
            let alternation = names
                .iter()
                .map(|n| format!("\"\\\"{n}\\\"\""))
                .collect::<Vec<_>>()
                .join(" | ");
            format!(
                "{}\nname-value ::= ({alternation}) space",
                json_grammar.replacen(NAME_KV_STRING_RULE, NAME_KV_ENUM_RULE, 1)
            )
        }
    };
    Ok(format!(
        "{json_grammar}\nroot ::= \"<tool_call>\\n\" tool-json \"\\n</tool_call>\""
    ))
}

/// Owns the single loaded model + backend for the whole app (research.md
/// §24 — exactly one inference worker, one context, at any moment).
pub struct InferenceEngine {
    backend: LlamaBackend,
    model: LlamaModel,
    n_threads: i32,
}

impl InferenceEngine {
    pub fn load(model_path: &Path, n_threads: i32) -> Result<Self, InferenceError> {
        let backend = LlamaBackend::init().map_err(|e| InferenceError::Backend(e.to_string()))?;
        let model_params = LlamaModelParams::default();
        let model = LlamaModel::load_from_file(&backend, model_path, &model_params)
            .map_err(|e| InferenceError::ModelLoad(e.to_string()))?;
        Ok(Self { backend, model, n_threads })
    }

    /// Renders role-tagged `messages` through the model's own chat template
    /// — baked into the GGUF's metadata by whoever converted it — into the
    /// exact prompt string the model was instruction-tuned on (special
    /// tokens included, e.g. ChatML's `<|im_start|>`/`<|im_end|>`). Without
    /// this, a chat-tuned model like Qwen only ever sees raw concatenated
    /// text it was never trained to continue as a conversation, which is
    /// what produces the "ignores the question" / "rambles" / "answers as
    /// if completing a document" behavior — not necessarily the model being
    /// too small. Falls back to the generic "chatml" template name (the
    /// most common convention) if the model has no template of its own.
    pub fn render_chat_prompt(&self, messages: &[ChatMessage]) -> Result<String, InferenceError> {
        let tmpl = match self.model.chat_template(None) {
            Ok(t) => t,
            Err(_) => LlamaChatTemplate::new("chatml")
                .map_err(|e| InferenceError::Backend(format!("no usable chat template: {e}")))?,
        };

        let llama_messages: Vec<LlamaChatMessage> = messages
            .iter()
            .map(|m| LlamaChatMessage::new(m.role.clone(), m.text()))
            .collect::<Result<_, _>>()
            .map_err(|e| InferenceError::Backend(e.to_string()))?;

        self.model
            .apply_chat_template(&tmpl, &llama_messages, true)
            .map_err(|e| InferenceError::Backend(e.to_string()))
    }

    /// The model's configured context window, in tokens
    /// (010-context-window-management) — currently always
    /// `CONTEXT_WINDOW_TOKENS`, since `generate()` builds every context with
    /// that same fixed `n_ctx`. Exposed as a method (rather than callers
    /// reading the constant directly) so a future per-model context size
    /// would only need to change here.
    pub fn context_window(&self) -> u32 {
        CONTEXT_WINDOW_TOKENS
    }

    /// Tokenizes `text` and returns its token count, without decoding —
    /// cheap enough to call before every generation to check the prompt
    /// against the context budget (010-context-window-management). Shares
    /// the exact tokenization `generate()` itself uses, so a count from
    /// this function always matches what `generate()` would actually
    /// decode for the same string.
    pub fn count_tokens(&self, text: &str) -> Result<usize, InferenceError> {
        let tokens = self
            .model
            .str_to_token(text, AddBos::Always)
            .map_err(|e| InferenceError::Backend(e.to_string()))?;
        Ok(tokens.len())
    }

    /// Trims `messages` down to what actually fits this model's context
    /// window, verified against the real chat template + tokenizer rather
    /// than trusted as an estimate. `context::fit_to_budget`'s per-message
    /// sums (counted on each message's own text, not its rendered form) are
    /// a fast first pass that doesn't account for the chat template's
    /// per-turn overhead (role tags, etc.), so this renders the candidate
    /// and re-checks, dropping one more message from the oldest end of the
    /// kept (non-pinned) suffix and retrying if it's still over — the same
    /// "render then count, trust the real number" discipline
    /// `context::usage_from_history` already uses elsewhere, rather than
    /// trusting the estimate to be exact. `reserve` is subtracted from the
    /// window up front to leave room for output tokens that don't exist yet
    /// to count.
    pub fn fit_to_context(
        &self,
        messages: &[ChatMessage],
        pinned_prefix: usize,
        reserve: u32,
    ) -> Result<Vec<ChatMessage>, InferenceError> {
        let budget = self.context_window().saturating_sub(reserve);
        let costs: Vec<u32> = messages
            .iter()
            .map(|m| self.count_tokens(&m.text()).map(|n| n as u32))
            .collect::<Result<_, _>>()?;

        let mut candidate = crate::context::fit_to_budget(messages, &costs, budget, pinned_prefix);
        loop {
            let rendered = self.render_chat_prompt(&candidate)?;
            let actual = self.count_tokens(&rendered)? as u32;
            let pinned = pinned_prefix.min(candidate.len());
            if actual <= budget || candidate.len() <= pinned {
                return Ok(candidate);
            }
            candidate.remove(pinned);
        }
    }

    /// Builds the grammar-constrained sampler that guarantees any
    /// `<tool_call>` the model starts producing completes as Qwen's own
    /// trained tool-call shape: `{"name": string, "arguments": object}`
    /// JSON inside `<tool_call></tool_call>` tags — lazy
    /// (`LlamaSampler::grammar_lazy`), so it only activates once the model
    /// actually starts down that path; a plain-text final answer is
    /// completely unconstrained the whole time. Once the closing tag is
    /// produced the grammar is complete, which also forecloses the
    /// observed run-on failure of appending a second call in the same
    /// response.
    ///
    /// `allowed_names`, when set, additionally constrains the `name` field
    /// to that enum (see `tool_call_grammar`): the plan engine's per-state
    /// tool gating now lives HERE, at the sampler, because its system
    /// prompt is a byte-stable union of both states' tools — a tool outside
    /// the current state's set must be unsamplable, not merely
    /// un-advertised. `None` keeps the historical static schema (any name),
    /// which `dispatch::execute` already handles gracefully for
    /// unrecognized names.
    fn tool_call_grammar_sampler(
        &self,
        required: bool,
        allowed_names: Option<&[&str]>,
    ) -> Result<LlamaSampler, InferenceError> {
        let json_grammar = json_schema_to_grammar(&tool_call_schema().to_string())
            .map_err(|e| InferenceError::Backend(e.to_string()))?;
        let grammar_str = tool_call_grammar(&json_grammar, allowed_names)?;
        if required {
            LlamaSampler::grammar(&self.model, &grammar_str, "root")
                .map_err(|e| InferenceError::Backend(e.to_string()))
        } else {
            LlamaSampler::grammar_lazy(
                &self.model,
                &grammar_str,
                "root",
                [b"<tool_call>".as_slice()],
                &[],
            )
            .map_err(|e| InferenceError::Backend(e.to_string()))
        }
    }

    /// Opens a fresh persistent inference context whose KV cache can be
    /// reused across `PromptSession::generate` calls (prefix reuse). A host
    /// holds ONE session for the length of an agent turn: each turn's prompt
    /// extends the previous one's, so all but the newest slice of the
    /// (large, growing) history is served from the KV cache rather than
    /// re-decoded — the difference between O(N^2) and O(N) prefill across a
    /// turn. Every context is built with the same fixed `n_ctx`
    /// (`CONTEXT_WINDOW_TOKENS`) and thread counts `generate` has always used,
    /// so a session is behaviourally identical to the old per-call context
    /// on its first call and only diverges (favourably) on later calls.
    pub fn new_session(&self) -> Result<PromptSession<'_>, InferenceError> {
        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(CONTEXT_WINDOW_TOKENS))
            .with_n_threads(self.n_threads)
            .with_n_threads_batch(self.n_threads);
        let ctx = self
            .model
            .new_context(&self.backend, ctx_params)
            .map_err(|e| InferenceError::Backend(e.to_string()))?;
        Ok(PromptSession {
            ctx,
            cached: Vec::new(),
        })
    }

    /// Generation used for tier-2 summarization, subagents (until
    /// migrated), and integration tests, invoking `on_token` as each token
    /// is produced so the caller can stream progress rather than waiting
    /// for the full response. `prompt` is expected to already be
    /// chat-template-rendered (see `render_chat_prompt`) — this function
    /// just tokenizes and decodes whatever string it's given. `tool_calls`
    /// gates the grammar-constrained sampler above — tier-2 summarization
    /// never sets it, since it neither wants (nor should be able to
    /// produce) a `<tool_call>` response.
    ///
    /// Implemented as a convenience wrapper over a throwaway `PromptSession`:
    /// it decodes the whole prompt from a clean context every call (no prefix
    /// reuse), which is exactly right for one-shot callers. There is
    /// deliberately only ONE decode implementation —
    /// `PromptSession::generate` — so this path and the prefix-reusing agent
    /// path can never drift in sampler setup, streaming, or cancellation
    /// semantics.
    /// `allowed_tools` (only meaningful with `Allow`/`Require`) further
    /// constrains the sampled tool `name` to that enum — the plan engine's
    /// per-state gating; `None` keeps the name unconstrained.
    pub fn generate(
        &self,
        prompt: &str,
        max_tokens: i32,
        tool_calls: ToolCallMode,
        allowed_tools: Option<&[&str]>,
        on_token: impl FnMut(&str),
        should_cancel: impl FnMut() -> bool,
    ) -> Result<String, InferenceError> {
        self.new_session()?.generate(
            self,
            prompt,
            max_tokens,
            tool_calls,
            allowed_tools,
            on_token,
            should_cancel,
        )
    }
}

/// A persistent inference context whose KV cache is reused across `generate`
/// calls. Created by `InferenceEngine::new_session`; a host (`RealBackend`)
/// holds one for the length of a single agent turn.
///
/// The KV cache holds a token sequence at positions `[0, cached.len())` on
/// llama.cpp sequence 0. Each `generate` call finds the longest prefix its
/// new prompt shares with `cached`, drops the KV entries past that prefix,
/// decodes only the divergent suffix, and samples as usual — so a turn that
/// re-feeds the same growing history pays to decode only each turn's newest
/// slice (the last tool exchange), not the whole history every time.
pub struct PromptSession<'m> {
    ctx: LlamaContext<'m>,
    /// The token sequence currently materialized in the KV cache (sequence
    /// 0, positions `[0, cached.len())`). Includes both the prompt tokens
    /// AND the tokens this session itself sampled, so the next call's prefix
    /// comparison also covers this call's own output — which the agent loop
    /// re-feeds verbatim as part of the next prompt. Kept exactly in step
    /// with the KV cache on every path: truncated the instant the KV is
    /// truncated, extended only once a decode has actually landed, and
    /// cleared entirely on any decode failure (a failed decode can leave the
    /// KV in an undefined state, so the safe move is to force the next call
    /// to re-prefill from scratch rather than trust a stale prefix —
    /// correctness beats reuse).
    cached: Vec<LlamaToken>,
}

// SAFETY: `LlamaContext` is `!Send` solely because it wraps a raw
// `NonNull<llama_context>` pointer (llama-cpp-2 marks `LlamaModel` itself
// `Send + Sync`, but conservatively leaves the context auto-derived
// `!Send`). The underlying llama.cpp context is plain heap state (KV-cache
// and logits buffers) with no thread affinity — `llama_decode` spins up its
// own threadpool per call and does not rely on the caller's thread identity
// — so it is safe to *move* between threads. The only real hazard is
// concurrent *use* from two threads, and a `PromptSession` structurally
// cannot be used concurrently: it is neither `Sync` nor `Clone`, it is owned
// by exactly one agent turn, and every method takes `&mut self`. That turn
// holds the engine `Mutex` for its whole duration, so at most one thread
// ever touches the context. Implementing `Send` (not `Sync`) lets tokio
// migrate the owning task between worker threads across `.await` points —
// the only reason this is needed, since a `PromptSession` now lives across
// the agent loop's awaits — which is a sequential move, never a data race.
unsafe impl Send for PromptSession<'_> {}

impl PromptSession<'_> {
    /// Like `InferenceEngine::generate`, but reuses the KV prefix shared with
    /// the previous call rather than decoding the whole prompt from scratch.
    /// See the struct doc comment for the overall strategy.
    ///
    /// `engine` is the same engine `new_session` was called on — it is
    /// threaded back in here (rather than stored on the session) because the
    /// borrow that produced `self.ctx` already holds the model immutably, and
    /// re-passing `&InferenceEngine` for tokenization / piece-decoding /
    /// grammar-sampler construction sidesteps a second, self-referential
    /// borrow of the model through the context. A FRESH sampler chain is
    /// built per call (grammar mode is per-call via `tool_calls`, and the
    /// name-enum gate is per-call via `allowed_tools` — the plan engine
    /// passes a different tool set as its state changes; determinism is
    /// per-call via `generation_seed`) — a sampler is never reused across
    /// calls.
    #[allow(clippy::too_many_arguments)]
    pub fn generate(
        &mut self,
        engine: &InferenceEngine,
        prompt: &str,
        max_tokens: i32,
        tool_calls: ToolCallMode,
        allowed_tools: Option<&[&str]>,
        mut on_token: impl FnMut(&str),
        mut should_cancel: impl FnMut() -> bool,
    ) -> Result<String, InferenceError> {
        let tokens = engine
            .model
            .str_to_token(prompt, AddBos::Always)
            .map_err(|e| InferenceError::Backend(e.to_string()))?;

        // How much of this prompt is already materialized in the KV cache
        // from the previous call. Capped at `tokens.len() - 1` so at least
        // one token is always decoded below: if the entire prompt were
        // already cached (`common == tokens.len()`, e.g. the prompt is a
        // prefix of what we hold), there would be no suffix to decode and
        // thus no fresh logits at the final position to sample the first
        // output token from — so we deliberately re-decode the last prompt
        // token to regenerate them.
        let common =
            common_prefix_len(&tokens, &self.cached).min(tokens.len().saturating_sub(1));

        // Drop the KV entries past the shared prefix — range `[common, end)`
        // on sequence 0 (`p1 = None` means "to the end"; llama.cpp's range is
        // half-open `[p0, p1)`), leaving `[0, common)` intact for reuse.
        // Truncate `cached` in the same breath so the two never disagree.
        let common = match self.ctx.clear_kv_cache_seq(Some(0), Some(common as u32), None) {
            Ok(true) => common,
            // `Ok(false)` means the backend REFUSED the partial-range
            // removal (llama.cpp documents partial sequence removals as
            // fallible on some cache implementations; full removals always
            // succeed). Stale KV entries past `common` may have survived —
            // reusing the prefix would silently sample against them, so
            // degrade to a full clear and a re-prefill from position 0:
            // correctness beats reuse, and the cost is one turn's worth of
            // whole-prompt prefill, exactly what a fresh session would pay.
            Ok(false) => {
                self.ctx.clear_kv_cache();
                self.cached.clear();
                0
            }
            Err(e) => {
                self.cached.clear();
                return Err(InferenceError::Backend(e.to_string()));
            }
        };
        self.cached.truncate(common);

        // Prefill only the divergent suffix `[common, tokens.len())`, with
        // absolute positions continuing from `common` (n_past = common), so
        // the reused prefix's KV entries keep their original positions. Same
        // fixed-capacity chunked prefill the whole-prompt path uses (a
        // suffix can still exceed BATCH_CAPACITY); only the very last token
        // overall gets `logits = true`, since sampling only needs the final
        // position's distribution. On any failure `cached` is cleared (the
        // KV may be partial), forcing a clean re-prefill next call.
        let mut batch = LlamaBatch::new(BATCH_CAPACITY, 1);
        let last_idx = tokens.len() - 1;
        for chunk in prefill_chunks_from(common, tokens.len(), BATCH_CAPACITY) {
            batch.clear();
            for i in chunk.clone() {
                if let Err(e) = batch.add(tokens[i], i as i32, &[0], i == last_idx) {
                    self.cached.clear();
                    return Err(InferenceError::Backend(e.to_string()));
                }
            }
            if let Err(e) = self.ctx.decode(&mut batch) {
                self.cached.clear();
                return Err(InferenceError::Backend(e.to_string()));
            }
            // Now durably in the KV cache — safe to record as cached.
            self.cached.extend_from_slice(&tokens[chunk]);
        }

        // A plain greedy (always-argmax) sampler tends to degenerate into
        // repeated loops on chat-tuned models, which reads as "confused" or
        // "too raw" even once the chat template is correct — this chain
        // matches the defaults most chat-completion APIs use: a repeat
        // penalty over recent tokens, then temperature + top-k/top-p to
        // pick among the remaining reasonable candidates.
        let seed = generation_seed();
        // The grammar sampler, when present, goes first in the chain —
        // matching upstream llama.cpp's own convention of masking to
        // grammar-legal tokens before penalty/temperature/top-k/top-p
        // shaping ever sees the distribution.
        let mut chain = Vec::with_capacity(6);
        match tool_calls {
            ToolCallMode::Forbid => {}
            ToolCallMode::Allow => {
                chain.push(engine.tool_call_grammar_sampler(false, allowed_tools)?)
            }
            ToolCallMode::Require => {
                chain.push(engine.tool_call_grammar_sampler(true, allowed_tools)?)
            }
        }
        // Qwen3-*-2507's own recommended sampling (model card): temp 0.7,
        // top-p 0.8, top-k 20, min-p 0 — with presence-penalty for
        // repetition control instead of repeat-penalty (repeat-penalty
        // taxes the tokens JSON repeats BY DESIGN — braces, quotes, key
        // names — and inside an active grammar it can only distort
        // argument content).
        chain.extend([
            LlamaSampler::penalties(64, 1.0, 0.0, 1.0),
            LlamaSampler::top_k(20),
            LlamaSampler::top_p(0.8, 1),
            LlamaSampler::min_p(0.0, 1),
            LlamaSampler::temp(0.7),
            LlamaSampler::dist(seed),
        ]);
        let mut sampler = LlamaSampler::chain_simple(chain);
        let mut output = String::new();
        let mut decoder = encoding_rs::UTF_8.new_decoder();

        // Starts from `tokens.len()` (the full prompt length), not
        // `batch.n_tokens()` — that now only reflects however many tokens
        // the *last prefill chunk* held, not the full prompt, since prefill
        // runs in chunks; using it here would silently restart position
        // numbering partway through the prompt.
        for n_cur in (tokens.len() as i32..).take(max_tokens as usize) {
            // Checked between decode steps (research.md §24 / tasks.md
            // T018), not just before starting — a cancellation should stop
            // generation promptly rather than only at the next request.
            if should_cancel() {
                break;
            }
            let token = sampler.sample(&self.ctx, batch.n_tokens() - 1);
            if engine.model.is_eog_token(token) {
                break;
            }
            let piece = engine
                .model
                .token_to_piece(token, &mut decoder, true, None)
                .unwrap_or_default();
            on_token(&piece);
            output.push_str(&piece);

            batch.clear();
            if let Err(e) = batch.add(token, n_cur, &[0], true) {
                self.cached.clear();
                return Err(InferenceError::Backend(e.to_string()));
            }
            if let Err(e) = self.ctx.decode(&mut batch) {
                self.cached.clear();
                return Err(InferenceError::Backend(e.to_string()));
            }
            // Decoded into the KV — record it so the next call's prefix
            // comparison covers this generated token too.
            self.cached.push(token);
        }

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_prompt_has_no_chunks() {
        assert_eq!(prefill_chunks_from(0, 0, 512), Vec::<std::ops::Range<usize>>::new());
    }

    #[test]
    fn prompt_under_capacity_is_a_single_chunk() {
        assert_eq!(prefill_chunks_from(0, 100, 512), vec![0..100]);
    }

    #[test]
    fn prompt_exactly_at_capacity_is_a_single_chunk() {
        assert_eq!(prefill_chunks_from(0, 512, 512), vec![0..512]);
    }

    #[test]
    fn prompt_one_token_over_capacity_splits_into_two_chunks() {
        // The exact reported bug: a 513-token prompt against a 512-capacity
        // batch used to overflow on the 513th `batch.add()` call
        // (BatchAddError::InsufficientSpace(512), surfaced to users as
        // "Insufficient Space of 512") instead of starting a new chunk.
        assert_eq!(prefill_chunks_from(0, 513, 512), vec![0..512, 512..513]);
    }

    #[test]
    fn prompt_several_times_capacity_splits_evenly() {
        assert_eq!(prefill_chunks_from(0, 1024, 512), vec![0..512, 512..1024]);
    }

    #[test]
    fn prompt_several_times_capacity_plus_remainder() {
        assert_eq!(
            prefill_chunks_from(0, 1025, 512),
            vec![0..512, 512..1024, 1024..1025]
        );
    }

    #[test]
    fn every_token_position_is_covered_exactly_once() {
        for n_tokens in [1, 511, 512, 513, 1000, 2048, 2049] {
            let chunks = prefill_chunks_from(0, n_tokens, 512);
            let covered: Vec<usize> = chunks.iter().flat_map(|r| r.clone()).collect();
            let expected: Vec<usize> = (0..n_tokens).collect();
            assert_eq!(covered, expected, "n_tokens={n_tokens}");
            assert!(
                chunks.iter().all(|r| r.len() <= 512),
                "a chunk exceeded batch capacity for n_tokens={n_tokens}"
            );
        }
    }

    #[test]
    fn common_prefix_len_of_equal_slices_is_the_full_length() {
        let a = vec![LlamaToken(1), LlamaToken(2), LlamaToken(3)];
        let b = vec![LlamaToken(1), LlamaToken(2), LlamaToken(3)];
        assert_eq!(common_prefix_len(&a, &b), 3);
    }

    #[test]
    fn common_prefix_len_of_disjoint_slices_is_zero() {
        let a = vec![LlamaToken(1), LlamaToken(2)];
        let b = vec![LlamaToken(9), LlamaToken(8)];
        assert_eq!(common_prefix_len(&a, &b), 0);
    }

    #[test]
    fn common_prefix_len_when_one_is_a_prefix_of_the_other_is_the_shorter_length() {
        let short = vec![LlamaToken(1), LlamaToken(2)];
        let long = vec![LlamaToken(1), LlamaToken(2), LlamaToken(3), LlamaToken(4)];
        // Symmetric: whichever way round, the answer is the shared run length.
        assert_eq!(common_prefix_len(&short, &long), 2);
        assert_eq!(common_prefix_len(&long, &short), 2);
    }

    #[test]
    fn common_prefix_len_stops_at_the_first_divergence() {
        let a = vec![LlamaToken(1), LlamaToken(2), LlamaToken(3), LlamaToken(4)];
        let b = vec![LlamaToken(1), LlamaToken(2), LlamaToken(99), LlamaToken(4)];
        assert_eq!(common_prefix_len(&a, &b), 2);
    }

    #[test]
    fn common_prefix_len_with_an_empty_slice_is_zero() {
        let a = vec![LlamaToken(1), LlamaToken(2)];
        let empty: Vec<LlamaToken> = Vec::new();
        assert_eq!(common_prefix_len(&a, &empty), 0);
        assert_eq!(common_prefix_len(&empty, &a), 0);
    }

    #[test]
    fn prefill_chunks_from_a_nonzero_start_covers_only_the_suffix() {
        // The suffix a PromptSession decodes after reusing a KV prefix of
        // length 3: positions are absolute (continue from 3), not re-based.
        assert_eq!(prefill_chunks_from(3, 10, 512), vec![3..10]);
        // A suffix longer than one batch splits, still absolute-positioned.
        assert_eq!(prefill_chunks_from(500, 1025, 512), vec![500..1012, 1012..1025]);
        // A one-token suffix (the degenerate `common == len - 1` case).
        assert_eq!(prefill_chunks_from(9, 10, 512), vec![9..10]);
    }

    // --- name-enum GBNF gating (stable-prefix prompt architecture) ---
    //
    // Pure string assembly, testable without a model:
    // `json_schema_to_grammar` is a model-free llama.cpp function, and
    // `tool_call_grammar` is this module's own transform on its output.

    #[test]
    fn tool_call_grammar_without_names_wraps_the_json_object_in_tool_call_tags() {
        let json_grammar = json_schema_to_grammar(&tool_call_schema().to_string()).unwrap();
        let grammar = tool_call_grammar(&json_grammar, None).unwrap();
        assert!(grammar.contains("tool-json ::="));
        assert!(grammar.contains("root ::= \"<tool_call>\\n\" tool-json \"\\n</tool_call>\""));
        // Unconstrained: the name field is still a plain JSON string.
        assert!(grammar.contains("name-kv ::= \"\\\"name\\\"\" space \":\" space string"));
    }

    #[test]
    fn tool_call_grammar_with_allowed_names_constrains_the_name_field_to_an_enum() {
        let json_grammar = json_schema_to_grammar(&tool_call_schema().to_string()).unwrap();
        let grammar = tool_call_grammar(&json_grammar, Some(&["CreatePlan", "AddStep"])).unwrap();
        assert!(grammar.contains("name-kv ::= \"\\\"name\\\"\" space \":\" space name-value"));
        assert!(grammar.contains(r#"name-value ::= ("\"CreatePlan\"" | "\"AddStep\"") space"#));
        assert!(
            !grammar.contains("name-kv ::= \"\\\"name\\\"\" space \":\" space string"),
            "the unconstrained name rule must be gone once an enum is requested"
        );
        // Everything else survives untouched.
        assert!(grammar.contains("arguments-kv ::="));
        assert!(grammar.contains("root ::= \"<tool_call>\\n\" tool-json \"\\n</tool_call>\""));
    }

    #[test]
    fn tool_call_grammar_with_a_single_allowed_name_is_a_one_literal_enum() {
        let json_grammar = json_schema_to_grammar(&tool_call_schema().to_string()).unwrap();
        let grammar = tool_call_grammar(&json_grammar, Some(&["FinishTask"])).unwrap();
        assert!(grammar.contains(r#"name-value ::= ("\"FinishTask\"") space"#));
    }

    #[test]
    fn tool_call_grammar_rejects_an_empty_allowed_names_list() {
        // An empty enum would be an unsatisfiable grammar -- a host bug,
        // surfaced loudly rather than compiled into a sampler that can
        // never produce a token.
        let json_grammar = json_schema_to_grammar(&tool_call_schema().to_string()).unwrap();
        assert!(tool_call_grammar(&json_grammar, Some(&[])).is_err());
    }

    #[test]
    fn tool_call_grammar_errors_when_the_name_rule_shape_is_missing() {
        // Guards against a llama.cpp upgrade changing json_schema_to_grammar's
        // output shape: gating must fail loudly, never silently un-gate.
        assert!(tool_call_grammar("root ::= something-else", Some(&["Read"])).is_err());
    }

    #[test]
    fn generation_seed_honors_the_env_var_and_falls_back_to_entropy() {
        // Serial-unsafe env mutation is confined to this one test.
        std::env::set_var("DOCE_GEN_SEED", "42");
        assert_eq!(generation_seed(), 42);
        std::env::set_var("DOCE_GEN_SEED", "not-a-number");
        let a = generation_seed();
        let b = generation_seed();
        // Entropy fallback: not asserted unequal (could collide), just valid.
        let _ = (a, b);
        std::env::remove_var("DOCE_GEN_SEED");
    }
}
