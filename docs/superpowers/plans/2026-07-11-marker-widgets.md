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

### Task 3: Composer overlay inside the scroller

**Files:**

- Modify: `src/views/workspace/Workspace.tsx`, `Workspace.test.tsx`, `PlanTracker.test.tsx` (placement guard)

- [ ] **Step 1:** Test updates first: (a) Workspace docking test — tracker/status/composer now INSIDE `[data-slot="message-scroller"]`, in an overlay wrapper (`data-testid="composer-overlay"`), document order tracker → status(when shown) → composer shell; the old "tracker not inside scroller root" guard flips to "tracker inside the overlay"; (b) a new assertion: `workspace-transcript-content` (or its column) carries the clearance padding class; (c) scroll button still present.
- [ ] **Step 2:** Restructure Workspace's return: move `<PlanTracker/>`, the `showGenericStreamingStatus && <StreamingStatus/>`, and the composer shell div into

```tsx
<div className="absolute inset-x-0 bottom-0 z-10 flex flex-col bg-background" data-testid="composer-overlay">
```

as the LAST child inside `<MessageScroller>` (after `MessageScrollerButton`). Add `className="pb-40"` to the transcript column wrapper (the `mx-auto w-full max-w-3xl` div) for clearance, and `className="bottom-44"` to `MessageScrollerButton` so it floats above the overlay (adjust both numbers together; they are layout-tunable).

- [ ] **Step 3:** Keep the composer shell's `[view-transition-name:chat-composer]` and conditional border exactly; keep `key={conversationId}` Provider semantics.
- [ ] **Step 4:** `npx vitest run src/views/workspace/` + `npx tsc -b` green; oxfmt; commit `refactor(workspace): composer floats inside the message scroller`.

---

### Task 4: Gates + combined runtime pass

- [ ] **Step 1:** `npm run build && npm test && npm run lint && npm run format:check` → all green.
- [ ] **Step 2 (controller-owned):** one runtime pass: real transcript renders marker rows (collapsed + expanded), Working shimmers during a turn (or static check of the class), composer floats with content scrolling behind it and correct clearance at bottom, scroll button above the overlay, empty composer NOT washed out (InputGroup fix), light + dark screenshots.
- [ ] **Step 3:** Commit anything the gates surfaced.
