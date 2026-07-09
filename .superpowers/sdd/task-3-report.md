What I implemented

- Added `StickyUserMessage` as a standalone sticky wrapper around `UserMessageBubble`.
- Kept the wrapper local-state only: collapsed by default, expands on focus or click, collapses when focus leaves the sticky region.
- Applied the required outer classes and attributes: `data-testid="chat-message"`, `data-sticky-user-message="true"`, `sticky top-4 z-40 mb-8 sm:mb-6`, `role="group"`, and `aria-label="You said"`.
- Passed the required collapsed and expanded bubble classes through `bubbleClassName`.

What I tested and test results

- Ran the focused Vitest file for the new component.
- Result after implementation: `3 passed`.

TDD Evidence

- RED: `npx vitest run src/views/workspace/StickyUserMessage.test.tsx`
- Relevant failure before implementation: `Failed to resolve import "./StickyUserMessage" from "src/views/workspace/StickyUserMessage.test.tsx". Does the file exist?`
- Why expected: the test was written first and the component file did not exist yet.

- GREEN: `npx vitest run src/views/workspace/StickyUserMessage.test.tsx`
- Relevant passing output after implementation: `Test Files 1 passed (1)` and `Tests 3 passed (3)`.

Files changed

- `src/views/workspace/StickyUserMessage.tsx`
- `src/views/workspace/StickyUserMessage.test.tsx`
- `.superpowers/sdd/task-3-report.md`

Self-review findings

- The focus handler is guarded so nested focus inside the bubble does not retrigger the scroll callback.

Issues or concerns

- None noted from the focused test run.

Fix evidence

- RED: `npx vitest run src/views/workspace/StickyUserMessage.test.tsx`
- Relevant failure before the fix: `expected "vi.fn()" to be called 1 times, but got 0 times` in `invokes onScrollToTurn again when clicked while already focused`.
- Why expected: the click handler still only expanded locally and did not call `onScrollToTurn` on its own.

- GREEN: `npx vitest run src/views/workspace/StickyUserMessage.test.tsx`
- Relevant passing output after the fix: `Test Files 1 passed (1)` and `Tests 4 passed (4)`.

Re-review fix evidence

- RED: `npx vitest run src/views/workspace/StickyUserMessage.test.tsx`
- Relevant failure before the corrected fix: `expected "vi.fn()" to be called 1 times, but got 0 times` in `calls onScrollToTurn once per pointer click, including an already-focused target`.
- Why expected: the click path was still suppressing the callback instead of letting the pointer click invoke it exactly once.

- GREEN: `npx vitest run src/views/workspace/StickyUserMessage.test.tsx`
- Relevant passing output after the corrected fix: `Test Files 1 passed (1)` and `Tests 4 passed (4)`.
