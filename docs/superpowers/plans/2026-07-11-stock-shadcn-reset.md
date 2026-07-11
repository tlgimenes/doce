# Stock Shadcn Reset Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Standard shadcn everywhere — neutral theme + Geist font, every `src/components/ui` file byte-stock from the base-nova registry, all custom ui components deleted, app fallout fixed.

**Architecture:** Pre-work first (Tasks 1–3 move app code off the to-be-deleted primitives while the tree stays green), then the CLI overwrite + Button-API sweep (Task 4), the cmdk Command restore + CommandCenter rewire (Task 5), the theme swap (Task 6), gallery regeneration (Task 7), and gates (Task 8). Every task ends green.

**Tech Stack:** shadcn CLI v4.13.0 (base-nova/Base UI), Tailwind v4, cmdk, @fontsource-variable/geist, Vitest.

**Spec:** `docs/superpowers/specs/2026-07-11-stock-shadcn-reset-design.md` — read it first. One spec correction discovered while planning: UserAskWidget's CURRENT behavior is select-then-submit for BOTH single- and multi-select (per its 2026-07-08 redesign) — preserve that, not the spec's "option click answers immediately" line.

## Global Constraints

- Work on `main`, in place. Tasks run in order; each ends with green focused suites + `npx tsc -b`.
- NEVER run bare `npm run format` (repo-wide drift); `npx oxfmt <files>` only. Explicit `git add <paths>`; never sweep `.superpowers/`.
- Preserve every `data-testid` and data-slot named per task (tests + e2e pin them).
- Decorative Spinners keep `role="presentation" aria-label={undefined}`.
- Known red pre-existing: `npm run format:check` (~72 drifted files) — not a gate. ConversationList full-suite flake — isolate-rerun if sole failure.
- Commits end with: `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.

---

### Task 1: Inline widget-frame/code-block into their consumers; delete both primitives

**Files:**

- Modify: `src/views/chat/tool-widgets/` — BashWidget.tsx, EditDiffWidget.tsx, ReadWidget.tsx, ReadPreview.tsx, SearchResultsWidget.tsx, TaskWidget.tsx, UnknownToolWidget.tsx, WriteWidget.tsx, AskUserQuestionWidget.tsx, ViewFullOutput.tsx
- Delete: `src/components/ui/widget-frame.tsx`, `widget-frame.test.tsx`, `src/components/ui/code-block.tsx`, `code-block.test.tsx`
- Tests: the widgets' colocated tests (assertion targets unchanged — data-slots survive inline)

**Interfaces:**

- Consumes: stock `Collapsible/CollapsibleTrigger/CollapsibleContent`, `Item` family (`size="xs"` is registry-stock, verified), `ChevronRight` from lucide.
- Produces: identical DOM contracts, now inline. Three reusable shapes (write them verbatim per widget, adjusting testids/content):

Header-only frame:

```tsx
<div
  data-slot="widget-frame"
  className="overflow-hidden rounded-lg border border-border bg-card text-sm"
  data-testid="…"
>
  <Item data-slot="widget-frame-header" size="xs" className="w-full">
    {/* ItemMedia / ItemContent / trailing spans exactly as today */}
  </Item>
  {/* error Alert block if the widget has one */}
</div>
```

Collapsible frame (add `defaultOpen` where the widget passes it today):

```tsx
<Collapsible
  data-slot="widget-frame"
  className="overflow-hidden rounded-lg border border-border bg-card text-sm"
  data-testid="…"
>
  <CollapsibleTrigger
    nativeButton={false}
    render={
      <Item
        data-slot="widget-frame-header"
        size="xs"
        className="group/widget-frame w-full cursor-pointer rounded-none hover:bg-accent"
      />
    }
  >
    {/* header children exactly as today */}
    <ChevronRight
      aria-hidden="true"
      data-slot="widget-frame-chevron"
      className="ml-auto size-4 shrink-0 text-muted-foreground transition-transform group-aria-expanded/widget-frame:rotate-90"
    />
  </CollapsibleTrigger>
  <CollapsibleContent data-slot="widget-frame-content" className="border-t border-border">
    {/* body exactly as today */}
  </CollapsibleContent>
</Collapsible>
```

Code block replacements (exact class strings):

- `CodeBlock` → `<pre data-slot="code-block" data-tone="default" className="overflow-x-auto px-3 py-2 font-mono text-xs whitespace-pre-wrap wrap-break-word text-foreground">…</pre>`; `tone="destructive"` → same with `text-destructive` (keep `data-tone="destructive"`). EditDiff's `className="p-0 whitespace-pre"` merges into the pre's classes (`p-0` replaces the padding, `whitespace-pre` replaces `whitespace-pre-wrap`).
- `CodeBlockLine variant={v}` → `<div data-slot="code-block-line" data-variant={v} className={…}>` with `px-3 py-0.5 whitespace-pre` + variant classes: default `text-foreground`, added `bg-emerald-500/15 text-emerald-700 dark:text-emerald-400`, removed `bg-destructive/15 text-destructive` (EditDiff can keep a small local `const lineClass = { default: …, added: …, removed: … }` map — a lookup table, not a component).
- `CodeInline` → `<code data-slot="code-inline" className="font-mono text-xs">…</code>`.

- [ ] **Step 1:** Rewrite the 10 consumer files per the shapes above (no import from `@/components/ui/widget-frame` or `@/components/ui/code-block` remains). `git rm` the four ui files.
- [ ] **Step 2:** Run: `npx vitest run src/views/chat/tool-widgets/ src/views/workspace/` — all pass unchanged (they assert data-slots/testids, which survive). Fix only assertions that referenced the deleted test files' imports.
- [ ] **Step 3:** `npx tsc -b` clean; `grep -rn "widget-frame\"\|code-block\"" src/components/ui/` empty; `grep -rn "from \"@/components/ui/widget-frame\|from \"@/components/ui/code-block" src/` empty.
- [ ] **Step 4:** `npx oxfmt` changed files; commit `refactor(widgets): inline frame and code-block composition, drop custom ui primitives`.

---

### Task 2: KeyboardShortcut → stock Kbd/KbdGroup

**Files:**

- Modify: `src/views/chat/ConversationList.tsx` (lines ~219–222, ~236–239), `src/views/shortcuts/ShortcutsDialog.tsx` (import + line ~46)
- Delete: `src/components/ui/KeyboardShortcut.tsx`
- Tests: `ConversationList.test.tsx`, `ShortcutsDialog.test.tsx` (visible-text contracts unchanged)

**Interfaces:** stock `Kbd` (styled `<kbd>`), `KbdGroup` (flex gap-1 wrapper) from `@/components/ui/kbd`.

- [ ] **Step 1:** ConversationList hover hints: `<KeyboardShortcut keys={["⌘","N"]} className={X}/>` → `<KbdGroup className={X}><Kbd>⌘</Kbd><Kbd>N</Kbd></KbdGroup>` (same for ⌘F). ShortcutsDialog: `<KeyboardShortcut keys={s.combo.split("+")} data-testid={…}/>` →

```tsx
<KbdGroup data-testid={`shortcut-combo-${s.id}`}>
  {s.combo.split("+").map((key, i) => (
    <Fragment key={`${key}-${i}`}>
      {i > 0 && <span aria-hidden="true">+</span>}
      <Kbd>{key}</Kbd>
    </Fragment>
  ))}
</KbdGroup>
```

(preserves the test's visible `Cmd+L` textContent). `git rm src/components/ui/KeyboardShortcut.tsx`.

- [ ] **Step 2:** `npx vitest run src/views/chat/ConversationList.test.tsx src/views/shortcuts/` pass; `npx tsc -b` clean; `grep -rn "KeyboardShortcut" src/` empty.
- [ ] **Step 3:** oxfmt; commit `refactor(ui): replace KeyboardShortcut with stock Kbd`.

---

### Task 3: UserAskWidget full migration + stale e2e answer-flow fix

**Files:**

- Modify: `src/views/chat/tool-widgets/UserAskWidget.tsx`, `UserAskWidget.test.tsx`
- Modify: `tests/e2e/specs/tool-call-widgets.spec.ts` (~line 91)

**Interfaces:** stock `RadioGroup/RadioGroupItem`, `Checkbox`, `Field/FieldContent/FieldLabel/FieldTitle/FieldDescription`, existing `Button`/`RichInput`/IPC. Behavior contract preserved: selecting only selects; BOTH modes answer via the submit button; free-text fallback unchanged; testids `user-ask-widget`, `question-close`, `question-back-to-options`, `question-submit`, `question-answer-input`, `question-answer-send` unchanged.

- [ ] **Step 1:** Rewrite the options module. Delete `OptionRow`, `SUBMIT_BUTTON_CLASSES`, the `doce-ask-option-row-enter` class + `animationDelay`, and the `[view-transition-name:user-ask-module]` class (keep the `runViewTransition` mode switch — it degrades to the root transition gracefully). New options body (replaces lines ~183–219; single-select shown, checkbox arm analogous):

```tsx
<div className="flex flex-col gap-2 rounded-lg border border-border bg-card px-3 py-2 shadow-xs transition-shadow focus-within:shadow-sm">
  {detail.multiSelect ? (
    <div className="flex flex-col gap-0.5" role="group" aria-labelledby={questionId}>
      {detail.options.map((option) => (
        <FieldLabel key={option.label} htmlFor={`${questionId}-${option.label}`}>
          <Field orientation="horizontal" data-testid="question-option">
            <Checkbox
              id={`${questionId}-${option.label}`}
              checked={selected.includes(option.label)}
              onCheckedChange={() => toggleOption(option.label)}
              disabled={submitting}
            />
            <FieldContent>
              <FieldTitle>{option.label}</FieldTitle>
              {option.description && <FieldDescription>{option.description}</FieldDescription>}
            </FieldContent>
          </Field>
        </FieldLabel>
      ))}
    </div>
  ) : (
    <RadioGroup
      value={selected[0] ?? null}
      onValueChange={(value) => setSelected(value == null ? [] : [String(value)])}
      aria-labelledby={questionId}
      disabled={submitting}
      className="flex flex-col gap-0.5"
    >
      {detail.options.map((option) => (
        <FieldLabel key={option.label} htmlFor={`${questionId}-${option.label}`}>
          <Field orientation="horizontal" data-testid="question-option">
            <RadioGroupItem id={`${questionId}-${option.label}`} value={option.label} />
            <FieldContent>
              <FieldTitle>{option.label}</FieldTitle>
              {option.description && <FieldDescription>{option.description}</FieldDescription>}
            </FieldContent>
          </Field>
        </FieldLabel>
      ))}
    </RadioGroup>
  )}
  <div className="flex items-center justify-between gap-2">
    <span className="text-xs text-muted-foreground">
      {detail.multiSelect && selected.length > 0 ? `${selected.length} selected` : ""}
    </span>
    <Button
      type="button"
      variant="primary"
      size="icon"
      disabled={selected.length === 0 || submitting}
      onClick={() => submit(selected)}
      aria-label="Send answer"
      data-testid="question-submit"
    >
      <SendHorizontal size={16} />
    </Button>
  </div>
</div>
```

(`variant="primary"` is intentional here — Task 4 renames it to `default` in its sweep. Verify Base UI RadioGroup's `onValueChange` value type against `radio-group.tsx`/its Base UI props and coerce as needed. Verify `FieldLabel htmlFor` + nested control click-through works in the first test run; if double-toggle occurs, drop the outer `FieldLabel` and use per-control `FieldLabel` inside `FieldContent`.)

- [ ] **Step 2:** Update `UserAskWidget.test.tsx`: role assertions move to the native controls (`getAllByRole("radio")`/`"checkbox"` still work — Base UI primitives carry them), selection via clicking the option label text, everything else (submit disabled until selection, answer IPC payloads, mode switch, free-text) unchanged. Run: `npx vitest run src/views/chat/tool-widgets/UserAskWidget.test.tsx` → pass.
- [ ] **Step 3:** Fix the stale e2e flow at `tests/e2e/specs/tool-call-widgets.spec.ts:91`: after `await (await widget.$("button=Red")).click();` — the 2026-07-08 redesign made selection NOT submit — add the missing submit click. With the new markup "Red" is a label, not a `<button>`, so replace the selector too:

```ts
await (await widget.$("[data-testid='question-option']=Red")).click();
// selecting never submits (select-then-submit contract); answer explicitly:
await (await widget.$("[data-testid='question-submit']")).click();
```

(Use `widget.$("div=Red")`/text-within-testid per wdio selector support — verify the selector form compiles; behavior contract: click the Red option row, then question-submit.) Note in your report: this test could never pass since 2026-07-08 — yesterday's "backend stall" finding is thereby reclassified as a stale test.

- [ ] **Step 4:** `npx tsc -b`; `grep -rn "doce-ask-option-row-enter\|user-ask-module" src/views/` empty. oxfmt; commit `refactor(widgets): UserAskWidget on stock RadioGroup, Checkbox, and Field`.

---

### Task 4: Registry overwrite (all but command) + Button-API sweep

**Files:**

- Overwrite: all of `src/components/ui/*.tsx` via CLI, then restore the current custom `command.tsx` (Task 5 handles it)
- Delete: `src/components/ui/button.test.tsx`
- Modify: `src/views/chat/rich-input/RichInput.tsx`, `src/views/chat/tool-widgets/UserAskWidget.tsx`, `src/views/settings/Settings.tsx`, `src/views/design-system/WidgetGallery.tsx`, `src/views/chat/ConversationList.tsx`, `src/views/chat/SearchPanel.tsx`, `src/App.tsx`, `src/views/shortcuts/ShortcutsDialog.tsx`, `src/views/workspace/TranscriptRow.tsx`, `src/views/onboarding/Onboarding.tsx`
- Tests: `ConversationList.test.tsx`, `sidebar.test.tsx`, `src/styles/theme.test.ts` (if it trips)

**Interfaces:** stock Button (variants default/outline/secondary/ghost/destructive/link; sizes default/xs/sm/lg/icon/icon-xs/icon-sm/icon-lg; exports `Button`, `buttonVariants` only). Stock bubble (no `user` variant).

- [ ] **Step 1:** `npx shadcn@4.13.0 add --all --overwrite` (answer prompts non-interactively if flags exist: `--yes`). Then `git checkout -- src/components/ui/command.tsx` (defer to Task 5) and `git rm src/components/ui/button.test.tsx`. Confirm `git status` shows only `src/components/ui/` changes. If the CLI adds files for components not previously present, keep them (stock). If the CLI errors, fall back to fetching each `https://ui.shadcn.com/r/styles/base-nova/<name>.json` and writing `files[0].content` with install substitutions (strip `"use client"` per rsc:false is NOT needed — registry files ship it and vite tolerates it; rewrite `@/registry/...`/IconPlaceholder imports to lucide + `@/components/ui`).
- [ ] **Step 2:** App sweep (exact sites from the fallout inventory):
  - `variant="primary"` → `variant="default"`: RichInput.tsx:525, UserAskWidget.tsx (Task 3's submit), Settings.tsx:162, WidgetGallery.tsx:104.
  - ConversationList.tsx:361 `variant="icon"` → `variant="ghost"`, and its `size="icon-sm"` → `size="icon-xs"` (keeps 24px).
  - `size="icon-sm"` → `size="icon-xs"`: App.tsx:367, ShortcutsDialog.tsx:29, UserAskWidget.tsx:159, WidgetGallery.tsx:88.
  - Settings.tsx:79,213 underline-ghost buttons → `variant="link"` (drop the `p-0 … underline hover:bg-transparent` overrides that reimplemented it).
  - Settings.tsx:162 `size="sm"` → `size="default"` (stock sm shrank to h-7).
  - SearchPanel.tsx:163 `variant="secondary"` → `variant="outline"`.
  - RichInput.tsx: delete `SEND_BUTTON_CLASSES` (line ~17) and its usage — stock default styling stands.
  - TranscriptRow.tsx:63 `<Bubble align="end" variant="user">` → `variant="secondary"`.
  - Onboarding.tsx:85 progress fill `bg-[var(--color-doce-caramel)]`-style → `bg-primary`.
  - ConversationList.tsx STATUS_COLOR: `in_progress: "bg-primary animate-pulse"`, `requires_action: "bg-chart-1"` (failed/done unchanged).
- [ ] **Step 3:** Test triage: ConversationList.test.tsx:351 `toHaveClass("bg-transparent","size-6")` → assert `aria-label` + `size-6` only if `icon-xs` emits it (stock icon-xs is size-6 — verify from the stock cva; else drop class assertions for behavior). `sidebar.test.tsx`: rewrite the two strip-assertions to stock behavior (Cmd/Ctrl+B toggles — fire keydown, assert state change; cookie write present). `theme.test.ts`: if its ui-var scan trips on new stock vars, extend its allowlist (do NOT weaken the assertion style). Run: `npx vitest run src/` full → green (isolate ConversationList flake if sole failure).
- [ ] **Step 4:** `npx tsc -b`; oxfmt changed app files (NOT the generated ui files — leave CLI output byte-stock; if oxfmt reformats them the byte-diff gate in Task 8 must normalize formatting, so simply exclude `src/components/ui` from oxfmt here); commit `refactor(ui)!: reset ui layer to stock shadcn registry (base-nova)`.

---

### Task 5: Stock cmdk Command + CommandCenter rewire

**Files:**

- Overwrite: `src/components/ui/command.tsx` via `npx shadcn@4.13.0 add command --overwrite`
- Delete: `src/components/ui/command.test.tsx`
- Modify: `src/views/command/CommandCenter.tsx`
- Tests: `CommandCenter.test.tsx`, `src/App.test.tsx` (command-center blocks)

**Interfaces:** stock cmdk Command set (same export names). New dep: `npm install cmdk`.

- [ ] **Step 1:** `npm install cmdk` then `npx shadcn@4.13.0 add command --overwrite`. Read the generated file: exports must match the current names (Command, CommandDialog, CommandInput, CommandList, CommandEmpty, CommandGroup, CommandItem, CommandShortcut, CommandSeparator).
- [ ] **Step 2:** Rewire `CommandCenter.tsx` — the root's value/onValueChange means highlighted-item in cmdk, and cmdk filters + handles Enter natively. Full replacement of the component body (keep the `CommandCenterAction` interface and props):

```tsx
export default function CommandCenter({ open, onOpenChange, actions }: CommandCenterProps) {
  const [query, setQuery] = useState("");

  useEffect(() => {
    if (!open) {
      setQuery("");
    }
  }, [open]);

  const runAction = (action: CommandCenterAction) => {
    action.run();
    onOpenChange(false);
  };

  return (
    <Dialog
      open={open}
      onClose={() => onOpenChange(false)}
      title="Command center"
      description="Run application actions."
      contentClassName="w-[34rem]"
    >
      <div className="w-full" data-testid="command-center">
        <Command className="rounded-lg border border-border/70 bg-popover p-0">
          <CommandInput
            autoFocus
            aria-label="Command search"
            placeholder="Type a command or search"
            value={query}
            onValueChange={setQuery}
          />
          <CommandList className="max-h-80 p-1">
            <CommandEmpty>No matching actions.</CommandEmpty>
            <CommandGroup heading="Actions">
              {actions.map((action) => (
                <CommandItem
                  key={action.id}
                  value={action.label}
                  keywords={[action.id, action.shortcut ?? ""]}
                  disabled={action.disabled}
                  onSelect={() => runAction(action)}
                >
                  <span>{action.label}</span>
                  {action.shortcut ? <CommandShortcut>{action.shortcut}</CommandShortcut> : null}
                </CommandItem>
              ))}
            </CommandGroup>
          </CommandList>
        </Command>
      </div>
    </Dialog>
  );
}
```

(Deletes `matchesActionQuery`, `visibleEnabledActions`, `handleInputKeyDown` — cmdk's filtering covers value+keywords and Enter runs the highlighted item, which lands on the first match by default. Delete the now-unused `useMemo`/`KeyboardEvent` imports.)

- [ ] **Step 3:** `git rm src/components/ui/command.test.tsx` (pins deleted internals). Update `CommandCenter.test.tsx` + `App.test.tsx` command blocks: items are `role="option"` (aria-disabled, not toBeDisabled), the input is cmdk's (`getByRole("combobox")` or placeholder query), Enter-on-first-match now asserts cmdk behavior (type query → press Enter → action ran), disabled actions are skipped by Enter (cmdk skips disabled highlighted items — assert the behavior, adapt to what cmdk actually does and document it). The modal shortcut-blocking tests from commits 8f40215/1e96f51 (search dialog over command center) MUST stay green — they live at the App level and should survive; if they interact with focus, re-verify.
- [ ] **Step 4:** Run: `npx vitest run src/views/command/ src/App.test.tsx` → green; full `npm test` once; `npx tsc -b`. oxfmt app files only; commit `refactor(command): restore stock cmdk command and rewire the command center`.

---

### Task 6: Theme swap

**Files:**

- Rewrite: `src/styles/theme.css`
- Modify: `package.json` (`npm install @fontsource-variable/geist`), `src/main.tsx` (font import if not via CSS), `src/styles/theme.test.ts`

**Interfaces:** the standard neutral token blocks captured verbatim at `/private/tmp/claude-501/-Users-gimenes-code-doce/ee089bbc-b313-414b-b7ba-e1c60a797261/scratchpad/fresh-nova/src/index.css` (source of truth — READ IT; it is byte-identical to the registry `neutral` `cssVarsV4`).

- [ ] **Step 1:** `npm install @fontsource-variable/geist`. Rebuild `theme.css` in this order:
  1. `@import "tailwindcss";` + `@import "tw-animate-css";` + `@import "shadcn/tailwind.css";` + `@import "@fontsource-variable/geist";` — copy the exact import list from the fresh-nova file, dropping any import whose package is absent (verify `shadcn/tailwind.css` resolves — it ships with the `shadcn` package the repo has; if absent, install what fresh-nova's package.json used).
  2. `@plugin "@tailwindcss/typography";` (carried over — MarkdownPreview prose).
  3. `@custom-variant dark (&:is(.dark *));`
  4. The fresh-nova `@theme inline` block verbatim (includes `--font-sans: 'Geist Variable'…` and `--radius-*`).
  5. The fresh-nova `:root` + `.dark` token blocks verbatim.
  6. The fresh-nova `@layer base` block verbatim.
  7. Carried-over infrastructure, copied byte-for-byte from the CURRENT theme.css: the `:focus-visible` rule, the enabled-button cursor rule, the `chat-surface` + `chat-composer` view-transition rules and `doce-chat-surface-in/out` keyframes and the root-stilling `::view-transition-old/new(root)` rule, and the `prefers-reduced-motion` block MINUS any `user-ask-module`/`doce-ask-option-row` selectors (those die).
     Delete everything else (doce palette, bridges, radius clamps, `.dark` doce overrides, doce-ask keyframes, user-ask-module rules).
- [ ] **Step 2:** Check `next-themes`/dark-mode wiring still matches (`@custom-variant dark` uses `.dark` class — the current app already toggles a `.dark` class via next-themes; verify in `src/main.tsx`/App and adjust the custom-variant line to the current mechanism if it differs — the CURRENT theme.css line 4 is the reference).
- [ ] **Step 3:** `src/styles/theme.test.ts`: first test (alias/radius) should pass; the ui-var scan must pass against stock files + new theme — extend its known-vars list minimally. Run: `npx vitest run src/styles/ && npm test` → green. `grep -n "doce" src/styles/theme.css` → only `doce-chat-surface` keyframe names remain (motion infra); `grep -rn "color-doce" src/` → empty.
- [ ] **Step 4:** `npx tsc -b`; `npm run build` (Tailwind must compile the new imports). Commit `feat(theme)!: standard shadcn neutral theme with Geist`.

---

### Task 7: WidgetGallery regeneration

**Files:**

- Modify: `src/views/design-system/WidgetGallery.tsx`, `WidgetGallery.test.tsx` (or wherever the "Brand Accent Workbench"/"--color-doce-caramel" assertions live — grep first)

- [ ] **Step 1:** Button showcase → enumerate stock variants (default/outline/secondary/ghost/destructive/link) and sizes (default/xs/sm/lg + icon set); rename the visible "Primary" label to "Default". Swatch section: replace the doce swatch rows with standard tokens (background/foreground/primary/secondary/muted/accent/destructive/border/chart-1..5), section heading "Theme tokens" (drop "Brand Accent Workbench").
- [ ] **Step 2:** Update the gallery test's literal assertions to the new heading/token names. Run: `npx vitest run src/views/design-system/ src/App.test.tsx` → green; `npx tsc -b`. oxfmt; commit `chore(gallery): showcase stock variants and standard tokens`.

---

### Task 8: Byte-diff sweep + gates + runtime pass

- [ ] **Step 1:** Registry byte-diff: for every `src/components/ui/*.tsx` (61 kebab files), fetch `https://ui.shadcn.com/r/styles/base-nova/<name>.json`, extract `files[0].content`, normalize install-time substitutions (icon imports, alias paths, formatting via `npx oxfmt --check`-insensitive compare: strip whitespace-only differences with `diff -wB`), and diff. Expected: zero substantive deviations; write the per-file result table into your report. Any deviation = fix by re-fetching stock.
- [ ] **Step 2:** Custom-component absence: `ls src/components/ui/ | grep -vE '\.test\.tsx$'` contains only registry names (no KeyboardShortcut/widget-frame/code-block); `grep -rn "doce-" src --include="*.tsx" --include="*.ts"` → empty; `grep -rn "color-doce" src/` → empty; `rg "@radix-ui|radix-ui" src package.json` → empty.
- [ ] **Step 3:** Gates: `npm run build`, `npm test`, `npm run lint` → green.
- [ ] **Step 4 (controller-owned):** runtime visual pass in the real app — neutral light theme, Geist font, stock radii; sidebar rows, transcript bubbles, widgets, command center (Cmd+K type-filter-Enter), UserAsk options flow, dark mode toggle.
- [ ] **Step 5:** Commit anything the sweep fixed: `fix(review): registry byte-diff sweep fixes`.
