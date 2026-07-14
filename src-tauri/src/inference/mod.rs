use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaChatMessage, LlamaChatTemplate, LlamaModel};
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
/// Named and public rather than a bare literal so both the budget/compaction
/// calculations in `crate::context` and any future IPC surface can read the
/// same value, instead of each guessing at (or duplicating) the number.
/// Generation itself runs in the llama-server sidecar (launched with
/// `--ctx-size 20480`, see `inference::server::launch_args`); this budget is
/// kept below that server context on purpose, leaving headroom for the
/// output tokens the server still has to decode on top of the prompt.
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

/// The single seam every NEW prompt-token estimate (restore-output-cap
/// task's `clamp_output_tokens` call sites) routes through, rather than
/// calling `InferenceEngine::count_tokens` directly. Today it's just that:
/// a real tokenizer count, falling back to a `len/4` chars heuristic only if
/// tokenization itself errors. A later task (B4) re-points the body to a
/// pure chars/4 heuristic and drops the `engine` parameter entirely -- kept
/// as one function now specifically so that swap touches one place instead
/// of every call site that estimates a prompt's size.
pub fn token_estimate(engine: &InferenceEngine, text: &str) -> u32 {
    engine
        .count_tokens(text)
        .map(|n| n as u32)
        .unwrap_or_else(|_| (text.len() / 4) as u32)
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

/// Owns the single loaded model + backend for the whole app (research.md
/// §24 — exactly one inference worker, one context, at any moment).
pub struct InferenceEngine {
    /// The llama.cpp backend init guard, kept alive for the engine's whole
    /// lifetime: dropping it deinitializes the global backend and would
    /// invalidate `model`. No longer read directly now that the in-process
    /// generation path (which built decode contexts from it) was removed in
    /// the llama-server cutover — `render_chat_prompt`/`count_tokens` touch
    /// only `model` — but it must still be held.
    #[allow(dead_code)]
    backend: LlamaBackend,
    model: LlamaModel,
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
        // only tokenizes + renders the chat template for context MEASUREMENT
        // — all generation lives in the llama-server sidecar. So load just
        // the vocabulary + metadata (which includes the
        // `tokenizer.chat_template` string that drives `render_chat_prompt`
        // and `dialect` detection), NOT the ~2.7 GB of weights, and skip the
        // GPU entirely. The sidecar (built with Metal by
        // scripts/build-llama-server.sh) owns all GPU inference.
        let model_params = LlamaModelParams::default().with_vocab_only(true);
        let model = LlamaModel::load_from_file(&backend, model_path, &model_params)
            .map_err(|e| InferenceError::ModelLoad(e.to_string()))?;
        let dialect = model
            .meta_val_str("tokenizer.chat_template")
            .map(|t| ToolDialect::detect(&t))
            .unwrap_or_default();
        Ok(Self {
            backend,
            model,
            dialect,
        })
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
            .map(|m| LlamaChatMessage::new(m.role.clone(), m.text_for(self.dialect)))
            .collect::<Result<_, _>>()
            .map_err(|e| InferenceError::Backend(e.to_string()))?;

        let mut rendered = self
            .model
            .apply_chat_template(&tmpl, &llama_messages, true)
            .map_err(|e| InferenceError::Backend(e.to_string()))?;
        // Thinking-models design: thinking templates open the reasoning
        // block IN the generation prompt (MiniCPM5's enable_thinking=true
        // renders "<think>\n" after the assistant header; Qwen3-Thinking's
        // template does the same unconditionally). llama.cpp's template
        // pattern-matcher can't render that branch, so this is the ONE
        // template feature hand-rendered here — without it the model
        // starts cold in a state it was never trained for (observed
        // 2026-07-14: degenerate asterisk-run "reasoning" from MiniCPM5).
        // The Require grammar's think-prefix keeps its opening tag
        // OPTIONAL for exactly this, and strip_think_blocks handles the
        // resulting orphan close.
        rendered.push_str("<think>\n");
        Ok(rendered)
    }

    /// The model's configured context window, in tokens
    /// (010-context-window-management) — currently always
    /// `CONTEXT_WINDOW_TOKENS`. Generation now lives in the llama-server
    /// sidecar (launched with `--ctx-size 20480`, see
    /// `inference::server::launch_args`); this constant is the in-process
    /// INPUT budget, deliberately kept below the server's context so there's
    /// output headroom, and it's what `crate::context`'s compaction sizes
    /// against. Exposed as a method (rather than callers reading the constant
    /// directly) so a future per-model context size would only need to change
    /// here.
    pub fn context_window(&self) -> u32 {
        CONTEXT_WINDOW_TOKENS
    }

    /// Tokenizes `text` and returns its token count, without decoding —
    /// cheap enough to call before every turn to check the prompt against
    /// the context budget (010-context-window-management). Uses the same
    /// vocabulary the llama-server sidecar loads from the same GGUF, so a
    /// count from this function matches what the server will actually decode
    /// for the same string.
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

    /// The tool-call dialect this model was trained on (detected from its
    /// chat template at load) — the agent loop routes output parsing
    /// through it.
    pub fn dialect(&self) -> ToolDialect {
        self.dialect
    }
}
