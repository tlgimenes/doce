use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::json_schema_to_grammar;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaChatMessage, LlamaChatTemplate, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
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
/// calls via `prefill_chunks`, not one `batch.add()` loop over every token.
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
pub const CONTEXT_WINDOW_TOKENS: u32 = 8192;

/// Splits `n_tokens` positions into `[start, end)` ranges of at most
/// `batch_capacity` each, in order — the sequence of chunks `generate()`
/// prefills the prompt in. Pure and independent of llama.cpp so the
/// off-by-one-prone boundary math (the exact bug this fixes: a prompt of
/// precisely `batch_capacity + 1` tokens) can be unit-tested without a real
/// model.
fn prefill_chunks(n_tokens: usize, batch_capacity: usize) -> Vec<std::ops::Range<usize>> {
    (0..n_tokens)
        .step_by(batch_capacity)
        .map(|start| start..(start + batch_capacity).min(n_tokens))
        .collect()
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
/// (plain chat, summarization). `Allow`: lazy grammar — constrains output
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

/// Owns the single loaded model + backend for the whole app (research.md
/// §24 — exactly one inference worker, one context, at any moment).
pub struct InferenceEngine {
    backend: LlamaBackend,
    model: LlamaModel,
}

impl InferenceEngine {
    pub fn load(model_path: &Path, n_threads: i32) -> Result<Self, InferenceError> {
        let backend = LlamaBackend::init().map_err(|e| InferenceError::Backend(e.to_string()))?;
        let model_params = LlamaModelParams::default();
        let model = LlamaModel::load_from_file(&backend, model_path, &model_params)
            .map_err(|e| InferenceError::ModelLoad(e.to_string()))?;
        let _ = n_threads; // applied when building the context per-generation
        Ok(Self { backend, model })
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
    /// response. Deliberately doesn't constrain `name` to an enum of
    /// currently-available tools — `dispatch::execute` already handles an
    /// unrecognized tool name gracefully as an ordinary tool-error result
    /// fed back into the loop, so keeping this schema static avoids
    /// threading a dynamic tool list through every `generate()` call for a
    /// marginal benefit.
    fn tool_call_grammar_sampler(&self, required: bool) -> Result<LlamaSampler, InferenceError> {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "arguments": { "type": "object" }
            },
            "required": ["name", "arguments"]
        });
        let json_grammar = json_schema_to_grammar(&schema.to_string())
            .map_err(|e| InferenceError::Backend(e.to_string()))?;
        // json_schema_to_grammar's output defines `root` for the bare JSON
        // object; demote that to a sub-rule and wrap it in the literal
        // tags (matching Qwen's template newlines) as the real root.
        let json_grammar = json_grammar.replacen("root ::=", "tool-json ::=", 1);
        let grammar_str = format!(
            "{json_grammar}\nroot ::= \"<tool_call>\\n\" tool-json \"\\n</tool_call>\""
        );
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

    /// Generation used for the chat path (User Story 2), the agent tool-use
    /// loop (User Story 3), and integration tests, invoking `on_token` as
    /// each token is produced so the caller can emit `assistant-token`
    /// events in real time rather than waiting for the full response.
    /// `prompt` is expected to already be chat-template-rendered (see
    /// `render_chat_prompt`) — this function just tokenizes and decodes
    /// whatever string it's given. `tool_calls` gates the
    /// grammar-constrained sampler above — the plain chat path and
    /// tier-2 summarization never set it, since neither ever wants (or
    /// should be able to produce) a `<tool_call>` response.
    pub fn generate(
        &self,
        prompt: &str,
        max_tokens: i32,
        tool_calls: ToolCallMode,
        mut on_token: impl FnMut(&str),
        mut should_cancel: impl FnMut() -> bool,
    ) -> Result<String, InferenceError> {
        let ctx_params =
            LlamaContextParams::default().with_n_ctx(NonZeroU32::new(CONTEXT_WINDOW_TOKENS));
        let mut ctx = self
            .model
            .new_context(&self.backend, ctx_params)
            .map_err(|e| InferenceError::Backend(e.to_string()))?;

        let tokens = self
            .model
            .str_to_token(prompt, AddBos::Always)
            .map_err(|e| InferenceError::Backend(e.to_string()))?;

        // The batch is a fixed-size client buffer (BATCH_CAPACITY slots) —
        // llama.cpp can't decode more tokens than that in one call, so a
        // prompt longer than BATCH_CAPACITY (system prompt + tool list +
        // growing conversation history routinely exceeds it in agent mode)
        // must be prefilled in sequential chunks, not one `batch.add()` loop
        // over every token (that overflows with `BatchAddError::
        // InsufficientSpace`, surfaced to users as "llama.cpp backend error:
        // Insufficient Space of 512"). Only the very last token overall gets
        // `logits = true`, since sampling only needs the final position's
        // distribution.
        let mut batch = LlamaBatch::new(BATCH_CAPACITY, 1);
        let last_idx = tokens.len() - 1;
        for chunk in prefill_chunks(tokens.len(), BATCH_CAPACITY) {
            batch.clear();
            for i in chunk {
                batch
                    .add(tokens[i], i as i32, &[0], i == last_idx)
                    .map_err(|e| InferenceError::Backend(e.to_string()))?;
            }
            ctx.decode(&mut batch)
                .map_err(|e| InferenceError::Backend(e.to_string()))?;
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
            ToolCallMode::Allow => chain.push(self.tool_call_grammar_sampler(false)?),
            ToolCallMode::Require => chain.push(self.tool_call_grammar_sampler(true)?),
        }
        chain.extend([
            LlamaSampler::penalties(64, 1.1, 0.0, 0.0),
            LlamaSampler::top_k(40),
            LlamaSampler::top_p(0.9, 1),
            LlamaSampler::temp(0.7),
            LlamaSampler::dist(seed),
        ]);
        let mut sampler = LlamaSampler::chain_simple(chain);
        let mut output = String::new();
        let mut decoder = encoding_rs::UTF_8.new_decoder();

        // Starts from `tokens.len()` (the full prompt length), not
        // `batch.n_tokens()` — that now only reflects however many tokens
        // the *last prefill chunk* held, not the full prompt, now that
        // prefill runs in chunks; using it here would silently restart
        // position numbering partway through the prompt.
        for n_cur in (tokens.len() as i32..).take(max_tokens as usize) {
            // Checked between decode steps (research.md §24 / tasks.md
            // T018), not just before starting — a cancellation should stop
            // generation promptly rather than only at the next request.
            if should_cancel() {
                break;
            }
            let token = sampler.sample(&ctx, batch.n_tokens() - 1);
            if self.model.is_eog_token(token) {
                break;
            }
            let piece = self
                .model
                .token_to_piece(token, &mut decoder, true, None)
                .unwrap_or_default();
            on_token(&piece);
            output.push_str(&piece);

            batch.clear();
            batch
                .add(token, n_cur, &[0], true)
                .map_err(|e| InferenceError::Backend(e.to_string()))?;
            ctx.decode(&mut batch)
                .map_err(|e| InferenceError::Backend(e.to_string()))?;
        }

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_prompt_has_no_chunks() {
        assert_eq!(prefill_chunks(0, 512), Vec::<std::ops::Range<usize>>::new());
    }

    #[test]
    fn prompt_under_capacity_is_a_single_chunk() {
        assert_eq!(prefill_chunks(100, 512), vec![0..100]);
    }

    #[test]
    fn prompt_exactly_at_capacity_is_a_single_chunk() {
        assert_eq!(prefill_chunks(512, 512), vec![0..512]);
    }

    #[test]
    fn prompt_one_token_over_capacity_splits_into_two_chunks() {
        // The exact reported bug: a 513-token prompt against a 512-capacity
        // batch used to overflow on the 513th `batch.add()` call
        // (BatchAddError::InsufficientSpace(512), surfaced to users as
        // "Insufficient Space of 512") instead of starting a new chunk.
        assert_eq!(prefill_chunks(513, 512), vec![0..512, 512..513]);
    }

    #[test]
    fn prompt_several_times_capacity_splits_evenly() {
        assert_eq!(prefill_chunks(1024, 512), vec![0..512, 512..1024]);
    }

    #[test]
    fn prompt_several_times_capacity_plus_remainder() {
        assert_eq!(
            prefill_chunks(1025, 512),
            vec![0..512, 512..1024, 1024..1025]
        );
    }

    #[test]
    fn every_token_position_is_covered_exactly_once() {
        for n_tokens in [1, 511, 512, 513, 1000, 2048, 2049] {
            let chunks = prefill_chunks(n_tokens, 512);
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
