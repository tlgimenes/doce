# Follow-Ups Batch Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Execute every deferred follow-up from the transcript/widgets/tracker/stock-reset reviews: dark-mode toggler, Cmd+B/Bold conflict, widget polish (titles, ItemActions, valid markup, empty-Bash, error politeness, multi-hunk test), dead-code removals, stale comments, and repo format hygiene.

**Architecture:** Four independent tasks. Dark mode wires the already-present `next-themes` dep (sonner already calls its `useTheme`). Format hygiene excludes byte-stock `src/components/ui/` from oxfmt via `.prettierignore`, then formats the rest of the repo once so `format:check` becomes a working gate again.

**Tech Stack:** next-themes (present), stock shadcn Select, TipTap StarterKit config, oxfmt.

## Global Constraints

- `main`, in place; each task ends green (focused suites + `npx tsc -b`).
- `src/components/ui/**` stays byte-stock — never edit or format those files (exception: none in this plan).
- App-level composition with utility classes is fine (current project standard); no new custom ui components.
- NEVER bare `npm run format` until Task 4 makes it safe; before that, `npx oxfmt <files>`.
- Commits end with: `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.

---

### Task 1: Dark-mode toggler (next-themes + Settings control)

**Files:**

- Modify: `src/main.tsx` (ThemeProvider), `src/views/settings/Settings.tsx` (Appearance row), `src/styles/theme.css` (line ~7: `@custom-variant dark (&:where(.dark, .dark *));` → the stock `@custom-variant dark (&:is(.dark *));`), `src/test/setup.ts` (matchMedia stub if absent)
- Test: `src/views/settings/Settings.test.tsx` (or the Settings suite — find it), `src/styles/theme.test.ts` only if the variant line is asserted

**Interfaces:**

- Consumes: `ThemeProvider`, `useTheme` from `next-themes` (already a dep; `src/components/ui/sonner.tsx` already imports `useTheme`); stock `Select/SelectTrigger/SelectValue/SelectContent/SelectItem` from `@/components/ui/select` (verify export names in the file first).
- Produces: `.dark` class toggling on `<html>`; a Settings "Appearance" control with options System/Light/Dark, `data-testid="theme-select"`.

- [ ] **Step 1:** Wrap the app in `src/main.tsx`:

```tsx
import { ThemeProvider } from "next-themes";
// inside the existing render tree, directly around the top-level app element:
<ThemeProvider attribute="class" defaultTheme="system" enableSystem disableTransitionOnChange>
  {/* existing app */}
</ThemeProvider>;
```

- [ ] **Step 2:** In Settings.tsx, following the file's existing section/row pattern, add an "Appearance" row:

```tsx
const { theme, setTheme } = useTheme();
…
<Select value={theme ?? "system"} onValueChange={setTheme}>
  <SelectTrigger data-testid="theme-select" aria-label="Theme">
    <SelectValue />
  </SelectTrigger>
  <SelectContent>
    <SelectItem value="system">System</SelectItem>
    <SelectItem value="light">Light</SelectItem>
    <SelectItem value="dark">Dark</SelectItem>
  </SelectContent>
</Select>
```

(Adapt to the stock select.tsx API — Base UI's Select may use `value/onValueChange` or `items`; read the file and the WidgetGallery/Settings existing usage. If stock Select proves awkward in jsdom, `NativeSelect` from ui is an acceptable stock alternative — note the choice.)

- [ ] **Step 3:** theme.css: realign the dark custom-variant to stock `(&:is(.dark *))`. Add a matchMedia stub to `src/test/setup.ts` if jsdom lacks it (next-themes calls `matchMedia("(prefers-color-scheme: dark)")`):

```ts
if (typeof window.matchMedia === "undefined") {
  window.matchMedia = ((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addListener: () => {},
    removeListener: () => {},
    addEventListener: () => {},
    removeEventListener: () => {},
    dispatchEvent: () => false,
  })) as unknown as typeof window.matchMedia;
}
```

- [ ] **Step 4:** Tests: Settings suite — new test: changing the select calls through and flips the html class (render inside ThemeProvider; assert `document.documentElement.classList` gains `dark` after selecting Dark). Run Settings + App + styles suites; `npx tsc -b`; commit `feat(theme): dark-mode toggler via next-themes`.

---

### Task 2: Composer/UserAsk/Command micro-cleanups

**Files:**

- Modify: `src/views/chat/rich-input/RichInput.tsx` (StarterKit config), `src/views/chat/tool-widgets/UserAskWidget.tsx` (dead branch), `src/views/command/CommandCenter.tsx` (dead aria-label)
- Test: existing suites only (behavior unchanged)

- [ ] **Step 1:** RichInput: find the `StarterKit` extension registration and disable Bold (Cmd+B currently both bolds and toggles the stock sidebar): `StarterKit.configure({ bold: false })` (TipTap v3 — verify the option key against the installed package types; if the config differs, disable via the documented mechanism and note it). Confirm no test asserts bold behavior.
- [ ] **Step 2:** UserAskWidget: delete the unreachable `if (!detail.multiSelect) { setSelected([label]); return; }` early-return in `toggleOption` (only the Checkbox branch calls it; RadioGroup handles single-select via `onValueChange`).
- [ ] **Step 3:** CommandCenter: remove the now-dead `aria-label="Command search"` from `CommandInput` (the root `label` wins the accname).
- [ ] **Step 4:** Run: `npx vitest run src/views/chat/rich-input/ src/views/chat/tool-widgets/UserAskWidget.test.tsx src/views/command/`; `npx tsc -b`; commit `chore: composer and command micro-cleanups`.

---

### Task 3: Widget polish batch

**Files:**

- Modify: `src/views/chat/tool-widgets/` — EditDiffWidget.tsx, WriteWidget.tsx, SearchResultsWidget.tsx, BashWidget.tsx, ReadWidget.tsx; `src/views/workspace/TranscriptRow.tsx`, `src/views/workspace/PlanTracker.tsx`
- Test: EditDiffWidget.test.tsx (multi-hunk + markup), BashWidget.test.tsx (empty-output), TranscriptRow.test.tsx (error politeness)

- [ ] **Step 1 — hover titles:** add `title` attrs to the remaining clamped titles: EditDiff both branches (`title={detail.filePath ?? undefined}` on the path ItemTitle), Write both branches (`title={detail.filePath}`), Search interrupted-branch ItemTitle (`title={detail.pattern}`).
- [ ] **Step 2 — ItemActions:** replace the raw `<span className="flex items-center gap-2">` trailing-badge wrappers with stock `ItemActions` (identical classes) in BashWidget (completed branch), EditDiffWidget (success), ReadWidget (success), SearchResultsWidget (success), and PlanTracker's trigger trailing span. Import from `@/components/ui/item`.
- [ ] **Step 3 — valid diff markup:** EditDiffWidget body: outer `<pre data-slot="code-block" …>` → `<div data-slot="code-block" …>` with the same classes (div-in-pre is invalid HTML); drop the inert `wrap-break-word` (next to `whitespace-pre`) while there. Keep all data-slots/variants/testids.
- [ ] **Step 4 — empty-output Bash:** in the completed branch, when `!stdout && !stderr && !payloadPath && !stdoutTrunc.truncated && !stderrTrunc.truncated`, render the header-only (non-collapsible) frame variant instead of a collapsible with an empty panel. New test: completed Bash with empty outputs renders no `role="button"` header and no `widget-frame-content`.
- [ ] **Step 5 — historical error politeness:** TranscriptRow's `contentType === "error"` branch: pass `role="status"` to the `Alert` (persisted history must not fire assertive announcements per row on conversation load; Alert spreads props after its `role="alert"` default — verify override lands in the DOM). The LIVE error Alerts in Workspace.tsx/TranscriptTurn.tsx keep the default. Update the TranscriptRow error test to assert `role="status"` + keep `data-testid="error-message"`.
- [ ] **Step 6 — multi-hunk badge test:** EditDiffWidget.test.tsx: fixture with two non-adjacent edits (e.g. old `"a\nb\nc\nd\ne"` new `"a\nX\nc\nY\ne"`) asserting `+2`/`−2` badges (locks the reduce-across-hunks math).
- [ ] **Step 7:** Run: `npx vitest run src/views/chat/tool-widgets/ src/views/workspace/`; `npx tsc -b`; commit `polish(widgets): titles, ItemActions, valid markup, empty-bash, polite history errors`.

---

### Task 4: Format hygiene + stale comments

**Files:**

- Modify: `.prettierignore` (add `src/components/ui/`), repo-wide formatting, `src/views/workspace/Workspace.tsx` (~line 432 comment), `src/views/workspace/Workspace.test.tsx` (~line 2015 stale StickToBottom comment if still present)

- [ ] **Step 1:** Append `src/components/ui/` to `.prettierignore` (oxfmt honors it — the e2e specs exclusion proves the mechanism).
- [ ] **Step 2:** Stale comments: Workspace.tsx ~432 — reword the comment that still narrates "use-stick-to-bottom" as if present (keep the load-bearing intent: sending re-engages autoscroll); Workspace.test.tsx ~2015 — remove/reword any remaining StickToBottom-era comment (grep `StickToBottom` in src/ — expected: none after this).
- [ ] **Step 3:** `npx oxfmt .` (now safe — ui dir ignored). Inspect `git status`: expect the ~70 known drifted files + docs. Verify `npm run format:check` exits 0.
- [ ] **Step 4:** Full gates: `npm test` (expect current count green; formatting must not change behavior), `npx tsc -b`, `npm run lint`, `npm run build`.
- [ ] **Step 5:** Commit everything as `chore: repo-wide format with byte-stock ui excluded` (formatting) — put the comment fixes in the same commit only if oxfmt touched those files anyway; otherwise a separate `docs(comments): remove stick-to-bottom era narration` first.
