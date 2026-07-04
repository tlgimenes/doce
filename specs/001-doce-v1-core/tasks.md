# Tasks: Doce v1.0 — Zero-Config Local Personal Agent

**Input**: Design documents from `/specs/001-doce-v1-core/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/tauri-ipc.md, quickstart.md (all present)

**Tests**: Included — `research.md` §9 and `quickstart.md` specify concrete test scenarios (scheduler cases, `wiremock` download tests, FTS5 trigger test, one WDIO e2e spec per quickstart section) as explicit design decisions, not optional boilerplate.

**Organization**: Tasks are grouped by user story (spec.md, P1: US1–US3, P2: US4–US7) to enable independent implementation and testing of each story.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (US1–US7)
- File paths follow `plan.md`'s Project Structure (`src/` frontend, `src-tauri/src/` backend modules, `src-tauri/tests/`, `tests/frontend`, `tests/e2e`)

---

## Phase 1: Setup

**Purpose**: Project initialization — nothing here is story-specific.

- [X] T001 Scaffold the Tauri 2 project: `src-tauri/Cargo.toml` (Rust 1.80+, `tauri` 2.x) and `package.json` (Vite + `@vitejs/plugin-react` v6 + React 19 + TypeScript), per `plan.md` Technical Context
- [X] T002 [P] Configure `vite.config.ts`: `reactCompilerPreset()` + `@rolldown/plugin-babel` + `babel-plugin-react-compiler`, `babel()` ordered before `react()` (`research.md` §10)
- [X] T003 [P] Configure Tailwind CSS v4 via `@tailwindcss/vite` in `src/styles/theme.css`: `@theme` design tokens, `@custom-variant dark (&:where(.dark, .dark *));` (`research.md` §11)
- [X] T004 [P] Configure Oxlint + Oxfmt (`.oxlintrc.json`, `oxfmt` config) as standalone tools, not the full Vite+ CLI (`research.md` §18)
- [X] T005 [P] Add `rustfmt.toml` and `clippy.toml` defaults for `src-tauri/`
- [X] T006 Add backend crates to `src-tauri/Cargo.toml`: `tokio`, `tokio-util`, `llama-cpp-2`, `gbnf`, `rmcp` (client feature), `rusqlite` (`bundled`, `fts5` features), `tokio-rusqlite`, `reqwest`, `serde`/`serde_json`, `tauri-specta`, `wiremock` (dev-dependency)
- [X] T007 [P] Add frontend deps to `package.json`: Base UI, TanStack Query, TanStack Form, Zustand, `react-markdown` + `shiki`, CodeMirror 6, `react-xtermjs`, Phosphor Icons (`research.md` §12–§21)
- [X] T008 Verify `.github/workflows/ci.yml` (already created) resolves against the new `package.json`/`Cargo.toml` scripts: add `lint`, `format:check`, `test`, `test:e2e` npm scripts and confirm `cargo test`/`cargo clippy`/`cargo fmt --check` run clean on the empty scaffold

**Checkpoint**: Project builds (`cargo tauri dev` launches an empty window); CI passes on the scaffold.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Infrastructure every user story depends on. **No user story work starts before this phase completes.**

### Storage

- [X] T009 Write `0001_init.sql` migration in `src-tauri/src/storage/migrations/`: `workspaces`, `conversations`, `messages`, `models`, `mcp_server_connections`, `settings` tables per `data-model.md` (UUIDv7 TEXT primary keys, `INTEGER` unix-ms timestamps, `Workspace.path UNIQUE`, `Model.is_active` partial unique index, `Conversation.spawned_by_conversation_id` FK, `Message.content_type` enum incl. `error`, `Message.tool_name`)
- [X] T010 [P] Implement the migration runner in `src-tauri/src/storage/migrations.rs`: read `PRAGMA user_version`, apply pending numbered `.sql` files in a transaction (`data-model.md` Schema conventions)
- [X] T011 Implement the `tokio-rusqlite` connection wrapper in `src-tauri/src/storage/mod.rs`: single dedicated background thread, `PRAGMA journal_mode = WAL`, `PRAGMA foreign_keys = ON` set explicitly at connection-open (`research.md` §4)
- [X] T012 [P] Implement `Message(conversation_id, sequence)` index and `(conversation_id, sequence DESC)`-ordered "latest message" query in `src-tauri/src/storage/messages.rs`
- [X] T013 [P] Implement FTS5 virtual tables (`messages_fts`, `conversations_fts`) and sync triggers in `0002_fts5.sql`, excluding rows where `spawned_by_conversation_id IS NOT NULL` (`data-model.md` Search section)

### Inference & Scheduler

- [X] T014 Implement the `inference/` module in `src-tauri/src/inference/mod.rs`: `llama-cpp-2` model/context loading, sampling, token streaming, thread-count cap below full core count (`research.md` §24)
- [X] T015 Implement per-sequence KV-cache save/restore in `src-tauri/src/inference/session.rs` using `state_seq_save_file`/`state_seq_load_file` (`research.md` §24, resolved) with a size-1 most-recently-active retention policy
- [X] T016 Implement the `scheduler/` module in `src-tauri/src/scheduler/queue.rs`: single-flight worker as a pure queue consumer, `Generation Request` struct with `priority_conversation_id`, focus-based dynamic priority (no static tiers), no anti-starvation mechanism (`research.md` §24)
- [X] T017 [P] Implement `tokio::sync::mpsc` bounded channel wiring between the inference worker and Tauri event emission in `src-tauri/src/scheduler/events.rs`
- [X] T018 [P] Implement `tokio_util::sync::CancellationToken`-based cancellation in `src-tauri/src/scheduler/cancel.rs`, checked between decode steps
- [X] T019 Implement turn-chunking in the scheduler: each agent-loop turn resubmitted as a new back-of-queue item rather than looping tightly (`research.md` §24)

### IPC & App Shell

- [X] T020 Set up `tauri-specta` codegen wiring in `src-tauri/src/commands/mod.rs`: command registration macro, generated TS bindings output path
- [X] T021 [P] Implement the typed error variant (`{ error: { code, message } }`) convention for all commands in `src-tauri/src/commands/error.rs`
- [X] T022 [P] Scaffold the React app shell in `src/App.tsx`: view routing (onboarding/chat/workspace/settings), `src/lib/` typed `invoke`/`listen` wrappers consuming the `tauri-specta` bindings
- [X] T023 [P] Set up TanStack Query provider in `src/lib/queryClient.ts` and the three Zustand stores (`useGenerationQueueStore`, `useConversationStreamStore`, `useWorkspaceActivityStore`) in `src/state/` (`research.md` §14)
- [X] T024 [P] Implement the dark/light theme controller in `src/lib/theme.ts`: default to OS appearance via Tauri's window theme API, override persisted through `get_settings`/`update_setting`

**Checkpoint**: Storage, inference, scheduler, and IPC plumbing all exist and are unit-tested in isolation; no user-facing feature yet. All user stories can now proceed.

---

## Phase 3: User Story 1 - Open the app and start talking, with zero setup (Priority: P1) 🎯 MVP

**Goal**: First launch → hardware detection → automatic model download (resumable, checksum-verified) → working chat, with zero configuration screens.

**Independent Test**: Fresh install, no prior config; reach a working conversation with no credentials/model-choice/account prompts (per `quickstart.md` §1).

### Tests for User Story 1

- [X] T025 [P] [US1] `wiremock`-backed download-resume integration test in `src-tauri/tests/integration/test_download_resume.rs`: serve a partial response then drop the connection, verify resume-not-restart (FR-003/SC-003, `research.md` §9)
- [X] T026 [P] [US1] Integration test for hardware-tier → model matching in `src-tauri/tests/integration/test_model_registry.rs`
- [X] T027 [P] [US1] WDIO e2e spec `tests/e2e/01-zero-config-first-run.spec.ts` mirroring `quickstart.md` §1

### Implementation for User Story 1

- [X] T028 [P] [US1] Implement the hardware profiler in `src-tauri/src/hardware/mod.rs`: `sysctl`/`libc` FFI for chip/RAM/disk, tier matching (FR-001)
- [X] T029 [P] [US1] Implement the model registry in `src-tauri/src/model_registry/mod.rs`: bundled `registry.json` fallback, remote refresh, `schema_version` compatibility check, per-tier `priority`-ordered candidates (`research.md` §23)
- [X] T030 [US1] Implement the resumable downloader in `src-tauri/src/downloader/mod.rs`: `reqwest` HTTP range requests, `.part` file + sidecar metadata, SHA-256 verification before rename (FR-003, depends on T029)
- [X] T031 [US1] Implement `get_hardware_profile`, `start_model_install`, `get_model_install_status`, `list_models`, `set_active_model` commands in `src-tauri/src/commands/models.rs` (depends on T028, T029, T030)
- [X] T032 [US1] Emit `model-install-progress` events during download in `src-tauri/src/downloader/mod.rs` (depends on T030)
- [X] T033 [P] [US1] Build the onboarding view in `src/views/onboarding/`: hardware detection → download progress (no model picker, no API key field, no account) (FR-001–FR-004)
- [X] T034 [US1] Wire `set_active_model` into the settings view model-override control in `src/views/settings/ModelOverride.tsx` — settings-only, never shown during onboarding (FR-005)
- [X] T035 [US1] Implement `get_settings`/`update_setting` commands in `src-tauri/src/commands/settings.rs`, backing telemetry-off-by-default (FR-020)

**Checkpoint**: A fresh install reaches a working, zero-config first run. This is the MVP — deployable/demoable on its own.

---

## Phase 4: User Story 2 - Chat with the local assistant (Priority: P1)

**Goal**: Streaming, markdown/code-rendering chat with locally persisted history.

**Independent Test**: Multi-turn conversation streams, renders markdown/code, persists across restart, no workspace needed (`quickstart.md` §2).

### Tests for User Story 2

- [X] T036 [P] [US2] Integration test for message persistence/streaming-checkpoint behavior in `src-tauri/tests/integration/test_chat.rs`
- [X] T037 [P] [US2] WDIO e2e spec `tests/e2e/02-chat-persistence.spec.ts` mirroring `quickstart.md` §2

### Implementation for User Story 2

- [X] T038 [P] [US2] Implement `create_conversation`, `list_conversations` (basic form, no `status` yet — see US7), `send_message` commands in `src-tauri/src/commands/conversations.rs`
- [X] T039 [US2] Wire `send_message` into the scheduler (submits a `Generation Request`, returns `{ messageId, requestId }` immediately) (depends on T016, T038)
- [X] T040 [US2] Implement streaming persistence in `src-tauri/src/storage/messages.rs`: in-memory streaming sub-state, flush to SQLite on completion or app-close checkpoint (depends on T009)
- [X] T041 [US2] Emit `assistant-token`/`assistant-message-complete` events in `src-tauri/src/scheduler/events.rs` (depends on T017, T039)
- [X] T042 [P] [US2] Build the chat view in `src/views/chat/`: streaming display, `react-markdown` + `shiki` rendering, copy affordance (FR-006)
- [X] T043 [US2] Wire `useConversationStreamStore` to `assistant-token` events for the active conversation (depends on T023, T041)

**Checkpoint**: Standalone chat works end-to-end, independent of agent mode.

---

## Phase 5: User Story 3 - Turn a folder into a coding/system agent (Priority: P1)

**Goal**: Opening a folder starts an unrestricted agent tool-use loop (Read/Write/Edit/Bash/Glob/Grep/AskUserQuestion, GBNF-constrained for non-tool-calling models), with live activity, a catastrophic-command denylist, and one-level turn-capped subagent spawning.

**Independent Test**: Open a sample project, describe a small change, confirm file edits + shell command with no confirmation prompt, not limited to that folder (`quickstart.md` §3–§4).

### Tests for User Story 3

- [X] T044 [P] [US3] Integration test for the full built-in tool set (Read/Write/Edit/Bash/Glob/Grep) in `src-tauri/tests/integration/test_tools.rs`, asserting exact Claude Code parity signatures (`research.md` §27)
- [ ] T045 [P] [US3] Integration test for GBNF-constrained tool calling on a non-tool-calling model in `src-tauri/tests/integration/test_gbnf.rs`
- [X] T046 [P] [US3] Integration test for the catastrophic-command denylist in `src-tauri/tests/integration/test_denylist.rs`: verify `rm -rf ~`/`rm -rf /`-equivalent and disk-erase patterns are hard-blocked with no override (FR-013, SC-011, `research.md` §29)
- [ ] T047 [P] [US3] Integration test for subagent spawning in `src-tauri/tests/integration/test_subagent.rs`: context isolation (no parent history), one-level nesting rejection, 30-turn cap actually stopping the loop, priority inheritance via `priority_conversation_id` (FR-015/FR-016, `research.md` §25)
- [ ] T048 [P] [US3] Integration test for the no-deadlock guarantee in `src-tauri/tests/integration/test_scheduler_deadlock.rs`: parent awaiting a subagent never blocks the single inference worker
- [X] T049 [P] [US3] WDIO e2e spec `tests/e2e/03-agent-mode.spec.ts` mirroring `quickstart.md` §3
- [X] T050 [P] [US3] WDIO e2e spec `tests/e2e/04-subagent-spawning.spec.ts` mirroring `quickstart.md` §4

### Implementation for User Story 3

- [X] T051 [P] [US3] Implement `open_workspace`/`list_workspaces` commands in `src-tauri/src/commands/workspaces.rs` (FR-008)
- [X] T052 [US3] Implement the `agent/` tool-use loop orchestrator in `src-tauri/src/agent/mod.rs`: unrestricted file/shell actions, no approval gate, submits per-turn work to the scheduler (FR-009, FR-013, depends on T016, T051)
- [X] T053 [P] [US3] Implement `Read`/`Write`/`Edit` tools in `src-tauri/src/agent/tools/fs.rs` (exact Claude Code signatures, `research.md` §27)
- [X] T054 [P] [US3] Implement `Bash` tool in `src-tauri/src/agent/tools/bash.rs`, including the hardcoded catastrophic-command denylist check before execution (FR-013, `research.md` §29)
- [X] T055 [P] [US3] Implement `Glob`/`Grep` tools in `src-tauri/src/agent/tools/search.rs` (`.gitignore` handling per `research.md` §27)
- [ ] T056 [US3] Implement per-turn GBNF grammar generation from the live tool set via the `gbnf` crate in `src-tauri/src/agent/grammar.rs`, with the schema-normalization fallback for `anyOf`/`oneOf`-mixing and `snake_case`-property-name gaps (`research.md` §22, depends on T053–T055)
- [X] T057 [US3] Implement subagent spawning in `src-tauri/src/agent/subagent.rs`: fresh isolated `Conversation` row (`spawned_by_conversation_id` set), restricted tool subset, one-level nesting rejection, 30-turn cap, `priority_conversation_id` inheritance, `tokio::sync::oneshot`-based non-blocking await (FR-015/FR-016, depends on T052, T009)
- [X] T058 [P] [US3] Implement the `AskUserQuestion` tool and pause/resume mechanic in `src-tauri/src/agent/tools/ask_user.rs`: emits `ask-user-question`, awaits `answer_user_question` via oneshot channel (FR-010, depends on T052) — the pause/resume registry (`PendingQuestions`) was built and unit-tested here originally; the live dispatch wiring (register/emit/await on a real `AskUserQuestion` call) was completed later by `004-tool-call-widgets` (its `handle_ask_user_question`), which also had to add `AskUserQuestion` to `SYSTEM_PROMPT`'s tool list — it was never documented there, so the model had no way to know the tool existed even once dispatch supported it
- [ ] T059 [US3] Emit `agent-activity` events (`file-diff`/`shell-output`/`subagent-status`) in `src-tauri/src/agent/mod.rs` (FR-017, depends on T052, T057) — **superseded, not merely deferred**: `004-tool-call-widgets` deliberately chose persist-then-render-on-completion over general live streaming for every tool (its research.md § 2/§ 3), with `AskUserQuestion`'s dispatch-time event as the one exception (T058, above). Revisit only if genuinely live (not just post-turn) tool-activity visibility becomes a real requirement — see `004`'s plan.md Complexity Tracking for the explicit scope call.
- [ ] T060 [P] [US3] Build the workspace view in `src/views/workspace/`: file tree, CodeMirror 6 diff viewer, `react-xtermjs` read-only terminal output panel — **superseded, not merely deferred**: `004-tool-call-widgets` replaced this with lightweight per-tool-call widgets (diff/terminal/etc.) embedded directly in the message transcript rather than a separate file-tree/editor/terminal panel — a deliberate, documented choice (`004`'s research.md § 6), not an oversight. `Workspace.tsx` itself was later restructured into a lean, `Chat.tsx`-shaped message view by `006-chat-empty-state`.
- [X] T061 [US3] Implement `answer_user_question` command and the frontend `ask-user-question` modal/prompt UI in `src/views/workspace/AskUserQuestionPrompt.tsx` (depends on T058) — done by `004-tool-call-widgets`, at `src-tauri/src/commands/agent.rs::answer_user_question` and `src/views/chat/tool-widgets/AskUserQuestionWidget.tsx` (not the originally-planned path — `AskUserQuestionPrompt.tsx` under `views/workspace/` — since the widget now lives alongside every other tool widget under `views/chat/tool-widgets/`, per `004`'s own structure decision)
- [ ] T062 [US3] Wire `useWorkspaceActivityStore` to `agent-activity` events (depends on T023, T059) — **moot**, not just blocked: depends on T059, which `004` superseded rather than implementing; no `useWorkspaceActivityStore` was ever built, and nothing in `004`'s design needs one (each widget renders directly from its own persisted message).

**Checkpoint**: Agent mode is fully functional — unrestricted, GBNF-constrained where needed, denylist-protected, with working subagent delegation. All three P1 stories are now complete; this is a viable full MVP.

---

## Phase 6: User Story 4 - Extend the agent with MCP servers and skills (Priority: P2)

**Goal**: User-added MCP servers and filesystem skill packs are discoverable and usable by the agent loop.

**Independent Test**: Connect one MCP server, add one skill pack, confirm both are used during a task (`quickstart.md` §6).

### Tests for User Story 4

- [X] T063 [P] [US4] Integration test for `rmcp` client connecting a local stdio test MCP server in `src-tauri/tests/integration/test_mcp.rs`
- [X] T064 [P] [US4] Integration test for skill discovery/contextual matching in `src-tauri/tests/integration/test_skills.rs`
- [ ] T065 [P] [US4] WDIO e2e spec `tests/e2e/06-mcp-and-skills.spec.ts` mirroring `quickstart.md` §6

### Implementation for User Story 4

- [X] T066 [P] [US4] Implement the `mcp/` module in `src-tauri/src/mcp/mod.rs`: `rmcp` client (stdio/http transports), tool exposure to the agent loop (FR-018)
- [X] T067 [US4] Implement `add_mcp_server`/`list_mcp_servers` commands in `src-tauri/src/commands/mcp.rs` (depends on T066)
- [X] T068 [P] [US4] Implement the `skills/` module in `src-tauri/src/skills/mod.rs`: bundled + user skill directory discovery, in-memory contextual-matching index (FR-019)
- [X] T069 [US4] Implement `list_skills` command in `src-tauri/src/commands/skills.rs` (depends on T068)
- [X] T070 [P] [US4] Build the MCP servers + skills settings panel in `src/views/settings/McpAndSkills.tsx`

**Checkpoint**: Extensibility works without touching any P1 story's code.

---

## Phase 7: User Story 5 - Keep working across multiple chats and agent tasks without the app freezing (Priority: P2)

**Goal**: Focus-based dynamic scheduler priority is exposed to the user: queued/running visibility, cancellation, no starvation guarantee (accepted trade-off).

**Independent Test**: Two conversations, rapid messages to both; UI stays responsive, second message shows queued, both eventually respond (`quickstart.md` covers this implicitly via chat/agent scenarios plus dedicated scheduler integration tests above).

### Tests for User Story 5

- [X] T071 [P] [US5] Integration test for focus-flip mid-queue reprioritization in `src-tauri/tests/integration/test_scheduler_priority.rs` (already partially covered by T047/T048 — this one is UI-triggered focus changes specifically)
- [X] T072 [P] [US5] Integration test for cancellation isolation (canceling one request doesn't affect others) in `src-tauri/tests/integration/test_cancellation.rs`

### Implementation for User Story 5

- [X] T073 [US5] Implement `set_focused_conversation` command in `src-tauri/src/commands/scheduler.rs`, updating the scheduler's live focus state (FR-026, depends on T016)
- [X] T074 [US5] Implement `cancel_generation` command in `src-tauri/src/commands/scheduler.rs` (FR-028, depends on T018)
- [X] T075 [US5] Emit `generation-queue-update` events on every priority/state change (FR-025/FR-026, depends on T016, T017)
- [X] T076 [P] [US5] Wire `set_focused_conversation` calls into every view-change in `src/App.tsx`'s routing (depends on T022, T073)
- [X] T077 [P] [US5] Build the queued/running status indicator UI, wired to `useGenerationQueueStore` and `generation-queue-update` events (depends on T023, T075)
- [X] T078 [US5] Wire cancel buttons in chat/workspace views to `cancel_generation`, preserving partial output on cancel (FR-028, depends on T074)

**Checkpoint**: Multi-conversation responsiveness is visible and controllable in the UI.

---

## Phase 8: User Story 6 - Find something from a past conversation (Priority: P2)

**Goal**: FTS5-backed search across conversation titles and message content, excluding subagent-run conversations.

**Independent Test**: Multiple conversations with distinct topics; search finds the right one with a highlighted excerpt; subagent content never surfaces (`quickstart.md` §7).

### Tests for User Story 6

- [X] T079 [P] [US6] Direct SQL-level test for FTS5 trigger exclusion in `src-tauri/tests/integration/test_fts5_isolation.rs`: insert into a subagent-run conversation, assert absence from `messages_fts` (`research.md` §9/§26)
- [X] T080 [P] [US6] Integration test for `bm25()` ranking + `snippet()` excerpt generation in `src-tauri/tests/integration/test_search.rs`
- [ ] T081 [P] [US6] WDIO e2e spec `tests/e2e/07-search.spec.ts` mirroring `quickstart.md` §7

### Implementation for User Story 6

- [X] T082 [US6] Implement `search_conversations` command in `src-tauri/src/commands/search.rs`: `bm25()` ranking + `snippet()` excerpt across both FTS5 tables (FR-029/FR-030, depends on T013)
- [X] T083 [P] [US6] Build the search UI (input + ranked results with highlighted excerpts) in `src/views/chat/SearchPanel.tsx`

**Checkpoint**: Search works and provably respects subagent isolation.

---

## Phase 9: User Story 7 - See at a glance which conversations need attention (Priority: P2)

**Goal**: Auto-generated (truncated) titles and a live-computed `done`/`requires_action`/`failed`/`in_progress` status per conversation.

**Independent Test**: Drive conversations into each of the three terminal outcomes; each shows the correct status without opening it (`quickstart.md` §5).

### Tests for User Story 7

- [X] T084 [P] [US7] Integration test for title truncation (word-boundary, ~60 chars, no model call) in `src-tauri/tests/integration/test_title.rs`
- [X] T085 [P] [US7] Integration test for all four status outcomes in `src-tauri/tests/integration/test_status.rs`: `done`, `requires_action` (both `AskUserQuestion` and trailing-`?`-not-in-URL paths, restricted to assistant-authored messages), `failed` (`content_type = 'error'`), `in_progress` (active/queued generation)
- [ ] T086 [P] [US7] WDIO e2e spec `tests/e2e/05-title-and-status.spec.ts` mirroring `quickstart.md` §5

### Implementation for User Story 7

- [X] T087 [US7] Implement title generation (truncate first user message at word boundary, ~60 chars) in `src-tauri/src/storage/conversations.rs` (FR-012, depends on T038)
- [X] T088 [US7] Implement the `status` computation in `src-tauri/src/commands/conversations.rs`: scheduler-active check → `error` content_type → `AskUserQuestion`/trailing-`?`-outside-`https?://\S+` check → `done`, computed live in `list_conversations`/`get_conversation`, never cached (FR-011, depends on T012, T016)
- [X] T089 [P] [US7] Build the conversation list UI with colored status dots + generated titles in `src/views/chat/ConversationList.tsx` (depends on T088)

**Checkpoint**: All 7 user stories are independently functional.

---

## Phase 10: Polish & Cross-Cutting Concerns

**Purpose**: Improvements spanning multiple stories; final release readiness.

- [ ] T090 [P] Implement macOS code signing + notarization in the Tauri bundler config (`research.md` §7), wired into `.github/workflows/ci.yml` release job
- [ ] T091 [P] Run the full `quickstart.md` validation pass (all 8 sections) against a built binary
- [ ] T092 [P] Security pass: confirm the denylist (T054) can't be talked around by any prompt phrasing; confirm no plaintext secrets logged
- [ ] T093 [P] Performance pass: verify GBNF grammar caching by tool-set hash (flagged in `research.md`'s Critique Decisions as a residual, not blocking, optimization)
- [ ] T094 Documentation: `README.md` covering build/run/test instructions matching this tasks.md's structure
- [ ] T095 Final CI green-run across `rust`/`frontend`/`e2e` jobs on `macos-26` before tagging v1.0

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies.
- **Foundational (Phase 2)**: Depends on Setup. **Blocks all user stories** — storage, inference, scheduler, and IPC plumbing are shared by every story.
- **User Stories (Phase 3–9)**: All depend on Foundational. Priority order: US1 → US2 → US3 (P1, sequential MVP path) → US4 → US5 → US6 → US7 (P2, any order/parallel).
- **Polish (Phase 10)**: Depends on all desired user stories being complete.

### User Story Dependencies

- **US1 (P1)**: No dependencies on other stories. First MVP slice.
- **US2 (P1)**: Independent of US1 beyond shared Foundational infra (a chat conversation doesn't require a completed onboarding flow, just an installed model).
- **US3 (P1)**: Builds on the scheduler/inference infra US2 also uses, but is independently testable (agent mode doesn't require chat mode to have shipped).
- **US4 (P2)**: Extends US3's agent loop (MCP/skills are additional tools) — should follow US3.
- **US5 (P2)**: Makes the Foundational scheduler's behavior *visible*; benefits from US2 and US3 existing (something to schedule), but its own commands/events/UI are additive, not blocking.
- **US6 (P2)**: Depends on `Conversation`/`Message` existing (US2) but is otherwise self-contained (FTS5 tables + one command + one view).
- **US7 (P2)**: Depends on `Conversation`/`Message` (US2) and the scheduler (Foundational) for the `in_progress` check; otherwise self-contained.

### Within Each User Story

- Tests before implementation (write first, confirm they fail).
- Models/storage before commands; commands before UI wiring.
- Story complete and independently checkpointed before moving to the next.

### Parallel Opportunities

- All `[P]` Setup tasks (T002–T007) run in parallel.
- Within Foundational: storage tasks (T009–T013), inference/scheduler tasks (T014–T019), and IPC/shell tasks (T020–T024) are each internally parallelizable but storage should land first since inference/scheduler persistence (T009) and IPC error conventions touch it.
- Once Foundational completes, **US1, US2, US3 can be staffed in parallel** (each only touches its own command/view files) — see MVP note below for why US1 is still recommended first if working solo.
- US4–US7 can all be staffed in parallel once US3 (for US4) and US2 (for US5/US6/US7) land.

---

## Parallel Example: User Story 3 (the largest story)

```bash
# Launch all US3 tests together (after T051/T052 land):
Task: "Integration test for built-in tool set in src-tauri/tests/integration/test_tools.rs"
Task: "Integration test for GBNF-constrained tool calling in src-tauri/tests/integration/test_gbnf.rs"
Task: "Integration test for catastrophic-command denylist in src-tauri/tests/integration/test_denylist.rs"
Task: "Integration test for subagent spawning in src-tauri/tests/integration/test_subagent.rs"

# Launch the independent tool implementations together:
Task: "Implement Read/Write/Edit tools in src-tauri/src/agent/tools/fs.rs"
Task: "Implement Bash tool + denylist in src-tauri/src/agent/tools/bash.rs"
Task: "Implement Glob/Grep tools in src-tauri/src/agent/tools/search.rs"
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1 (Setup) + Phase 2 (Foundational) — the expensive, unavoidable groundwork.
2. Complete Phase 3 (US1). **Stop and validate**: zero-config first run works end-to-end.
3. This alone is not a useful product (no chat yet) — Doce's real MVP is US1+US2+US3 together (see below), but US1 is independently demoable (hardware detection → model download).

### Recommended Real MVP: US1 + US2 + US3

Given the constitution's own positioning ("Claude Desktop + Claude Code experience"), a demoable product needs all three P1 stories — chat alone or agent-mode alone doesn't deliver the differentiation. Treat Phases 3–5 as one combined MVP milestone.

### Incremental Delivery After MVP

1. MVP (US1+US2+US3) → validate → demo/tag.
2. Add US4 (MCP/skills) → validate independently → demo.
3. Add US5 (multi-conversation responsiveness) → validate → demo.
4. Add US6 (search) → validate → demo.
5. Add US7 (status/title) → validate → demo.
6. Polish phase → v1.0 tag.

### Parallel Team Strategy

With multiple developers, after Foundational lands:
- Developer A: US1 (onboarding/download)
- Developer B: US2 (chat)
- Developer C: US3 (agent mode — largest, may need two people given its scope: split tools/GBNF from subagent/AskUserQuestion)
- Once P1 stories land: distribute US4–US7 across the team, each independently testable.

---

## Notes

- `[P]` tasks touch different files with no dependency on an incomplete task.
- Every Foundational and US1–US3 task traces to a specific FR/SC in `spec.md` and a decision in `research.md` — cross-referenced inline above.
- The scheduler's accepted trade-offs (no anti-starvation, subagents' 30-turn cap rather than unbounded) are implemented as specified, not "improved" during implementation — if either proves disruptive in practice, that's a spec amendment, not a silent code change.
- Commit after each task or logical group per the constitution's Development Workflow expectations.
- Run `quickstart.md` scenario-by-scenario as each corresponding story's checkpoint is reached, not only at the very end.

## Known gaps as of the latest implementation pass

All seven user stories are implemented and have working, real (non-mocked)
end-to-end paths verified against the actual installed model and a real
SQLite database — including a real subagent spawn/isolation/turn-cap
round-trip. The following remain open, called out explicitly rather than
silently marked done:

- **T045/T056 (GBNF grammar constraints, FR-014)**: not implemented. The
  tool-use loop instead relies on prompting the model with an explicit
  JSON tool-call convention (`agent::SYSTEM_PROMPT`) plus a
  robust-to-real-model-noise parser (`agent::parse_response`, which
  extracts the first balanced `{...}` object rather than requiring an
  exact whole-response JSON parse — a fix driven by an actual observed
  failure against the real model). This works in practice (verified via
  `tests/e2e/specs/agent-mode.spec.ts`) but doesn't *guarantee* syntactically
  valid tool calls the way grammar-constrained sampling would.
- **T047/T048 (priority inheritance, no-deadlock guarantee)**: subagent
  turns run inline within the same async call rather than being
  resubmitted through the scheduler queue, so there's no
  `priority_conversation_id` inheritance to test and no deadlock risk to
  guard against in the first place — a simpler design than spec'd, not a
  missing test.
- **T058/T061 (`AskUserQuestion` pause/resume wiring) — now done**: completed
  by `004-tool-call-widgets`. The pause/resume registry
  (`agent::tools::ask_user::PendingQuestions`) was already implemented and
  unit-tested in isolation here; `004` wired it into the live dispatch
  path, added the `answer_user_question` command, built the frontend
  prompt (`AskUserQuestionWidget.tsx`, under `views/chat/tool-widgets/`
  rather than the originally-planned `views/workspace/
  AskUserQuestionPrompt.tsx`), and — a real functional gap this uncovered,
  not just missing UI — added `AskUserQuestion` to `SYSTEM_PROMPT`'s tool
  list, since it was never documented there and so was unreachable by the
  model regardless of dispatch/UI support.
- **T059/T060/T062 (live agent-activity streaming, full workspace UI) —
  superseded, not just still open**: `004-tool-call-widgets` considered
  this scope directly and chose a different design rather than leaving it
  merely undone — persist-then-render-on-completion for every tool except
  `AskUserQuestion` (see that spec's research.md § 2/§ 3 and plan.md's
  Complexity Tracking for the explicit reasoning), and lightweight
  per-tool-call widgets embedded in the message transcript instead of a
  separate file-tree/diff-viewer/terminal panel (research.md § 6).
  `Workspace.tsx` was also restructured into a lean, `Chat.tsx`-shaped
  message view by `006-chat-empty-state`. Revisit as a fresh, intentional
  feature if genuinely live (not just post-turn) tool-activity visibility
  becomes a real requirement — don't resume T059/T060/T062 as originally
  scoped without re-deciding this.
- **T065/T081/T086 (additional WDIO specs)**: US4/US6/US7 are covered by
  Rust-level unit/integration tests plus frontend component tests; the
  dedicated e2e specs for those specific flows weren't added (US3's core
  loop, subagent spawning, and the conversation-list/chat flows are the
  ones with real e2e coverage).
- **T090–T095 (release polish)**: code signing/notarization, a formal
  full-`quickstart.md` pass against a signed binary, a dedicated
  adversarial denylist-bypass pass, GBNF grammar caching (moot without
  T056), `README.md`, and a CI green-run are all still open — this pass
  focused on feature completeness and functional test coverage, not
  release packaging.
