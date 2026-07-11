# Task 4 Report: BashWidget onto the frame

## Implementation

Rewrote `src/views/chat/tool-widgets/BashWidget.tsx` to consume the landed
shadcn/base-nova primitives instead of hand-rolled divs/prose/borders:

- **Pending branch** (`!detail.outcome`): `WidgetFrame collapsible defaultOpen`
  with `WidgetFrameHeader` containing `ItemMedia` (Terminal icon), `ItemContent`
  → `ItemTitle` holding a `Spinner` + "Running…" text (`data-testid="bash-status"`),
  and `WidgetFrameContent` → `CodeBlock` for the command (`data-testid="bash-command"`).
- **Spawn-failed branch** (`outcome.ok === false`): non-collapsible `WidgetFrame`
  (no trigger), header shows `CodeInline` command + `ItemDescription` "Failed to
  run" status, body is an `Alert variant="destructive"` wrapping `AlertDescription`
  with the error text (renders `role="alert"`).
- **Completed branch**: collapsible `WidgetFrame` (closed by default), header
  shows `CodeInline` command plus a `bash-status` span with two `Badge`s
  (Success/secondary vs `Failed (exit N)`/destructive, and an `outline` exit/token
  badge). `WidgetFrameContent` holds `CodeBlock` for stdout, `CodeBlock
  tone="destructive"` for stderr, an `ItemDescription` "Output truncated" note,
  and `ViewFullOutput` when a payload path exists.
- Kept `OUTPUT_LINE_CAP`, `truncatedLines`, the header doc-comment, and all prop
  contracts (`detail: BashDetail`) verbatim per the brief.

This matches the task brief's full component listing exactly (verified via
`git diff` — the replacement matches the brief's Step 3 code block character
for character).

## RED evidence

Updated `BashWidget.test.tsx` first (before touching the component) and ran:

```
npx vitest run src/views/chat/tool-widgets/BashWidget.test.tsx
```

Result against the *old* component: `Test Files 1 failed (1)`, `Tests 7 failed
| 4 passed (11)`. Failures were exactly the ones expected from the frame
migration: no `role="button"` yet (collapsible header didn't exist), no
`[data-slot="spinner"]` inside `bash-status`, and `bash-stdout` was present
without a click (old markup never collapsed).

## GREEN evidence

After rewriting the component:

```
npx vitest run src/views/chat/tool-widgets/BashWidget.test.tsx src/views/workspace/TranscriptTurn.test.tsx
```

Result: `Test Files 2 passed (2)`, `Tests 16 passed (16)`.

Also ran the full tool-widgets directory and the whole suite as a broader
safety check (no other widget files were touched):

```
npx vitest run src/views/chat/tool-widgets/   → Test Files 11 passed (11), Tests 67 passed (67)
npx vitest run                                → Test Files 53 passed (53), Tests 411 passed (411)
```

`npx tsc -b` produced no output (clean typecheck).

## Which BashWidget.test.tsx assertions changed, and why

- Added `userEvent` import; most `it()` callbacks are now `async` and click
  `screen.getByRole("button")` before asserting on `bash-stdout`/`bash-stderr`,
  because the completed-Bash `WidgetFrame` is collapsible and closed by
  default — Base UI's `Collapsible.Panel` unmounts closed content, so those
  testids simply aren't in the DOM until expanded (per Task 2's real
  visibility behavior).
- "shows a dispatch-level failure…" now asserts `screen.getByRole("alert")`
  (the `Alert variant="destructive"` renders `role="alert"`) instead of a
  loose `getByText` on the error string alone, tightening the check that the
  spawn-failed branch uses the real Alert primitive.
- "visually distinguishes success from failure": replaced the old regex
  assertions (`/success|0/i`, `/fail|1/i`) with exact text assertions
  ("Success", "Failed (exit 1)") since the component now renders the literal
  text contract from the brief rather than ad hoc class-driven wording; also
  clicks the header before checking `bash-stderr` (closed-by-default).
- Pending-state test: added an explicit assertion that `bash-status` contains
  a `[data-slot="spinner"]` node (replacing the old implicit color-class
  check), and dropped the earlier bare regex-only check in favor of the same
  strengthened structural check. Did **not** assert "no button" for the
  pending branch — it's still `collapsible defaultOpen`, so a trigger with
  `role="button"` exists (just already expanded); only `bash-command` being
  visible without a click was asserted, per the brief.
- Added a new test, "collapses completed output by default until the header
  is clicked", mirroring `widget-frame.test.tsx`'s collapsed-by-default
  pattern: header (`bash-command`, `bash-status`) renders immediately;
  `bash-stdout` is `queryByTestId(...).not.toBeInTheDocument()` before the
  click, then visible after.
- Left all fixtures, IDs, and text contracts (`Running…`, `Failed to run`,
  `Success`, `Failed (exit N)`, `Output truncated`, `$ <command>`, `89 tok`,
  `view-full-output-button`) unchanged — only the interaction/visibility
  mechanics around them changed. No class-based assertions existed in the
  prior test file to drop (the brief anticipated some; this file had none).

## Files changed

- `src/views/chat/tool-widgets/BashWidget.tsx` (rewritten onto `WidgetFrame`/`CodeBlock`/`CodeInline`/`Badge`/`Spinner`/`Alert`/`Item*`)
- `src/views/chat/tool-widgets/BashWidget.test.tsx` (updated for real collapsible visibility + structural assertions)

Staged explicitly (not `git add -A`, to avoid sweeping up other in-flight
parallel tasks' report/working files):

```
git add src/views/chat/tool-widgets/BashWidget.tsx src/views/chat/tool-widgets/BashWidget.test.tsx
```

## Self-review

- Testids preserved exactly: `bash-widget`, `bash-status`, `bash-command`,
  `bash-stdout`, `bash-stderr`, `bash-output-truncated`. Text contracts
  preserved exactly.
- Did not touch `ViewFullOutput.tsx` (already migrated) or any other widget
  file (`ReadWidget`, `TaskWidget`, `EditDiffWidget`, etc.) — grep confirms no
  other files under `src/views/chat/tool-widgets/` or `src/views/workspace/`
  were modified by this task.
- `TranscriptTurn.test.tsx`'s pending-Bash-widget assertion
  (`getByTestId("bash-widget")`) still passes because the pending branch keeps
  `defaultOpen` (expanded), matching the brief's explicit callout.
- The ui `Badge` component has no `data-slot` attribute (confirmed by reading
  `src/components/ui/badge.tsx`), so the test asserts `Badge` text content
  via `bash-status`'s `toHaveTextContent` rather than any slot selector, as
  instructed.
- No stray console errors/warnings observed in the vitest runs (checked
  output tail; only pass/fail summaries and no act()-warning noise from the
  new async/userEvent clicks).
- Commit is scoped to exactly these two files; other modified files present
  in the working tree (`.superpowers/sdd/task-{1,2,3,5,7}-report.md`) belong
  to concurrently running sibling tasks and were left untouched/unstaged.

## Concerns

None. The rewrite is a literal, verified transcription of the brief's Step 3
listing; all pre-existing behavioral fixtures still pass, plus the two
suites the brief calls out (`BashWidget.test.tsx`,
`TranscriptTurn.test.tsx`), plus a full-repo `vitest run` and `tsc -b` as an
extra safety net.
