use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use std::num::NonZeroU32;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum InferenceError {
    #[error("llama.cpp backend error: {0}")]
    Backend(String),
    #[error("model load failed: {0}")]
    ModelLoad(String),
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

    /// Greedy-sampled generation used for the walking-skeleton chat path
    /// (User Story 2) and integration tests, invoking `on_token` as each
    /// token is produced so the caller can emit `assistant-token` events in
    /// real time rather than waiting for the full response. The full
    /// scheduler-backed tool-use loop (User Story 3) builds on this same
    /// model handle via `agent::run_turn`.
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

        let mut sampler = LlamaSampler::greedy();
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
