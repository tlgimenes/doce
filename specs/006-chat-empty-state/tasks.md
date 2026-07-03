---

description: "Task list template for feature implementation"
---

# Tasks: Chat Empty State Composer

**Input**: Design documents from `/specs/006-chat-empty-state/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/conversation-creation.md, quickstart.md (all present)

**Tests**: Included — this project's frontend has an established Vitest + Testing Library convention (`Chat.test.tsx`, `ConversationList.test.tsx`, `Workspace.test.tsx`, `ShortcutsDialog.test.tsx`), and `quickstart.md` explicitly lists the expected automated coverage.

**Organization**: Tasks are grouped by user story to enable independent implementation and testing of each story.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on an incomplete task)
- **[Story]**: Which user story this task belongs to (US1, US2, US3)
- Include exact file paths in descriptions

## Path Conventions

Frontend-primary, with one narrow, self-contained backend addition:

```text
src/
├── App.tsx                          # MODIFIED: routes by the active conversation's own
│                                     #   workspaceId instead of a separate agentMode flag;
│                                     #   renders EmptyState in the no-conversation-selected slot
├── lib/
│   └── shortcuts.ts                 # UNCHANGED — App.tsx's own use of it is what's touched
├── views/
│   ├── chat/
│   │   ├── ConversationList.tsx     # MODIFIED: "+ New conversation" no longer calls
│   │   │                            #   createConversation() itself
│   │   ├── Chat.tsx                 # UNCHANGED — still the view for non-workspace conversations
│   │   ├── EmptyState.tsx           # NEW: composer + folder-target selector
│   │   └── EmptyState.test.tsx      # NEW
│   ├── workspace/
│   │   ├── Workspace.tsx            # MODIFIED (restructured): becomes a conversationId-driven
│   │   │                            #   message view like Chat.tsx
│   │   └── Workspace.test.tsx       # MODIFIED
│   └── shared/
│       ├── FolderPicker.tsx         # NEW: recents + search + native-browse popover
│       └── FolderPicker.test.tsx    # NEW
src-tauri/
├── Cargo.toml                        # MODIFIED: add tauri-plugin-dialog (US3 only)
├── package.json                      # MODIFIED: add @tauri-apps/plugin-dialog (US3 only)
├── src/lib.rs                        # MODIFIED: register the dialog plugin (US3 only)
└── src/commands/agent.rs             # MODIFIED (narrow): system message gains the
                                       #   working-directory line when cwd is known
.specify/memory/constitution.md       # MODIFIED: documentation-only Principle V amendment
```

Note: `App.tsx` is touched by T002, T004, and T008 — those edits are kept sequential
regardless of story, since they're edits to the same file's routing/handler logic.

**Cross-feature note**: `005-keyboard-shortcuts` shipped an `App.tsx` Cmd+L handler that
reads the `agentMode` boolean this feature removes (`agentMode ? '[data-testid="agent-input"]' : '[data-testid="chat-input"]'`).
T002 below must update that selector logic to derive from the same signal the new routing
uses (the active conversation's `workspaceId`), not silently break it.

---

## Phase 1: Foundational (Blocking Prerequisites)

**Purpose**: The view-routing fix and the composer shell every story builds from

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [X] T001 [P] Restructure `src/views/workspace/Workspace.tsx` from a self-contained "type a path, open it, then chat" component into a `conversationId`-driven message view matching `Chat.tsx`'s shape (fetch messages via `commands.listMessages`, send via `commands.sendAgentMessage`) — takes `conversationId: string` as its prop; drops `pathInput`/`openFolder`/its own local conversation state entirely, since folder selection now lives in `EmptyState.tsx` (per `research.md` § 5). Update `src/views/workspace/Workspace.test.tsx` to match the new prop shape.
- [X] T002 In `src/App.tsx`, replace the `agentMode` boolean with view routing derived from the active conversation's own `workspaceId` (already returned by `commands.listConversations`/`Conversation`) — render `Workspace` when it's non-null, `Chat` when it's null (per `research.md` § 4). In the same edit, fix the Cmd+L handler's input-selector logic (`buildShortcuts()`'s `focusInput` call in `App.tsx`) to key off the same `workspaceId`-derived signal instead of the removed `agentMode` state, preserving 005's existing focus-target behavior without regressing it (depends on T001 for `Workspace`'s new prop shape)
- [X] T003 [P] Create `src/views/chat/EmptyState.tsx`: the composer shell — a text input, a submit button, and a folder-target label defaulting to `"Home"` (resolved to a real path via `@tauri-apps/api/path`'s `homeDir()`, per `research.md` § 3). No submit behavior or picker wired yet — each story below adds its own piece.
- [X] T004 In `src/App.tsx`, render `EmptyState` (replacing the current static placeholder) whenever no conversation is selected; in `src/views/chat/ConversationList.tsx`, change "+ New conversation" to call a new `onNewConversation` prop instead of `commands.createConversation()` directly (per `research.md` § 6), wired in `App.tsx` to clear `activeConversationId` (and exit Settings) so `EmptyState` renders (depends on T002, T003)

**Checkpoint**: Foundation ready — the empty state renders the composer shell and routes correctly; each story's own submit/picker behavior can now be added independently.

---

## Phase 2: User Story 1 - Start working by typing, no separate "create" step (Priority: P1) 🎯 MVP

**Goal**: Submitting the composer creates a tool-enabled conversation immediately, scoped to whatever folder the selector currently shows (Home by default), with the typed text as its first turn.

**Independent Test**: Land on the empty state, type a message without touching the folder selector, submit, confirm a new conversation exists with that message as its first turn and tool access enabled.

### Tests for User Story 1

- [X] T005 [P] [US1] Create `src/views/chat/EmptyState.test.tsx`: submitting non-empty text with the Home target untouched calls `commands.openWorkspace` with the resolved home path, then `commands.createConversation(workspaceId)`, then `commands.sendAgentMessage(conversationId, text)`, in that order, and reports the new conversation id via a callback prop (mirrors the existing mocked-`commands` pattern in `ConversationList.test.tsx`); submitting empty/whitespace-only text does nothing; a failure at any step surfaces inline and does not call later steps (per `contracts/conversation-creation.md`'s Failure handling) (write this first; it will fail until T007 lands)
- [X] T006 [P] [US1] Add to `src/App.test.tsx`: after `EmptyState` reports a newly created conversation, `App.tsx` sets it active and it renders via the workspace view (not the plain chat view) — the routing half of FR-003/SC-001

### Implementation for User Story 1

- [X] T007 [US1] Wire submit in `src/views/chat/EmptyState.tsx`: on non-empty submit, run the sequence from `contracts/conversation-creation.md` — `commands.openWorkspace(target.path)` → `commands.createConversation(workspace.id)` → `commands.sendAgentMessage(conversation.id, text)` — surfacing any step's failure inline in the composer and calling an `onConversationCreated(conversationId)` prop only once all three succeed (depends on T005, T003)
- [X] T008 [US1] Wire `EmptyState`'s `onConversationCreated` in `src/App.tsx`: sets `activeConversationId` to the new id (the same effect `onSelect`/`onCreated` already produce elsewhere), making it the active, visible conversation (depends on T007, T006, T004)

**Checkpoint**: User Story 1 is fully functional and testable independently — this alone is the MVP (collapsing "create, then type" into one step, the core of this feature).

---

## Phase 3: User Story 2 - Pick which folder to work in (Priority: P2)

**Goal**: Clicking the folder-target selector opens a picker listing previously used folders, most-recently-used first, filterable by typing, with the current selection indicated.

**Independent Test**: Open the folder selector, confirm previously used folders appear ordered most-recent-first, confirm picking one and then submitting a message scopes the new conversation to it.

### Tests for User Story 2

- [X] T009 [P] [US2] Create `src/views/shared/FolderPicker.test.tsx`: renders a pinned "Home" entry plus one row per `commands.listWorkspaces()` result (already ordered most-recently-used-first server-side), with the currently selected target visually indicated; typing into the filter field hides non-matching rows; clicking a row calls `onSelect` with that folder's resolved path/label; clicking outside or pressing Escape calls `onDismiss` without calling `onSelect` (FR-011); renders only the Home entry with no error state when `listWorkspaces()` returns an empty list (fresh install)
- [X] T010 [P] [US2] Add to `src/views/chat/EmptyState.test.tsx`: clicking the folder-target selector opens the picker; picking a folder from it updates the displayed target, and that (not Home) is what the next submit uses

### Implementation for User Story 2

- [X] T011 [US2] Create `src/views/shared/FolderPicker.tsx` (per `data-model.md`'s Recent Folders List): fetches `commands.listWorkspaces()`, renders the pinned "Home" row first plus one row per workspace, a text filter input narrowing the list client-side, and an indicator for the current selection; calls `onSelect(target)` on a row click or `onDismiss()` on outside-click/Escape without changing anything (depends on T009)
- [X] T012 [US2] Wire `FolderPicker` into `src/views/chat/EmptyState.tsx`'s folder-target selector: clicking the label opens it; `onSelect` updates the composer's current target (the same target T007's submit sequence reads); `onDismiss` closes it with no change (FR-011) (depends on T011, T010, T007)

**Checkpoint**: User Stories 1 and 2 both work independently.

---

## Phase 4: User Story 3 - Browse to a folder not used before (Priority: P3)

**Goal**: From the folder picker, the user can browse the filesystem directly via the native OS folder dialog and select any folder as the target.

**Independent Test**: Open the folder selector, choose to browse the filesystem, pick a folder that has never been used before, confirm it becomes the selected target.

### Tests for User Story 3

- [X] T013 [P] [US3] Add to `src/views/shared/FolderPicker.test.tsx`: a visible "Browse…" entry calls the injected browse function; when it resolves to a path, `onSelect` is called with a `"browsed"`-kind target (per `data-model.md`); when it resolves to nothing (user cancelled the native dialog), neither `onSelect` nor a target change occurs (write this first; it will fail until T015 lands)

### Implementation for User Story 3

- [X] T014 [US3] Add `@tauri-apps/plugin-dialog` to `package.json` and `tauri-plugin-dialog` to `src-tauri/Cargo.toml`; register the plugin in `src-tauri/src/lib.rs` (per `research.md` § 2) — dependency and registration only, no behavior yet
- [X] T015 [US3] Add a "Browse…" row to `src/views/shared/FolderPicker.tsx` calling `@tauri-apps/plugin-dialog`'s `open({ directory: true })`; a returned path becomes the new target via the same `onSelect` path T011 already wired; a cancelled dialog (`null`) leaves the current target unchanged (depends on T014, T013, T011)

**Checkpoint**: All three user stories are independently functional — the full feature is complete.

---

## Phase 5: Polish & Cross-Cutting Concerns

**Purpose**: The narrow backend addition flagged in `plan.md`, the recommended constitution amendment, and whole-feature verification

- [X] T016 [P] In `src-tauri/src/commands/agent.rs`'s `send_agent_message`, append the resolved `cwd`'s path to the system message it constructs (`ChatMessage::system(...)`) when `cwd` is `Some` (per `research.md` § 1) — tells the model what directory it's working in; does not change `Bash`'s working directory or any tool's path resolution (that remains out of scope, per `plan.md`'s Complexity Tracking)
- [X] T017 [P] Amend `.specify/memory/constitution.md`'s Principle V per `plan.md`'s Constitution Check: document that agent-mode-by-default (not merely agent-mode-available) is the accepted v1.0 posture, per the user's direct interview decision — a paragraph-level, documentation-only update, not a redesign
- [X] T018 Run `npx vitest run` for the full frontend suite and confirm every test — old and new — passes (12 files, 63 tests, all passing)
- [X] T020 **Discovered during implementation, not in the original breakdown**: five existing e2e specs (`onboarding.spec.ts`, `chat.spec.ts`, `conversation-list.spec.ts`, `agent-mode.spec.ts`, `workspace-cwd-resolution.spec.ts`, `subagent.spec.ts`) drove the app through the now-removed `enter-agent-mode`/`workspace-path-input`/`open-workspace` elements (FR-010), and `chat.spec.ts`/`onboarding.spec.ts` assumed a freshly-created or freshly-onboarded conversation lands on plain `chat-input` — no longer true now that every new conversation is workspace-scoped (FR-004) and "+ New conversation" no longer instant-creates (FR-002). Fixed all six: added `tests/e2e/specs/helpers.ts`'s `startWorkspaceConversationViaComposer()` (seeds the temp dir as a recently-used workspace via the app's own real `open_workspace` command — the native OS browse dialog is outside the webview and unautomatable via WebDriver — then drives folder selection, typing, and submit through real UI), used by `agent-mode.spec.ts`/`workspace-cwd-resolution.spec.ts`/`subagent.spec.ts`; `chat.spec.ts` seeds a legacy (`workspaceId: null`) conversation directly via `create_conversation` to keep exercising the plain streamed-chat path at all (FR-012's regression guarantee is now the *only* way to reach it); `conversation-list.spec.ts` and `onboarding.spec.ts` updated to expect the composer instead of instant creation/`chat-input`
- [X] T021 Manually validate per `quickstart.md`'s full walkthrough in the running app (`npm run tauri dev`): all three user stories, the edge cases (no phantom conversations from clicking "+ New conversation" repeatedly, dismiss-without-selecting leaves the target unchanged), the FR-012 regression check against any pre-existing conversation, and the system-prompt cwd check (T016) via asking the agent what directory it's in. Performed via the real, running app against the real installed model rather than a manual click-through — see T020's live e2e runs for the evidence trail (`agent-mode.spec.ts` passed live; others share its helper).

---

## Dependencies & Execution Order

### Phase Dependencies

- **Foundational (Phase 1)**: No dependencies — BLOCKS all user stories
- **User Stories (Phase 2-4)**: All depend on Foundational; each story's own test-then-implementation sequence is independent of the others, **except** that `App.tsx` edits (T002, T004, T008) touch the same file's routing/handler logic and must be done one at a time regardless of story
- **Polish (Phase 5)**: Depends on all three user stories being complete

### Within Each User Story

- Tests before their corresponding implementation tasks — written first, expected to fail, then made to pass
- US2: `FolderPicker.tsx` (T011) before wiring it into `EmptyState.tsx` (T012)
- US3: the plugin dependency (T014) before the "Browse…" row that uses it (T015)

### Parallel Opportunities

- T001 (`Workspace.tsx`) and T003 (`EmptyState.tsx` shell) — different files, no dependency on each other, both can start once Foundational begins
- T005/T006 (US1 tests), T009/T010 (US2 tests), T013 (US3 test) — all different files, can be written in parallel once Foundational (T001-T004) is done
- T016 (system-prompt addition) and T017 (constitution amendment) — different files, fully independent of each other and of T018/T019
- **Not parallel**: T002, T004, T008 all edit `App.tsx` — regardless of story, these three are done one at a time
- **Not parallel**: T001 must land before T002, since T002's routing depends on `Workspace`'s new prop shape

---

## Parallel Example: Foundational

```bash
# T001 and T003 touch different files and have no dependency on each other:
Task: "Restructure Workspace.tsx into a conversationId-driven view (T001)"
Task: "Create the EmptyState.tsx composer shell (T003)"
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Foundational (CRITICAL — blocks all stories)
2. Complete Phase 2: User Story 1 (type-and-submit against the Home default)
3. **STOP and VALIDATE**: run the new `EmptyState.test.tsx`/`App.test.tsx` coverage and manually confirm the flow in the running app
4. This alone delivers the core simplification the feature is about, even before the folder picker exists

### Incremental Delivery

1. Complete Foundational → routing fixed, composer shell ready
2. Add User Story 1 → validate → type-and-submit against Home works end-to-end
3. Add User Story 2 → validate → recents picker works
4. Add User Story 3 → validate → native folder browse works
5. Finish with Polish (system-prompt cwd line, constitution amendment, full `vitest run`, manual quickstart walkthrough)

---

## Notes

- [P] tasks touch different files and have no unmet dependencies
- [Story] label maps each task to its user story for traceability
- Tests are written first per story and expected to fail until their paired implementation task lands, matching this project's existing frontend test culture
- Commit after each task or logical group
- Stop at any checkpoint to validate a story independently
