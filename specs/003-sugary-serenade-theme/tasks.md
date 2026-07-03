---

description: "Task list template for feature implementation"
---

# Tasks: Sugary Serenade Color Theme

**Input**: Design documents from `/specs/003-sugary-serenade-theme/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, quickstart.md (all present; no `contracts/` — this feature adds no new external interface, per `plan.md`'s Project Structure)

**Tests**: Not included — the spec does not request automated tests, and this feature continues `002-landing-page`'s decision (`research.md` §5 there) against a test framework for this static page. Validation is the manual `quickstart.md` pass (final Polish phase task) plus the analytical WCAG contrast work already done in this feature's own `research.md` §1.

**Organization**: Tasks are grouped by user story to enable independent implementation and testing of each story.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on an incomplete task)
- **[Story]**: Which user story this task belongs to (US1, US2, US3)
- Include exact file paths in descriptions

## Path Conventions

This feature modifies the existing `002-landing-page` site in place — no new files or directories, per `plan.md`'s Structure Decision:

```text
site/
├── index.html            # UNCHANGED by this feature
└── assets/
    ├── styles.css         # MODIFIED: Sugary Serenade tokens + remapped rules
    ├── main.js             # UNCHANGED by this feature
    ├── favicon.png          # REGENERATED
    ├── favicon-32.png        # REGENERATED
    └── og-image.png          # REGENERATED
```

Note: nearly every US1 task edits the same `site/assets/styles.css` file, so those tasks are kept sequential (no `[P]`) even though the underlying styling work is logically independent per element.

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Introduce the theme's design tokens before anything consumes them

- [X] T001 Add the Sugary Serenade CSS custom properties to `:root` in `site/assets/styles.css`: `--color-1` through `--color-5`, `--gradient-linear`, `--gradient-radial`, `--color-text`, `--color-text-muted` — values per `data-model.md`'s token table

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Establish the page's base canvas and text color before any section is restyled

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [X] T002 Replace the `body` background (previously `#0d1117`) with `var(--color-4)` and the base text color (previously `#f1f5f9`) with `var(--color-text)` in `site/assets/styles.css`

**Checkpoint**: Foundation ready — story-specific restyling can now begin

---

## Phase 3: User Story 1 - See a cohesive Sugary Serenade theme (Priority: P1) 🎯 MVP

**Goal**: Every background, button, and accent on the page uses the Sugary Serenade palette or its gradients; all text stays legible.

**Independent Test**: Load the page and visually confirm no dark-navy/sky-blue element remains, the hero shows the linear gradient, hovering any button shows the radial gradient, and run a contrast check to confirm legibility.

### Implementation for User Story 1

- [X] T003 [US1] Restyle `#hero` background to `var(--gradient-linear)` and update the heading/tagline colors to `var(--color-text)` / `var(--color-text-muted)` in `site/assets/styles.css` (depends on T001, T002) — FR-001, FR-002
- [X] T004 [US1] Restyle `.button-primary` (Download): background `var(--color-1)`, text `var(--color-text)`, hover background `var(--gradient-radial)` in `site/assets/styles.css` (depends on T001) — FR-001, FR-003
- [X] T005 [US1] Restyle `.platform-note` color to `var(--color-text-muted)` in `site/assets/styles.css` (depends on T003) — FR-001
- [X] T006 [US1] Restyle `.star-badge` (background `var(--color-2)`, text `var(--color-text)`, hover background `var(--gradient-radial)`), the `.star-icon` SVG fill, and `#star-count` color to `var(--color-text)` in `site/assets/styles.css` (depends on T001) — FR-001, FR-003
- [X] T007 [US1] Restyle `.button-secondary` (Buy Me a Coffee): background `var(--color-3)`, text `var(--color-text)`, hover background `var(--gradient-radial)` in `site/assets/styles.css` (depends on T001) — FR-001, FR-003
- [X] T008 [US1] Audit `site/assets/styles.css` for any remaining previous-theme hex values (`#0d1117`, `#38bdf8`, `#7dd3fc`, `#f1f5f9`, `#cbd5e1`, `#94a3b8`, `#30363d`, `#facc15`, `#ffdd00`, `#ffe74c`) and remove/replace any found (depends on T002-T007) — FR-001
- [X] T009 [US1] Verify WCAG AA contrast for every text/background pairing actually rendered on the page against the values computed in `research.md` § 1 (depends on T003-T008) — FR-004, SC-002 (Lighthouse accessibility: 100, matching the pre-retheme baseline; hero+button hover states visually confirmed legible)

**Checkpoint**: User Story 1 is fully functional and testable independently — the page fully reflects Sugary Serenade.

---

## Phase 4: User Story 2 - Every existing feature still works exactly as before (Priority: P2)

**Goal**: Confirm the retheme changed nothing about content, link targets, or JS behavior — verification only, since `site/index.html` and `site/assets/main.js` are untouched by design.

**Independent Test**: Re-run `specs/002-landing-page/quickstart.md`'s checklist against the retheme page and confirm every item still passes unchanged.

### Verification for User Story 2

- [X] T010 [P] [US2] Diff the visible page copy (hero headline/tagline, platform note, star-count label, coffee-button label) against `specs/002-landing-page/spec.md`'s wording to confirm zero changes — FR-005 (index.html untouched by this feature, so wording is identical by construction; confirmed via accessibility-tree snapshot)
- [X] T011 [P] [US2] Re-verify the download button, star badge, and coffee button link targets are unchanged from `002-landing-page` — FR-006 (confirmed via snapshot: releases/latest, github.com/tlgimenes/doce, buymeacoffee.com/tlgimenes)
- [X] T012 [P] [US2] Re-verify the GitHub star-count live fetch + fallback behavior still functions identically (simulate a fetch failure per `specs/002-landing-page/quickstart.md` item 4) — FR-006 (repo still returns 404 since it isn't public; page renders the "0" fallback with no broken layout, same as before)
- [X] T013 [P] [US2] Re-verify the responsive layout at mobile and desktop widths still has no horizontal scroll or overlap under the new styles — FR-006 (confirmed at 375px and 1280px viewports)

**Checkpoint**: User Stories 1 and 2 both verified — theme applied with zero functional regression.

---

## Phase 5: User Story 3 - Brand surfaces outside the page match too (Priority: P3)

**Goal**: The favicon and Open Graph preview image reflect the new palette.

**Independent Test**: View the favicon in a browser tab and open `og-image.png` directly; confirm both read as Sugary Serenade rather than the previous dark-navy branding.

### Implementation for User Story 3

- [X] T014 [US3] Update the asset-generation script to use Sugary Serenade colors (cream background, espresso text/icon, peach accent per `research.md` § 5) and regenerate `site/assets/favicon.png`, `site/assets/favicon-32.png`, and `site/assets/og-image.png` — FR-007 (OG image upgraded to use the actual linear gradient across all 5 stops, matching the hero)
- [X] T015 [US3] Visually verify the regenerated favicon (browser tab) and `og-image.png` reflect the new palette, not the previous dark-navy branding (depends on T014) — SC-004 (both viewed directly — cream/espresso favicon, full 5-stop gradient OG image)

**Checkpoint**: All three user stories are independently functional/verified.

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Full end-to-end validation, including the one cross-cutting regression check (accessibility score)

- [X] T016 Run the full `specs/003-sugary-serenade-theme/quickstart.md` checklist end-to-end, first locally then again against the deployed GitHub Pages URL once merged to `main`, including the Lighthouse accessibility regression check against the `002-landing-page` baseline — SC-005 (local pass complete via browser check — Lighthouse Accessibility 100, matching baseline, zero new failures; deployed-URL pass pending first deploy to `main`, same as `002-landing-page`)

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — can start immediately
- **Foundational (Phase 2)**: Depends on Setup completion — BLOCKS all user stories
- **User Story 1 (Phase 3)**: Depends on Foundational; all its tasks share `site/assets/styles.css` so they run sequentially in the order listed
- **User Story 2 (Phase 4)**: Depends on User Story 1 being complete (there's nothing meaningful to re-verify against until the new styles exist), but touches no files itself
- **User Story 3 (Phase 5)**: Depends on Foundational only — touches a completely different set of files (image assets) than US1/US2, so it can run in parallel with Phases 3-4 if staffed separately
- **Polish (Phase 6)**: Depends on all three user stories being complete

### Within Each User Story

- US1: T003-T007 (per-element restyling) before T008 (cleanup audit) before T009 (contrast verification)
- US2: T010-T013 have no ordering dependency on each other, only on US1 being done
- US3: T014 (regenerate assets) before T015 (verify)

### Parallel Opportunities

- T010, T011, T012, and T013 (US2) can all run in parallel — none modify a file, so there's no conflict
- T014-T015 (US3) can run in parallel with Phases 3-4 (US1/US2) — no file overlap with `site/assets/styles.css`
- Within US1, no task pair is parallel-safe: T003-T008 all edit the same `site/assets/styles.css` file

---

## Parallel Example: User Story 2

```bash
# All four verification checks can run together (no file writes):
Task: "Diff visible page copy against specs/002-landing-page/spec.md wording"
Task: "Re-verify download/star/coffee link targets are unchanged"
Task: "Re-verify star-count fetch + fallback behavior"
Task: "Re-verify responsive layout at mobile and desktop widths"
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup
2. Complete Phase 2: Foundational (CRITICAL — blocks all stories)
3. Complete Phase 3: User Story 1
4. **STOP and VALIDATE**: Load the page and visually confirm the retheme, then run the contrast check (T009)
5. This alone is the visible deliverable the user asked for

### Incremental Delivery

1. Complete Setup + Foundational → tokens and base canvas ready
2. Add User Story 1 → validate visually + via contrast check → the retheme itself is done
3. Add User Story 2 → confirm zero regression → safe to ship
4. Add User Story 3 → regenerate brand assets → full feature complete
5. Finish with Polish (full quickstart + Lighthouse regression pass)

---

## Notes

- [P] tasks have no file-write conflicts and no unmet dependencies
- [Story] label maps each task to its user story for traceability
- No test tasks are generated; `T016`'s manual quickstart + Lighthouse pass is the acceptance gate
- Commit after each task or logical group
- Stop at any checkpoint to validate a story independently
