# Implementation Plan: Keyboard Shortcuts

**Branch**: `005-keyboard-shortcuts` | **Date**: 2026-07-03 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/005-keyboard-shortcuts/spec.md`

**Note**: This template is filled in by the `/speckit-plan` command. See `.specify/templates/plan-template.md` for the execution workflow.

## Summary

Three global keyboard shortcuts (Cmd+L focus input, Cmd+N new conversation,
Cmd+K shortcuts dialog) implemented as a single frontend keydown listener
mounted once in `App.tsx`, driven by one shared shortcut registry (so the
dialog can never drift out of sync with what's actually bound — FR-010).
Cmd+N's "create and switch to a new conversation" logic is exposed by
`ConversationList` via an imperative ref rather than duplicated in `App.tsx`.
Cmd+K opens the app's first modal — a small hand-rolled `Dialog` component
built directly on the `<dialog>` element (native focus trap, `Esc`-to-close,
and backdrop dismissal for free, no new dependency).

## Technical Context

**Language/Version**: TypeScript / React 19 (frontend only — no Rust/Tauri backend changes)

**Primary Dependencies**: None new. Built on the existing React + Tailwind v4 stack; the native HTML `<dialog>` element is used directly instead of adding a dialog/modal library

**Storage**: N/A — shortcuts are a fixed, hardcoded registry (per spec Assumptions: no rebinding/customization in this pass), not persisted data

**Testing**: Vitest + Testing Library (existing setup) for the keydown-handling logic and the Dialog component; no new test infrastructure needed

**Target Platform**: macOS desktop (Tauri/WKWebView) — same as the rest of the app; "Cmd" is read via `event.metaKey`, which WKWebView reports correctly for the physical Command key

**Project Type**: Frontend-only addition to the existing single Tauri + React desktop app

**Performance Goals**: Shortcut handling must be imperceptible (well under 100ms as an event-handler reaction, no debouncing needed) — this is normal synchronous keydown handling, not a performance-sensitive path

**Constraints**: Must not intercept any key combination other than the three specified (FR-008); must keep working while a text input has focus (FR-007), which requires the listener to be attached at `window`/`document` (capture or bubble, not scoped to a specific input) rather than relying on individual input `onKeyDown` handlers; must not fight the browser's own handling of `metaKey` combinations in a way that leaks the OS's default behavior for those same keys (Cmd+N/Cmd+L/Cmd+K have no meaningful default action inside a WKWebView with no browser chrome, so no conflict is expected, but `preventDefault()` is called defensively on all three)

**Scale/Scope**: One global listener, one small shortcut-registry data file, one new reusable `Dialog` component, one small change to `ConversationList` to expose its existing "create new conversation" logic imperatively — no new backend surface, no new persisted data

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

- **I. Zero-Config First Run** — N/A. Doesn't touch onboarding or first-launch
  flow.
- **II. Local-By-Default Privacy** — PASS. No new network calls, no new data
  leaving the device, nothing persisted beyond the existing conversation
  data Cmd+N already creates through the existing, already-reviewed
  `create_conversation` command.
- **III. Native macOS Polish** — Directly relevant, not just N/A: adding
  keyboard shortcuts supports this principle's intent (a native app should
  feel keyboard-navigable, not just mouse-driven). One real trade-off
  flagged here rather than silently decided: this plan implements the
  shortcuts as a **frontend-only** keydown listener, not as native Tauri
  menu items with OS-level accelerators. A fully "native polish" version of
  Cmd+N in particular would live under a real macOS `File` menu (visible in
  the menu bar, standard OS discoverability) via Tauri's Rust-side menu
  API. That's a materially bigger change (new Rust menu-building code, IPC
  wiring from a native menu click back into React state) than what the spec
  asked for, and Cmd+L in particular isn't the kind of action that
  typically has its own menu-bar entry in native macOS apps either (compare
  a browser's address-bar-focus shortcut, which is also JS/UI-only, not a
  menu item). Decision: ship the frontend-only version now; native menu-bar
  integration is a reasonable, explicitly-deferred v1.1+ follow-up, not a
  silent gap.
- **IV. Extensibility via MCP and Skills** — N/A.
- **V. v1 Scope Discipline** — PASS. Pure interaction-layer addition to the
  already-shipped app; doesn't expand platform support or add services.

No violations requiring justification; Complexity Tracking is left empty.

## Project Structure

### Documentation (this feature)

```text
specs/005-keyboard-shortcuts/
├── plan.md              # This file (/speckit-plan command output)
├── research.md          # Phase 0 output (/speckit-plan command)
├── data-model.md        # Phase 1 output (/speckit-plan command)
├── quickstart.md        # Phase 1 output (/speckit-plan command)
└── tasks.md             # Phase 2 output (/speckit-tasks command - NOT created by /speckit-plan)
```

No `contracts/` directory — this feature adds no Tauri IPC command, no
new backend surface, and no external interface; it's a pure frontend
interaction addition.

### Source Code (repository root)

```text
src/
├── App.tsx                          # MODIFIED: mounts the global keydown listener, owns dialog-open state
├── lib/
│   └── shortcuts.ts                 # NEW: the single shortcut registry (keys, description, action) — both the
│                                     #      listener and the dialog read from this, per FR-010
├── components/
│   └── Dialog.tsx                   # NEW: the app's first reusable modal, built on native <dialog>
└── views/
    ├── chat/
    │   └── ConversationList.tsx     # MODIFIED: exposes "create + switch to new conversation" via an imperative ref
    └── shortcuts/
        └── ShortcutsDialog.tsx      # NEW: renders the registry inside <Dialog>, opened by Cmd+K
```

**Structure Decision**: All changes live in the existing single-project
frontend (`src/`) — no backend/API split, matching the rest of the app.
`lib/shortcuts.ts` is the single source of truth the spec's FR-010 requires
(one array of `{ combo, description, action }`, consumed by both the
keydown listener and the dialog, so they cannot drift apart). `Dialog.tsx`
is placed under `components/` (alongside the existing `Timer.tsx`) since,
like `Timer`, it's a small generic UI primitive with no feature-specific
knowledge, reusable beyond this feature.

## Complexity Tracking

> **Fill ONLY if Constitution Check has violations that must be justified**

No violations — this section is intentionally empty.

## Post-Phase 1 Constitution Re-check

Re-evaluated after `data-model.md` and `quickstart.md` were drafted: the
design adds no new network calls, no new persisted data, and no new
external interface — only a keydown listener, a static data file, and one
generic, unstyled-by-default dialog primitive. The Constitution Check
verdicts above (PASS on II and V, a flagged-and-accepted trade-off on III,
N/A on I/IV) still hold unchanged.
