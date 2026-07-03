use llama_cpp_2::context::params::LlamaContextParams;
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

/// A single role-tagged conversation turn. Chat-tuned models like Qwen are
/// trained on turns wrapped in special tokens (e.g. ChatML's
/// `<|im_start|>role\n...<|im_end|>`), not on raw concatenated text — see
/// `InferenceEngine::render_chat_prompt`, which is what actually produces
/// those tokens from a list of these.
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
        }
    }
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
            .map(|m| LlamaChatMessage::new(m.role.clone(), m.content.clone()))
            .collect::<Result<_, _>>()
            .map_err(|e| InferenceError::Backend(e.to_string()))?;

        self.model
            .apply_chat_template(&tmpl, &llama_messages, true)
            .map_err(|e| InferenceError::Backend(e.to_string()))
    }

    /// Generation used for the chat path (User Story 2), the agent tool-use
    /// loop (User Story 3), and integration tests, invoking `on_token` as
    /// each token is produced so the caller can emit `assistant-token`
    /// events in real time rather than waiting for the full response.
    /// `prompt` is expected to already be chat-template-rendered (see
    /// `render_chat_prompt`) — this function just tokenizes and decodes
    /// whatever string it's given.
    pub fn generate(
        &self,
        prompt: &str,
        max_tokens: i32,
        mut on_token: impl FnMut(&str),
        mut should_cancel: impl FnMut() -> bool,
    ) -> Result<String, InferenceError> {
        let ctx_params = LlamaContextParams::default().with_n_ctx(NonZeroU32::new(2048));
        let mut ctx = self
            .model
            .new_context(&self.backend, ctx_params)
            .map_err(|e| InferenceError::Backend(e.to_string()))?;

        let tokens = self
            .model
            .str_to_token(prompt, AddBos::Always)
            .map_err(|e| InferenceError::Backend(e.to_string()))?;

        let mut batch = LlamaBatch::new(512, 1);
        let last_idx = tokens.len() as i32 - 1;
        for (i, token) in tokens.iter().enumerate() {
            batch
                .add(*token, i as i32, &[0], i as i32 == last_idx)
                .map_err(|e| InferenceError::Backend(e.to_string()))?;
        }
        ctx.decode(&mut batch)
            .map_err(|e| InferenceError::Backend(e.to_string()))?;

        // A plain greedy (always-argmax) sampler tends to degenerate into
        // repeated loops on chat-tuned models, which reads as "confused" or
        // "too raw" even once the chat template is correct — this chain
        // matches the defaults most chat-completion APIs use: a repeat
        // penalty over recent tokens, then temperature + top-k/top-p to
        // pick among the remaining reasonable candidates.
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let mut sampler = LlamaSampler::chain_simple([
            LlamaSampler::penalties(64, 1.1, 0.0, 0.0),
            LlamaSampler::top_k(40),
            LlamaSampler::top_p(0.9, 1),
            LlamaSampler::temp(0.7),
            LlamaSampler::dist(seed),
        ]);
        let mut output = String::new();
        let mut decoder = encoding_rs::UTF_8.new_decoder();

        for n_cur in (batch.n_tokens()..).take(max_tokens as usize) {
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
