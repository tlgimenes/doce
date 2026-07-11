# Marker Widgets Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Widgets as stock Marker activity rows, shimmering Working status, and the composer floating inside the MessageScroller.

**Architecture:** Three surface changes in dependency order: the tiny shimmer change first, the widget re-idiom second (biggest), the Workspace overlay third (touches placement tests), then gates + one combined runtime pass.

**Tech Stack:** stock Marker/Spinner/Collapsible/Badge, the `shimmer` utility (ships with the shadcn chat set; precedent in attachment.tsx), MessageScroller.

**Spec:** `docs/superpowers/specs/2026-07-11-marker-widgets-design.md` — read it first; its per-widget table is the contract.

## Global Constraints

- `main`, in place; each task green (suites + `npx tsc -b`).
- No new ui components; `src/components/ui/**` untouched. App-level utility classes are fine.
- Every `data-testid` survives; `data-slot="widget-frame*"`/`"code-block*"` attributes are RETIRED (update tests to marker slots / testids); e2e-pinned testids (`bash-widget`, `bash-stdout`, `edit-diff`, `question-answered`, etc.) must keep working — check `tests/e2e/specs/tool-call-widgets.spec.ts` for every selector it uses and preserve those hooks (the US2 test clicks the bash header via `[data-slot='widget-frame-header']` — give the new bash trigger `data-slot="marker"` is automatic; UPDATE the e2e selector to the new trigger hook (`[data-testid='bash-widget'] [role='button']`) as part of Task 2).
- Decorative Spinners: `role="presentation" aria-label={undefined}`.
- NEVER bare `npm run format`; `npx oxfmt <changed files>` (not ui/\*\*, not tests/e2e — prettierignore covers both).
- Commits end with: `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.

---

### Task 1: Shimmer on StreamingStatus

**Files:**

- Modify: `src/views/workspace/StreamingStatus.tsx`, `StreamingStatus.test.tsx`

- [ ] **Step 1:** Test first: in the main accessibility test, add `expect(screen.getByText("Working")).toHaveClass("shimmer")` (or on `MarkerContent` if the class lands there — write the assertion against the span, run, and place the class where the assertion passes; docs say MarkerContent, but the visible text node must carry the animation — try MarkerContent first, fall back to the inner span, and note which).
- [ ] **Step 2:** Apply `className="shimmer"` to the `MarkerContent` (or span) in StreamingStatus. Nothing else changes — chron, roles, suppression untouched.
- [ ] **Step 3:** `npx vitest run src/views/workspace/StreamingStatus.test.tsx src/views/workspace/Workspace.test.tsx` + `npx tsc -b` green; oxfmt; commit `feat(status): shimmer on the working marker`.

---

### Task 2: Widgets as Marker rows

**Files:**

- Modify: all of `src/views/chat/tool-widgets/` except UserAskWidget (Bash, EditDiff, Read, ReadPreview [wrapper only if needed], Search, Task, Unknown, Write, AskUserQuestionWidget) + their tests
- Modify: `tests/e2e/specs/tool-call-widgets.spec.ts` (bash header selector per Global Constraints)

**Interfaces:**

- Consumes: `Marker/MarkerIcon/MarkerContent` (Marker is useRender-based → composes with `CollapsibleTrigger nativeButton={false} render={<Marker …/>}`), `Collapsible/CollapsibleContent`, `Badge`, `Spinner`, `ChevronRight`, existing icons.
- Produces per the spec's per-widget table. Two shapes:

Header-only marker (Write/Task/Ask/failed/interrupted branches):

```tsx
<Marker data-testid="…">
  <MarkerIcon><FilePlus /></MarkerIcon>
  <MarkerContent className="flex min-w-0 flex-col">
    <span className="truncate" title={…}>{primary}</span>
    {secondary && <span className="text-xs">{secondary}</span>}
  </MarkerContent>
  <Badge variant="…" className="ml-auto shrink-0">…</Badge>
</Marker>
```

Collapsible marker (Bash-done, EditDiff, Read, Search, Unknown):

```tsx
<Collapsible data-testid="…" {...(defaultOpen ? { defaultOpen: true } : {})}>
  <CollapsibleTrigger
    nativeButton={false}
    render={<Marker className="group/marker-row cursor-pointer" />}
  >
    <MarkerIcon><Terminal /></MarkerIcon>
    <MarkerContent className="min-w-0 truncate" title={…}>{primary}</MarkerContent>
    <span className="ml-auto flex shrink-0 items-center gap-2">
      {…badges…}
      <ChevronRight
        aria-hidden="true"
        className="size-4 shrink-0 transition-transform group-aria-expanded/marker-row:rotate-90"
      />
    </span>
  </CollapsibleTrigger>
  <CollapsibleContent className="pl-6">
    {/* existing body content unchanged: pres keep their mono classes,
        previews/match lists/JSON as today; the old bordered wrapper divs
        and data-slot="widget-frame-content"/"code-block" attributes are
        dropped (keep body testids like bash-stdout/read-preview on the
        same elements) */}
  </CollapsibleContent>
</Collapsible>
```

- [ ] **Step 1:** Read every widget file + its test; map each to the spec table. Preserve: all text contracts (status strings verbatim), all data-testids, running-Bash `defaultOpen`, EditDiff `defaultOpen`, empty-output Bash header-only, hover `title`s, decorative-Spinner neutralization, ViewFullOutput placement inside bodies, FR-011 fallback.
- [ ] **Step 2:** Rewrite tests alongside each widget: structure assertions move from `widget-frame` slots to `data-slot="marker"`/roles/testids; collapse behavior still via `role="button"` + panel unmount; drop all `data-slot="widget-frame*"`/`code-block*` assertions.
- [ ] **Step 3:** Update the e2e bash-header selector (Global Constraints) — verify every other selector in that spec still resolves against the new DOM by reading the spec against the new markup (do NOT run e2e).
- [ ] **Step 4:** `npx vitest run src/views/chat/tool-widgets/ src/views/workspace/` + `npx tsc -b` green; `grep -rn "widget-frame\|data-slot=\"code-block" src/` → empty; oxfmt app files; commit `refactor(widgets): marker activity rows replace card frames`.

---

### Task 3 (AMENDED per the user's pasted official demo): demo-aligned Provider + bubble-gray composer

**Files:**

- Modify: `src/views/workspace/Workspace.tsx` (Provider span only), `src/views/chat/rich-input/RichInput.tsx` (+ its test)

- [ ] **Step 1:** Workspace: widen `MessageScrollerProvider` so it wraps the tracker/status/composer block as well (move `</MessageScrollerProvider>` below the composer shell div, exactly like the demo wraps the whole Card). NO other placement change — the composer stays a sibling BELOW the scroller; the earlier overlay idea is superseded. `key={conversationId}` stays on the Provider.
- [ ] **Step 2:** RichInput: the InputGroup goes bubble-gray per the directive ("same styles as the bubble, gray; remove the outline, shadow-sm when focused"):

```tsx
<InputGroup className="border-transparent bg-secondary shadow-none focus-within:shadow-sm has-[[data-slot=input-group-control]:focus-visible]:border-transparent has-[[data-slot=input-group-control]:focus-visible]:ring-0">
```

(bg-secondary matches the user bubble's variant="secondary"; ring-0/border-transparent neutralize the stock focus ring; tailwind-merge resolves the has-[] conflicts — verify in the rendered class list that ring-0 wins, adjust ordering if not.)

- [ ] **Step 3:** Test: RichInput.test gains an assertion that the group carries `bg-secondary` and `focus-within:shadow-sm` and not the stock ring (class-list assertion is acceptable here — it IS the styling contract). Run rich-input + workspace + EmptyState + UserAskWidget suites + `npx tsc -b`; oxfmt; commit `feat(composer): bubble-gray input aligned with the scroller demo`.

---

### Task 5: Sidebar top strip drags the window

**Files:**

- Modify: `src/App.tsx` (sidebar header strip) — read `src/components/Topbar.tsx` FIRST for the app's drag mechanism (Tauri `getCurrentWindow().startDragging()` wiring and the `data-topbar-no-drag` opt-out convention)
- Test: `src/App.test.tsx` if the topbar drag has an existing test pattern to mirror

- [ ] **Step 1:** Locate the sidebar's top strip in App.tsx (the row hosting the shortcuts/keyboard button above the sidebar content). Wire the SAME drag behavior the main topbar uses (reuse the exported handler/component from Topbar.tsx if one exists — do not duplicate logic; extract the smallest reusable piece if needed, e.g. an exported `useWindowDrag` or the existing mousedown handler). Interactive children (the shortcuts button) must keep working — apply the same `data-topbar-no-drag` guard the main topbar uses.
- [ ] **Step 2:** Mirror the main topbar's test pattern (drag-region present, button still clickable). Run `npx vitest run src/App.test.tsx src/components/Topbar.test.tsx` + `npx tsc -b`; oxfmt; commit `feat(shell): sidebar top strip drags the window`.

---

### Task 4: Gates + combined runtime pass

- [ ] **Step 1:** `npm run build && npm test && npm run lint && npm run format:check` → all green.
- [ ] **Step 2 (controller-owned):** one runtime pass: real transcript renders marker rows (collapsed + expanded), Working shimmers during a turn (or static check of the class), composer floats with content scrolling behind it and correct clearance at bottom, scroll button above the overlay, empty composer NOT washed out (InputGroup fix), light + dark screenshots.
- [ ] **Step 3:** Commit anything the gates surfaced.
