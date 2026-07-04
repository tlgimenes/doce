---

description: "Task list template for feature implementation"
---

# Tasks: doce Landing Page

**Input**: Design documents from `/specs/002-landing-page/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/, quickstart.md (all present)

**Tests**: Not included — the spec does not request automated tests, and `research.md` § 5 explicitly decided against a test framework for this static page in favor of manual `quickstart.md` validation (final Polish phase task).

**Organization**: Tasks are grouped by user story to enable independent implementation and testing of each story.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on an incomplete task)
- **[Story]**: Which user story this task belongs to (US1, US2, US3)
- Include exact file paths in descriptions

## Path Conventions

Single static-site project, per `plan.md`'s Project Structure:

```text
site/
├── index.html
└── assets/
    ├── styles.css
    └── main.js

.github/workflows/pages.yml
```

Note: `site/index.html` and `site/assets/styles.css` are each touched by tasks across multiple stories. Within any one file, tasks are kept sequential (no `[P]`) even when the underlying work is logically independent, to avoid concurrent edits to the same file.

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Project initialization and basic structure

- [X] T001 Create `site/` directory with empty `site/index.html`, `site/assets/styles.css`, `site/assets/main.js` per plan.md Project Structure
- [X] T002 [P] Add placeholder favicon and Open Graph preview image assets under `site/assets/` (different files from T001; referenced later by T016)

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: The shared page skeleton and styling/script scaffolding every user story builds into

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [X] T003 Build the base HTML skeleton in `site/index.html`: doctype, charset, responsive viewport meta, `<title>`, meta description, links to `assets/styles.css` and `assets/main.js`, and empty section placeholders for hero, star-count, and coffee-button content
- [X] T004 [P] Build the base CSS reset and mobile-first responsive layout scaffold (flex/grid container, breakpoints) in `site/assets/styles.css` — required by FR-008 for every story's content
- [X] T005 [P] Build the base `site/assets/main.js` module scaffold (a single `DOMContentLoaded`-gated init function with no story-specific logic yet)

**Checkpoint**: Foundation ready — user story implementation can now begin

---

## Phase 3: User Story 1 - Learn about doce and download it (Priority: P1) 🎯 MVP

**Goal**: A first-time visitor understands, within the first screen, that doce is a fully local, zero-config personal AI agent for macOS, and can click a prominent button to download it.

**Independent Test**: Load the page in a fresh browser session; without scrolling, confirm the value proposition, download button, and platform requirement are all visible; click the button and confirm it opens the GitHub Releases page.

### Implementation for User Story 1

- [X] T006 [US1] Write the hero/value-proposition markup and copy in `site/index.html` (headline + subheading covering: fully local, zero-config, personal AI agent, macOS, no API keys, no cloud dependency) — FR-001
- [X] T007 [US1] Add the primary download call-to-action button in `site/index.html`, linking to the project's GitHub Releases page per `contracts/external-interfaces.md` § 2 — FR-002
- [X] T008 [US1] Add a visible platform-requirement note ("macOS · Apple Silicon") directly next to the download button in `site/index.html` — FR-006
- [X] T009 [US1] Style the hero section, download button, and mobile/desktop responsive breakpoints (both above the fold, no horizontal scroll/overlap) in `site/assets/styles.css` (depends on T006-T008) — SC-001, SC-002, FR-008

**Checkpoint**: User Story 1 is fully functional and testable independently — this alone is a shippable MVP landing page.

---

## Phase 4: User Story 2 - Gauge project credibility via GitHub stars (Priority: P2)

**Goal**: The page displays the doce repository's current GitHub star count, degrading gracefully if it can't be fetched.

**Independent Test**: Load the page and confirm a star count renders. Then, via devtools, block the star-count request and reload — confirm the page still renders a complete, unbroken layout showing the static fallback count.

### Implementation for User Story 2

- [X] T010 [US2] Add the star-count display markup in `site/index.html`, with the baked-in static fallback number as its initial content (`data-model.md` § Repository Star Count)
- [X] T011 [US2] Implement a star-count fetch function in `site/assets/main.js`: call GitHub's public unauthenticated repository endpoint, cache the result in `localStorage` for ~1 hour, and update the star-count element on success — `contracts/external-interfaces.md` § 1, FR-003
- [X] T012 [US2] Implement fallback handling in the same function (depends on T011) so any fetch error, timeout, or rate-limit response leaves the baked-in static fallback count in place instead of an error/blank state — FR-007
- [X] T013 [US2] Style the star-count element (icon + number) in `site/assets/styles.css` (depends on T010)

**Checkpoint**: User Stories 1 and 2 both work independently.

---

## Phase 5: User Story 3 - Support the project financially (Priority: P3)

**Goal**: A visitor can click a "Buy Me a Coffee" button to reach the project's support page.

**Independent Test**: Confirm the button is visible on the page and clicking it opens the Buy Me a Coffee page in a new tab.

### Implementation for User Story 3

- [X] T014 [US3] Add a "Buy Me a Coffee" link/button in `site/index.html` opening in a new tab (`target="_blank" rel="noopener noreferrer"`), as a plain link with no embedded widget script — FR-004, `research.md` § 4
- [X] T015 [US3] Style the "Buy Me a Coffee" button in `site/assets/styles.css` (depends on T014)

**Checkpoint**: All three user stories are independently functional.

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Cross-cutting requirements that span all stories, plus publishing the site

- [X] T016 Wire up the favicon and Open Graph/social preview meta tags in `site/index.html`'s `<head>`, referencing the assets added in T002
- [X] T017 Create `.github/workflows/pages.yml` implementing GitHub's "deploy from Actions" flow (`actions/upload-pages-artifact` + `actions/deploy-pages`), triggered on push to `main` for `site/**` changes plus `workflow_dispatch` — FR-005, `research.md` § 2, `plan.md` Structure Decision
- [X] T018 [P] Manually verify, via the browser network tab, that no third-party tracking/analytics requests fire beyond at most a basic privacy-respecting page-view count (no file changes, so safe alongside T017) — FR-009, FR-010
- [X] T019 Run the full `quickstart.md` validation checklist end-to-end, first locally then again against the deployed GitHub Pages URL (local pass complete via browser check — see implementation notes; deployed-URL pass pending first deploy to `main`)

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — can start immediately
- **Foundational (Phase 2)**: Depends on Setup completion — BLOCKS all user stories
- **User Stories (Phase 3-5)**: All depend on Foundational phase completion; independently testable and can proceed in priority order (P1 → P2 → P3) or in parallel if staffed
- **Polish (Phase 6)**: T016/T018 depend on the relevant story content existing; T017 (deploy workflow) can be built anytime after Setup but is only meaningful once at least US1 exists; T019 depends on all desired stories being complete

### User Story Dependencies

- **User Story 1 (P1)**: Can start after Foundational (Phase 2) — no dependency on other stories
- **User Story 2 (P2)**: Can start after Foundational (Phase 2) — adds the star-count element independently of US1's hero content
- **User Story 3 (P3)**: Can start after Foundational (Phase 2) — adds one button independently of US1/US2

### Within Each User Story

- Markup before styling (styling tasks note the markup task(s) they depend on)
- Fetch logic before fallback handling (US2: T011 before T012)
- Story complete before moving to next priority (if working sequentially)

### Parallel Opportunities

- T002 (Setup) can run in parallel with T001 — different files
- T004 and T005 (Foundational) can run in parallel once T003 exists — different files, no interdependency
- Once Foundational completes, all three user story phases can be worked in parallel by different people, since each touches `index.html`/`styles.css` in non-overlapping sections and `main.js` only in US2
- T018 (tracking check) can run in parallel with T017 (deploy workflow) — no file overlap

---

## Parallel Example: Foundational Phase

```bash
# After T003 (HTML skeleton) lands, these can run together:
Task: "Build the base CSS reset and responsive layout scaffold in site/assets/styles.css"
Task: "Build the base main.js module scaffold in site/assets/main.js"
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup
2. Complete Phase 2: Foundational (CRITICAL — blocks all stories)
3. Complete Phase 3: User Story 1
4. **STOP and VALIDATE**: Walk through `quickstart.md` items 1-2 against User Story 1 alone
5. Optionally deploy (T017) to demo the MVP page live

### Incremental Delivery

1. Complete Setup + Foundational → skeleton page ready
2. Add User Story 1 → validate → this is the MVP landing page
3. Add User Story 2 → validate star count + fallback → richer social proof
4. Add User Story 3 → validate coffee button → support channel live
5. Finish with Polish (favicon/OG tags, deploy workflow, tracking check, full quickstart pass)

---

## Notes

- [P] tasks touch different files and have no unmet dependencies
- [Story] label maps each task to its user story for traceability
- No test tasks are generated per `research.md` § 5's decision; `T019`'s manual quickstart pass is the acceptance gate instead
- Commit after each task or logical group
- Stop at any checkpoint to validate a story independently
