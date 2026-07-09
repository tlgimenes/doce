## What I implemented

- Added `src/views/workspace/TranscriptTurn.tsx` as a thin transcript-turn renderer.
- Added `PendingTurnWidget` with the exact bash/task union contract.
- Rendered `StickyUserMessage` and the sticky background strip when `turn.user` exists.
- Rendered turn rows through existing `MessageContent` so tool dispatch and row semantics stay centralized.
- Rendered pending `BashWidget` / `TaskWidget` content when supplied.
- Rendered turn-local error content when supplied.

## What I tested and test results

- Focused test file: `src/views/workspace/TranscriptTurn.test.tsx`
- Result after implementation: 4 tests passed.

## TDD Evidence

### RED

- Command: `npx vitest run src/views/workspace/TranscriptTurn.test.tsx`
- Relevant failing output:

  ```text
  Error: Failed to resolve import "./TranscriptTurn" from "src/views/workspace/TranscriptTurn.test.tsx". Does the file exist?
  ```

- Why this failure was expected: the test imported the new renderer before it existed, so the suite had to fail at module resolution.

### GREEN

- Command: `npx vitest run src/views/workspace/TranscriptTurn.test.tsx`
- Result: `Test Files  1 passed (1)`, `Tests  4 passed (4)`

## Files changed

- `src/views/workspace/TranscriptTurn.tsx`
- `src/views/workspace/TranscriptTurn.test.tsx`
- `.superpowers/sdd/task-4-report.md`

## Self-review findings

- The renderer stays thin and delegates row semantics to `MessageContent`, which matches the task contract.
- `isLastTurn` is accepted and preserved on the root element as metadata, but it does not change rendering behavior in this task.

## Issues or concerns

- None.
