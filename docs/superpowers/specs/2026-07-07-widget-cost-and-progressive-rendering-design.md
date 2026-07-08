# Widget cost badges + progressive rendering

**Status**: Approved, not yet implemented
**Context**: Follow-up to building `src/views/design-system/WidgetGallery.tsx` (a live catalog of every tool-call widget). Reviewing all 8 widgets side by side surfaced two gaps worth closing.

## Motivation

Two independent but complementary problems with the existing tool-call widgets (`src/views/chat/tool-widgets/*`):

1. **No cost visibility.** A widget shows *what* a tool call did, but not what it cost against the context budget — the exact thing this app's context-window-management work (`context::fit_to_budget`, the context usage gauge) already tracks at the conversation level, just not per tool call.
2. **No progressive rendering.** Every tool-call row is invisible (`MessageContent.tsx` renders `null` for a lone `tool_call` row) until its paired `tool_result` lands — even though the data model already persists `tool_call` and `tool_result` as two separate rows, with a live event (`onAgentMessagePersisted`) firing after each. For a slow `Bash` command or a `Task` subagent delegation that can run for minutes, the widget just doesn't exist until it's already done.

## Scope

- Cost badges: `Read`, `Bash`, `Grep`, `Glob` — the widgets whose output size actually varies. `Write`/`Edit`/`Task`/`AskUserQuestion` are skipped (their cost is small and roughly fixed).
- Progressive rendering: `Bash` and `Task` only — the two tools whose execution can genuinely take long enough for a pending state to matter. `Read`/`Write`/`Edit`/`Glob`/`Grep` are fast local ops where the pending window is imperceptible; not worth two rendering paths for a state nobody will ever see.
- No new aggregate/rollup widget (e.g. a per-conversation total-cost view) — considered and explicitly deferred.

## Section 1: Token/byte cost badges

### Backend (`src-tauri`)

One shared helper — not duplicated per call site, matching this codebase's existing "one shared function, not three copies" precedent (`context::fit_turn_to_budget`):

```rust
// src-tauri/src/context/mod.rs
pub fn annotate_with_token_count(engine: &InferenceEngine, outcome: ToolOutcome) -> ToolOutcome
```

Computes `engine.count_tokens(&outcome.model_text)` and merges a `tokenCount: number` field into `outcome.detail`. Called from all three `AgentBackend::execute_tool` implementations (`RealBackend` in `commands/agent.rs`, `SubagentBackend` in `commands/agent.rs`, `BenchBackend` in `tests/agent_benchmark.rs`) immediately after `dispatch::execute()` returns, before the result is persisted or (in the benchmark) traced. Only applied when `detail.toolName` is one of `Read`/`Bash`/`Grep`/`Glob` — the other tool names pass through unchanged.

Uses the real tokenizer (the same one `fit_to_budget`/the context gauge already use), not a client-side character-count estimate — the whole point is that this number has to match the real budget math, since an approximation could mislead exactly when a user is checking it to understand spend.

Byte count needs no backend change: `content.length` (Read), `stdout.length`/`stderr.length` (Bash), and match-list length (Grep/Glob) are already present in each `detail` shape.

### Frontend (`src`)

Extends `ReadDetail`/`BashDetail`/`GrepDetail`/`GlobDetail` in `src/lib/ipc.ts` with an optional `tokenCount?: number` field (optional so older persisted rows without it still parse). Reuses the existing user-message token-meter convention (`formatTokenCount`, muted small text) rather than inventing new styling:

- `ReadWidget`: file-path line grows a trailing `· 1.2KB · 312 tok`.
- `BashWidget`: existing `flex justify-between` status row gets the cost on the right, alongside the exit code.
- `SearchResultsWidget` (Glob/Grep): match-count line grows the same trailing badge.

## Section 2: Progressive/pending rendering (Bash, Task)

### Backend

**Correction from the original draft of this doc**: this section originally claimed no backend change was needed, on the assumption that the `tool_call` row is always persisted immediately, before execution. That's only true for `AskUserQuestion`, which is deliberately special-cased (`handle_ask_user_question` persists `tool_call`, blocks on the answer, then persists `tool_result` separately). Traced against the actual code: for the general tool path and for `Task`, `execute_top_level_tool` runs `dispatch::execute()` (or, for `Task`, the *entire* subagent `run_loop`) to completion **first**, and only then makes one bundled call, `persist_tool_call_and_result(...)`, writing both rows together. There is currently no moment where a lone `tool_call` row exists for Bash or Task.

Real fix, mirroring what `AskUserQuestion` already does: split that one bundled call into two, in `execute_top_level_tool` (`src-tauri/src/commands/agent.rs`):
- For the general (non-Task, non-AskUserQuestion) branch: call `persist_tool_call` immediately after receiving `call`, *before* `dispatch::execute()` runs. Call `persist_tool_result` after execution finishes (after the existing offload-if-oversized step), with the same content it already builds.
- For `Task`: call `persist_tool_call` immediately after extracting `prompt` (before `spawn_subagent`/`run_loop`). Call `persist_tool_result` after the subagent's `run_loop` returns, with the same content it already builds.

`persist_tool_call_and_result` (the existing helper) is already just `persist_tool_call` followed by `persist_tool_result` — this is a call-site restructuring (moving *when* each half is called relative to execution), not new persistence logic.

### Frontend

1. Generalize `Workspace.tsx`'s `pendingQuestion` derivation (currently AskUserQuestion-only) into a broader `pendingToolCall` `useMemo`. Same underlying guarantee it already relies on: sequence ordering means a `tool_result` can only ever land immediately after its `tool_call`, so "the latest message is an unpaired `tool_call`" is a reliable "still in flight" signal for any tool, not just `AskUserQuestion`. Only acts on `toolName === "Bash" | "Task"` — other tool names are left alone (they resolve too fast for it to matter, per the scope decision above).
2. New parse helpers alongside `parseAskUserQuestionCallDetail` in `lib/ipc.ts`: `parsePendingBashCallDetail` (→ a `BashDetail`-shaped object with `outcome` omitted, just `command` known) and `parsePendingTaskCallDetail` (→ `{toolName: "Task", prompt, state: "running"}`).
3. Rendered in the same spot `pendingQuestion` renders today (right after the message list), not inside `MessageContent`'s per-row `tool_call` branch — which keeps returning `null`, unchanged.
4. `BashWidget` grows a third branch (pending): the `$ command` line renders as today, the status row is replaced with a "Running…" treatment styled to match `TaskWidget`'s existing sky-blue "Running…" text — consistent running-state styling across both widgets.
5. `TaskWidget` needs **zero code changes**. `TaskDetail.state: "running"` already exists in the type and the widget already branches on it — today nothing ever produces that value (the backend only persists `tool_result` once the subagent has already finished, so `state` is hardcoded `"complete"`). This plumbing is what finally makes `"running"` real.
6. Not the same kind of "pending" as `AskUserQuestion`: a pending Bash/Task isn't waiting on the user, it resolves on its own once the next `onAgentMessagePersisted` fires for the paired `tool_result`. Purely a live status card, no interaction.

## Section 3: `WidgetGallery` updates

Once both sections land, `src/views/design-system/WidgetGallery.tsx` gets:
- Cost badges visible in its existing Read/Bash/Grep/Glob examples (just populate `tokenCount` in the sample data — no new example needed).
- A new "Pending / running" example for Bash and Task, alongside their existing Success/Failure states.

## Testing

- Backend: unit test for `annotate_with_token_count` (asserts `tokenCount` is present for the four applicable tool names, absent for the others).
- Frontend: unit tests for the two new `parsePending*CallDetail` helpers (mirroring `parseAskUserQuestionCallDetail`'s existing test shape), and a `Workspace.test.tsx` case exercising the generalized `pendingToolCall` derivation (a lone `tool_call` row as the latest message renders the pending widget; a paired `tool_result` after it does not).
- `WidgetGallery`'s existing test coverage (if any) extended for the new example states.

## Out of scope (explicitly deferred)

- A per-conversation cost rollup/summary widget.
- Pending-state rendering for Read/Write/Edit/Glob/Grep (execution too fast to matter).
- Cost badges on Write/Edit/Task/AskUserQuestion (cost is small and roughly fixed for these).
