---
description: "Task list for the Button slice of the shared design system"
---

# Tasks: Shared Design System — Button

**Input**: Design documents from `/specs/008-shared-design-system/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/button.md, quickstart.md

**Tests**: Included — the codebase's existing convention (e.g. `Settings.test.tsx`) is colocated Vitest + Testing Library tests, and the spec's success criteria (SC-001/SC-002) are directly testable this way.

**Scope**: This task list covers the Button component only (research.md's "Decision: Migration scope for this pass"). Checkbox/Radio/Select/Dialog/Combobox are explicitly out of scope and will get their own `/speckit-tasks` pass later.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (US1/US2/US3)

## Path Conventions

Single frontend project — all paths relative to repo root, under `src/`.

---

## Phase 1: Setup (Shared Infrastructure)

- [X] T001 Add `@radix-ui/react-slot` dependency (`npm install @radix-ui/react-slot`) — needed for `Button`'s `asChild` polymorphism (research.md)
- [X] T002 [P] Create `src/lib/cn.ts` exporting `cn(...inputs: ClassValue[]) => twMerge(clsx(inputs))`, using the already-installed `clsx` and `tailwind-merge` (research.md)

**Checkpoint**: Dependencies and the shared `cn()` helper are in place.

---

## Phase 2: Foundational (Blocking Prerequisites)

- [X] T003 Create `src/components/ui/` directory with `src/components/ui/button.tsx` scaffold (component shell + exported `ButtonProps`/`ButtonVariant`/`ButtonSize` types per contracts/button.md, no styling yet) — depends on T002

**Checkpoint**: Foundation ready — Button implementation can begin.

---

## Phase 3: User Story 1 - Every clickable element looks and feels clickable (Priority: P1) 🎯 MVP

**Goal**: A single shared `Button` component with pointer-cursor/hover/active/disabled states, styled via the existing theme tokens.

**Independent Test**: Render `<Button>` and `<Button disabled>` in isolation; hovering the enabled one shows a pointer cursor and hover style, the disabled one shows neither.

### Tests for User Story 1

- [X] T004 [US1] Write `src/components/ui/button.test.tsx`: renders children/`variant`/`size` class combinations; enabled button has pointer-cursor + hover classes; `disabled` button has neither and does not fire `onClick` when clicked — depends on T003 (test should fail against the scaffold)

### Implementation for User Story 1

- [X] T005 [US1] Implement `buttonVariants` class map (variant × size → Tailwind classes, using `--color-primary`/`--color-destructive`/`--color-muted`/`--color-border` tokens) in `src/components/ui/button.tsx` per data-model.md — depends on T004
- [X] T006 [US1] Implement `Button`'s default (native `<button>`) render path wired to `buttonVariants` + `cn()`, including `disabled:cursor-not-allowed disabled:pointer-events-none`-style disabled handling — depends on T005
- [X] T007 [US1] Verify T004's tests pass; manually verify hover/disabled states in both light and dark theme via `npm run dev` (quickstart.md step 2) — depends on T006

**Checkpoint**: `Button` is fully usable stand-alone; User Story 1 is independently demonstrable.

---

## Phase 4: User Story 2 - Every control is usable without a mouse (Priority: P2)

**Goal**: `Button` is fully keyboard-operable with a visible focus indicator, and supports rendering as a different element (e.g. `<a>`) via `asChild` while keeping button semantics.

**Independent Test**: Tab to a `Button` — see a visible focus ring; press Enter/Space — `onClick` fires. Render `<Button asChild><a href="...">...</a></Button>` and confirm it behaves like a button (click/keyboard-activatable) while remaining a real `<a>`.

### Tests for User Story 2

- [X] T008 [US2] Extend `src/components/ui/button.test.tsx`: `userEvent.tab()` focuses the button and shows the focus-visible style; Enter and Space both trigger `onClick` while focused; `asChild` renders the child element (e.g. an `<a>`) with the button's classes/behavior applied and no extra wrapper element — depends on T007

### Implementation for User Story 2

- [X] T009 [US2] Add `focus-visible:ring-2 focus-visible:ring-ring`-style focus styling to `buttonVariants` in `src/components/ui/button.tsx` — depends on T008
- [X] T010 [US2] Implement `asChild` via `@radix-ui/react-slot`'s `Slot` in `src/components/ui/button.tsx` (render `Slot` instead of `button` when `asChild` is true) — depends on T009
- [X] T011 [US2] Verify T008's tests pass — depends on T010

**Checkpoint**: `Button` is fully keyboard/AT-operable and polymorphic; User Stories 1 and 2 both independently demonstrable.

---

## Phase 5: User Story 3 - The whole app is migrated, not just new code (Priority: P3)

**Goal**: Every existing hand-rolled `<button>` in `src/views`/`src/components` is replaced with the shared `Button`, preserving all existing `data-testid`/`onClick`/`disabled` behavior.

**Independent Test**: `grep -rn "<button" src --include="*.tsx"` (excluding `button.tsx` itself and test files) returns nothing; `npm test && npm run test:e2e` pass unchanged.

### Implementation for User Story 3

(One task per file, matched to the quickstart.md audit table; each replaces every `<button>` in that file with `<Button variant=... size=...>`, preserving `data-testid`/`onClick`/`disabled`/`className` overrides.)

- [X] T012 [P] [US3] Migrate `src/App.tsx` (line 60) — depends on T011
- [X] T013 [P] [US3] Migrate `src/views/chat/ConversationList.tsx` (lines 69, 76, 84, 95) — depends on T011
- [X] T014 [P] [US3] Migrate `src/views/settings/Settings.tsx` (lines 55, 84, 100) — depends on T011
- [X] T015 [P] [US3] Migrate `src/views/chat/SearchPanel.tsx` (lines 56, 62) — depends on T011
- [X] T016 [P] [US3] Migrate `src/views/chat/Chat.tsx` (lines 209, 235) — depends on T011
- [X] T017 [P] [US3] Migrate `src/views/workspace/Workspace.tsx` (lines 73, 140) — depends on T011
- [X] T018 [US3] Confirm zero raw `<button>` remain in `src/views`/`src/components` outside `button.tsx` (`grep -rn "<button" src --include="*.tsx"`), documenting any exemption with a reason per FR-008 — depends on T012-T017. True at the time (T012-T017 landed a genuine zero). Four raw `<button>`s exist now in `src/views/chat/EmptyState.tsx` and `src/views/shared/FolderPicker.tsx` — not a regression in this migration, but new code added afterward by `006-chat-empty-state` (those files didn't exist yet when T012-T017 ran). Reviewed and kept as an explicit FR-008 exemption rather than migrated: the folder-target trigger and the picker's compact icon+label list rows don't have a natural fit in `Button`'s variant system, and the hand-tuned look is the intentionally-kept one — documented inline at each site, per this task's own instruction.
- [X] T019 [US3] Run `npm test` and `npm run test:e2e`; fix any break without changing an existing `data-testid` (FR-009/SC-004) — depends on T018

**Checkpoint**: All three user stories complete; app-wide migration done.

---

## Phase 6: Polish & Cross-Cutting Concerns

- [X] T020 Run `npm run lint` and `npm run format:check`; fix any violations introduced by this feature
- [X] T021 Walk through quickstart.md end-to-end as a final sign-off

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies
- **Foundational (Phase 2)**: Depends on Setup — blocks all user stories
- **User Story 1 (Phase 3)**: Depends on Foundational
- **User Story 2 (Phase 4)**: Depends on User Story 1 (same file, `Button` must exist with its base states before adding focus/asChild)
- **User Story 3 (Phase 5)**: Depends on User Story 2 (migration needs the finished, accessible `Button`)
- **Polish (Phase 6)**: Depends on all above

Unlike a typical spec-kit feature, User Stories 1–3 here are **sequential, not parallel** — they all modify the same `button.tsx` (US1 → US2) or consume its finished API (US3), so this is intentionally a single incremental build rather than three independently-staffed stories.

### Parallel Opportunities

- T012–T017 (per-file migrations in US3) touch disjoint files and can run in parallel once T011 is done.
- T001 and T002 (Setup) can run in parallel.

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1 (Setup) and Phase 2 (Foundational)
2. Complete Phase 3 (User Story 1) — a usable, visually-correct `Button` component
3. **STOP and VALIDATE**: manually check hover/disabled states per quickstart.md step 2
4. Ship — even without migration, new code can start using `<Button>` immediately

### Incremental Delivery

1. Setup + Foundational → Button scaffold exists
2. User Story 1 → visually correct, usable `Button` (MVP)
3. User Story 2 → keyboard/AT-accessible, polymorphic `Button`
4. User Story 3 → whole app migrated, old inconsistency eliminated

## Notes

- [P] tasks = different files, no dependencies
- Verify T004/T008 tests fail before their corresponding implementation tasks land
- Commit after each checkpoint
