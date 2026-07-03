---

description: "Task list template for feature implementation"
---

# Tasks: Keyboard Shortcuts

**Input**: Design documents from `/specs/005-keyboard-shortcuts/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, quickstart.md (all present; no `contracts/` — this is a pure frontend addition with no new IPC surface, per `plan.md`'s Project Structure)

**Tests**: Included — this project's frontend has an established Vitest + Testing Library convention (`Chat.test.tsx`, `ConversationList.test.tsx`, `Workspace.test.tsx`, `button.test.tsx`), and `quickstart.md` explicitly lists the expected automated coverage. This feature adds the app's first `App.test.tsx`.

**Organization**: Tasks are grouped by user story to enable independent implementation and testing of each story.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on an incomplete task)
- **[Story]**: Which user story this task belongs to (US1, US2, US3)
- Include exact file paths in descriptions

## Path Conventions

Frontend-only, entirely within the existing `src/` tree:

```text
src/
├── App.tsx                          # MODIFIED: global keydown listener, dialog-open state
├── lib/
│   └── shortcuts.ts                 # NEW: Shortcut type + buildShortcuts() factory
├── components/
│   ├── Dialog.tsx                   # NEW: native <dialog>-based modal primitive
│   └── Dialog.test.tsx              # NEW
└── views/
    ├── chat/
    │   ├── ConversationList.tsx     # MODIFIED: forwardRef + useImperativeHandle exposing createNew
    │   └── ConversationList.test.tsx # MODIFIED
    └── shortcuts/
        ├── ShortcutsDialog.tsx      # NEW: renders the registry inside <Dialog>
        └── ShortcutsDialog.test.tsx # NEW
App.test.tsx                          # NEW (repo-root-level src/App.test.tsx)
```

Note: `App.tsx`'s `buildShortcuts()` call is touched by all three user stories (each adds its own registry entry to the same array literal). Those specific edits are kept sequential regardless of story, even though the stories themselves are otherwise independent, since they're edits to the same array in the same file.

---

## Phase 1: Foundational (Blocking Prerequisites)

**Purpose**: The shared registry and the single generic listener every story's shortcut plugs into

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [X] T001 [P] Create `src/lib/shortcuts.ts`: define the `Shortcut` type (`id`, `combo`, `metaKey`, `key`, `description`, `action`) per `data-model.md`, and a `buildShortcuts(handlers)` factory function that will return the array of entries — starts with an empty/stub shape that each story extends with its own entry
- [X] T002 In `src/App.tsx`, add the single global keydown listener (per `research.md` § 1): mounted once via a `useEffect` on `window`, iterating whatever `buildShortcuts(...)` currently returns, matching `event.metaKey` + `event.key.toLowerCase()` against each entry's `metaKey`/`key`, calling `event.preventDefault()` and the matched entry's `action()`. Also add a `showShortcutsDialog` boolean state (starts `false`) and gate the listener per FR-009: while `showShortcutsDialog` is `true`, only the entry whose `id === "show-shortcuts"` may act — every other match is ignored (depends on T001)

**Checkpoint**: Foundation ready — each story's own shortcut can now be added independently

---

## Phase 2: User Story 1 - Jump straight into typing (Priority: P1) 🎯 MVP

**Goal**: Cmd+L moves focus to the primary message input from anywhere, regardless of current focus.

**Independent Test**: Focus something else entirely (or nothing), press Cmd+L, confirm the message input is now focused.

### Tests for User Story 1

- [X] T003 [P] [US1] Create `src/App.test.tsx` covering: Cmd+L focuses `[data-testid="chat-input"]` when a conversation is active; focuses `[data-testid="agent-input"]` instead when in agent mode; has no effect when Settings is open or no conversation/workspace exists (FR-002); pressing Cmd+L while already focused in the input leaves focus undisturbed; typing a plain `"l"` without Cmd does nothing (write this first; it will fail until T004 lands)

### Implementation for User Story 1

- [X] T004 [US1] In `src/App.tsx`, add a `"focus-input"` entry to the array `buildShortcuts()` returns: its `action` reads the current `showSettings`/`agentMode`/`activeConversationId` state (the same state that already decides what renders — per `research.md` § 4) and calls `.focus()` on `document.querySelector('[data-testid="chat-input"]')` or `'[data-testid="agent-input"]'` accordingly, doing nothing if neither is present (depends on T002, T003)

**Checkpoint**: User Story 1 is fully functional and testable independently — this alone is the MVP (the highest-frequency shortcut of the three).

---

## Phase 3: User Story 2 - Start a new conversation from the keyboard (Priority: P2)

**Goal**: Cmd+N creates a new conversation and switches to it, exactly like clicking "+ New conversation".

**Independent Test**: Press Cmd+N from anywhere in the app, confirm a new, empty conversation is created and becomes the active view.

### Tests for User Story 2

- [X] T005 [P] [US2] Extend `src/views/chat/ConversationList.test.tsx`: rendering with a ref and calling `ref.current.createNew()` triggers the same `commands.createConversation` + `onCreated` flow that clicking the `"new-conversation"` button already triggers (reuses the existing mock setup)
- [X] T006 [P] [US2] Add to `src/App.test.tsx`: Cmd+N from the chat view, from Settings, and from agent mode each result in a new conversation being created and the view switching to show it (mock `ConversationList`'s exposed ref)

### Implementation for User Story 2

- [X] T007 [US2] Convert `ConversationList` in `src/views/chat/ConversationList.tsx` to `forwardRef<ConversationListHandle, ConversationListProps>`, exposing `{ createNew }` via `useImperativeHandle` (per `research.md` § 3) — the exact same function the "+ New conversation" button already calls internally, not a duplicate (depends on T005)
- [X] T008 [US2] Wire Cmd+N in `src/App.tsx`: hold a ref to `ConversationList` (using T007's exposed handle type), add a `"new-conversation"` entry to `buildShortcuts()` whose `action` calls `ref.current?.createNew()` (depends on T002, T007, T006; sequential with T004/T014 since all three touch the same `buildShortcuts()` array)

**Checkpoint**: User Stories 1 and 2 both work independently.

---

## Phase 4: User Story 3 - Discover what shortcuts exist (Priority: P3)

**Goal**: Cmd+K opens a dialog listing every shortcut and its description, dismissible via Escape or a visible close control.

**Independent Test**: Press Cmd+K from anywhere, confirm a dialog appears listing all shortcuts, then confirm it can be dismissed via Escape and via a visible control.

### Tests for User Story 3

- [X] T009 [P] [US3] Create `src/components/Dialog.test.tsx`: renders children and calls `.showModal()` when `open` becomes `true`, calls `.close()` when it becomes `false`; `onClose` fires on the native `cancel` event (Escape); `onClose` fires on a click landing on the backdrop (the `<dialog>` element itself) but NOT on a click inside the dialog's content
- [X] T010 [P] [US3] Create `src/views/shortcuts/ShortcutsDialog.test.tsx`: renders one row per entry in the shared shortcuts registry (combo + description, per FR-010), and clicking the visible close button invokes the passed `onClose`

### Implementation for User Story 3

- [X] T011 [US3] Create `src/components/Dialog.tsx` (per `research.md` § 2): a small reusable modal wrapping the native `<dialog>` element — `open: boolean`, `onClose: () => void`, `children` props; an effect syncing `open` to `.showModal()`/`.close()`; the native `cancel` event wired to call `onClose` (Escape — FR-005); a backdrop-click handler comparing `event.target` against the dialog element itself, the standard pattern for this element (depends on T009)
- [X] T012 [US3] Create `src/views/shortcuts/ShortcutsDialog.tsx`: renders `<Dialog>` wrapping a list built directly from the shared shortcuts registry passed in as a prop (per FR-010/`research.md` § 5 — no separate hardcoded description list), with a visible close button calling `onClose` (depends on T011, T010)
- [X] T013 [P] [US3] Add to `src/App.test.tsx`: Cmd+K opens the dialog; pressing Cmd+K again while open closes it (toggle, not stacked — FR-006); Escape and the close button both close it; while open, Cmd+L and Cmd+N have no effect on the conversation (FR-009); after closing, Cmd+L/Cmd+N work again
- [X] T014 [US3] Wire Cmd+K in `src/App.tsx`: add a `"show-shortcuts"` entry to `buildShortcuts()` whose `action` toggles `showShortcutsDialog`; render `<ShortcutsDialog open={showShortcutsDialog} onClose={() => setShowShortcutsDialog(false)} shortcuts={shortcuts} />` (depends on T002, T012, T013; sequential with T004/T008 since all three touch the same `buildShortcuts()` array)

**Checkpoint**: All three user stories are independently functional — the full feature is complete.

---

## Phase 5: Polish & Cross-Cutting Concerns

**Purpose**: Whole-feature verification, including the regression guarantee (FR-008) this feature must not violate

- [X] T015 Run `npx vitest run` for the full frontend suite and confirm every test — old and new — passes (10 files, 45 tests, all passing)
- [X] T016 **Deviation (stronger than planned)**: rather than a manual `npm run tauri dev` walkthrough, validated live against the real WebKit-driven app via a new `tests/e2e/specs/keyboard-shortcuts.spec.ts` (same pattern as 007's e2e coverage) — Cmd+N creates and switches to a new conversation (US2), Cmd+L focuses the chat input from elsewhere (US1), Cmd+K opens the dialog listing all three shortcuts and a second Cmd+K closes it (US3, FR-006), and Escape plus the close button both dismiss it (FR-005). All 4 assertions passed (4m7.8s). The dialog-open gating (FR-009) and agent-mode input targeting are covered by `App.test.tsx`'s unit suite instead, which already exercises both.

---

## Dependencies & Execution Order

### Phase Dependencies

- **Foundational (Phase 1)**: No dependencies — BLOCKS all user stories
- **User Stories (Phase 2-4)**: All depend on Foundational; each story's own test-then-implementation sequence is independent of the others, **except** that the `App.tsx` edits adding an entry to `buildShortcuts()` (T004, T008, T014) touch the same array and must be done one at a time regardless of which story they belong to
- **Polish (Phase 5)**: Depends on all three user stories being complete

### Within Each User Story

- Tests before their corresponding implementation tasks — written first, expected to fail, then made to pass
- US2: `ConversationList`'s ref (T007) before wiring Cmd+N to it in `App.tsx` (T008)
- US3: `Dialog.tsx` (T011) before `ShortcutsDialog.tsx` (T012) before wiring Cmd+K in `App.tsx` (T014)

### Parallel Opportunities

- T001 (shortcuts.ts) has no dependencies — starts immediately
- T003 (US1 test) and T005/T006 (US2 tests) and T009/T010 (US3 tests) — all different files, can be written in parallel once Foundational (T001, T002) is done
- T007 (ConversationList ref) and T011 (Dialog.tsx) — different files, can be implemented in parallel by different people even though they're different stories
- **Not parallel**: T004, T008, T014 all edit `App.tsx`'s `buildShortcuts()` array — regardless of story, these three are done one at a time
- **Not parallel**: T002 must land before any of T004/T008/T014, since they all extend the listener it sets up

---

## Parallel Example: Foundational + early story tests

```bash
# Once T001 lands, story tests can be written in parallel (different files):
Task: "Write App.test.tsx Cmd+L coverage (T003)"
Task: "Extend ConversationList.test.tsx for the createNew ref (T005)"
Task: "Write Dialog.test.tsx (T009)"
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Foundational (CRITICAL — blocks all stories)
2. Complete Phase 2: User Story 1 (Cmd+L)
3. **STOP and VALIDATE**: run the new `App.test.tsx` suite and manually confirm Cmd+L in the running app
4. This alone ships the single highest-frequency shortcut of the three

### Incremental Delivery

1. Complete Foundational → shared registry + generic listener ready
2. Add User Story 1 → validate → Cmd+L works
3. Add User Story 2 → validate → Cmd+N works
4. Add User Story 3 → validate → Cmd+K + dialog work, including the dialog-open gating
5. Finish with Polish (full `vitest run`, manual quickstart walkthrough, regression check)

---

## Notes

- [P] tasks touch different files and have no unmet dependencies
- [Story] label maps each task to its user story for traceability
- Tests are written first per story and expected to fail until their paired implementation task lands, matching this project's existing frontend test culture
- Commit after each task or logical group
- Stop at any checkpoint to validate a story independently
