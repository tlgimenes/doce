# Quickstart: Context Window Management for Chat and Agent Mode

Validates the new context-tracking, tiered compaction, and tool-output
offloading against `spec.md`'s acceptance scenarios. Automated tests cover
the pure logic (threshold math, tier-1 clearing, migration correctness);
proving the live UI indicator and an actual model-driven summarization pass
requires the running app (`npm run tauri dev`) with a real, installed local
model — an automated test can assert the right `ContextUsage` was computed
and the right `context_notice` row was persisted, but "the user sees the
indicator move and a notice appear while chatting" (spec.md's Independent
Tests for US1/US2) needs a live run.

## Automated validation

```bash
cd src-tauri && cargo test    # backend: count_tokens, tier-1 clearing algorithm,
                               # threshold math, load_history_annotated splicing,
                               # 0004_context_notice_content_type migration
npx vitest run                 # frontend: contextUsageStore, ContextUsageIndicator
                               # states, context_notice parsing/rendering
```

Should cover, at minimum (see `tasks.md` for the exact breakdown):

- `InferenceEngine::count_tokens` returns the same count `str_to_token(...).len()` already computes internally in `generate()` (no drift between the two).
- The tier-1 clearing function, given a synthetic `Vec<HistoryMessage>` with more than `TOOL_KEEP_N` tool_call/tool_result pairs, replaces every one beyond the most recent N with the placeholder string and leaves everything else untouched — pure, no model/DB needed (same style as the existing `prefill_chunks` tests).
- Threshold math: given a token budget and the four settings values, `ContextState` classification (`Normal`/`Warning`) matches the documented fraction boundaries, including the clamping invariant (`warnThresholdPct <= compactThresholdPct <= hardLimitPct`) when settings are hand-edited out of order.
- `load_history_annotated` splices in a persisted `kind:"summarized"` row's `summary` in place of everything before it, and leaves everything after it untouched; a second, later `summarized` row supersedes the first.
- The `0004_context_notice_content_type` migration preserves existing rows' `rowid` (so `messages_fts` search results stay correctly linked) and successfully inserts a `context_notice` row post-migration where it would previously have violated the `content_type` CHECK — same verification style as the existing `0003` migration test.
- Tool-output offloading: a synthetic tool result over `toolOutputOffloadChars` is written to `<app_data_dir>/tool-outputs/<conversation_id>/<tool_call_id>.txt` and the in-memory `ChatMessage` pushed into `messages` contains only the preview + pointer, not the full text.
- `contextUsageStore` updates from a mock `context-usage-update` event and from a direct `getContextUsage` call, keyed correctly per `conversationId`.
- `ContextUsageIndicator` renders its three visual states correctly from `ContextUsage.state`, and its "Compact now" affordance calls `compactConversation` and reflects the returned usage.

## Manual validation (live app, real model)

1. `npm run tauri dev` with a model already installed and active.
2. **US1 (visibility)**: Open a new conversation. Confirm the context indicator shows near-zero usage. Send several messages back and forth until the conversation has meaningfully grown; confirm the indicator's fraction visibly increases without a page refresh, and confirm it visibly switches to its warning state once `context.warnThresholdPct` is crossed (lower this setting temporarily via a direct `update_setting` call, or via a settings UI if implemented, to make this reachable quickly in a short manual session rather than needing dozens of real turns).
3. **US2 (compaction keeps the conversation alive)**: In agent mode, run enough tool calls (e.g. several `Bash`/`Read` calls) that `context.compactThresholdPct` is crossed. Confirm: (a) an inline "conversation condensed" or "old tool results cleared" notice appears in the transcript at the right point, (b) the conversation keeps producing coherent, on-topic responses afterward rather than erroring or degrading, (c) triggering "Compact now" manually before the threshold is reached also produces a notice and visibly frees up the indicator.
4. **US3 (tool-output offloading)**: In agent mode, ask the agent to run a command known to produce a large amount of output (e.g. a verbose recursive listing or a large log dump). Confirm the transcript's `Bash` widget shows a "view full output" affordance, that opening it shows the complete original output, and that the context indicator's jump from that single tool call is modest (a preview's worth), not the full output's size.
5. **Edge case — oversized single message**: Paste an extremely large block of text as a single chat message (large enough to alone exceed `hardLimitPct` of the 2048-token budget). Confirm the app responds with a clear, specific error rather than hanging, crashing, or silently truncating the pasted content without telling the user.
6. **Edge case — reopen correctness (FR-014)**: With a conversation that has already been compacted at least once, close and reopen the app (or navigate away and back). Confirm the context indicator immediately shows the correct, already-compacted usage — not a stale or zeroed value — without needing to send a new message first.
