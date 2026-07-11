# Task 7 Report: ReadWidget + ReadPreview onto WidgetFrame

## What I implemented

- `ReadWidget.tsx`: replaced the `ToolDisclosure` wrapper with `WidgetFrame` /
  `WidgetFrameHeader` / `WidgetFrameContent` (per the brief's literal Step 3
  code). Failure branch uses a plain `WidgetFrame` + `Alert
  variant="destructive"`. Success branch is `collapsible` without
  `defaultOpen` (collapsed by default), with `FileText` icon, `read-summary`
  path title, and byte/token counts as `Badge` elements in the header. Body
  (`WidgetFrameContent data-testid="read-preview"`) keeps the
  `max-h-80 overflow-y-auto p-3` inner wrapper, renders `ReadPreview` +
  `ViewFullOutput` when a payload path exists.
- `ReadPreview.tsx`: kept extension tables, `readPreviewKind`, and
  `NativeReadPreview`'s fetch effect byte-identical. Replaced only
  presentational returns: text branch now renders `CodeBlock`; loading state
  is a `Spinner` + text line; error and "preview unavailable" states use
  `Empty`/`EmptyHeader`/`EmptyTitle`/`EmptyDescription`; media elements
  (`img`/`video`/`audio`) keep their testids and sizing classes, dropping only
  `rounded-md` (frame body now clips).
- `ToolDisclosure.tsx` left untouched, per instructions (Task 8 deletes it).

## Test changes

- `ReadWidget.test.tsx`: rewrote the collapse-contract assertions from the old
  `<details open>` check to `aria-expanded` on the header trigger button
  (`getByRole("button")`), and `read-preview` presence/absence instead of
  `<details>` semantics. Chevron assertion now checks for
  `[data-slot="widget-frame-chevron"]` inside `read-widget` (the old
  `tool-disclosure-chevron` testid no longer applies). Byte/token count
  assertions switched from a single concatenated `read-summary` string to
  `read-summary` holding just the path plus separate `getByText` checks for
  the `Badge` text, since the brief's literal header structure puts those in
  untagged sibling badges rather than inside the title. Failure-state test
  now asserts `role="alert"` + error text and absence of a button/`read-summary`,
  instead of the old `border-destructive/40` class check.
- `ReadPreview.test.tsx`: added a new "shows a loading spinner" test (none
  existed before) asserting `[data-slot="spinner"]` inside
  `read-preview-loading`; unavailable/error tests now assert
  `data-slot="empty"` on the same element in addition to existing text
  assertions; text-preview test additionally asserts `data-slot="code-block"`.
  Markdown/image/video/audio assertions unchanged.

## Verification

- `npx vitest run src/views/chat/tool-widgets/ReadWidget.test.tsx src/views/chat/tool-widgets/ReadPreview.test.tsx`
  → `Test Files 2 passed (2)`, `Tests 17 passed (17)`.
- `npx vitest run` (full suite) → `Test Files 53 passed (53)`, `Tests 414 passed (414)`.
- `npx tsc -b` → clean, no output.
- `npx oxfmt src/views/chat/tool-widgets/ReadWidget.tsx src/views/chat/tool-widgets/ReadPreview.tsx src/views/chat/tool-widgets/ReadWidget.test.tsx src/views/chat/tool-widgets/ReadPreview.test.tsx` → formatted 4 files; re-ran tests + typecheck after, still clean.

## Files changed

- `src/views/chat/tool-widgets/ReadWidget.tsx`
- `src/views/chat/tool-widgets/ReadPreview.tsx`
- `src/views/chat/tool-widgets/ReadWidget.test.tsx`
- `src/views/chat/tool-widgets/ReadPreview.test.tsx`

Committed with explicit `git add <paths>` (not `-A`), since the working tree
also has unrelated pre-existing uncommitted drift in
`.superpowers/sdd/task-1..5-report.md` and other files from an earlier,
differently-numbered SDD batch — left untouched.

## Concerns

- The brief's "Interfaces" prose says the summary text is
  `Read <path> · <bytes>[ · N tok]` and to "assert via read-summary testid
  text," but the brief's own literal Step 3 code puts byte/token counts in
  separate `Badge` siblings outside the `read-summary` `ItemTitle`, with no
  middot separator and no wrapping testid (unlike `BashWidget`'s
  `bash-status` span). I followed the literal code as authoritative and
  adjusted test assertions to match the actual rendered structure
  (`read-summary` = path only; badge text checked via `getByText`). Worth a
  glance from whoever wrote the brief in case a `data-testid` on that badge
  span was intended but dropped.
