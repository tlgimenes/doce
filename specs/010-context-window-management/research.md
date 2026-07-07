# Research: Context Window Management for Chat and Agent Mode

## Reference implementation survey (Claude Code / Claude Desktop / Anthropic API)

A full technical brief was compiled beforehand from five independent research passes (compaction mechanics, the Anthropic API's context-editing/compaction/memory-tool primitives, subagent isolation, large-tool-output offloading, and persistent memory) plus a live confirmation of Claude Code's bash-output-to-file convention. Headline findings actually load-bearing for this design (full brief retained in conversation history, not duplicated here):

- Claude Code exposes usage via a status line / `/context` and manages it with a tiered, cheapest-first pipeline: clear old tool results → (optionally) session-memory notes → full LLM-driven summarization — matching this feature's two-tier design (§ Decision: Tiered compaction below).
- The Anthropic Messages API's `context-management` beta (`clear_tool_uses_20250919`, `compact_20260112`) formalizes the same shape server-side for third-party developers, but there is **no evidence Claude Code itself calls that primitive** — its own compaction predates and appears independent of it. Since llama.cpp has no server at all in this sense, doce cannot adopt either the CLI's bespoke internals or the API primitive directly; both are precedent for *shape*, not literal reuse.
- Large tool outputs in Claude Code are truncated to a preview, written in full to a session-scoped file, and re-readable via the `Read` tool with offset/limit — directly portable, and the one mechanism in the whole brief with **no dependency on the Anthropic API** (it's app-level plumbing). This is the strongest precedent in the brief and the one this design follows most literally.
- The memory tool and subagent isolation are both explicitly out of scope for this feature (per spec.md's Assumptions) — not investigated further here.

## Decision: Where token accounting is computed from

**Decision**: Context usage is always computed by rendering the *exact* prompt that would be sent to `generate()` (system message + effective history + any pending new message) through `render_chat_prompt` and then `count_tokens`, never from a separately-maintained running counter.

**Rationale**: `load_history()` today reloads the entire conversation from SQLite on every turn — there is no persistent in-memory session object per conversation (the app can be closed and reopened; FR-014 requires correctness after that). Recomputing from the persisted source of truth is the only way to guarantee the indicator is accurate after a reopen without adding a new cache-invalidation problem. Tokenization is cheap (no forward pass), so recomputing per-turn is not a performance concern.

**Alternatives considered**: A running counter updated incrementally as messages are added — rejected because it would need to be persisted and reconciled on reopen anyway (no simpler than recomputing), and it would drift the moment compaction mutates what's "in" the effective history.

## Decision: n_ctx becomes a named constant, not dynamically configurable

**Decision**: Replace the bare `2048` literal in `inference/mod.rs`'s `generate()` with a `pub const CONTEXT_WINDOW_TOKENS: u32 = 2048;` and a `InferenceEngine::context_window() -> u32` accessor. No per-model dynamic sizing is introduced.

**Rationale**: The spec (Assumption 1) explicitly scopes this feature to making the existing hardcoded value load-bearing and query-able, not to introducing dynamic context-size selection per model/hardware-tier — that's a larger, separate concern (would touch `model_registry`/hardware-tier matching) and isn't required for any FR in this spec.

**Alternatives considered**: Reading `n_ctx` from GGUF model metadata (`llama_model_n_ctx_train` or similar) — rejected for this pass; worth a follow-up feature once dynamic model-specific context sizing is desired, but out of scope now since only one model is ever active at a time and the constant already reflects the app's actual configured window.

## Decision: Tiered compaction, run pre-flight (not inside the scheduler)

**Decision**: Compaction (tier 1 lightweight clearing, tier 2 summarization) runs synchronously inside `send_message`/`send_agent_message`, *before* a `GenerationRequest` is submitted to the scheduler (chat mode) or before a turn's `generate()` call (agent mode) — not as a step inside `scheduler::worker::run_generation`.

**Rationale**: The scheduler (per `specs/001-doce-v1-core/research.md` §24) treats a request's `messages: Vec<ChatMessage>` as an opaque payload to forward to `InferenceEngine::generate` — it has no awareness of conversation/message-persistence concepts, and giving it that awareness would be a much larger change than this feature needs. `send_message` and `send_agent_message` already own history assembly (per the codebase survey: both independently call `load_history`/build `initial_messages`), so both already have a natural, single place to run a pre-flight check before their respective generation paths begin. Agent mode's existing, documented bypass of the scheduler (`commands/agent.rs`) means it needs this pre-flight step wired in separately from the chat path regardless of where compaction logically "lives" — putting the logic in a shared `context` module and calling it from both sites is simpler than teaching the scheduler about it once and still having agent mode bypass the scheduler anyway.

**Alternatives considered**: Compacting inside the scheduler worker — rejected because agent mode's scheduler bypass would leave it uncovered, defeating the point (User Story 3 is agent-mode-specific); compacting reactively only after a decode failure — rejected because the spec requires proactive warning/compaction *before* generation is attempted (FR-005, User Story 2), not reactive error recovery.

## Decision: Tier 1 (lightweight clearing) is a pure, idempotent, load-time transform; no persisted "cut" marker required for correctness

**Decision**: Tier 1 clearing is implemented as a pure function over the sequence-ordered, content-type-tagged history: walk oldest-to-newest, and for every `tool_call`/`tool_result` message beyond the most recent `TOOL_KEEP_N` (constant, default 4) such messages, replace its `ChatMessage.content` with a fixed placeholder (`"[Old tool result cleared to save context space]"`), preserving role/ordering so the chat template still alternates correctly. This transform is recomputed fresh every time history is loaded — it does not depend on remembering *when* a clearing pass "happened."

A context-notice message row **is** still persisted the first time a given clearing actually changes what would be sent (so the user sees it happened, per FR-008), but that row is informational only — deleting it would not break tier-1 correctness, since the keep-most-recent-N rule is independently re-derivable from `content_type`+`sequence` alone every time.

**Rationale**: This mirrors Claude Code's own "microcompact" (time/count-based clearing that needs no model call and is safely re-evaluated on the fly) and keeps tier 1 trivially unit-testable as a pure function (same style as the existing `prefill_chunks` precedent in `inference/mod.rs`), independent of persistence concerns. It also sidesteps a whole class of bugs around "what if the notice row is missing/stale" since the actual clearing behavior never depends on it.

**Alternatives considered**: Destructively rewriting the SQLite `content` column for cleared rows — rejected: it would permanently discard data the user might want to inspect later (undermining `list_messages`'s role as a complete transcript), complicate the FTS5 sync triggers for no benefit, and buys nothing since the transform is already cheap to recompute at load time.

## Decision: Tier 2 (summarization) is a persisted "splice point", tier 1 clearing is not

**Decision**: When tier 2 fires, the app makes one real `generate()` call (small `max_tokens`, e.g. 256) asking the model to summarize everything except the most recent `PROTECTED_RECENT_MESSAGES` (constant, default 10) messages, then persists a single new message row: `role='assistant'`, `content_type='context_notice'`, `content` = a JSON blob `{"kind":"summarized","summary":"<model output>","notice":"Conversation condensed to save space"}`. On every subsequent load, `load_history_annotated` recognizes a `context_notice` row of kind `summarized` and — for reconstructing the *effective* prompt history only — replaces every message before it with a single synthesized `ChatMessage::system(summary)`, while `list_messages` (used for the full UI transcript) continues to return every row untouched, so the user never loses transcript history, only the model's future prompts do.

**Rationale**: Unlike tier 1 (a keep-last-N rule, always re-derivable without a marker), tier 2 genuinely discards specific content from the model's future context — there's no rule to reconstruct "which messages were summarized" after the fact except by recording it. Persisting the splice point makes this correct across reopens (FR-014) for free: the next `load_history_annotated` call just re-applies the same splice.

**Alternatives considered**: Re-running summarization from scratch on every load instead of persisting the result — rejected as wasteful (re-invokes the model on every single turn) and non-deterministic (summaries could drift turn to turn, confusing the user about what "the summary" actually says).

## Decision: content_type gains a `context_notice` variant (new migration)

**Decision**: `messages.content_type` CHECK constraint widens from `('text','tool_call','tool_result','error','rich_text')` to add `'context_notice'`, via a new migration `0004_context_notice_content_type.sql` following the exact table-rebuild pattern migration `0003_rich_text_content_type.sql` already established (SQLite can't `ALTER` a `CHECK` constraint in place; rebuild preserving `rowid` for the FTS5 external-content sync, recreate the index and the three `messages_ai`/`ad`/`au` triggers).

**Rationale**: `context_notice` rows need the same treatment `error` rows already get in `load_history` (excluded from the model-facing `ChatMessage` extraction by default, since they're not part of the conversation the model "said" — tier 2's splice logic is the one exception that reads a `context_notice` row's embedded `summary` field directly) while still needing to appear in `list_messages` for the frontend's inline notice rendering. Reusing the existing, already-proven migration pattern is the smallest correct change.

**Alternatives considered**: Encoding compaction events in the `settings` table or a brand-new table instead of a message row — rejected: a message row is naturally sequence-ordered and already flows through the existing `list_messages`/frontend-transcript pipeline for free, whereas a side table would need its own interleaving logic to render in the right place in the transcript.

## Decision: Threshold defaults, sized for a 2048-token budget

**Decision**: New settings keys (existing `get_settings`/`update_setting` JSON-string mechanism, no schema change needed there):

| Key | Default | Meaning |
|---|---|---|
| `context.warnThresholdPct` | `"0.5"` | Fraction of `CONTEXT_WINDOW_TOKENS` at which the indicator turns to its warning state |
| `context.compactThresholdPct` | `"0.75"` | Fraction at which the pre-flight compaction pipeline runs before the next generation |
| `context.hardLimitPct` | `"0.9"` | Fraction beyond which, even after compaction, the app refuses to generate and tells the user why |
| `context.toolOutputOffloadChars` | `"2000"` | Tool-result character length beyond which the result is offloaded to a file (agent mode) |

**Rationale**: Claude Code's own thresholds (~80% warning, ~83–97% auto-compact depending on model) are calibrated for 200K–1M-token windows where there's slack to spare; doce's 2048-token window leaves far less absolute headroom (a single verbose tool result can be 10%+ of the entire budget), so thresholds here are deliberately more conservative/earlier, prioritizing "never hit the wall" over "maximize usable context before intervening." `toolOutputOffloadChars` of 2000 chars (~500 tokens, roughly a quarter of the whole budget) is picked to catch genuinely large dumps (a verbose `Bash` command) without offloading routine, modestly-sized results — chosen to be in the same ballpark as the existing `Read` tool's already-established 2000-*line* cap convention, translated to characters since tool outputs aren't line-oriented in general.

**Alternatives considered**: Percentages copied directly from Claude Code — rejected per the above (mismatched absolute scale). A single combined "danger" threshold instead of separate warn/compact/hard-limit — rejected because the spec (FR-004/FR-005, Edge Cases) explicitly requires the user to see a warning *before* automatic action taken, and requires a distinct "genuinely stuck" state from "about to auto-compact."

## Decision: Tool-output offloading reuses the existing `Read` tool verbatim

**Decision**: When a tool result's `model_text` exceeds `context.toolOutputOffloadChars`, the full text is written to `<app_data_dir>/tool-outputs/<conversation_id>/<tool_call_id>.txt`, and the message actually pushed into history becomes a short preview (first 500 chars) plus an explicit instruction: `[Use Read on "<path>" to view the rest]`. No new retrieval tool is introduced — the model uses the existing `Read` tool (`agent/tools/fs.rs`), which already accepts an arbitrary absolute path with `offset`/`limit` and is not workspace-restricted (consistent with Constitution Principle V's already-accepted unscoped-filesystem-access posture for v1.0).

**Rationale**: This is the single most directly portable mechanism from the Claude Code research (§ "Large tool-output offloading" in the brief) — it's app-level plumbing with no dependency on any Anthropic-specific API, and doce already has an equivalent `Read` tool with the right shape (offset/limit pagination, 2000-line cap). Per-conversation subdirectories make cleanup natural (deleting a conversation can delete its `tool-outputs/<id>/` folder, though that cleanup itself is not required by this feature's FRs and is left as a natural follow-up).

**Alternatives considered**: A dedicated new `ReadToolOutput` tool scoped only to this directory — rejected as unnecessary complexity; the spec's own Assumptions section explicitly says this feature should reuse the existing retrieval concept rather than invent a new one.

## Decision: Frontend indicator is a new shared component, not folded into existing widgets

**Decision**: `src/components/ContextUsageIndicator.tsx` is a new, small, shared component (props: `conversationId`), rendered by both `Chat.tsx` and `Workspace.tsx` near the compose box, fed by a new `contextUsageStore.ts` Zustand slice (mirroring `conversationStreamStore.ts`'s existing shape) that's updated by a new `context-usage-update` Tauri event.

**Rationale**: `MessageContent.tsx`'s existing doc comment (spec 004, FR-013/SC-006) already establishes the project convention that anything rendered in both `Chat.tsx` and `Workspace.tsx` must be one shared component so the two views can't drift — this indicator follows the same discipline. A dedicated small always-visible bar/badge (not a dense multi-row breakdown) matches the spec's explicit "Claude Desktop, not Claude Code CLI's `/context`" steer.

**Alternatives considered**: Deriving usage client-side from message lengths (character counts) instead of a real backend token count — rejected: character-based heuristics are exactly the kind of approximation this feature exists to replace; the spec's whole premise is *exact* accounting via the model's own tokenizer.

## Dependencies

No new external dependencies (Cargo crates or npm packages) are introduced by this feature — it is built entirely from the existing `llama-cpp-2`, `rusqlite`, `tauri`/`tauri-specta`, Zustand, and React stack already in use.
