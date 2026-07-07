# Contract: Context Window Management for Chat and Agent Mode

## `get_context_usage` (new command)

**Signature**: `get_context_usage(conversation_id: string) -> Result<ContextUsage, string>`

Computes and returns the conversation's current context usage by loading its effective history (`load_history_annotated`, tier-1 clearing applied live, tier-2 splice applied if a `context_notice` row exists) plus the active system prompt (mode-dependent: `CHAT_SYSTEM_PROMPT` or the agent system prompt, mirroring how `send_message`/`send_agent_message` already pick one), rendering it through `InferenceEngine::render_chat_prompt`, and counting tokens via the new `InferenceEngine::count_tokens`. Returns an error if no model is currently loaded (same `"No model loaded"` convention `count_message_tokens`-style commands already use elsewhere in this codebase).

Called by the frontend: once when a conversation is opened/switched to (FR-014 — correctness after reopen), independent of any live event.

## `compact_conversation` (new command)

**Signature**: `compact_conversation(conversation_id: string) -> Result<ContextUsage, string>`

Forces the same tier-1-then-tier-2 pipeline `send_message`/`send_agent_message` run pre-flight, immediately, regardless of whether the compaction threshold has actually been crossed (FR-009 — manual trigger). Persists a `context_notice` row for whichever tier(s) actually changed something; if nothing was eligible to clear or summarize, returns the current usage unchanged (see `data-model.md`'s Validation Rules — no fabricated notice). Emits `context-usage-update` as a side effect, same as the automatic path.

## `send_message` (modified)

**New parameter**: gains a `State<'_, InferenceState>` (previously only used inside `scheduler::worker`) so it can run token counting and, if needed, the tier-2 summarization `generate()` call synchronously before submitting to the scheduler. The public IPC signature (`conversation_id`, `content`, `rich_content`) is unchanged — this is a Rust-internal parameter addition, invisible to the frontend.

**New behavior**, inserted after persisting the user's message and before `scheduler.submit(request)`:
1. Call `load_history_annotated`, build the effective `Vec<ChatMessage>` (system prompt + history + the just-persisted user message).
2. Compute usage via `count_tokens(render_chat_prompt(...))`.
3. If `tokens_used >= compactThresholdPct * token_budget`: run tier 1 (in-memory clearing over the `HistoryMessage` list); recompute; if still over threshold, run tier 2 (one `generate()` call + persist the `context_notice` row); recompute again.
4. If, after both tiers, `tokens_used >= hardLimitPct * token_budget` (or the single new user message alone already exceeds it before any history is even considered): return `Err("This message is too large for the model's context window, even after compacting the conversation. Try a shorter message or start a new conversation.")` instead of proceeding to `scheduler.submit` — the user message stays persisted (so it's not lost, satisfying FR-017's "never silently drop" in spirit even in the failure path) but no assistant turn is queued.
5. Otherwise, proceed exactly as today (submit to scheduler) using the now-possibly-compacted effective history, and emit `context-usage-update` reflecting the post-tiering usage.

**Concurrency**: if `compact_conversation` or another `send_message` call for the same conversation is already running its compaction step, a new `send_message` call queues behind it rather than racing (guarded by the same per-conversation mutex discipline `ActiveGenerations`/scheduler already use elsewhere — no new locking primitive introduced, reuse the existing one scoped to this conversation id) — this is FR-017's concurrency guarantee.

## `send_agent_message` (modified)

Same pre-flight sequence as `send_message` above, run once before the agent's `run_loop` begins (using `initial_messages`), **and again before each subsequent turn inside the loop** (since agent-mode turns can each add a tool_call/tool_result pair that pushes usage over threshold mid-loop, unlike chat mode's single-shot check). `context-usage-update` is emitted after each turn's persistence step (alongside the existing tool_call/tool_result persistence), not just once at the start.

## `run_loop` / tool-result push (modified, `agent/mod.rs`)

Before `messages.push(ChatMessage::user(format!("Tool result for {tool_name}: {result}")))`, check `result.len() > toolOutputOffloadChars`. If exceeded: write `result` to `<app_data_dir>/tool-outputs/<conversation_id>/<tool_call_id>.txt`, and push `ChatMessage::user(format!("Tool result for {tool_name} (truncated — {len} chars total, full output saved): {preview}...\n[Use Read on \"{path}\" to view the rest]", ...))` instead, where `preview` is the first 500 chars of `result`. The persisted `tool_result` message row's JSON `detail` gains `offloadedTo: Some(path)` (else `None`) — this is what `BashWidget`/`ReadWidget` read to show the "view full output" affordance; the actual `model_text` substitution above is independent of what's persisted for display (the widget can still show the *full* stdout/stderr for user inspection even though the model itself only saw the preview — offloading affects what enters the model's context, not what the transcript UI is capable of displaying, consistent with `BashWidget`'s existing display-only truncation already doing something conceptually similar).

## `context-usage-update` (new event)

**Payload**: `ContextUsage` (see `data-model.md`).

**Emitted from**:
- `commands::conversations::send_message`, after step 5 above.
- `scheduler::worker::run_generation`, after persisting the assistant's reply (reflects usage growth from the model's own output).
- `commands::agent::send_agent_message`'s loop, after each turn's persistence step (tool call/result or final answer).
- `commands::context::compact_conversation`, always (even the "nothing changed" no-op case still reports current usage so the frontend's manual "Compact now" action gets a definitive response).

## Frontend contracts (internal, not IPC)

### `contextUsageStore` (`src/state/contextUsageStore.ts`)

Mirrors `conversationStreamStore.ts`'s shape:

```typescript
interface ContextUsageStoreState {
  usage: Record<string, ContextUsage>; // keyed by conversationId
  setUsage: (u: ContextUsage) => void;
}
```

Populated by: (a) `events.onContextUsageUpdate` (live updates during a turn/compaction), and (b) a direct `commands.getContextUsage(conversationId)` call fired once when a conversation becomes the active view (covers FR-014 — a freshly opened conversation shows correct usage before any event has fired).

### `ContextUsageIndicator` (`src/components/ContextUsageIndicator.tsx`)

**Props**: `{ conversationId: string }`.

Reads `usage[conversationId]` from `contextUsageStore`; renders nothing (or a neutral empty state) if not yet loaded. Renders a slim bar/badge with three visual states matching `ContextState`, plus a small "Compact now" affordance that calls `commands.compactConversation(conversationId)` and calls `setUsage` with the result on completion. Rendered by both `Chat.tsx` and `Workspace.tsx` near their respective compose boxes — no view-specific variant, per the `MessageContent.tsx`-precedent shared-component discipline.

### `MessageContent.tsx` (modified)

Gains one new dispatch branch: a message with `contentType === "context_notice"` renders a small inline notice (parsed via the new sibling `parseContextNoticeDetail`, not routed through `ToolWidget`/`parseToolResultDetail`) instead of being handled by the existing `tool_result`/`rich_text`/`text` branches.
