# Queue & Steer Messages — Design

**Date:** 2026-07-20
**Status:** Approved

## Problem

While an agent turn is running, the composer is inert: the send button is
replaced by a stop button and the editor is `disabled`
(`Workspace.tsx:784`), and `send()` hard-returns `false` for any submit while a
turn is in flight (`Workspace.tsx:596`). A second `send_agent_message` would in
any case deadlock on the single supervised `llama-server` inference lock. So a
user who thinks of a correction or a follow-up mid-turn has no option but to
interrupt (Stop) and lose the running work, or wait.

Claude Code and OpenAI Codex both solve this. Codex has the model worth copying:
it separates two operations available during an active turn —

- **Steer** — inject a message *into the running turn*; the agent consumes it at
  the next step boundary (after the current model output / tool call). No
  restart. Implemented via a per-turn pending-input queue (`inject_if_running`).
- **Queue** — hold a message client-side and dispatch it as a *new turn* once
  the agent goes idle.

Claude Code only has queuing (buffer at the turn boundary) and no true steering.

## Decision

Bring both to doce with an explicit-UI model chosen with the owner (no new
implicit keybindings):

- **Queue by default.** While a turn is in flight, submitting from the composer
  appends the message to a **client-side per-conversation queue**, rendered as
  **preview rows** just above the composer. It is not sent.
- **Per-row "Send now" = steer.** Each queued row carries three controls —
  **Send now** (steer: inject into the running turn), **Edit** (recall the
  message back into the composer), **Delete** (remove). Steering is only ever
  triggered by this explicit button.
- **Drain FIFO on completion.** When the running turn finishes naturally, any
  remaining queued messages are dispatched one at a time, in order, each as its
  own new turn.
- **Idle submit is unchanged.** With no turn in flight, submitting sends
  immediately exactly as today (preserving `/compact` interception and the
  `pendingInitialTurn` contract).
- **A manual Stop leaves the queue intact.** Stopping does not auto-drain;
  queued rows remain and only drain after the next naturally-completing turn.

## Architecture

### Backend — the steer channel (Rust/Tauri)

Steering performs **no inference** — it only persists a user row and enqueues
it — so it never contends the `llama-server` lock. The running turn's `generate`
holds the model; the steer is consumed at the next `run_loop` boundary.

- **Reuse `ActiveGenerations`, do not add managed state.** `send_agent_message`
  is already at specta's 9-argument `SpectaFn` ceiling (`agent.rs:2145`); a new
  `State` param would push it to 10 and break bindings export. Change the map's
  value from a bare `CancellationToken` to
  `ActiveGeneration { cancel, steers: Vec<String> }`
  (`conversations.rs:16`). Membership of the map already means "a regular,
  steerable turn is live for this conversation" — the exact steer target — and
  the existing `ActiveGenerationGuard` RAII cleanup already clears it on every
  exit path. `is_generation_active` / `list_conversations` key usage is
  unchanged; only `stop_generation` (`entry.cancel.cancel()`) and the insert
  site change.
- **Drain at the step boundary.** Add a sync `AgentBackend::drain_steers()
  -> Vec<ChatMessage>` with a **default no-op** (so `SubagentBackend` and test
  backends are unaffected — subagents must never drain the parent's steers).
  `RealBackend` overrides it to `mem::take` the conversation's `steers` under a
  brief lock (no `await` held). `run_loop` calls it at the **top of each
  iteration**, before `measure`/`compact` and before `generate`
  (`agent/mod.rs:251`), appending each steer as a `user` turn — so it is counted
  for budget and reaches the very next `generate`.
- **`steer_generation` command.** New command taking
  `{ conversationId, message: { content, richContent? } }`. Logic (via a
  testable `steer_core` that takes an `emit_persisted` closure, mirroring
  `handle_ask_user_question`'s `emit_question`):
  1. If the conversation is **not** in `ActiveGenerations` → return
     `NoActiveTurn` (or `Rejected` if it is in a new `CompactingConversations`
     set — a standalone `/compact` is running). Persist nothing.
  2. Otherwise persist the user row via `persist_user_turn` (returns the
     rich-expanded `model_text`), fire `emit_persisted` (`agent-message-persisted`,
     the same event tool rows use), push `model_text` onto `steers`, return
     `Injected`.

  Persist-then-enqueue keeps the accept decision race-safe without holding the
  mutex across the `await`.
- **Compaction gating.** Add `CompactingConversations(Mutex<HashSet<String>>)`,
  marked via RAII inside `compact_conversation` (`context.rs:74`). This is the
  *standalone* `/compact` command only. The automatic per-turn compaction inside
  a regular turn happens while the conversation is in `ActiveGenerations`, so
  steers arriving then are correctly queued and drained, not rejected.

### Frontend — queue + UX (React/TypeScript)

- **Per-conversation queue registry.** Store the queue in a module-global
  `Map<conversationId, QueuedMessage[]>` read via `useSyncExternalStore`,
  mirroring the existing `conversationsWithSendInFlight` registry
  (`Workspace.tsx:70`). This gives per-conversation isolation for free
  (Workspace is not remounted on conversation switch) and survives the mid-turn
  remount window the send-in-flight registry was built for. Ship a
  `__resetQueueRegistryForTests()` hook.

  `QueuedMessage = { id, content, richContent?, setGoal? }`.
- **Branch in the composer caller, not inside `send()`.** Keep `send()` as the
  pure "send now, or no-op if busy" primitive (the `pendingInitialTurn` effect
  and `handleSendAsGoal` depend on it returning `false` while busy). In the
  composer `onSubmit`: `turnInFlight || pendingToolCall` → `enqueue`, else
  `send`.
- **Drain effect.** A `useEffect` keyed on the busy flags falling to idle
  dispatches the head of the queue via `send()`, removing it on the truthy
  return. A `drainSuppressedRef` set inside `handleStop` skips the drain on a
  *manual* stop, so Stop leaves the queue intact.
- **Steer action.** "Send now" calls `commands.steerGeneration`. On `injected`,
  remove the row (the existing `agent-message-persisted` → `refreshMessages()`
  path renders it — do not insert manually). On `noActiveTurn`, fall back to
  `send()`. On `rejected`, keep the row and show a subtle error.
- **Composer changes (`RichInput`).** The editor becomes editable while busy,
  and the submit button is shown **alongside** the stop button (they currently
  swap) so click-to-queue works. A new `recall` prop clears and prefills the
  editor (full-fidelity via `richMessageContentToDoc`) for Edit.
- **New `QueuedMessages.tsx`** preview-rows component between the streaming
  status and the composer.

### IPC contract (shared)

```ts
steerGeneration(conversationId, message: { content, richContent? })
  -> "injected" | "noActiveTurn" | "rejected"
```

The outcome enum is a specta-exported camelCase union — the frontend consumes
the **generated `bindings.ts` type**, not a hand-written string union. The
steered message is persisted and surfaced by the backend through the existing
`agent-message-persisted` event; the frontend never inserts it manually.

## Key decisions & edge cases

- **Goal-mode queued rows hide "Send now".** `steer_generation` has no goal
  flag; rather than silently drop goal intent, goal-mode queued messages can
  only drain as a normal goal turn. (Owner decision.)
- **Finish-boundary steer deferred (KNOWN v1 LIMITATION).** A steer accepted
  (`Injected`) during the turn's *final* `generate`/`execute_tool` — i.e. after
  `run_loop` has already passed the top-of-iteration drain that would have folded
  it in, and the model then returns `FinishTask` — is persisted (so it renders in
  the transcript, looking answered) but the loop returns before another drain, so
  the leftover `entry.steers` are discarded when the `ActiveGenerationGuard`
  drops. Net: in that narrow race the steered instruction is silently NOT
  processed and NOT re-dispatched (it is a backend steer, not a frontend-queued
  message, so the FIFO drain does not pick it up). No hang, no data loss, but the
  user's steer is ignored. The fix (re-dispatch any non-empty `entry.steers` as a
  fresh turn after `run_loop` returns, or an in-loop `FinishTask`-arm re-drain)
  needs `PlanState`/observer-completion verification and is out of scope for v1.
- **Accept-decision race is non-lossy.** If the turn ends during the
  `persist_user_turn` await, the message is already persisted, so we still
  return `Injected` (never a `NoActiveTurn` that would double-send via the
  fallback). Residual: it shows as a trailing user turn rather than injected —
  microsecond window.
- **Subagent isolation** is correct by construction: `SubagentBackend` keeps the
  no-op `drain_steers`; a parent steer waits for the parent's next boundary
  after `Task` returns.
- **Manual Stop** leaves the queue; **conversation switch** keeps each
  conversation's queue via registry keying. The Stop-suppression flag is
  conversation-scoped and consumed on the first idle pass regardless of queue
  contents, so an empty-queue Stop cannot strand it and poison a later drain.
- **Multiple queued `/compact` (minor).** A queued `/compact` drains via `send`'s
  intercept, which returns without marking in-flight, so several queued
  `/compact` messages can fire overlapping `compact_conversation` calls. Niche
  (requires deliberately queuing multiple `/compact`); not serialized in v1.

## Verification

- **Rust unit tests:** `run_loop` drains steers at the boundary + FIFO + default
  no-op unchanged (`agent/mod.rs`); `steer_core`
  injected/noActiveTurn/rejected/rich-expansion/malformed/FIFO (`agent.rs`);
  value-type refactor + guard cleanup (`conversations.rs`).
- **Vitest:** queue-while-busy (no send), FIFO drain on completion,
  Send-now→steer+remove, noActiveTurn fallback, rejected keeps row, edit
  recalls, delete, idle still sends, Stop leaves queue, per-conversation
  isolation, goal-mode queue hides Send-now (`Workspace.test.tsx`); recall prop +
  both-buttons-while-generating + editable-while-busy (`RichInput.test.tsx`).
- **Visual:** drive the real app via the `verify` skill — queue two messages
  mid-turn, steer one, confirm it appears in the transcript, let the rest drain.

## Out of scope

- The Finish-arm re-drain (steer answered within the same finishing turn).
- Steering goal-mode messages (goal drain only).
- Any new implicit keybindings for steer/queue.
- The new-conversation composer (`EmptyState.tsx`) — idle-only, unaffected.
