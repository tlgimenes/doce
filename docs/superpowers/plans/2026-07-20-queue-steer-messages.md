# Queue & Steer Messages Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** While an agent turn is running, a user can **queue** follow-up messages
(client-side, shown as preview rows above the composer) and **steer** any queued
message into the running turn via a per-row "Send now" button. Queued messages
drain FIFO as new turns when the turn completes; idle submit is unchanged.

**Architecture:** Backend adds a steer channel by extending the existing
`ActiveGenerations` map value with a `steers: Vec<String>` queue (reusing its
per-turn lifecycle + RAII cleanup — avoids specta's 9-arg ceiling on
`send_agent_message`), draining it into `run_loop` at each step boundary via a
new no-op-default `AgentBackend::drain_steers()`. A new `steer_generation`
command persists + enqueues (no inference, so no `llama-server` lock contention).
Frontend holds the queue in a per-conversation module registry (mirroring
`conversationsWithSendInFlight`), branches queue-vs-send in the composer caller
(never inside `send()`), drains on the busy→idle edge, and renders a new
`QueuedMessages` preview-rows component. The steered message is persisted
backend-side and shown through the existing `agent-message-persisted` event.

**Tech Stack:** Rust + Tauri + specta (backend); React + Tailwind + Tiptap
(frontend). Rust tests via `cargo test` (in `src-tauri/`). Frontend tests via
`npm test` (vitest). Lint `npm run lint` (oxlint), format `npm run format:check`
(oxfmt — NOT prettier). Bindings regenerate via the ignored specta export test.

**Spec:** `docs/superpowers/specs/2026-07-20-queue-steer-messages-design.md`

## Global Constraints

- Queue is the default while busy; steering is only ever via the explicit per-row
  "Send now" button — **no new implicit keybindings**.
- `steer_generation` performs **no inference** — persist + enqueue only.
- Do **not** add a new managed-`State` param to `send_agent_message` (specta
  9-arg ceiling, `agent.rs:2145`). Extend `ActiveGenerations`' value instead.
- `AgentBackend::drain_steers()` default is a **no-op** — subagents and test
  backends must not drain the parent's steers.
- Keep `send()` a pure "send-now-or-noop-if-busy" primitive; branch queue-vs-send
  in the composer caller so the `pendingInitialTurn` contract survives.
- The steered message is persisted + emitted by the backend via the **existing**
  `agent-message-persisted` event; the frontend must NOT insert it manually.
- The outcome union comes from generated `bindings.ts`
  (`"injected" | "noActiveTurn" | "rejected"`), not a hand-written literal.
- A manual Stop leaves the queue intact (no auto-drain).
- Goal-mode queued rows hide "Send now" (drain-only).
- Work happens directly on `main`, in place (no worktrees — project convention).
- Backend tasks (1–2) land before frontend tasks (3–4): the frontend binding
  depends on the generated types.

---

### Task 1: Backend steer channel — registry value + loop drain plumbing

Refactor `ActiveGenerations` to carry a per-turn steer queue and drain it into
the agent loop. No new command yet; the tree must compile and existing tests pass.

**Files:**

- Modify: `src-tauri/src/commands/conversations.rs:16-17` (`ActiveGenerations`
  value type), `:423-427` (`stop_generation`), tests `:429+`
- Modify: `src-tauri/src/agent/mod.rs:196-220` (`AgentBackend` trait), `:251`
  (`run_loop` drain), tests `:363+`
- Modify: `src-tauri/src/commands/agent.rs:664-701` (`RealBackend` struct),
  `:703+` (`impl AgentBackend for RealBackend`), `:2275-2279` (insert site),
  `:2453-2467` (RealBackend construction)

**Interfaces:**

- Produces: `ActiveGeneration { cancel, steers: Vec<String> }` value type;
  `AgentBackend::drain_steers(&mut self) -> Vec<ChatMessage>` (default no-op);
  `run_loop` appends drained steers as `user` turns at the top of each iteration.
  Task 2's `steer_generation` pushes onto `steers`; Task 2's `steer_core` reads
  the same map.

- [ ] **Step 1: Change the `ActiveGenerations` value type**

In `conversations.rs`, replace the token-only value with:

```rust
pub struct ActiveGeneration {
    pub cancel: tokio_util::sync::CancellationToken,
    /// FIFO queue of already-persisted, rich-expanded steered user turns,
    /// drained into the running loop at each step boundary.
    pub steers: Vec<String>,
}
pub struct ActiveGenerations(pub std::sync::Mutex<std::collections::HashMap<String, ActiveGeneration>>);
```

Keep any `Default`/constructor parity with the current definition. Update
`stop_generation` (`:424-425`) to `entry.cancel.cancel()`. Leave `.keys()`
(list_conversations), `.contains_key` (is_generation_active), and the guard's
`.remove` untouched.

- [ ] **Step 2: Update the insert + construction sites in `agent.rs`**

At the `ActiveGenerations` insert (`:2275-2279`), insert
`ActiveGeneration { cancel: cancel.clone(), steers: Vec::new() }`. Add an
`active_generations: &'a ActiveGenerations` field to `RealBackend` and pass
`active_generations: &active_generations` at construction (`:2453-2467`) — it
Deref-coerces from the existing `State`, exactly as `_active_guard` does at
`:2281`.

- [ ] **Step 3: Add the `drain_steers` trait method (default no-op) + RealBackend override**

In `agent/mod.rs`, add to `AgentBackend`:

```rust
fn drain_steers(&mut self) -> Vec<ChatMessage> { Vec::new() }
```

In `agent.rs`, override it on `RealBackend`:

```rust
fn drain_steers(&mut self) -> Vec<ChatMessage> {
    let mut g = self.active_generations.0.lock().unwrap();
    match g.get_mut(self.conversation_id) {
        Some(e) => std::mem::take(&mut e.steers).into_iter().map(ChatMessage::user).collect(),
        None => Vec::new(),
    }
}
```

(Adjust `self.conversation_id` to the field name RealBackend actually holds.)
Brief lock, no `await` held.

- [ ] **Step 4: Drain at the top of the `run_loop` iteration**

In `agent/mod.rs` at the top of the `for _turn in 0..context.max_turns` body
(before the `measure` call at `:252`):

```rust
for steered in backend.drain_steers() {
    messages.push(steered);
}
```

Owned `Vec` return → no borrow overlap with `messages` in the generic loop.

- [ ] **Step 5: Rust tests — loop drain**

In `agent/mod.rs` `mod tests`, add a small backend that overrides `drain_steers`
and records the user text each `generate` observed (mirror `ScriptedBackend`).
Add:

- `run_loop_drains_pending_steers_at_the_boundary_and_they_reach_generate`
- `run_loop_preserves_fifo_order_of_multiple_drained_steers`
- `run_loop_default_drain_is_a_noop` (a backend using the default; behavior
  identical to the existing `loop_runs_tools_until_a_final_answer` baseline)

In `conversations.rs` `mod tests`:

- `stop_generation_fires_the_token_on_the_new_value_shape`
- `active_generation_guard_clears_the_entry_including_queued_steers`

- [ ] **Step 6: Build, test, lint**

Run: `cd src-tauri && cargo test` (or the repo's usual Rust test entrypoint).
Expected: PASS. Then `npm run lint && npm run format:check` at repo root for any
touched TS (none expected this task) — both exit 0.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/commands/conversations.rs src-tauri/src/agent/mod.rs src-tauri/src/commands/agent.rs
git commit -m "feat(agent): steer channel — per-turn steer queue drained at the loop boundary"
```

---

### Task 2: Backend `steer_generation` command + compaction gating + bindings

Add the command that persists + enqueues a steer, the standalone-compaction
marker, register it, and regenerate the TS bindings.

**Files:**

- Modify: `src-tauri/src/commands/agent.rs` (`SteerResult`, `SteerMessageInput`,
  `steer_core`, `steer_generation`, tests `:2958+`)
- Modify: `src-tauri/src/commands/conversations.rs` (`CompactingConversations`)
- Modify: `src-tauri/src/commands/context.rs:74-124` (`compact_conversation` RAII
  marker)
- Modify: `src-tauri/src/commands/mod.rs:16-46` (`collect_commands!`)
- Modify: `src-tauri/src/lib.rs:67-73` (`.manage(CompactingConversations)`)
- Generate: `src/lib/bindings.ts`

**Interfaces:**

- Consumes: `ActiveGeneration.steers` + `drain_steers` from Task 1.
- Produces: `commands.steerGeneration` bound in `bindings.ts` returning
  `SteerResult` (`"injected" | "noActiveTurn" | "rejected"`). Tasks 3–4 consume
  this binding.

- [ ] **Step 1: Add the result enum + input struct**

In `agent.rs`:

```rust
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum SteerResult { Injected, NoActiveTurn, Rejected }

#[derive(Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SteerMessageInput { pub content: String, pub rich_content: Option<String> }
```

- [ ] **Step 2: Add `CompactingConversations` + compaction RAII marker**

In `conversations.rs`, next to `ActiveGenerations`:

```rust
#[derive(Default)]
pub struct CompactingConversations(pub std::sync::Mutex<std::collections::HashSet<String>>);
```

In `context.rs`, add a `State<'_, CompactingConversations>` param to
`compact_conversation`, insert the `conversation_id` on entry and remove it via a
small RAII guard covering the `maybe_compact` call (`:112-124`).

- [ ] **Step 3: Implement `steer_core` + `steer_generation`**

`steer_core` (no `AppHandle`; takes an `emit_persisted: impl FnOnce()` closure,
mirroring `handle_ask_user_question`'s `emit_question` at `agent.rs:577`):

```rust
async fn steer_core(
    active: &ActiveGenerations,
    compacting: &CompactingConversations,
    conn: &tokio_rusqlite::Connection,
    transcript_dir: Option<PathBuf>,
    skills_dir: &Path,
    conversation_id: &str,
    content: &str,
    rich_content: Option<&str>,
    emit_persisted: impl FnOnce(),
) -> Result<SteerResult, String> {
    if !active.0.lock().unwrap().contains_key(conversation_id) {
        return Ok(if compacting.0.lock().unwrap().contains(conversation_id)
                  { SteerResult::Rejected } else { SteerResult::NoActiveTurn });
    }
    let (_, model_text) = persist_user_turn(conn, transcript_dir, skills_dir,
                                            conversation_id, now_ms(), content, rich_content).await?;
    emit_persisted();
    if let Some(e) = active.0.lock().unwrap().get_mut(conversation_id) { e.steers.push(model_text); }
    Ok(SteerResult::Injected)
}
```

The `#[tauri::command] pub async fn steer_generation(app, db_cell,
active_generations, compacting, conversation_id, message: SteerMessageInput)`
wrapper resolves `skills_dir`/`transcript_dir` from `app.path().app_data_dir()`
(as `send_agent_message` does at `:2172-2184`), gets `conn` from `db_cell`, and
passes `|| { let _ = app.emit("agent-message-persisted",
AgentMessagePersisted { conversation_id: conversation_id.clone() }); }` as
`emit_persisted` (the exact event `persist_tool_call` emits at `:371-376`).
Return `Result<SteerResult, String>`.

- [ ] **Step 4: Register the command + managed state**

Add `agent::steer_generation` to `collect_commands!` (`mod.rs:16-46`). Add
`.manage(CompactingConversations::default())` in `lib.rs` (`:67-73`). No
`collect_events!` change (reuses `AgentMessagePersisted`).

- [ ] **Step 5: Rust tests — `steer_core` + compaction guard**

In `agent.rs` `mod tests` (reuse the `persist_user_turn` tokio_rusqlite
harness at `:2958+`; pass a counter closure for `emit_persisted`):

- `steer_core_with_an_active_turn_persists_enqueues_and_returns_injected`
- `steer_core_preserves_fifo_across_multiple_injects`
- `steer_core_with_no_active_turn_returns_no_active_turn_and_persists_nothing`
- `steer_core_during_compaction_returns_rejected_and_persists_nothing`
- `steer_core_expands_rich_content_into_the_enqueued_turn`
- `steer_core_returns_err_on_malformed_rich_content` (nothing persisted, no emit)

In `conversations.rs` `mod tests`: `compacting_guard_registers_and_clears`.

- [ ] **Step 6: Regenerate bindings**

Run the ignored specta export test (per `mod.rs:73-82`), e.g.
`cd src-tauri && cargo test --lib export_typescript_bindings -- --ignored`.
Confirm `src/lib/bindings.ts` now has `steerGeneration` and the
`SteerResult`/`SteerMessageInput` types. Then `npm run format:check`.

- [ ] **Step 7: Build, test, commit**

Run: `cd src-tauri && cargo test`. Expected: PASS.

```bash
git add src-tauri/src/commands/agent.rs src-tauri/src/commands/conversations.rs src-tauri/src/commands/context.rs src-tauri/src/commands/mod.rs src-tauri/src/lib.rs src/lib/bindings.ts
git commit -m "feat(agent): steer_generation command + standalone-compaction gating"
```

---

### Task 3: Frontend composer — editable while busy, dual buttons, recall + IPC binding

Make the composer usable mid-turn and add the `steerGeneration` IPC binding.

**Files:**

- Modify: `src/lib/ipc.ts` (`steerGeneration` binding near `stopGeneration` at
  `:614`; re-export/alias the `SteerResult` union)
- Modify: `src/views/chat/rich-input/RichInput.tsx:734-767` (dual buttons),
  `:505-529` / `:784` (editable while busy), props + recall effect
- Modify: `src/views/chat/rich-input/RichInput.test.tsx`

**Interfaces:**

- Consumes: `commands.steerGeneration` from the Task 2 bindings.
- Produces: `RichInput` accepts `recall?: { token, content, richContent? }`,
  stays editable while `isGenerating`, and renders submit + stop together. Task 4
  drives `recall`, `onSubmit` routing, and calls `commands.steerGeneration`.

- [ ] **Step 1: Add the `steerGeneration` IPC binding**

In `ipc.ts`, near `stopGeneration` (`:614`), following the generated binding
shape:

```ts
steerGeneration: (conversationId: string, content: string, richContent?: string) =>
  invoke<SteerGenerationOutcome>("steer_generation", {
    conversationId,
    message: { content, richContent },
  }),
```

Type `SteerGenerationOutcome` from the generated `bindings.ts` `SteerResult`
(`"injected" | "noActiveTurn" | "rejected"`) — re-export or alias it; do not
hand-write the literal.

- [ ] **Step 2: Add the `recall` prop + effect to RichInput**

Add `recall?: { token: number; content: string; richContent?: RichMessageContent }`
to `RichInputProps`. New effect keyed on `recall?.token` (mirror `autoFocusToken`
at `:576-579`):

```ts
if (recall === undefined) return;
editor?.commands.clearContent(true);
if (recall.richContent) editor?.commands.setContent(richMessageContentToDoc(recall.richContent));
else if (recall.content) editor?.commands.insertContent(recall.content);
editor?.commands.focus("end");
```

Import `richMessageContentToDoc` from `./serialize`.

- [ ] **Step 3: Show submit + stop together; keep editor editable while generating**

In the block-end addon (`:734-767`), while `isGenerating` render BOTH the stop
button (`stop-generation`, unchanged) AND the submit button (`submitTestId`,
`aria-disabled` when empty, `onClick={submitCurrentContent}`) — order
`… [submit] [stop]`. Ensure the editor is not `disabled` purely because a turn
is in flight (Task 4 passes `disabled={false}` during generation; the
pending-question case swaps in `UserAskWidget` upstream). Enter-to-submit already
routes through `submitCurrentContent` → `onSubmit`.

- [ ] **Step 4: RichInput tests**

In `RichInput.test.tsx` add:

- `recall prop clears and prefills the editor with the given text and focuses it`
- `recall prop rebuilds rich content (pastedText chip) via richMessageContentToDoc`
- `shows both the submit and stop buttons while generating`
- `the editor is editable while generating so a message can be composed to queue`

Update existing `swaps the send button for the stop button while generating` /
`returns to the send button once generation ends` to the new both-visible
behavior.

- [ ] **Step 5: Test, lint, format, commit**

Run: `npm test -- RichInput` then `npm run lint && npm run format:check`.
Expected: PASS / exit 0.

```bash
git add src/lib/ipc.ts src/views/chat/rich-input/RichInput.tsx src/views/chat/rich-input/RichInput.test.tsx
git commit -m "feat(composer): editable-while-busy, dual send/stop buttons, recall prop; steer IPC binding"
```

---

### Task 4: Frontend queue registry + Workspace wiring + QueuedMessages UI

Hold the queue, route queue-vs-send, drain on completion, steer via "Send now",
and render the preview rows.

**Files:**

- Create: `src/views/workspace/messageQueueRegistry.ts` (or inline into
  `Workspace.tsx` next to the send-in-flight registry)
- Create: `src/views/workspace/QueuedMessages.tsx`
- Modify: `src/views/workspace/Workspace.tsx` (queue read, `enqueue`, drain
  effect, `handleSteer`, `handleEditQueued`, `handleStop` suppress-ref, composer
  wiring at `:779-796`, render `QueuedMessages` at `:775`)
- Modify: `src/views/workspace/Workspace.test.tsx`

**Interfaces:**

- Consumes: `commands.steerGeneration` and the `RichInput` `recall` /
  dual-button / editable-while-busy behavior from Task 3.
- Produces: the complete feature.

- [ ] **Step 1: Per-conversation queue registry**

Create a module-global `Map<string, QueuedMessage[]>` with
`subscribeToQueue`, `getQueueSnapshot(conversationId)` (return a shared
module-const `EMPTY` when absent for reference stability), `enqueueMessage`,
`removeQueuedMessage`, `replaceQueue`, and `__resetQueueRegistryForTests()`.
Model 1:1 on `conversationsWithSendInFlight` (`Workspace.tsx:70-115`).
`QueuedMessage = { id, content, richContent?, setGoal? }` (id via
`crypto.randomUUID()`).

- [ ] **Step 2: `QueuedMessages.tsx` preview rows**

Props: `{ items, onSteer, onEdit, onDelete, steerError }`. Return `null` when
empty. Container `data-testid="queued-messages"`; each row
`data-testid="queued-message-row"` with a truncated preview and three buttons —
`queued-message-send-now` (aria "Send now"), `queued-message-edit`,
`queued-message-delete`. **Hide `queued-message-send-now` when `item.setGoal`**
(goal rows drain only). When `steerError`, render `data-testid="queue-steer-error"`.
Reuse the existing chip surface classes used by the goal banner.

- [ ] **Step 3: Wire Workspace state + handlers**

- Read: `const queue = useSyncExternalStore(subscribeToQueue,
() => getQueueSnapshot(conversationId), getServerSnapshot)`.
- `const [steerError, setSteerError] = useState<string | null>(null)`;
  `const [recall, setRecall] = useState<{token; content; richContent?} | null>(null)`;
  `const drainSuppressedRef = useRef(false)`.
- `enqueue(content, richContent?, setGoal=false)`: whitespace/empty guard, then
  `enqueueMessage(conversationId, { id: crypto.randomUUID(), content, richContent, setGoal })`;
  clear `steerError`.
- **Drain effect** (deps `[turnInFlight, pendingToolCall, queue, send]`):
  `if (turnInFlight || pendingToolCall || queue.length === 0) return;`
  `if (drainSuppressedRef.current) { drainSuppressedRef.current = false; return; }`
  `const head = queue[0]; if (send(head.content, head.richContent, head.setGoal ?? false)) removeQueuedMessage(conversationId, head.id);`
- `handleStop`: set `drainSuppressedRef.current = true` before
  `commands.stopGeneration(...)`.
- `handleSteer(item)`: `const rc = item.richContent ? JSON.stringify(item.richContent) : undefined;`
  `const outcome = await commands.steerGeneration(conversationId, item.content, rc);`
  `injected` → `removeQueuedMessage` + clear error (rely on
  `agent-message-persisted` refresh to render it);
  `noActiveTurn` → `if (send(item.content, item.richContent, item.setGoal ?? false)) removeQueuedMessage(...)`;
  `rejected` → keep row + `setSteerError("Couldn't send now — the turn isn't accepting messages.")`.
- `handleEditQueued(item)`: `setRecall({ token: Date.now(), content: item.content, richContent: item.richContent })` then `removeQueuedMessage`.

- [ ] **Step 4: Composer wiring + render**

At `:779-796`: `disabled={false}` during generation (keep pending-question
handling as-is upstream); keep `isGenerating={turnInFlight}`,
`onStop={handleStop}`; `onSubmit={(content, rc) => (turnInFlight || pendingToolCall) ? enqueue(content, rc) : send(content, rc)}`;
`goal.onSendAsGoal = (text) => (turnInFlight || pendingToolCall) ? enqueue(text, undefined, true) : handleSendAsGoal(text)`;
add `recall={recall ?? undefined}`. Render
`<QueuedMessages items={queue} onSteer={handleSteer} onEdit={handleEditQueued} onDelete={(id) => removeQueuedMessage(conversationId, id)} steerError={steerError} />`
inside the `max-w-xl` column at `:775`, above the composer.

- [ ] **Step 5: Workspace tests**

Add `steerGeneration: vi.fn()` to the `commands` mock; call
`__resetQueueRegistryForTests()` in `beforeEach`; default
`mockResolvedValue("injected")`. Cases:

- `queues a composer submission while a turn is in flight instead of sending it`
- `drains the queue FIFO as sequential turns when the running turn completes`
- `Send now steers via steerGeneration and removes the row on injected`
- `Send now falls back to sendAgentMessage when steer returns no_active_turn`
- `Send now keeps the row and surfaces an error when steer is rejected`
- `edit recalls a queued message into the composer and removes the row`
- `delete removes a queued message without sending or steering`
- `an idle submit still sends immediately without queuing`
- `a manual Stop leaves the queued messages intact and does not auto-drain`
- `queued messages are isolated per conversation`
- `a goal-mode submit while busy queues and the row hides Send now`

Update the existing `blocks the composer …` and `keeps the composer blocked
after a reload …` tests: composer is now editable (`contenteditable="true"`) but
a busy submit enqueues (no new `sendAgentMessage`), preserving the
anti-duplicate-send guarantee.

- [ ] **Step 6: Test, lint, format, commit**

Run: `npm test` (full suite) then `npm run lint && npm run format:check`.
Expected: PASS / exit 0.

```bash
git add src/views/workspace/messageQueueRegistry.ts src/views/workspace/QueuedMessages.tsx src/views/workspace/Workspace.tsx src/views/workspace/Workspace.test.tsx
git commit -m "feat(workspace): queue messages while busy, steer via Send now, drain FIFO on completion"
```

---

### Task 5: End-to-end verification + whole-branch review

Prove the feature in the real app and review the full diff.

**Files:** none (verification only; apply any review fixes to the relevant files).

- [ ] **Step 1: Full suites green**

Run: `cd src-tauri && cargo test` and `npm test` at repo root. Both PASS.
`npm run lint && npm run format:check` exit 0.

- [ ] **Step 2: Visual verification via the `verify` skill**

Drive the real app: start a turn, queue two messages mid-turn (confirm preview
rows appear and no second turn starts), click "Send now" on one (confirm it
appears in the transcript within the running turn), let the turn finish and
confirm the remaining queued message drains as a new turn. Screenshot the
queued-rows state. Confirm a manual Stop leaves the queue intact.

- [ ] **Step 3: Whole-branch review**

Review the full diff from the plan baseline. Confirm: no `send_agent_message`
arg-count regression; steer never runs inference; goal rows hide Send-now;
`agent-message-persisted` (not manual insertion) renders steered messages; queue
isolation per conversation. Apply Critical/Important fixes; triage Minors.

- [ ] **Step 4: Update the progress ledger and push**

Record final state in `.superpowers/sdd/progress.md` and push to `origin/main`.
