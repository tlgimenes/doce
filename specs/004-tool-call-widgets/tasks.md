---

description: "Task list template for feature implementation"
---

# Tasks: Tool Call Widgets

**Input**: Design documents from `/specs/004-tool-call-widgets/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/tool-widgets.md, quickstart.md (all present)

**Tests**: Included — this project's established convention on both sides: Rust `#[cfg(test)]` for every touched backend module, Vitest + Testing Library for every frontend component.

**Organization**: Tasks are grouped by user story to enable independent implementation and testing of each story.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on an incomplete task)
- **[Story]**: Which user story this task belongs to (US1, US2, US3, US4)
- Include exact file paths in descriptions

## Path Conventions

```text
src-tauri/src/
├── agent/
│   └── dispatch.rs                     # MODIFIED: execute() returns ToolOutcome{model_text, detail}
├── commands/
│   └── agent.rs                        # MODIFIED: persists tool_call/tool_result pairs; AskUserQuestion
│                                        #   interception arm; PendingQuestions state; answer_user_question
└── lib.rs                              # MODIFIED: manage(PendingQuestions::default())

src/
├── lib/
│   └── ipc.ts                          # MODIFIED: ToolResultDetail union, answer_user_question,
│                                        #   ask-user-question event
├── components/
│   ├── MessageContent.tsx              # NEW: shared per-message widget dispatch (FR-013)
│   └── MessageContent.test.tsx         # NEW
└── views/
    ├── chat/
    │   ├── Chat.tsx                    # MODIFIED: renders via MessageContent
    │   └── tool-widgets/
    │       ├── EditDiffWidget.tsx(.test.tsx)        # NEW — US1
    │       ├── BashWidget.tsx(.test.tsx)            # NEW — US2
    │       ├── AskUserQuestionWidget.tsx(.test.tsx) # NEW — US3
    │       ├── ReadWidget.tsx(.test.tsx)            # NEW — US4
    │       ├── WriteWidget.tsx(.test.tsx)           # NEW — US4
    │       ├── SearchResultsWidget.tsx(.test.tsx)   # NEW — US4 (Glob + Grep)
    │       ├── TaskWidget.tsx(.test.tsx)            # NEW — US4
    │       └── UnknownToolWidget.tsx                # NEW — Foundational (FR-011 fallback)
    └── workspace/
        └── Workspace.tsx               # MODIFIED: renders via MessageContent

tests/e2e/specs/
└── tool-call-widgets.spec.ts           # NEW — Polish
```

Note: `dispatch.rs`'s `execute()` return-type change (T001) is atomic across
all six existing match arms (Rust match exhaustiveness ties them to the
same signature) — it cannot be split per story despite Read/Write/Edit/
Bash/Glob/Grep belonging to different stories. `MessageContent.tsx`'s
dispatch switch (T004, extended by T009/T013/T021/T033) is touched by
every story for the same reason `App.tsx`'s `buildShortcuts()` array was
in `005`/`006` — sequential regardless of story.

**Deviation from strict TDD ordering**: because T001 is atomic, the
Rust tests for each tool's specific `ToolOutcome` shape (T006, T010, T022,
T023) are written *against* the already-multi-tool-aware `execute()` from
T001, not before a story-scoped implementation that doesn't yet exist —
they're real regression tests proving each tool's specific shape, just not
literally red-before-T001-lands the way a story-isolated change would be.
Frontend widget tests keep the normal write-first-expect-fail-then-implement
shape per widget.

---

## Phase 1: Foundational (Blocking Prerequisites)

**Purpose**: The structured tool-outcome data, its persistence as real message rows, and the shared widget-dispatch component every story's widget plugs into

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [X] T001 In `src-tauri/src/agent/dispatch.rs`, add a `ToolOutcome { model_text: String, detail: serde_json::Value }` struct and rewrite every existing match arm of `execute()` (`Read`, `Write`, `Edit`, `Bash`, `Glob`, `Grep`) to return it instead of a bare `String` — `model_text` is exactly today's existing formatted string (unchanged model-facing behavior), `detail` is that tool's shape from `data-model.md` (`{toolName, ...fields, outcome: {ok, ...} }`, or `{matches: [...]}` for `Glob`/`Grep`, which never fail at this level) (per research.md § 4; atomic across all six arms, not story-separable)
- [X] T002 In `src-tauri/src/commands/agent.rs`, add a `persist_tool_call_and_result(conn, conversation_id, tool_name, arguments, outcome: ToolOutcome)` helper that inserts a `tool_call` row (`content = {"arguments": ...}`) immediately followed by a `tool_result` row (`content = outcome.detail`), both at the next available sequence numbers (per `data-model.md`'s row-shape table); wire it into `execute_top_level_tool`'s `dispatch::execute` call site so every dispatch-routed tool call now persists both rows (depends on T001)
- [X] T003 [P] Add the `ToolResultDetail` discriminated union to `src/lib/ipc.ts` (one variant per tool per `data-model.md`, discriminated on `toolName`), plus an `UnknownToolDetail` fallback shape
- [X] T004 Create `src/components/MessageContent.tsx`: dispatches each `Message` by `contentType`/`toolName` — `"text"` and `"error"` render exactly as `Chat.tsx`/`Workspace.tsx` do today (moved, not changed); `"tool_call"` renders nothing standalone (research.md § 5); `"tool_result"` renders the matching widget, falling back to a new `UnknownToolWidget.tsx` (tool name + a readable rendering of its arguments/outcome, per FR-011) for any `toolName` without a dedicated widget yet — which, until the stories below land, is every `toolName` (depends on T003)
- [X] T005 Wire `src/views/chat/Chat.tsx` and `src/views/workspace/Workspace.tsx` to render each message via `<MessageContent message={m} />` instead of their current independent inline JSX for the message body (FR-013), removing the now-duplicated markup from both (depends on T004)

**Checkpoint**: any real tool call in agent mode now persists two real message rows and renders — as the fallback widget, since no dedicated widget exists yet. This alone already fixes "tool calls render nothing at all."

---

## Phase 2: User Story 1 - See a file edit as a diff, not raw text (Priority: P1) 🎯 MVP

**Goal**: A file edit renders as a real, labeled diff — not plain text, not raw data.

**Independent Test**: Have the agent edit a file and confirm the resulting message renders as a labeled diff (file path plus distinguishable added/removed lines).

### Tests for User Story 1

- [X] T006 [P] [US1] Add Rust tests in `dispatch.rs` for the `Edit` arm: a successful edit's `ToolOutcome.detail` matches `data-model.md`'s `Edit` shape (`filePath`, `oldString`, `newString`, `replaceAll`, `outcome.ok == true`); an edit whose `old_string` isn't found in the file produces `outcome.ok == false` with a non-empty `error` (write this first; it will fail until T001 lands — see the Deviation note above for why this isn't a pre-T001 red test)
- [X] T007 [P] [US1] Create `src/views/chat/tool-widgets/EditDiffWidget.test.tsx`: a success detail renders a labeled diff (file path visible, added and removed lines visually/structurally distinguishable per FR-002); a failure detail (`outcome.ok === false`) renders a failed-edit state, not an empty or misleading diff (spec.md Acceptance Scenario 2)

### Implementation for User Story 1

- [X] T008 [US1] Add the `diff` npm package to `package.json`; create `src/views/chat/tool-widgets/EditDiffWidget.tsx` using `diffLines(detail.oldString, detail.newString)` to render the diff, with a distinct failure state for `outcome.ok === false` (depends on T007, T006)
- [X] T009 [US1] Wire `EditDiffWidget` into `MessageContent.tsx`'s dispatch for `toolName === "Edit"` (depends on T008, T004; sequential with T013/T021/T033 since all touch the same dispatch switch)

**Checkpoint**: User Story 1 is fully functional and testable independently — this alone is the MVP (the highest-value widget per spec.md's own prioritization).

---

## Phase 3: User Story 2 - See shell commands and their output clearly (Priority: P2)

**Goal**: A shell command renders with its command and output shown together, terminal-style, success/failure clear at a glance.

**Independent Test**: Have the agent run a shell command and confirm the message renders the command and output in a distinct, terminal-like presentation, with success/failure visible without reading the output text.

### Tests for User Story 2

- [X] T010 [P] [US2] Add Rust tests in `dispatch.rs` for the `Bash` arm: a successful command's `ToolOutcome.detail` matches `data-model.md`'s `Bash` shape (`command`, `timeoutMs`, `outcome.ok == true`, `exitCode`, `stdout`, `stderr`); a non-zero-exit command still has `outcome.ok == true` with `exitCode != 0` (a completed-but-failed run, not a dispatch failure — per `contracts/tool-widgets.md`'s Failure handling); a denylist-rejected command (e.g. `rm -rf ~`) produces `outcome.ok == false` (write this first; it will fail until T001 lands)
- [X] T011 [P] [US2] Create `src/views/chat/tool-widgets/BashWidget.test.tsx`: command and stdout/stderr render together, monospaced, distinguishable from prose (FR-003); `exitCode == 0` vs `!= 0` are visually distinguishable without reading the output; long output is truncated or collapsible rather than rendered in full (FR-004)

### Implementation for User Story 2

- [X] T012 [US2] Create `src/views/chat/tool-widgets/BashWidget.tsx` (depends on T011)
- [X] T013 [US2] Wire `BashWidget` into `MessageContent.tsx`'s dispatch for `toolName === "Bash"` (depends on T012, T004; sequential with T009/T021/T033)

**Checkpoint**: User Stories 1 and 2 both work independently.

---

## Phase 4: User Story 3 - Answer the agent's clarifying questions inline (Priority: P3)

**Goal**: `AskUserQuestion` pauses the task and shows a real interactive prompt; answering resumes the task; the answer persists as read-only afterward.

**Independent Test**: Have the agent ask a clarifying question and confirm the user can select an option (or options) via the rendered prompt, with the task visibly continuing afterward.

### Tests for User Story 3

- [X] T014 [P] [US3] Add a Rust test extending `agent/tools/ask_user.rs`'s existing `PendingQuestions` coverage: dispatching an `AskUserQuestion` tool call registers a pending question and the resulting future only resolves once `PendingQuestions::answer` is called with the matching `question_id` (write this first; it will fail until T017/T018 land)
- [X] T015 [P] [US3] Add a Rust test for `answer_user_question`: calling it with an unknown or already-answered `questionId` returns an error rather than succeeding silently (FR-009's guard; `PendingQuestions::answer`'s one-shot-consume semantics are already unit-tested in isolation — this tests the command wrapping it) (write this first; it will fail until T019 lands)
- [X] T016 [P] [US3] Create `src/views/chat/tool-widgets/AskUserQuestionWidget.test.tsx`: renders clickable options; single- vs multi-select is visually indicated per `multiSelect` (FR-008); clicking an option (or confirming a multi-select) calls `answer_user_question`; once the detail's `answer` is non-null, renders a read-only "you chose: …" state and does not render clickable options anymore (FR-009)

### Implementation for User Story 3

- [X] T017 [US3] Add a `PendingQuestions` managed Tauri state in `src-tauri/src/lib.rs` (`.manage(PendingQuestions::default())`, matching `ActiveGenerations`/`InferenceState`'s existing pattern)
- [X] T018 [US3] In `execute_top_level_tool` (`commands/agent.rs`), add an `AskUserQuestion` interception arm before the fallthrough to `dispatch::execute` (matching how `Task` is already special-cased): persists a `tool_call` row plus a pending `tool_result` row (`answer: null`), registers with `PendingQuestions`, emits `ask-user-question` (per `contracts/tool-widgets.md`), awaits the oneshot receiver, then updates the `tool_result` row's `content` with the resolved `answer` before returning the tool result text to the loop (depends on T014, T017, T002)
- [X] T019 [US3] Implement the `answer_user_question` Tauri command in `commands/agent.rs`: looks up `PendingQuestions` from state, calls `.answer(question_id, answer)`, returns an error if it returns `false` (depends on T015, T017)
- [X] T020 [US3] Add `answer_user_question`'s binding and the `ask-user-question` event type to `src/lib/ipc.ts`; create `src/views/chat/tool-widgets/AskUserQuestionWidget.tsx` (depends on T016)
- [X] T021 [US3] Wire `AskUserQuestionWidget` into `MessageContent.tsx`'s dispatch for `toolName === "AskUserQuestion"` (depends on T020, T004; sequential with T009/T013/T033)

**Checkpoint**: User Stories 1-3 all work independently.

---

## Phase 5: User Story 4 - Recognize other tool activity at a glance (Priority: P4)

**Goal**: `Read`, `Write`, `Glob`/`Grep`, and `Task` each render as their own compact, recognizable widget; any tool without one still renders legibly.

**Independent Test**: Have the agent read a file, write a file, search the codebase, and delegate to a subagent, confirming each renders as its own distinct widget; exercise a tool with no dedicated widget and confirm it still renders legibly.

### Tests for User Story 4

- [X] T022 [P] [US4] Add Rust tests in `dispatch.rs` for the `Read`/`Write`/`Glob`/`Grep` arms: success and failure shapes for `Read`/`Write` (per `data-model.md`); `Glob`/`Grep` with matches and with zero matches (write this first; it will fail until T001 lands)
- [X] T023 [P] [US4] Add a Rust test in `commands/agent.rs` (or `agent/mod.rs`, wherever `Task` delegation is tested today): delegating via `Task` persists a `tool_call`+`tool_result` pair on the *parent* conversation with `detail.state == "complete"` (research.md § 2 — synchronous execution means `"running"` is never observed this pass); the subagent's own tool activity persists only under its own conversation row, never appearing in the parent's messages (FR-015/SC-008 regression guard, extending `001`'s existing subagent-isolation tests)
- [X] T024 [P] [US4] Create `src/views/chat/tool-widgets/ReadWidget.test.tsx` and `WriteWidget.test.tsx`: each renders a compact file-reference card (at minimum the file path); visually distinct from each other and from a plain reply (FR-005/FR-006)
- [X] T025 [P] [US4] Create `src/views/chat/tool-widgets/SearchResultsWidget.test.tsx` covering both `Glob` and `Grep` details: renders a match list (files and/or matching locations), not an undifferentiated dump (FR-007); renders a legible zero-matches state
- [X] T026 [P] [US4] Create `src/views/chat/tool-widgets/TaskWidget.test.tsx`: renders a running/complete status indicator only — no subagent-internal content ever rendered from this widget (FR-010)
- [X] T027 [P] [US4] Extend `MessageContent.test.tsx`: a `tool_result` row with an unrecognized `toolName` renders `UnknownToolWidget` showing the tool's name and a readable rendering of its input/output, not blank, broken, or silently dropped (SC-004)

### Implementation for User Story 4

- [X] T028 [US4] Add a `Task`-delegation persistence call in `execute_top_level_tool` using T002's `persist_tool_call_and_result` helper, with `detail = {toolName: "Task", prompt, subagentConversationId, state: "complete"}` (depends on T023, T002)
- [X] T029 [P] [US4] Create `ReadWidget.tsx` and `WriteWidget.tsx` (depends on T024)
- [X] T030 [P] [US4] Create `SearchResultsWidget.tsx` (depends on T025)
- [X] T031 [P] [US4] Create `TaskWidget.tsx` (depends on T026, T028)
- [X] T032 [US4] Verify `UnknownToolWidget.tsx` (built in T004) satisfies T027's expectations, extending it if needed
- [X] T033 [US4] Wire `ReadWidget`/`WriteWidget`/`SearchResultsWidget`/`TaskWidget` into `MessageContent.tsx`'s dispatch (depends on T029, T030, T031, T004; sequential with T009/T013/T021)

**Checkpoint**: All four user stories are independently functional — the full feature is complete.

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Whole-feature verification

- [X] T034 [P] Run `cargo test` for the full `src-tauri` workspace, `cargo fmt --check`, and `cargo clippy --all-targets -- -D warnings`, and confirm all clean
- [X] T035 [P] Run `npx vitest run` for the full frontend suite and confirm every test — old and new — passes
- [X] T036 **Deviation (partial live coverage, honestly scoped)**: validated live against the real app/real model via T037's e2e spec — US1 (`EditDiffWidget`) and US2 (`BashWidget`) both confirmed rendering correctly from a real tool call, including the removed/added lines and the real stdout/exit-status. US3 (`AskUserQuestion`) was not live-validated in the running app — per `quickstart.md`'s own caveat, the installed small model doesn't reliably choose to call it unprompted (the same limitation noted for `007`'s subagent-delegation live check); its dispatch-level wiring (pause/register/emit/resolve) is covered instead by `commands::agent`'s real-async-DB test. US4's `Read`/`Glob`/`Grep` are exercised live indirectly (`agent-mode.spec.ts` already drives a real `Read` call through the same rendering path this feature adds), but not asserted on their specific widget output in a dedicated live run — unit/component coverage (T022/T024/T025) is the evidence for their detail-shape and rendering correctness. FR-013's cross-view check was validated at the unit level (`MessageContent.test.tsx` is the single function both `Chat.tsx` and `Workspace.tsx` call — there is no code path for them to diverge) rather than a live side-by-side, since reaching a stray tool-call message in the plain `Chat.tsx` view requires a pre-existing legacy conversation this pass doesn't fabricate.
- [X] T037 [P] Added `tests/e2e/specs/tool-call-widgets.spec.ts` covering US1 (a real edit renders a diff) and US2 (a real shell command renders terminal-style with visible exit status) against the real running app, following `agent-mode.spec.ts`'s pattern via `startWorkspaceConversationViaComposer`. Both passed live (8m46s total). Found and fixed a real bug in the shared helper along the way: it never returned to the empty-state composer before starting a second conversation in the same test session, so any spec calling it more than once (this is the first one to) would fail on the second call — fixed by having the helper click "+ New conversation" defensively at its start, benefiting every current and future caller.

---

## Dependencies & Execution Order

### Phase Dependencies

- **Foundational (Phase 1)**: No dependencies — BLOCKS all user stories
- **User Stories (Phase 2-5)**: All depend on Foundational; each story's own test-then-implementation sequence is independent of the others, **except** that `MessageContent.tsx`'s dispatch switch (T009, T013, T021, T033) is touched by every story and must be edited one at a time regardless of story
- **Polish (Phase 6)**: Depends on all four user stories being complete

### Within Each User Story

- Tests before their corresponding implementation tasks, with the Deviation note above applied to the Rust `dispatch.rs` tests specifically
- US3: `PendingQuestions` state (T017) before the dispatch interception arm (T018) and before `answer_user_question` (T019); both before the frontend widget can be meaningfully wired (T021)
- US4: `Task`'s persistence path (T028) before `TaskWidget`'s wiring (T033)

### Parallel Opportunities

- T003 (TS types) can start alongside T001/T002 (different files/languages)
- Within each story, the `[P]`-marked test tasks (different files) can be written in parallel
- T029/T030/T031 (US4's four widget components — `ReadWidget`/`WriteWidget`/`SearchResultsWidget`/`TaskWidget`) are different files and can be implemented in parallel once their respective tests exist
- T034/T035/T037 (Polish) are independent of each other
- **Not parallel**: T009, T013, T021, T033 all edit `MessageContent.tsx`'s dispatch switch — regardless of story, these four are done one at a time
- **Not parallel**: T001 must land before any story's Rust tests (T006, T010, T014/T015, T022/T023), since they test its output shapes

---

## Parallel Example: Foundational

```bash
# T003 (frontend types) has no dependency on T001/T002 (backend):
Task: "Add ToolResultDetail union to ipc.ts (T003)"
Task: "Add ToolOutcome + rewrite execute()'s six arms in dispatch.rs (T001)"
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Foundational (CRITICAL — blocks all stories; also already fixes "nothing renders at all" via the fallback widget)
2. Complete Phase 2: User Story 1 (Edit → real diff)
3. **STOP and VALIDATE**: run `EditDiffWidget.test.tsx` and `dispatch.rs`'s new `Edit` tests, then manually confirm a real file edit renders as a diff in the running app
4. This alone delivers spec.md's own highest-priority slice

### Incremental Delivery

1. Complete Foundational → persistence + fallback rendering work for every tool
2. Add User Story 1 → validate → `Edit` shows a real diff
3. Add User Story 2 → validate → `Bash` shows a real terminal-style block
4. Add User Story 3 → validate → `AskUserQuestion` is genuinely interactive
5. Add User Story 4 → validate → `Read`/`Write`/`Glob`/`Grep`/`Task` each get their own widget, fallback confirmed for anything else
6. Finish with Polish (full test suites, manual quickstart walkthrough, a real e2e spec)

---

## Notes

- [P] tasks touch different files and have no unmet dependencies
- [Story] label maps each task to its user story for traceability
- Commit after each task or logical group
- Stop at any checkpoint to validate a story independently
