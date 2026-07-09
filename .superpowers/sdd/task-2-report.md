# Task 2 Report

## What I implemented

- Added `src/lib/shortcuts.test.ts` and updated `src/lib/shortcuts.ts` so the shortcut registry now owns:
  - `openCommandCenter(): void`
  - `open-command-center` on `Cmd+K`
  - `Cmd+F` remaining dedicated to conversation search
- Moved app-owned surface state into `src/App.tsx`:
  - `showSearch`
  - `showCommandCenter`
  - existing `showShortcutsDialog`
  - existing `showSettings`
  - existing `showWidgetGallery`
- Moved the search dialog surface out of `src/views/chat/ConversationList.tsx` and into `src/App.tsx`, while keeping sidebar Search and `Cmd+F` opening the same search panel.
- Added `ConversationListProps.onOpenSearch` and removed the imperative `openSearch()` handle so `ConversationList` reports search intent upward instead of owning the dialog locally.
- Updated the global keydown gate in `App` so app-owned surfaces block other shortcuts unless the matched shortcut is `open-command-center`.
- Added the required skipped app-level test in `src/App.test.tsx` with the exact Task 2 body.

## TDD evidence

### RED

Command:

```bash
npm test -- src/lib/shortcuts.test.ts
```

Result:

- Exit code: `1`
- `src/lib/shortcuts.test.ts` failed `2` tests
- Failure 1: `open-command-center` shortcut was `undefined`
- Failure 2: `search-conversations` still exposed the old `⌘F` / `Open conversation search` metadata instead of the Task 2 values

### GREEN

Command:

```bash
npm test -- src/lib/shortcuts.test.ts
```

Result:

- Exit code: `0`
- `Test Files  1 passed (1)`
- `Tests  2 passed (2)`

## What I tested and exact results

1. Required Task 2 verification:

```bash
npm test -- src/lib/shortcuts.test.ts src/views/chat/ConversationList.test.tsx
```

Result:

- Exit code: `0`
- `Test Files  2 passed (2)`
- `Tests  16 passed (16)`

2. Focused app-shell regression coverage for the new state wiring:

```bash
npm test -- src/App.test.tsx
```

Result:

- Exit code: `0`
- `Test Files  1 passed (1)`
- `Tests  19 passed | 1 skipped (20)`

## Files changed

- `src/lib/shortcuts.ts`
- `src/lib/shortcuts.test.ts`
- `src/App.tsx`
- `src/App.test.tsx`
- `src/views/chat/ConversationList.tsx`
- `src/views/chat/ConversationList.test.tsx`

## Self-review findings

- Search ownership is now at the app shell boundary, but the existing search panel behavior remains intact for both sidebar Search and `Cmd+F`.
- `ConversationList` stayed scoped to sidebar concerns; it no longer owns a modal surface.
- I kept backend commands, storage behavior, and Tauri IPC contracts unchanged.
- I deliberately made `openCommandCenter` toggle the app-owned boolean while Task 4 UI is absent, so `Cmd+K` cannot leave the global shortcut gate stuck in an invisible open state.

## Concerns

- No blocking concerns. `Cmd+K` now routes to app-owned command-center state, but the visible command-center surface remains deferred to Task 4 as intended.
