# Task 5 Report: EditDiffWidget onto CodeBlockLine

## What I implemented

Followed the brief (`.superpowers/sdd/task-5-brief.md`) verbatim.

### `src/views/chat/tool-widgets/EditDiffWidget.tsx`

- Failed-edit branch: `<div data-testid="edit-failed">` hand-rolled destructive
  card → `WidgetFrame` (non-collapsible) with `WidgetFrameHeader`
  (`ItemMedia variant="icon"` holding lucide `FilePen`, `ItemContent`/
  `ItemTitle` for the file path) + an `Alert variant="destructive"`/
  `AlertDescription` for the error, matching the BashWidget failure-branch
  shape.
- Success branch: `WidgetFrame collapsible defaultOpen data-testid="edit-diff"`
  (expanded by default, per the brief — this diff body is visible without
  any click). Header adds `+N`/`−N` `Badge`(`variant="outline"`) counts next
  to the file path, computed via `diffLines` + a `lineCount` helper
  (trailing-newline-stripped line count) reduced over added/removed hunks.
- Body: the old hand-rolled `<pre>` + `bg-emerald-500/15`/`bg-red-500/15`
  row divs → `CodeBlock className="p-0 whitespace-pre"` (layout-only
  overrides so `CodeBlockLine`s own their padding and long lines scroll
  instead of wrap, same behavior as the old `<pre>`) wrapping
  `CodeBlockLine variant={added|removed|default}` per line, preserving the
  `+ `/`- `/`  ` prefix and the `diff-added`/`diff-removed` testid wrapper
  divs exactly as before.
- Import set matches the brief exactly: `diff`, lucide `FilePen`, `Alert`/
  `AlertDescription`, `Badge`, `CodeBlock`/`CodeBlockLine`, `ItemContent`/
  `ItemMedia`/`ItemTitle`, `WidgetFrame`/`WidgetFrameContent`/
  `WidgetFrameHeader`, `EditDetail`.

### `src/views/chat/tool-widgets/EditDiffWidget.test.tsx`

- Kept the two existing tests' content assertions (`diff-added`/
  `diff-removed` presence + text) but added `data-variant` checks on the
  contained `CodeBlockLine`: `removed.querySelector('[data-slot="code-block-line"]')`
  → `data-variant="removed"`, same for `added` → `"added"`. No class
  assertions (`bg-emerald-500/15` etc.) existed in the original file to
  remove — the prior test never asserted on classes, only text/testid
  presence, so nothing to replace there.
- Added a new test asserting the header's `+N`/`−N` badges: for the fixture
  (`oldString`/`newString` differing by one line, "old line" → "new line"),
  independently verified via `diffLines` in a scratch node run that the
  diff resolves to exactly one added hunk and one removed hunk (1 line
  each) → asserts `screen.getByText("+1")` and `screen.getByText("−1")`
  (U+2212 minus sign, matching the component's literal, verified byte-for-
  byte against the brief).
- Failed-edit test left unchanged (already exercised `edit-failed`,
  `diff-added`/`diff-removed` absence, and the error text — all still
  valid against the new markup).

## Tests

- Fail-before (Step 2): `npx vitest run src/views/chat/tool-widgets/EditDiffWidget.test.tsx`
  → 2 failed / 1 passed, as expected (new badge test + variant assertions
  don't exist yet against the old component).
- After rewriting `EditDiffWidget.tsx`, same command → **3 passed / 3 tests**.
- `npx tsc -b`: clean, no errors.
- `npx oxlint src/views/chat/tool-widgets/EditDiffWidget.tsx src/views/chat/tool-widgets/EditDiffWidget.test.tsx`: clean, no output.
- `npx oxfmt src/views/chat/tool-widgets/EditDiffWidget.tsx src/views/chat/tool-widgets/EditDiffWidget.test.tsx`: ran (per binding constraint — never bare `npm run format`), no diff produced (files already formatted).
- Full suite: `npx vitest run` → **53 files passed, 412 tests passed**.

## Format / scope hygiene

Used `npx oxfmt <files>` (not `npm run format`) scoped to only the two
target files, so no repo-wide reformat churn occurred. Staged only the two
target files explicitly (`git add src/views/chat/tool-widgets/EditDiffWidget.tsx
src/views/chat/tool-widgets/EditDiffWidget.test.tsx`), not `git add -A`.
`.superpowers/sdd/task-{1,2,3,4,7}-report.md` were already modified in the
working tree before I started (evidently from other in-flight/prior task
executions in this same session) and were left completely untouched.
`task-5-report.md` itself is overwritten here per this task's explicit
instruction to write the report to this exact path (previously held an
unrelated PlanTracker report from an earlier SDD numbering cycle — same
overwrite situation noted in that PlanTracker report).

## Self-review (per brief checklist)

- Testids preserved unchanged: `edit-diff`, `edit-failed`, `diff-added`,
  `diff-removed` — confirmed via test assertions and a `grep` over the
  file.
- Diff's `variant` ternary (`added`/`removed`/`default`) matches
  `CodeBlockLine`'s `variant` prop type exactly — `tsc -b` is clean.
- `defaultOpen` is set on the success-path `WidgetFrame` so the diff body
  renders without any expansion click — verified: no `userEvent.click` was
  needed anywhere in the test file for the diff to be visible (unlike
  BashWidget's collapsed-by-default completed state).
- No leftover `bg-emerald-500/15`/`bg-red-500/15`/hand-rolled classes in
  the component — confirmed via `grep -n "emerald-500|red-500" EditDiffWidget.tsx`, no matches (colors now live entirely in `CodeBlockLine`'s cva variants).

## Files changed

- `src/views/chat/tool-widgets/EditDiffWidget.tsx`
- `src/views/chat/tool-widgets/EditDiffWidget.test.tsx`

Commit: `c1eca24` — "refactor(widgets): EditDiffWidget on WidgetFrame and CodeBlockLine"

## Concerns / deviations from the brief

None. The rewrite matches the brief's Step 3 snippet verbatim (confirmed
byte-for-byte on the `−` (U+2212) minus-sign literal in both the Badge
and the diff-row prefix), and the test additions are exactly what Step 1
specified — no un-briefed fixes or type-signature widenings were needed
this time.
