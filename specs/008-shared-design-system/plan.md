# Implementation Plan: Shared Design System for Interactive Elements

**Branch**: `008-shared-design-system` | **Date**: 2026-07-03 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/008-shared-design-system/spec.md`

## Summary

Introduce a small set of shared, accessible interactive components
(Button first; Dialog and a searchable Combobox/Picker soon after, per
specs 005 and 006) under `src/components/ui/`, built on top of Radix UI
Primitives for the parts that need real keyboard/focus/ARIA machinery
(Dialog, Combobox), styled with the existing Tailwind v4 theme tokens.
Every future building-block interactive element is added here rather
than inlined in a view (FR-011). Existing hand-rolled buttons across
`src/views` are migrated to the shared `Button`, preserving their
`data-testid` hooks.

## Technical Context

**Language/Version**: TypeScript 5 (via `typescript@^6` toolchain), React 19

**Primary Dependencies**: Tailwind CSS v4 (`@tailwindcss/vite`), `clsx` +
`tailwind-merge` (already installed, unused so far — this feature adds
the `cn()` helper that combines them), Radix UI Primitives (new
dependency — see research.md for the library choice), `@phosphor-icons/react`
(already used for iconography)

**Storage**: N/A (presentational component library, no persistence)

**Testing**: Vitest + `@testing-library/react` + `@testing-library/user-event`
for component-level tests (keyboard operability, disabled state, ARIA
attributes); existing WebdriverIO e2e suite (`tests/e2e`) for the
migration's regression check (FR-009/SC-004)

**Target Platform**: Tauri webview on macOS (Apple Silicon), per
constitution Principle III/V — desktop only, no touch-specific handling

**Project Type**: Single frontend project (existing `src/` React app
inside the Tauri shell) — no backend/Rust involvement

**Performance Goals**: N/A beyond normal UI responsiveness; no
component should introduce a perceptible input-to-paint delay on
hover/focus/click

**Constraints**: Must reuse the existing theme tokens in
`src/styles/theme.css` (no parallel styling system); must not change
any migrated screen's visible content or behavior beyond interaction
styling/accessibility (FR-010); must preserve existing `data-testid`
hooks through migration (FR-009)

**Scale/Scope**: Initial component set is Button only for this pass
(User Story 1 + start of User Story 3's audit); Checkbox/Radio/Select
are explicitly deferred (no current or planned usage found in the
codebase); Dialog and Combobox are the next two additions, tracked for
specs 005 and 006 but not built as part of this task

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

- **Principle I (Zero-Config First Run)**: N/A — no onboarding/config
  surface touched.
- **Principle II (Local-By-Default Privacy)**: N/A — no data leaves the
  device; this is purely a UI/component-layer change.
- **Principle III (Native macOS Polish)**: Directly supported — the
  whole point of this feature is consistent, polished interactive
  states, which is part of the native-feel bar this principle sets.
- **Principle IV (Extensibility via MCP and Skills)**: N/A — not
  touched.
- **Principle V (v1 Scope Discipline)**: Consistent — stays within the
  existing macOS desktop, React/Tauri frontend; no new platform, no new
  external service.
- **Technology constraint (Frontend: React + TypeScript in Tauri
  webview)**: Satisfied — Radix UI Primitives is a React library with
  no native/Tauri-incompatible dependencies.

No violations. Complexity Tracking section left empty.

## Project Structure

### Documentation (this feature)

```text
specs/008-shared-design-system/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md         # Phase 1 output
├── quickstart.md         # Phase 1 output
└── tasks.md              # Phase 2 output (/speckit-tasks — not yet created)
```

### Source Code (repository root)

```text
src/
├── components/
│   ├── ui/                    # NEW — the shared design system lives here
│   │   ├── button.tsx         # Button (this pass)
│   │   ├── dialog.tsx         # Later (spec 005)
│   │   └── combobox.tsx       # Later (spec 006)
│   └── Timer.tsx               # existing, unaffected
├── lib/
│   ├── cn.ts                  # NEW — clsx + tailwind-merge className helper
│   ├── ipc.ts                  # existing
│   └── bindings.ts             # existing
├── views/
│   ├── chat/                   # migration target (existing <button> usages)
│   ├── settings/                # migration target
│   ├── workspace/                # migration target
│   └── onboarding/               # migration target
└── styles/
    └── theme.css                # existing theme tokens, reused as-is
```

**Structure Decision**: Single existing frontend project. New shared
components live under `src/components/ui/` (one file per component,
lowercase filename matching shadcn/ui-style convention already implied
by the codebase's other lowercase file naming in `src/lib/`), imported
via the `@/components/ui/...` path alias already configured for `@/`.
No new top-level project or package is introduced.

## Complexity Tracking

*No Constitution Check violations — section intentionally left empty.*
