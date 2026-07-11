# Task 3 report: ViewFullOutput + UnknownToolWidget on stock primitives

## What was implemented

Followed the brief in `.superpowers/sdd/task-3-brief.md` exactly (TDD: update
tests -> RED -> rewrite components -> GREEN -> typecheck/format/commit).

### `src/views/chat/tool-widgets/ViewFullOutput.tsx`

- Kept imports/state/IPC logic (the `useState`/`load` function) byte-identical.
- Replaced the loaded-text branch: bare `<pre data-testid="view-full-output-content">`
  -> `<CodeBlock data-testid="view-full-output-content">{fullText}</CodeBlock>`.
- Replaced the idle/error branch: raw `<button>` with manual Tailwind classes
  -> `<Button variant="ghost" size="sm" onClick={load} disabled={loading}
  data-testid="view-full-output-button">`, with a `<Spinner
  role="presentation" aria-label={undefined} />` prefixed while `loading`, and
  the error now rendered via `<Alert variant="destructive"><AlertDescription>`
  instead of a raw `<p className="text-destructive">`.
- Added imports: `Alert`/`AlertDescription` (`@/components/ui/alert`),
  `Button` (`@/components/ui/button`), `CodeBlock` (`@/components/ui/code-block`),
  `Spinner` (`@/components/ui/spinner`).
- Dropped the old `border-t border-border` wrapper and underline-link
  styling — callers' frames (BashWidget/ReadWidget, unchanged in this task)
  provide the visual separation, per the brief.

### `src/views/chat/tool-widgets/UnknownToolWidget.tsx`

Full JSX replacement exactly as specified in the brief: `WidgetFrame
collapsible data-testid="unknown-tool-widget"` wrapping a `WidgetFrameHeader`
(Wrench icon in `ItemMedia variant="icon"`, tool name in `ItemTitle` inside
`ItemContent`) and a `WidgetFrameContent` holding a `CodeBlock` with the
pretty-printed JSON detail. The FR-011 doc comment and the `UnknownToolWidgetProps`
type/signature (`{ detail: ToolResultDetail | UnknownToolDetail }`) are
unchanged — never-blank contract preserved (WidgetFrame always renders the
header; only the JSON body collapses).

## RED evidence

Before rewriting the components, ran the updated
`ViewFullOutput.test.tsx` (new structural assertions: button `tagName ===
"BUTTON"` + text, `data-slot="code-block"` on the loaded content, and a new
pending-state test asserting `disabled` + a `[data-slot="spinner"]` child)
against the *old* implementation:

```
❯ npx vitest run src/views/chat/tool-widgets/ViewFullOutput.test.tsx
 × shows a 'View full output' button that fetches and displays the full text on click
   Expected data-slot="code-block", received null
 × shows a disabled button with a spinner while the fetch is pending
   AssertionError: expected null not to be null   (no [data-slot="spinner"])
 Test Files  1 failed (1)
      Tests  2 failed | 1 passed (3)
```

## GREEN evidence

After the component rewrite:

```
❯ npx vitest run src/views/chat/tool-widgets/ViewFullOutput.test.tsx src/views/workspace/TranscriptRow.test.tsx
 Test Files  2 passed (2)
      Tests  25 passed (25)
```

Also ran the full suite as a safety net (`WidgetGallery.test.tsx` renders
`UnknownToolWidget` too, and there are no other consumers of either
component besides `BashWidget`/`ReadWidget`, which are out of scope for this
task and untouched):

```
❯ npx vitest run
 Test Files  53 passed (53)
      Tests  410 passed (410)
```

`npx tsc -b` — clean, no errors.

## TranscriptRow.test.tsx adjustments

**None were needed.** The two fallback-widget tests
(`renders the fallback widget for a tool_result whose toolName has no
dedicated widget…` and `degrades to the fallback widget on unparseable
tool_result content…`, around lines 231-261) only assert
`screen.getByTestId("unknown-tool-widget")).toBeInTheDocument()` and
`screen.getByText("SomeMcpTool")` — the tool name now lives in the always-
visible `WidgetFrameHeader`/`ItemTitle`, not inside the collapsed
`WidgetFrameContent` JSON body, so both assertions still pass unmodified.
No test in that file asserts on the JSON body text, so nothing needed
expanding via `userEvent.click` on the header. The never-blank contract is
intact: `WidgetFrame`'s header (with the Wrench icon + tool name) always
renders regardless of the collapsible content's mount state.

## Files changed

- `src/views/chat/tool-widgets/ViewFullOutput.tsx` (rewritten per brief)
- `src/views/chat/tool-widgets/UnknownToolWidget.tsx` (rewritten per brief)
- `src/views/chat/tool-widgets/ViewFullOutput.test.tsx` (updated: structural
  assertions on the button/content, plus a new pending-state test using the
  manual-resolve-promise pattern already used in `PlanTracker.test.tsx`)

Not touched (out of scope for Task 3, callers of `ViewFullOutput`):
`BashWidget.tsx`, `ReadWidget.tsx` — their own `border-t border-border`
wrappers still apply; the removed border/spacing from `ViewFullOutput` is a
later-task concern per the brief ("callers' frames provide separation").

## Self-review notes

- `Spinner`'s default `aria-label="Loading"` is intentionally overridden to
  `undefined` per the brief's exact code block, so it doesn't double up
  screen-reader announcements next to the button's own "Loading…" text
  label — the button text already communicates state.
- Verified `Button`'s custom implementation (no Radix/shadcn `data-slot`)
  really has no `link` variant, confirming the brief's note; used
  `variant="ghost" size="sm"` as instructed, text-only (no manual underline
  styling re-added).
- Confirmed `.superpowers/` is git-ignored for new files (verified with
  `git check-ignore`), though some `.superpowers/sdd/*-report.md` files
  were already tracked before the ignore rule was added and show as
  modified in `git status` from concurrent parallel-task activity; per
  instructions I committed only the 3 task-relevant source files via
  explicit `git add <path>`, not `git add -A`, leaving those unrelated
  pre-existing modifications alone.
- `npx oxfmt <files>` run on all 3 changed files (not bare `npm run
  format`); no diffs produced (already formatted).
- Full-repo `npx vitest run` (53 files / 410 tests) and `npx tsc -b` both
  clean after the change, confirming no regressions in other consumers
  (`WidgetGallery.tsx`/`WidgetGallery.test.tsx`, `TranscriptRow.tsx`).
- Note: this report file previously held content from an unrelated,
  differently-numbered "Task 3" (a conversation search dialog feature from
  an earlier SDD ledger); overwritten per instructions with this task's
  report.

## Commit

`cd93a22` — `refactor(widgets): ViewFullOutput and UnknownToolWidget on stock primitives`
(3 files changed, 64 insertions(+), 24 deletions(-))
