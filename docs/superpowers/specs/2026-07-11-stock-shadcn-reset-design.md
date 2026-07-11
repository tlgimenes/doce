# Stock Shadcn Reset Design

## Summary

Reset the entire UI foundation to standard shadcn: the default neutral theme
(registry oklch tokens, standard 0.625rem radius, Geist Variable font) and a
`src/components/ui/` directory that is byte-stock from the base-nova
registry. Every custom ui component is deleted — the hand-rolled `button.tsx`
(predates the shadcn bootstrap), the from-scratch `command.tsx`
reimplementation, `KeyboardShortcut.tsx`, and the project-added
`widget-frame.tsx`/`code-block.tsx` — along with every registry modification
(radius cap, Button-API adaptations in 11 files, sidebar behavior strips,
bubble `user` variant). App code composes stock components directly; where a
deleted primitive had consumers, the composition is inlined with plain
elements and utility classes.

This supersedes the ui-layer premises of the 2026-07-09 redesign (doce Brand
Accent Workbench palette, radius cap, sanctioned ui-layer adaptations) and
the "primitives live in the ui layer" choice of
2026-07-10-tool-widgets-shadcn-unify-design.md. Behavior contracts from all
prior specs (scroll semantics, FR-011 fallback, plan-tracker lifecycle,
streaming-status invariants, Read/Search collapse) still bind.

## Decisions (user-confirmed)

1. Standard shadcn neutral theme + Geist font; visuals change everywhere by
   design.
2. Zero custom ui components — registry-stock only, verified by byte-diff.
3. UserAskWidget: FULL migration now (stock RadioGroup/Checkbox/Field/Item
   rows, stock default Button submit, doce entry animations dropped).
4. Execution proceeds straight through (spec → plans → subagents) without
   further pauses.

## Theme (`src/styles/theme.css` rebuilt)

- Base: the standard neutral tokens a fresh `shadcn init` (v4.13.0, vite,
  base-nova) produces — `:root` + `.dark` oklch blocks (byte-identical to
  the registry's `neutral` `cssVarsV4`), stock `@theme inline` mapping
  (`--color-*` aliases, `--radius-sm..4xl` multiples of 0.625rem,
  `--font-sans: 'Geist Variable'`), stock `@layer base`. Reference copy
  captured at scratchpad `fresh-nova/src/index.css`; the legacy HSL
  `r/themes/neutral.json` must NOT be used.
- New deps: `@fontsource-variable/geist` (font), `cmdk` (command).
- Carried over onto the stock file (app infrastructure the registry css
  lacks): `@plugin "@tailwindcss/typography"` (MarkdownPreview prose),
  `:focus-visible` outline rule, enabled-button cursor rule, the
  `chat-surface`/`chat-composer` view-transition rules + their keyframes +
  the shell-stilling root rule, and the `prefers-reduced-motion` block.
- Deleted: all `--color-doce-*` tokens, the doce `:root`/`.dark` bridge and
  radius clamps, `doce-ask-option-row-in` keyframes,
  `.doce-ask-option-row-enter`, and the `user-ask-module` view-transition
  rules (die with the UserAskWidget migration).
- `var(--color-primary)`-style arbitrary references in app code keep
  resolving (stock `@theme inline` still emits `--color-*`); only
  `--color-doce-*` references die and are repointed (below).

## UI layer (`src/components/ui/` byte-stock)

- Mechanism: `npx shadcn@4.13.0 add --all --overwrite` (CLI verified
  viable; components.json aliases resolve to `@/lib/utils`, which exists),
  followed by a verification diff of every file against its fetched
  `https://ui.shadcn.com/r/styles/base-nova/<name>.json` source (normalized
  for install-time substitutions) — zero deviations allowed.
- Deleted files: `KeyboardShortcut.tsx`, `widget-frame.tsx`,
  `code-block.tsx`, `widget-frame.test.tsx`, `code-block.test.tsx`,
  `button.test.tsx`, `command.test.tsx` (the latter two pin deleted custom
  internals). `sidebar.test.tsx` is rewritten to assert stock behavior
  (Cmd/Ctrl+B toggle exists, cookie write happens).
- Notable reverts: stock Button cva (variants default/outline/secondary/
  ghost/destructive/link; sizes default/xs/sm/lg/icon/icon-xs/icon-sm/
  icon-lg; exports Button + buttonVariants only); stock cmdk-backed Command
  (restores combobox ARIA + arrow-key nav); stock radii (rounded-lg/xl grow);
  stock sidebar (Cmd+B toggle + cookie persistence return — known quirk:
  TipTap StarterKit Bold also handles Cmd+B while typing; accepted, noted);
  stock bubble variants only; item/kbd/spinner/etc. already stock stay put.

## App-code fallout (complete map)

### Button API

- `variant="primary"` → `variant="default"`: RichInput.tsx:525,
  UserAskWidget.tsx:207, Settings.tsx:162, WidgetGallery.tsx:104.
- `variant="icon"` → `variant="ghost"`: ConversationList.tsx:361.
- 24px icon buttons: `size="icon-sm"` → `size="icon-xs"` (stock icon-sm is
  28px): App.tsx:367, ShortcutsDialog.tsx:29, UserAskWidget.tsx:159,
  WidgetGallery.tsx:88, ConversationList.tsx archive button.
- Settings.tsx:79,213 link-styled ghost buttons → stock `variant="link"`.
- SearchPanel.tsx:163 result rows `variant="secondary"` →
  `variant="outline"` (stock secondary is borderless; outline matches the
  old border+card look).
- Settings "Add server": `size="sm"` → `size="default"` (stock sm shrank to
  h-7; default is the old h-8).
- Both send-button gradient constants (RichInput.tsx SEND_BUTTON_CLASSES,
  UserAskWidget.tsx SUBMIT_BUTTON_CLASSES) are deleted — stock default
  variant styling applies.

### Deleted-primitive consumers

- KeyboardShortcut → stock `Kbd`/`KbdGroup` (kbd.tsx): ConversationList
  hover hints (⌘N/⌘F), ShortcutsDialog combos — the dialog's visible
  `Cmd+L`-style text contract is preserved by interleaving literal `+`
  spans between Kbds.
- WidgetFrame (8 widget files) → inlined composition, keeping the exact
  chrome and slots so tests/e2e survive: header-only frames become
  `<div data-slot="widget-frame" className="overflow-hidden rounded-lg
  border bg-card text-sm">` + `<Item size="xs">`; collapsible frames become
  `Collapsible` (same slot/class) + `CollapsibleTrigger
  nativeButton={false} render={<Item size="xs" …/>}` + chevron
  (`data-slot="widget-frame-chevron"`, rotate on aria-expanded) +
  `CollapsibleContent className="border-t" data-slot="widget-frame-content"`.
  `Item size="xs"` is registry-stock (verified) — no change there.
- CodeBlock/CodeBlockLine/CodeInline (Bash, EditDiff, Unknown, ReadPreview,
  ViewFullOutput, SearchResults) → plain `<pre>`/`<div>`/`<code>` with the
  same utility strings and `data-slot="code-block"`/`"code-block-line"`
  (+ `data-variant`) attributes kept inline; diff tints become inline
  classes in EditDiffWidget (`bg-emerald-500/15 …` added /
  `bg-destructive/15 text-destructive` removed).
- Bubble `user` variant → stock `variant="secondary"` at TranscriptRow.tsx:63
  ("tinted" degenerates under the neutral theme; "default" is too
  high-contrast for chat).

### CommandCenter rewire (cmdk semantics)

Stock Command's root `value`/`onValueChange` means highlighted item, not
query. CommandCenter moves its query state onto a controlled `CommandInput`;
the custom Enter-runs-first-enabled-action handler is deleted in favor of
cmdk's highlighted-item Enter; disabled actions use cmdk item `disabled`.
Tests (CommandCenter.test.tsx, App.test.tsx command-center blocks) move from
button/textbox roles to cmdk's `option`/`combobox` roles. The modal
shortcut-blocking behaviors from commits 8f40215/1e96f51 (search dialog over
command center) must be re-verified against the stock component and their
tests kept green.

### UserAskWidget full migration

- Single-select rows → stock `RadioGroup`/`RadioGroupItem` composed with
  `Field`/`FieldLabel` (or `Item` rows) — whichever the stock field.tsx
  API supports directly; multi-select rows → stock `Checkbox` in the same
  row layout. Hand-rolled `border-[1.5px]`/`rounded-[4px]` glyphs die.
- Submit → stock default Button (gradient constant deleted).
- `.doce-ask-option-row-enter` usage and inline animationDelay staggering
  removed with their CSS.
- Behavior contracts preserved: option click answers single-select
  immediately; multi-select confirms via submit; free-text fallback via
  RichInput untouched; `user-ask-widget` testid and answer IPC unchanged.
  Its tests keep the toBeDisabled/answer-flow assertions, retargeted to
  radio/checkbox roles.

### Token repoints (doce vars die)

- ConversationList STATUS_COLOR: in_progress → `bg-primary animate-pulse`,
  requires_action → `bg-chart-1`, failed → `bg-destructive` (unchanged),
  done → `bg-muted-foreground/45` (unchanged).
- Onboarding.tsx:85 download-progress fill → `bg-primary`.
- WidgetGallery swatch rows: "Brand Accent Workbench" section becomes a
  standard-token swatch board (primary/secondary/muted/accent/destructive/
  chart-1..5); its test's literal "--color-doce-caramel" assertion updated.

### Tests

- Deleted with their components: button.test, command.test,
  widget-frame.test, code-block.test.
- `src/styles/theme.test.ts`: the var-reference scan must pass against the
  stock file — extend its allowlist for tailwind-core vars stock components
  reference; the alias/radius test already passes.
- ConversationList.test.tsx:351 archive-button class assertion retargeted
  to behavior/aria (stock ghost emits no bg-transparent).
- sidebar.test.tsx rewritten for stock (toggle + cookie exist).
- ShortcutsDialog.test.tsx "Cmd+L" text contract preserved.
- All data-testid/e2e contracts unchanged.

## Verification gates

- Registry byte-diff sweep: every `src/components/ui/*.tsx` matches its
  fetched registry source (after normalizing install substitutions);
  the only non-registry files in the directory are tests shipped by us —
  none for deleted components.
- `grep -rn "doce-" src/ --include="*.tsx" --include="*.ts"` → empty;
  `grep -rn "color-doce" src/styles/theme.css` → empty.
- `npm run build`, `npm test`, `npm run lint`; no-Radix grep still clean
  (stock base-nova is Base UI).
- Real-app visual pass: neutral light/dark theme, stock radii, Geist font,
  transcript + widgets + sidebar + command center render correctly.

## Risks

- Biggest visual change of the project: warm beige → neutral white/dark
  everywhere. Intended.
- cmdk rewire is the largest behavioral risk (Enter semantics, dialog
  shortcut blocking); its tests are the contract.
- Stock destructive Button is tinted (not solid) and stock secondary is
  borderless — accepted stock looks.
- Cmd+B: stock sidebar toggle + TipTap Bold both fire while typing in the
  composer — accepted quirk, candidate follow-up (disable StarterKit Bold).
- `npm run format:check` remains red on pre-existing drift (72 files) —
  out of scope; the CLI-written stock files may add to or subtract from
  that set; gate stays build/test/lint.
