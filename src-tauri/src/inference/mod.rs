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

/// The client-side `LlamaBatch`'s fixed capacity — llama.cpp can't decode
/// more tokens than this in a single call, so any prompt longer than this
/// (system prompt + tool list + growing conversation history routinely
/// exceeds it in agent mode) must be prefilled across multiple `decode()`
/// calls via `prefill_chunks`, not one `batch.add()` loop over every token.
const BATCH_CAPACITY: usize = 512;

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
}
