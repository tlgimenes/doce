## What I implemented

Wired `Workspace.tsx` to render grouped transcript turns instead of flat `MessageContent` rows.

The workspace now:

- derives `transcriptTurns` with `groupTranscriptTurns(messages)`
- renders earlier turns in a centered `mx-auto max-w-3xl` block
- renders the latest turn in a dedicated `data-testid="last-transcript-turn-viewport"` wrapper with `min-h-[100cqh]`
- passes pending Bash/Task tool calls into the latest `TranscriptTurn`
- keeps AskUserQuestion pending UI composer-only
- keeps generic `Working` status outside the transcript
- adds `[container-type:size]` to the `StickToBottom` root and `overflow-x-clip` to the transcript content wrapper
- preserves the existing sticky-bottom ownership model

## What I tested and results

Focused verification:

- `npx vitest run src/views/workspace/Workspace.test.tsx --testNamePattern "sticky turn anchors|sticky-safe|pending Bash|pending Task|pending question widget|Working when the latest message"`
  - passed: 8 tests

Full file verification:

- `npx vitest run src/views/workspace/Workspace.test.tsx`
  - passed: 49 tests

## TDD Evidence

### RED

Command:

```bash
npx vitest run src/views/workspace/Workspace.test.tsx --testNamePattern "sticky turn anchors|sticky-safe|pending Bash|pending Task"
```

Relevant failures before implementation:

- `Unable to find an element by: [data-testid="transcript-turn"]`
- `expect(element).toHaveClass("[container-type:size]")` failed because the root container still had `@container`
- `status.closest('[data-testid="transcript-turn"]')` returned `null` for pending Bash/Task widgets

Why expected:

The workspace still rendered a flat transcript, the sticky-safe wrappers were not present, and pending Bash/Task widgets were still rendered outside transcript turns.

### GREEN

Command:

```bash
npx vitest run src/views/workspace/Workspace.test.tsx
```

Result:

- `49 passed`

## Files changed

- `src/views/workspace/Workspace.tsx`
- `src/views/workspace/Workspace.test.tsx`
- `.superpowers/sdd/task-5-report.md`

## Self-review findings

- No functional regressions found in the focused workspace spec.
- The latest-turn error fallback behaves as intended: it renders inside the last turn when one exists, and falls back to a standalone error block for empty transcripts.

## Issues or concerns

- None.
