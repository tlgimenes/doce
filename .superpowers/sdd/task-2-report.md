# Task 2 Report: Reusable user message bubble

## What I implemented

- Extracted the user-message bubble internals into `src/components/UserMessageBubble.tsx`.
- Kept the existing transcript-row wrapper in `MessageContent` intact, including `data-testid="chat-message"`, `role="group"`, and `aria-label="You said"`.
- Extended `MarkdownPreview` with an optional `testId` prop mapped to `data-testid`.
- Switched the user branch in `MessageContent` to delegate bubble rendering to `UserMessageBubble`.
- Added focused tests for the new reusable bubble and a regression assertion for the outer user row contract.

## What I tested and test results

- `npx vitest run src/components/UserMessageBubble.test.tsx`
- `npx vitest run src/components/UserMessageBubble.test.tsx src/components/MessageContent.test.tsx`

Results:

- The focused suite passed after implementation: 2 files, 22 tests passing.

## TDD Evidence

### RED

- Command: `npx vitest run src/components/UserMessageBubble.test.tsx`
- Relevant failure:

```text
Error: Failed to resolve import "./UserMessageBubble" from "src/components/UserMessageBubble.test.tsx". Does the file exist?
```

- Why expected: the test was written before `UserMessageBubble.tsx` existed, so the initial run should fail at import resolution.

### GREEN

- Command: `npx vitest run src/components/UserMessageBubble.test.tsx src/components/MessageContent.test.tsx`
- Relevant passing output:

```text
Test Files  2 passed (2)
Tests       22 passed (22)
```

## Files changed

- `src/components/MarkdownPreview.tsx`
- `src/components/UserMessageBubble.tsx`
- `src/components/UserMessageBubble.test.tsx`
- `src/components/MessageContent.tsx`
- `src/components/MessageContent.test.tsx`

## Self-review findings

- The extraction is narrow and preserves the existing outer user row contract.
- `UserMessageBubble` keeps the same visual defaults and token-meter behavior for both plain text and rich text messages.

## Issues or concerns

- None.
