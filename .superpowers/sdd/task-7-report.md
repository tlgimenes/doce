# Task 7 Report: Chat Transcript, Composer, And Tool Widget Redesign

## What I implemented

- Added the transcript chat primitive marker on `TranscriptTurn` with `data-chat-turn="true"`, preserving sticky user anchoring, pending Bash/Task widget rendering, and error rendering behavior.
- Added the required transcript/message marker coverage:
  - `TranscriptTurn.test.tsx` now asserts `data-chat-turn="true"` and `min-w-0` on the body wrapper.
  - `MessageContent.test.tsx` now asserts context notices render as `role="status"` marker rows with the expected notice text.
- Restyled chat transcript message shells in `MessageContent.tsx` per the brief:
  - user row wrapper `mb-5`
  - assistant text row wrapper `mb-5 max-w-none`
  - error row `mb-5 rounded-md border border-destructive/25 bg-destructive/10 p-3 text-sm text-destructive`
  - summarized context notice `mb-5 rounded-md border border-border bg-muted p-3 text-sm text-muted-foreground`
  - cleared context notice `mb-5 text-xs text-muted-foreground/70`
- Restyled `UserMessageBubble.tsx` to the specified cream card shell while preserving props, test ids, token meter behavior, markdown rendering, and rich text rendering.
- Kept `StickToBottom` in `Workspace.tsx` and added the required comment explaining why `MessageScroller` is not wired in this pass.
- Restyled `RichInput.tsx` shell/actions without changing Tiptap setup, attachment flows, or submit semantics:
  - shell: `rounded-lg border border-border bg-card shadow-sm`
  - editor content: `min-h-12 px-3 py-2 text-sm`
  - attach button: `variant="ghost" size="icon"`
  - send button: `variant="primary" size="icon"` with the existing Brand Accent Workbench gradient token treatment
- Restyled `ToolDisclosure.tsx` to the required `rounded-md`/`shadow-sm` disclosure shell and `px-3 py-2` header.
- Replaced Phosphor control icons in the affected chat surfaces with lucide equivalents:
  - `ArrowDownIcon` -> `ArrowDown`
  - `PaperPlaneRightIcon` -> `SendHorizontal`
  - `PlusIcon` -> `Plus`
  - `CaretRightIcon` -> `ChevronRight`
- Kept test ids unchanged throughout the touched chat/composer/tool widget surfaces.
- Updated class-based tests to match the redesign where assertions depended on old shell classes.

## TDD evidence

### RED

Command:

```bash
npm test -- src/components/MessageContent.test.tsx src/views/workspace/TranscriptTurn.test.tsx
```

Result:

- `src/views/workspace/TranscriptTurn.test.tsx` failed on:
  - `marks transcript turns with chat primitive data attributes`
  - expected `data-chat-turn="true"`
  - received `null`
- `src/components/MessageContent.test.tsx` passed, confirming the new notice marker assertion already matched current behavior.

### GREEN

Command:

```bash
npm test -- src/components/MessageContent.test.tsx src/views/workspace/TranscriptTurn.test.tsx
```

Result:

- `Test Files  2 passed (2)`
- `Tests  27 passed (27)`

## What I tested and exact results

Focused marker suite:

```bash
npm test -- src/components/MessageContent.test.tsx src/views/workspace/TranscriptTurn.test.tsx
```

Result:

- `Test Files  2 passed (2)`
- `Tests  27 passed (27)`

Required chat suite:

```bash
npm test -- src/components/MessageContent.test.tsx src/components/UserMessageBubble.test.tsx src/views/workspace/TranscriptTurn.test.tsx src/views/workspace/Workspace.test.tsx src/views/workspace/StreamingStatus.test.tsx src/views/chat/rich-input/RichInput.test.tsx src/views/chat/rich-input/RichInput.attachments.test.tsx src/views/chat/rich-input/RichInput.skills.test.tsx src/views/chat/rich-input/UserMessageContent.test.tsx src/views/chat/tool-widgets
```

Result:

- `Test Files  20 passed (20)`
- `Tests  178 passed (178)`

Additional verification:

```bash
git diff --check
```

Result:

- passed with no whitespace or patch-format issues

## Files changed

- `src/components/MessageContent.tsx`
- `src/components/MessageContent.test.tsx`
- `src/components/UserMessageBubble.tsx`
- `src/components/UserMessageBubble.test.tsx`
- `src/views/workspace/TranscriptTurn.tsx`
- `src/views/workspace/TranscriptTurn.test.tsx`
- `src/views/workspace/Workspace.tsx`
- `src/views/chat/rich-input/RichInput.tsx`
- `src/views/chat/rich-input/RichInput.test.tsx`
- `src/views/chat/tool-widgets/ToolDisclosure.tsx`
- `src/views/chat/tool-widgets/ToolDisclosure.test.tsx`
- `src/views/chat/tool-widgets/UserAskWidget.tsx`
- `.superpowers/sdd/task-7-report.md`

## Self-review findings

- The implementation stays within Task 7 scope and does not touch `site/`, IPC contracts, storage/model behavior, tool parsing/fallback behavior, or Tiptap setup.
- Required test ids remain unchanged on the touched chat/composer/tool widget surfaces.
- The required `StickToBottom` retention comment is present above the `StickToBottom` usage.
- The Lucide replacements were applied only where the brief specified direct matches.
- I also aligned `UserAskWidget`'s send button styling with the redesigned `RichInput` send button so the live question composer remains visually consistent after the shared icon/button treatment changed.

## Concerns

- None.
