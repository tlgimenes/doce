use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::LlamaModel;
use std::path::Path;

pub mod dialect;
pub mod http;
pub mod server;
pub use dialect::ToolDialect;

#[derive(Debug, thiserror::Error)]
pub enum InferenceError {
    #[error("llama.cpp backend error: {0}")]
    Backend(String),
    #[error("model load failed: {0}")]
    ModelLoad(String),
    /// A `http::LlamaServerClient::chat` call was cut short by its
    /// `CancellationToken` — either already cancelled before the request
    /// started, or cancelled mid-stream. Distinct from `Backend` (a real
    /// transport/protocol failure) because a cancelled turn is an
    /// intentional stop, not an error the caller should retry or surface as
    /// a backend fault.
    #[error("inference cancelled")]
    Cancelled,
}

/// The in-process INPUT budget, in tokens (010-context-window-management).
/// DERIVED from `server::SERVER_CTX_SIZE` (the sidecar's `--ctx-size`, the
/// single source of truth for the model's total token window) minus
/// `server::OUTPUT_RESERVE_TOKENS`, so the two can never drift apart —
/// changing the sidecar's context size automatically re-sizes this budget.
/// Named and public rather than a bare literal so both the budget/compaction
/// calculations in `crate::context` and any future IPC surface can read the
/// same value, instead of each guessing at (or duplicating) the number.
/// This budget is kept below the server's context on purpose, leaving
/// headroom for the output tokens the server still has to decode on top of
/// the prompt.
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
pub const CONTEXT_WINDOW_TOKENS: u32 = server::SERVER_CTX_SIZE - server::OUTPUT_RESERVE_TOKENS;

/// Estimates the token count of `text` with a char-based heuristic (qwen-code's
/// approach) — no tokenizer. ASCII text is ~4 chars/token; non-ASCII (CJK,
/// emoji) tokenizes far less efficiently, so it's weighted ~1.1 tokens/char.
/// A deliberate, conservative estimate: the compaction TRIGGER can fire a bit
/// early (safe); request validity is guaranteed structurally by the B1a output
/// clamp, not by this number's exactness.
///
/// The single seam every prompt-token estimate routes through (the
/// `clamp_output_tokens` call sites, the context-usage accounting, the
/// per-turn fit): the in-process llama tokenizer this used to wrap is gone —
/// the llama-server sidecar reports authoritative usage, so an exact local
/// count isn't needed for TRIGGER decisions.
pub fn token_estimate(text: &str) -> u32 {
    let mut ascii = 0usize;
    let mut non_ascii = 0usize;
    for c in text.chars() {
        if c.is_ascii() {
            ascii += 1
        } else {
            non_ascii += 1
        }
    }
    (ascii.div_ceil(4) + (non_ascii * 11).div_ceil(10)) as u32
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
        self.text_for(ToolDialect::HermesJson)
    }

    /// Like [`Self::text`], but replaying tool exchanges in the given
    /// dialect's trained shape (tool-dialects design) — the engine passes
    /// its detected dialect so a MiniCPM history replays as `<function>`
    /// XML, never Hermes JSON.
    pub fn text_for(&self, dialect: ToolDialect) -> String {
        match &self.content {
            MessageContent::Text(s) => s.clone(),
            MessageContent::ToolUse { name, input, .. } => dialect.render_tool_use(name, input),
            // No "Tool result for {tool_name}:" framing -- Qwen's own
            // convention (per its chat template) is just the raw content
            // inside the tags, relying on tool_call/tool_result ordering
            // (never more than one pending at a time in this loop) to
            // establish which tool it came from, not a repeated name in
            // the text itself. Qwen's own template actually wraps with
            // newlines (`\n<tool_response>\n` + content + `\n</tool_response>`)
            // but that's not preserved here -- a single-line wrap instead.
            MessageContent::ToolResult { content, .. } => dialect.render_tool_result(content),
        }
    }
}

/// Owns the loaded backend guard + detected chat dialect for the whole app
/// (research.md §24). Now that token counting is a pure chars/4 estimate
/// (`token_estimate`) and generation lives in the llama-server sidecar, the
/// only thing the in-process engine still exists for is dialect detection at
/// load — a later task (B4b) removes the struct and the `llama-cpp-2`
/// dependency entirely.
pub struct InferenceEngine {
    /// The llama.cpp backend init guard, kept alive for the engine's whole
    /// lifetime: dropping it deinitializes the global backend. Held (never
    /// read) purely so the global backend stays initialized while the app
    /// runs, matching the single-worker invariant.
    #[allow(dead_code)]
    backend: LlamaBackend,
    /// Detected once at load from the GGUF's embedded chat template —
    /// which output convention this model was trained on (tool-dialects
    /// design). Missing/unreadable template keeps the historical Hermes
    /// assumption.
    dialect: ToolDialect,
}

impl InferenceEngine {
    pub fn load(model_path: &Path) -> Result<Self, InferenceError> {
        let backend = LlamaBackend::init().map_err(|e| InferenceError::Backend(e.to_string()))?;
        // Vocab-only load (llama-server cutover): the in-process engine now
        // only reads the GGUF's metadata (specifically the
        // `tokenizer.chat_template` string that drives `dialect` detection),
        // NOT the ~2.7 GB of weights, and skips the GPU entirely. The sidecar
        // (built with Metal by scripts/build-llama-server.sh) owns all GPU
        // inference. The loaded `model` is consumed here just to read that one
        // metadata string and then dropped — nothing downstream needs it now
        // that token counting is a pure chars/4 estimate.
        let model_params = LlamaModelParams::default().with_vocab_only(true);
        let model = LlamaModel::load_from_file(&backend, model_path, &model_params)
            .map_err(|e| InferenceError::ModelLoad(e.to_string()))?;
        let dialect = model
            .meta_val_str("tokenizer.chat_template")
            .map(|t| ToolDialect::detect(&t))
            .unwrap_or_default();
        Ok(Self { backend, dialect })
    }

    /// The tool-call dialect this model was trained on (detected from its
    /// chat template at load) — the agent loop routes output parsing
    /// through it.
    pub fn dialect(&self) -> ToolDialect {
        self.dialect
    }
}

#[cfg(test)]
mod tests {
    use super::token_estimate;

    #[test]
    fn empty_text_estimates_zero_tokens() {
        assert_eq!(token_estimate(""), 0);
    }

    #[test]
    fn four_ascii_chars_estimate_one_token() {
        assert_eq!(token_estimate("abcd"), 1);
    }

    #[test]
    fn ascii_divides_by_four_rounding_up() {
        assert_eq!(token_estimate(&"a".repeat(400)), 100);
        // 401 ASCII chars -> ceil(401/4) = 101 (the div_ceil, not a floor).
        assert_eq!(token_estimate(&"a".repeat(401)), 101);
    }

    #[test]
    fn multibyte_text_weighs_above_a_plain_char_count_over_four() {
        // Non-ASCII is weighted ~1.1 tokens/char, so a multibyte string
        // estimates AT LEAST its char count (far above len/4) -- the
        // deliberately conservative side of the heuristic.
        let s = "世界";
        assert!(
            token_estimate(s) >= s.chars().count() as u32,
            "multibyte estimate must be >= its char count, got {}",
            token_estimate(s)
        );
        // Concretely: 2 non-ASCII chars -> ceil(2*11/10) = ceil(22/10) = 3.
        assert_eq!(token_estimate(s), 3);
    }
}
