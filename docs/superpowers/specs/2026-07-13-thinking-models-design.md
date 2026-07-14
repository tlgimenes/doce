# Thinking-only models

**Date:** 2026-07-13
**Status:** approved in conversation; implementation lands benchmark-gated
alongside the same-day cap-disclosure prompt changes.

## Decision

doce supports ONLY thinking (hybrid-reasoning) models from here on. The
registry moves to MiniCPM5-1B and Qwen3-4B-Thinking-2507; the inference
layer learns to (a) let a `<think>…</think>` block precede the
grammar-required tool call and (b) strip think blocks from every generated
output before anything downstream sees it.

## Why Require stays (and lazy doesn't replace it)

`ToolCallMode::Require` exists because prompt-level "every response must
be a tool call" demonstrably failed on small local models: `StepDone(...)`
emitted as prose ended a 20-file job at file 1. The non-lazy grammar makes
prose unsamplable — the failure is closed at the decoder. A lazy grammar
only guarantees a tool call is well-formed IF the model starts one; it
cannot force one to start, so drift reopens exactly under the long
repetitive contexts where tasks have the most to lose.

Thinking does not require giving that up. The Require grammar's language
is widened instead: `root ::= think-prefix? tool-call`. The model gets an
unconstrained scratchpad; the response still cannot END as prose.
Hypothesis for the benchmark: thinking-trained models put their drift
INSIDE the think block, making the constrained tail more reliable.

## Mechanics

1. **Grammar** (`inference::tool_call_grammar`): when built for Require
   mode, the root gains an optional think prefix:
   `think-prefix ::= "<think>"? think-char* "</think>" ws*` with the
   standard GBNF until-literal ladder for `think-char`. The opening tag is
   optional because thinking templates (Qwen3-Thinking) pre-open the block
   in the generation prompt, so the model's own output starts mid-think
   and only emits the closing tag. Mild, benign side effect: a handful of
   `<`-adjacent character sequences become unsamplable inside think
   content. Lazy (Allow) mode is unchanged — its trigger already lets any
   prefix through.

2. **Stripping** (`inference::strip_think_blocks`, applied at
   `PromptSession::generate`'s single return point, so every consumer —
   agent loop, subagents, summarization — sees clean text): removes
   complete `<think>…</think>` blocks, treats an orphan leading
   `</think>` as a template-pre-opened block (drop everything through
   it), and drops an unclosed trailing `<think>…` (token budget ran out
   mid-think). Tool-call parsing therefore never sees think content — a
   model thinking ABOUT `<tool_call>` syntax cannot confuse the parser.

3. **Template**: unchanged code path — `render_chat_prompt` already
   applies the GGUF's own embedded chat template. History replay is
   already think-free by construction (assistant turns are rebuilt from
   structured `ToolUse` messages, not raw output), which is the standard
   convention for thinking models. Verify against each real GGUF that
   llama.cpp's template detection produces the pre-opened `<think>` — a
   manual quickstart-style check, not unit-testable.

4. **Registry**: MiniCPM5-1B Q4_K_M (688MB, `openbmb/MiniCPM5-1B-GGUF`)
   is the PRIMARY model in every tier (user decision, 2026-07-13:
   "MiniCPM5-1B instead of qwen3"); Qwen3-4B-Thinking-2507 Q4_K_M
   (bartowski) stays as the 16GB tier's priority-2 fallback and the
   benchmark comparison point. sha256 values are the HF LFS oids.

5. **Token accounting**: think tokens are stripped before persistence, so
   the ↓ counters undercount raw generation by the think length. Accepted
   for now; surfacing "thought for N tokens" is a follow-up.

6. **Live generation ticker (implemented with this change)**: every
   sampled piece of the top-level agent's generation streams to the
   frontend as an `agent-generation-piece` event; the working shimmer
   shows the newest ~160 chars as a muted one-line tail under the
   Working row. Ticker text is ephemeral — buffers clear at every
   `agent-message-persisted` boundary and on turn end, and think content
   never enters the transcript. Subagent generations don't stream (their
   activity is deliberately isolated to the Task widget).

## Verification

- Unit: strip_think_blocks cases (plain, wrapped, pre-opened, unclosed,
  multiple, think-containing-tool_call-text); grammar string asserts for
  the think-prefix root; existing name-enum tests unchanged.
- Benchmark (the real gate): the full agent_tasks ladder against both new
  models, thinking on — compared against the Qwen3-4B-Instruct baseline.
  `tests/agent_tasks.rs` / `real_model_smoke.rs` model paths flip to the
  new GGUFs when running.
