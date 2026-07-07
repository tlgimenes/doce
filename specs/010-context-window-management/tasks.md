# Tasks: Context Window Management for Chat and Agent Mode

**Input**: Design documents from `/specs/010-context-window-management/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/context-window-management.md, quickstart.md

**Tests**: Not TDD-gated (no test-first requirement in spec.md). Unit tests are added alongside each implementation task, matching this codebase's existing convention (e.g. `prefill_chunks`'s `#[cfg(test)] mod tests` colocated in `inference/mod.rs`, and migration `0003`'s own test).

**Organization**: Tasks are grouped by user story (US1 visibility, US2 tiered compaction, US3 tool-output offloading) so each is independently implementable, buildable, and demoable.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on incomplete tasks)
- **[Story]**: Maps the task to spec.md's US1/US2/US3
- Every task names its exact file path(s)

## Path Conventions

Single Tauri desktop project — `src-tauri/src/` (Rust backend), `src/` (React frontend), per `plan.md`'s Project Structure. No new project/package is created.

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Create the empty module skeletons this feature's tasks will fill in — no new dependencies needed (research.md confirms this feature uses only the existing stack).

- [X] T001 Create `src-tauri/src/context/mod.rs` and `src-tauri/src/context/offload.rs` as empty modules; add `pub mod context;` to `src-tauri/src/lib.rs`
- [X] T002 [P] Create empty `src/state/contextUsageStore.ts` and `src/components/ContextUsageIndicator.tsx` stub files (no exports yet) so later tasks edit existing files rather than creating them mid-story

**Checkpoint**: `cargo build` and `npm run build` still succeed with the new empty files present.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: The token-counting primitive, the widened schema, and the annotated history loader that every user story builds on.

**⚠️ CRITICAL**: No user story task may begin until this phase is complete and `cargo test` passes.

- [X] T003 In `src-tauri/src/inference/mod.rs`: add `pub const CONTEXT_WINDOW_TOKENS: u32 = 2048;`, replace the bare `2048` literal in `generate()`'s `LlamaContextParams::default().with_n_ctx(NonZeroU32::new(2048))` with this constant, add `pub fn context_window(&self) -> u32` and `pub fn count_tokens(&self, text: &str) -> Result<usize, InferenceError>` (wraps `str_to_token`). Add a unit test asserting `count_tokens(s)` equals `str_to_token(s, AddBos::Always).unwrap().len()` for a sample string.
- [X] T004 Create `src-tauri/src/storage/migrations/0004_context_notice_content_type.sql` widening `messages.content_type`'s CHECK constraint to add `'context_notice'`, following the exact table-rebuild pattern of `0003_rich_text_content_type.sql` (rebuild preserving `rowid`, recreate the index and the three `messages_ai`/`ad`/`au` FTS5 triggers verbatim). Add a migration test in `src-tauri/src/storage/` mirroring the existing `0003` migration test: insert a `context_notice` row post-migration and confirm it succeeds, and confirm a pre-existing row's `rowid` (and therefore its `messages_fts` linkage) survives the rebuild.
- [X] T005 In `src-tauri/src/storage/conversations.rs`: define `pub struct HistoryMessage { pub chat: ChatMessage, pub content_type: String, pub sequence: i64 }`; add `pub fn load_history_annotated(conn: &Connection, conversation_id: &str, skills_dir: &Path) -> rusqlite::Result<Vec<HistoryMessage>>` (same query as today's `load_history`, now also selecting `content_type`/`sequence`; excludes `context_notice` rows from the returned list **except** that the most recent `kind:"summarized"` row's embedded `summary` field is spliced in as a single synthesized `HistoryMessage` in place of everything before it — see data-model.md for the exact splicing rule). Redefine `load_history` as a thin wrapper: `load_history_annotated(...).into_iter().map(|m| m.chat).collect()`. Add unit tests: a conversation with no notices returns everything unchanged; a conversation with one `summarized` notice returns the synthesized summary message in place of everything before it and leaves everything after it untouched; a conversation with two `summarized` notices only splices relative to the most recent one.
- [X] T006 [P] In `src-tauri/src/context/mod.rs`: define `pub struct ContextUsage { conversation_id: String, tokens_used: u32, token_budget: u32, state: ContextState }` and `pub enum ContextState { Normal, Warning, JustCompacted }` (both `#[derive(Debug, Clone, Serialize, specta::Type)]`, `#[serde(rename_all = "camelCase")]`/matching enum tag convention used elsewhere in `commands/`); define `struct ContextSettings { warn_threshold_pct: f64, compact_threshold_pct: f64, hard_limit_pct: f64, tool_output_offload_chars: usize }` and `impl ContextSettings { fn load(conn: &Connection) -> Self }` reading the four `context.*` keys from the `settings` table with the defaults and clamping invariant from research.md (`warnThresholdPct <= compactThresholdPct <= hardLimitPct`) applied when parsing fails or is out of order. Add unit tests: missing/unparseable settings fall back to defaults; out-of-order values get clamped; valid values pass through unchanged.
- [X] T007 [P] In `src/lib/ipc.ts`: add `export type ContextState = "normal" | "warning" | "justCompacted";`, `export interface ContextUsage { conversationId: string; tokensUsed: number; tokenBudget: number; state: ContextState; }`, and `export type ContextNoticeDetail = { kind: "cleared"; clearedCount: number; notice: string } | { kind: "summarized"; summary: string; notice: string };` per data-model.md.

**Checkpoint**: `cargo build && cargo test` and `npm run build` all pass. Nothing is wired together yet, but every foundational piece compiles and is unit-tested in isolation.

---

## Phase 3: User Story 1 - See how full the conversation is, at a glance (Priority: P1) 🎯 MVP

**Goal**: A live, always-visible context-usage indicator in the chat UI, correct on reopen.

**Independent Test**: Open a conversation, send messages until it grows substantially, and watch the indicator move through Normal → Warning purely by observing the chat UI.

### Implementation for User Story 1

- [X] T008 [US1] In `src-tauri/src/context/mod.rs`: implement `pub async fn compute_usage(conn: &Connection, engine: &InferenceEngine, conversation_id: &str, skills_dir: &Path, is_agent_mode: bool) -> Result<ContextUsage, String>` — calls `load_history_annotated`, prepends the mode-appropriate system message (`CHAT_SYSTEM_PROMPT` or the agent system prompt), renders via `engine.render_chat_prompt`, counts via `engine.count_tokens`, classifies `ContextState::Normal`/`Warning` against `ContextSettings::load(conn)` (never `JustCompacted` from this function — that variant is only ever set by the compaction path added in US2).
- [X] T009 [US1] In new `src-tauri/src/commands/context.rs`: add `#[tauri::command] #[specta::specta] pub async fn get_context_usage(app: AppHandle, db_cell: State<'_, DbCell>, inference_state: State<'_, InferenceState>, conversation_id: String) -> Result<ContextUsage, String>` calling `compute_usage`; return `Err("No model loaded")` if the engine isn't loaded yet, matching this codebase's existing error-string convention.
- [X] T010 [US1] In `src-tauri/src/commands/mod.rs`: add `pub mod context;`, register `context::get_context_usage` in `collect_commands!`, and register a `context-usage-update` event (payload type `context::ContextUsage`) in `collect_events!`.
- [X] T011 [US1] In `src-tauri/src/commands/conversations.rs::send_message`: after persisting the user's message, call `compute_usage` and `app.emit("context-usage-update", usage)`. In `src-tauri/src/scheduler/worker.rs::run_generation`: after persisting the assistant's reply, call `compute_usage` again and emit the same event (usage after the model's own output).
- [X] T012 [US1] In `src-tauri/src/commands/agent.rs::send_agent_message`: after each turn's persistence step inside the loop (tool_call/tool_result persisted, and the final answer persisted), call `compute_usage` (with `is_agent_mode: true`) and emit `context-usage-update`.
- [X] T013 [P] [US1] In `src/lib/ipc.ts`: add `getContextUsage: (conversationId: string) => invoke<ContextUsage>("get_context_usage", { conversationId })` to `commands`, and `onContextUsageUpdate: (cb) => listen<ContextUsage>("context-usage-update", (e) => cb(e.payload))` to `events`.
- [X] T014 [P] [US1] Implement `src/state/contextUsageStore.ts`: a Zustand store `{ usage: Record<string, ContextUsage>; setUsage: (u: ContextUsage) => void }`, mirroring `src/state/conversationStreamStore.ts`'s existing shape/conventions exactly.
- [X] T015 [US1] Implement `src/components/ContextUsageIndicator.tsx`: props `{ conversationId: string }`; on mount and whenever `conversationId` changes, calls `commands.getContextUsage(conversationId)` and feeds the result into the store (covers FR-014 — correct immediately on reopen, before any live event); subscribes to the store's `usage[conversationId]` and renders a slim bar/badge with distinct Normal/Warning styling (JustCompacted styling included but unreachable until US2 wires an emitter of that state).
- [X] T016 [US1] In `src/views/chat/Chat.tsx`: render `<ContextUsageIndicator conversationId={conversation.id} />` near the compose box; subscribe to `events.onContextUsageUpdate` (alongside the existing `onAssistantToken`/`onGenerationQueueUpdate` subscriptions) to call the store's `setUsage`.
- [X] T017 [US1] In `src/views/workspace/Workspace.tsx`: same indicator render + event subscription wiring as T016, keeping the two views' usage identical per the existing `MessageContent.tsx` shared-component discipline.
- [X] T018 [P] [US1] Unit tests (vitest): `contextUsageStore`'s `setUsage` correctly keys by `conversationId`; `ContextUsageIndicator` renders its Normal/Warning visual states from a mocked store value.

**Checkpoint**: `cargo build && cargo test` and `npm run build && npm test` all pass. In `npm run tauri dev`, the indicator visibly appears in both Chat and Workspace views and updates live as a conversation grows — **MVP demo point**.

---

## Phase 4: User Story 2 - The conversation keeps working instead of hitting a wall (Priority: P2)

**Goal**: Automatic tiered compaction (clear-old-tool-results, then summarize) runs pre-flight, with a visible in-transcript notice and a manual "Compact now" action.

**Independent Test**: Drive a conversation (via repeated agent-mode tool calls) past the compaction threshold and confirm it keeps producing coherent responses, with a visible notice, instead of failing.

### Implementation for User Story 2

- [X] T019 [US2] In `src-tauri/src/context/mod.rs`: implement `pub fn apply_lightweight_clearing(history: &mut Vec<HistoryMessage>, keep_n: usize) -> usize` — a pure function: walking oldest-to-newest, replaces every `tool_call`/`tool_result`-content-type message's `chat.content` beyond the most recent `keep_n` such messages with the placeholder `"[Old tool result cleared to save context space]"`; returns the count actually cleared. Add unit tests mirroring `prefill_chunks`'s style: zero tool messages clears nothing; exactly `keep_n` tool messages clears nothing; `keep_n + 3` tool messages clears exactly 3, the oldest ones; non-tool messages are never touched.
- [X] T020 [US2] In `src-tauri/src/storage/conversations.rs`: add `pub fn persist_context_notice(conn: &mut Connection, conversation_id: &str, kind_json: &str) -> rusqlite::Result<()>` — inserts a `role='assistant'`, `content_type='context_notice'` row with `content = kind_json` at the next `sequence`, following the same insert pattern `create_conversation`/message-persist call sites already use.
- [X] T021 [US2] In `src-tauri/src/context/mod.rs`: implement `pub async fn summarize_and_persist(conn: &mut Connection, engine: &InferenceEngine, conversation_id: &str, history: &[HistoryMessage], protected_recent: usize) -> Result<Option<String>, String>` — if `history.len() <= protected_recent`, returns `Ok(None)` (nothing to summarize); otherwise builds a summarization prompt over `history[..history.len() - protected_recent]`, calls `engine.render_chat_prompt` + `engine.generate(..., 256, ..., ...)`, persists the result via `persist_context_notice` with `{"kind":"summarized","summary":"<output>","notice":"Conversation condensed to save space"}`, returns `Ok(Some(summary))`.
- [X] T022 [US2] In `src-tauri/src/context/mod.rs`: implement `pub async fn maybe_compact(conn: &mut Connection, engine: &InferenceEngine, conversation_id: &str, skills_dir: &Path, is_agent_mode: bool, force: bool) -> Result<ContextUsage, String>` orchestrating: compute usage → if `!force && tokens_used < compactThresholdPct * budget`, return current usage unchanged (`Normal`/`Warning`, per data-model.md's no-fabricated-notice rule) → otherwise run `apply_lightweight_clearing` (persist a `{"kind":"cleared",...}` notice via `persist_context_notice` if `cleared_count > 0`), recompute usage → if still over threshold, run `summarize_and_persist` → recompute final usage, setting `state: JustCompacted` if either tier actually changed something, else leaving the natural `Normal`/`Warning` classification (this is what makes `force`/manual "Compact now" a true no-op per data-model.md when there's nothing to do). Add unit tests for the threshold/force/no-op-when-nothing-to-clear logic using a fake in-memory history (no real model call — inject a stub summarizer closure for the tier-2 branch to keep this test unit-level).
- [X] T023 [US2] In `src-tauri/src/commands/conversations.rs::send_message`: add a `State<'_, InferenceState>` parameter; after persisting the user's message and before `scheduler.submit(request)`, call `maybe_compact(..., force: false)`; if the resulting usage's `tokens_used >= hardLimitPct * token_budget`, return `Err("This message is too large for the model's context window, even after compacting the conversation. Try a shorter message or start a new conversation.")` instead of submitting; otherwise proceed using the (possibly now-compacted) effective history and emit `context-usage-update` with the returned usage.
- [X] T024 [US2] In `src-tauri/src/commands/agent.rs::send_agent_message`: call `maybe_compact(..., force: false)` once before `run_loop` begins and again before each subsequent turn inside the loop, applying the same `hardLimitPct` block-with-error behavior as T023.
- [X] T025 [US2] In new `src-tauri/src/commands/context.rs`: add `compact_conversation(app, db_cell, inference_state, conversation_id) -> Result<ContextUsage, String>` calling `maybe_compact(..., force: true)`; register it in `commands/mod.rs`'s `collect_commands!`.
- [X] T026 [P] [US2] In `src/lib/ipc.ts`: add `commands.compactConversation`; add a `parseContextNoticeDetail(content: string)` sibling to `parseToolResultDetail`, degrading to plain-text rendering on parse failure.
- [X] T027 [US2] In `src/components/MessageContent.tsx`: add a dispatch branch for `contentType === "context_notice"` rendering a small inline notice via `parseContextNoticeDetail` — `cleared` renders as a muted, low-emphasis line; `summarized` renders as a clearer, distinct notice bubble.
- [X] T028 [US2] In `src/components/ContextUsageIndicator.tsx`: add a "Compact now" affordance calling `commands.compactConversation(conversationId)` and feeding the result into the store on completion.
- [X] T029 [P] [US2] Unit tests: `MessageContent.tsx`'s new `context_notice` dispatch branch renders both `cleared` and `summarized` shapes correctly, and degrades gracefully on malformed JSON (vitest).

**Checkpoint**: `cargo build && cargo test` and `npm run build && npm test` all pass. In `npm run tauri dev`, driving a conversation past the compaction threshold shows an inline notice and the conversation keeps responding; "Compact now" works on demand.

---

## Phase 5: User Story 3 - A single huge tool result doesn't blow the budget (Priority: P3)

**Goal**: Oversized agent-mode tool results are offloaded to a file with a short preview kept in context; the full content stays retrievable.

**Independent Test**: In agent mode, trigger a tool call with a very large result and confirm context usage rises only modestly while the full output remains viewable.

### Implementation for User Story 3

- [X] T030 [US3] In `src-tauri/src/context/offload.rs`: implement `pub fn offload_if_oversized(app: &AppHandle, conversation_id: &str, tool_call_id: &str, tool_name: &str, result: &str, threshold_chars: usize) -> Result<(String, Option<String>), String>` — if `result.len() <= threshold_chars`, returns `(result.to_string(), None)` unchanged; otherwise writes `result` verbatim to `<app_data_dir>/tool-outputs/<conversation_id>/<tool_call_id>.txt` (creating the directory as needed, mirroring the existing `skills_dir`-style `app.path().app_data_dir()` usage in `commands/conversations.rs`), and returns a `(preview_and_pointer_string, Some(path))` where the first 500 chars of `result` are shown followed by `"[Use Read on \"<path>\" to view the rest]"`. Add unit tests (using a tempdir) for: under-threshold passthrough, over-threshold file write + correct preview/pointer text, and correct file contents matching the original `result` exactly.
- [X] T031 [US3] In `src-tauri/src/agent/mod.rs::run_loop`: before `messages.push(ChatMessage::user(format!("Tool result for {tool_name}: {result}")))`, call `offload_if_oversized` (reading `ContextSettings::load`'s `tool_output_offload_chars`) and push the returned (possibly-substituted) text instead; thread the returned `Option<String>` path through to wherever the `tool_result` row is persisted in `commands/agent.rs` so its JSON `detail` gains `"offloadedTo": "<path>"` (or `null`).
- [X] T032 [P] [US3] In `src/lib/ipc.ts`: add `offloadedTo: string | null` to `BashDetail` and `ReadDetail`.
- [X] T033 [US3] In `src/views/chat/tool-widgets/BashWidget.tsx`: when `detail.offloadedTo` is set, show a "View full output" affordance that calls the existing `commands.readAttachedFile(offloadedTo)`, base64-decodes the result as UTF-8, and displays it (e.g. in a modal/expandable panel) — reusing the existing IPC command rather than adding a new one, per research.md's decision.
- [X] T034 [P] [US3] Same "View full output" affordance in `src/views/chat/tool-widgets/ReadWidget.tsx` for the case where a `Read` result itself was large enough to be offloaded.
- [X] T035 [P] [US3] Unit tests: `offload_if_oversized`'s threshold/preview/file-content behavior (cargo test, tempdir-based); the widgets' "View full output" rendering when `offloadedTo` is present vs. absent (vitest).

**Checkpoint**: `cargo build && cargo test` and `npm run build && npm test` all pass. In `npm run tauri dev` agent mode, a large `Bash` output shows a modest context-usage increase and a working "View full output" affordance.

---

## Phase 6: Polish & Cross-Cutting Concerns

- [X] T036 [P] Add a short doc comment on `ContextSettings::load` (T006) confirming no separate settings-seeding step is needed — defaults apply purely at read time when a key is absent, consistent with how `get_settings`/`update_setting` already behave elsewhere.
- [~] T037 Run the full `quickstart.md` manual validation pass end-to-end (US1 + US2 + US3 together) against `npm run tauri dev` with a real installed model; fix any drift found before considering the feature done. **Partially done**: confirmed `npm run tauri dev` builds and launches cleanly against this feature's changes with no crash (log-verified), and added `src-tauri/tests/real_model_smoke.rs` (an `#[ignore]`d integration test, run explicitly via `cargo test --test real_model_smoke -- --ignored`) that exercises `count_tokens`/`context_window`/`render_chat_prompt`/`generate`/tier-1-clearing/tier-2-summarization end-to-end against the real installed Qwen3-4B model — all passed, including a genuine model-produced summary. **Not done**: visually clicking through the live UI (indicator states, "Compact now", "View full output") in the actual native window — no GUI/screenshot tooling available in this environment for a native macOS window; left for the user (or a future e2e spec, see T038) to confirm visually.
- [ ] T038 [P] [Optional/stretch] Extend `tests/e2e` (wdio) with one scenario covering the indicator's visible state change; explicitly non-blocking for this feature's sign-off. Not attempted this session (time-boxed).

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies.
- **Foundational (Phase 2)**: Depends on Setup. **Blocks all user stories** — `count_tokens`, the `0004` migration, and `load_history_annotated` are load-bearing for every story.
- **User Story 1 (Phase 3)**: Depends only on Foundational. No dependency on US2/US3.
- **User Story 2 (Phase 4)**: Depends on Foundational; also depends on US1's `compute_usage`/event-emission plumbing (T008–T012) being in place, since `maybe_compact` calls `compute_usage` and reuses the same `context-usage-update` event. Cannot start before Phase 3 completes.
- **User Story 3 (Phase 5)**: Depends only on Foundational (specifically `ContextSettings` from T006). Independent of US2 — could be implemented in parallel with Phase 4 if staffed separately, since it only touches `agent/mod.rs`'s tool-result push and the offload module, not the compaction pipeline.
- **Polish (Phase 6)**: Depends on all three stories being complete.

### Within Each User Story

- Backend types/commands before the frontend code that calls them.
- `commands/mod.rs` registration (T010) before anything can actually invoke the new command/event at runtime.
- Store (T014) before the component that reads it (T015).
- Shared component (T015) before both view integrations (T016, T017).

### Parallel Opportunities

- T002 (frontend stubs) can run parallel to T001 (backend stubs).
- T006 and T007 (Rust types vs. TS types) are disjoint files — parallel.
- Within US1: T013/T014 (both frontend, disjoint files) parallel to each other and to T011/T012 (backend); T018 (tests) parallel to nothing it tests but can be written alongside T015–T017.
- Within US2: T026 (frontend types) parallel to T019–T025 (all backend); T029 (tests) can be written alongside T027.
- Within US3: T032 (frontend types) parallel to T030–T031 (backend); T033/T034 (two different widget files) parallel to each other; T035 (tests) parallel to nothing it tests but independent of T033/T034's exact wording.
- **US3 (Phase 5) as a whole can run in parallel with US2 (Phase 4)** once Phase 3 (US1) is complete, since neither touches the other's files (US2 touches `commands/conversations.rs`, `commands/agent.rs`'s pre-flight call, `MessageContent.tsx`; US3 touches `agent/mod.rs`'s tool-result push, the two tool widgets) — the only shared file is `commands/agent.rs`, and even there US2 adds a pre-flight call at the top of `send_agent_message` while US3 modifies `agent/mod.rs::run_loop`'s tool-result-push line, which are different functions in different files.

---

## Parallel Example: Foundational Phase

```bash
# After T001/T002 (Setup) complete, launch together:
Task: "Add CONTEXT_WINDOW_TOKENS + count_tokens in src-tauri/src/inference/mod.rs (T003)"
Task: "Create 0004_context_notice_content_type.sql migration + test (T004)"
# T005 depends on the migration existing (T004) conceptually (content_type value),
# but can be coded against the widened CHECK constraint in the same batch once T004's
# SQL is written, before T004's test necessarily passes — sequence T004 first in practice.
Task: "Define ContextUsage/ContextState/ContextSettings in src-tauri/src/context/mod.rs (T006)"
Task: "Add ContextUsage/ContextState/ContextNoticeDetail TS types to src/lib/ipc.ts (T007)"
```

## Parallel Example: User Story 1

```bash
Task: "Add getContextUsage + onContextUsageUpdate to src/lib/ipc.ts (T013)"
Task: "Implement src/state/contextUsageStore.ts (T014)"
# T015 depends on both of the above; T016/T017 depend on T015.
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1 (Setup) and Phase 2 (Foundational).
2. Complete Phase 3 (User Story 1).
3. **STOP and VALIDATE**: run `npm run tauri dev`, confirm the indicator appears and updates live in both Chat and Workspace views, confirm it's correct immediately after reopening a grown conversation.
4. This alone is a demoable, shippable increment — visibility without any automatic mitigation yet.

### Incremental Delivery

1. Setup + Foundational → nothing user-visible yet, but everything compiles and is unit-tested.
2. User Story 1 → live indicator → **MVP demo**.
3. User Story 2 → conversations survive past the old hard limit, with a visible notice and manual compaction → demo.
4. User Story 3 → huge tool outputs no longer dominate the budget → demo.
5. Polish → full quickstart.md pass, optional e2e coverage.

### Single-Session Delivery Note

This feature is being implemented end-to-end in one session rather than by a team, so phases run sequentially in the order above rather than in parallel by different people — the `[P]` markers identify tasks that are safe to reorder or batch within a phase, not a staffing plan.

---

## Notes

- `[P]` tasks touch disjoint files and have no incomplete-task dependency within their phase.
- Every user-story phase ends at a state where `cargo build && cargo test` and `npm run build && npm test` all pass — there is no phase that leaves the tree in a non-building state.
- Commit after each task or logical group, consistent with the repository's existing commit granularity (see recent commit history for style).
- US2 and US3 are independent of each other (only foundational-phase dependencies), so if time runs short, US3 can be deferred without blocking US2, or vice versa — both are already scoped as separate, working increments per spec.md's priority ordering.
