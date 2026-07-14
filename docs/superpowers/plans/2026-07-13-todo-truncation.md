# Todo List Text Truncation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Long step descriptions in the plan/todo tracker truncate to a single line with an ellipsis instead of blowing out the row layout.

**Architecture:** The root cause lives in the shared `Item` primitives (`src/components/ui/item.tsx`): `ItemContent` is `flex-1` without `min-w-0` so it can't shrink, and `ItemTitle` is `flex w-fit` so it grows to fit and — being a flex container — clips text without an ellipsis. Fix the primitives once, then wrap the plan card's step text in an inner `<span className="truncate">` (ellipsis only renders on a non-flex box), and add a long-text mock to the WidgetGallery for permanent visual coverage.

**Tech Stack:** React + Tailwind (Tauri app). Unit tests via `npm test` (vitest). Lint `npm run lint` (oxlint), format `npm run format:check` (oxfmt — NOT prettier).

**Spec:** `docs/superpowers/specs/2026-07-13-todo-truncation-design.md`

## Global Constraints

- Single-line ellipsis only — no multi-line clamping.
- `ItemTitle` must keep `display: flex` (Settings relies on `flex-wrap` with inline badges).
- Full step text stays reachable via the existing `title` hover attribute.
- No styling changes to other `Item` consumers beyond the primitive fix itself.
- Work happens directly on `main`, in place (no worktrees — project convention).
- These are CSS-behavior changes; vitest/jsdom cannot assert visual truncation, so tasks verify by keeping the existing test suite green and Task 3 verifies visually in the real app.

---

### Task 1: Fix the `Item` primitives (`min-w-0` / drop `w-fit`)

**Files:**
- Modify: `src/components/ui/item.tsx:120` (ItemContent base classes)
- Modify: `src/components/ui/item.tsx:133` (ItemTitle base classes)

**Interfaces:**
- Consumes: nothing from other tasks.
- Produces: `ItemContent` renders with `min-w-0` (flex children can shrink); `ItemTitle` renders with `min-w-0 max-w-full` instead of `w-fit`, still `display: flex`. Task 2's call-site truncation depends on both.

- [ ] **Step 1: Add `min-w-0` to ItemContent**

In `src/components/ui/item.tsx`, the `ItemContent` function's `cn(...)` currently reads:

```tsx
"flex flex-1 flex-col gap-1 group-data-[size=xs]/item:gap-0 [&+[data-slot=item-content]]:flex-none",
```

Change it to:

```tsx
"flex min-w-0 flex-1 flex-col gap-1 group-data-[size=xs]/item:gap-0 [&+[data-slot=item-content]]:flex-none",
```

- [ ] **Step 2: Replace `w-fit` with `min-w-0 max-w-full` on ItemTitle**

In the same file, the `ItemTitle` function's `cn(...)` currently reads:

```tsx
"line-clamp-1 flex w-fit items-center gap-2 text-sm leading-snug font-medium underline-offset-4",
```

Change it to:

```tsx
"line-clamp-1 flex min-w-0 max-w-full items-center gap-2 text-sm leading-snug font-medium underline-offset-4",
```

Do NOT remove `flex` — Settings (`src/views/settings/Settings.tsx:228`, `:381`) passes `flex-wrap` and inline badge children that need the flex display.

- [ ] **Step 3: Run the full unit test suite**

Run: `npm test`
Expected: PASS — all suites green, no snapshot/class-assertion failures. (Settings, WorkspaceTopbar, and PlanTracker tests query by text/testid, not classes, so nothing should break; if a test asserts on `w-fit`, update that assertion to the new classes.)

- [ ] **Step 4: Lint and format check**

Run: `npm run lint && npm run format:check`
Expected: both exit 0.

- [ ] **Step 5: Commit**

```bash
git add src/components/ui/item.tsx
git commit -m "fix(ui): let Item content/title shrink so truncation can work"
```

---

### Task 2: Truncate step text in PlanTracker + long-text gallery mock

**Files:**
- Modify: `src/views/workspace/PlanTracker.tsx:140-142` (expanded step rows)
- Modify: `src/views/workspace/PlanTracker.tsx:166-168` (collapsed trigger)
- Modify: `src/views/design-system/WidgetGallery.tsx:500` (insert a new Example after "Mid-execution")
- Test: `src/views/workspace/PlanTracker.test.tsx` (existing — must stay green)

**Interfaces:**
- Consumes: Task 1's primitives — `ItemContent` with `min-w-0`, `ItemTitle` with `min-w-0 max-w-full` (both from `@/components/ui/item`).
- Produces: rendered step rows where the step description is wrapped in `<span className="truncate">` inside `ItemTitle`; a `PlanTrackerCard` gallery example labeled "Long step text (truncated with ellipsis)". Task 3 verifies these visually.

- [ ] **Step 1: Wrap the expanded step row text in a truncating span**

In `src/views/workspace/PlanTracker.tsx`, inside the `pendingVisible.map(...)`, this block:

```tsx
<ItemTitle className="truncate" title={step.description}>
  {step.description}
</ItemTitle>
```

becomes (the `truncate` moves onto an inner span — `text-overflow: ellipsis` never renders on `ItemTitle` itself because it is a flex container):

```tsx
<ItemTitle title={step.description}>
  <span className="truncate">{step.description}</span>
</ItemTitle>
```

- [ ] **Step 2: Same change on the collapsed trigger one-liner**

In the same file, inside the `CollapsibleTrigger`, this block:

```tsx
<ItemTitle className="truncate" title={currentStep?.description ?? plan.goal}>
  {currentStep?.description ?? plan.goal}
</ItemTitle>
```

becomes:

```tsx
<ItemTitle title={currentStep?.description ?? plan.goal}>
  <span className="truncate">{currentStep?.description ?? plan.goal}</span>
</ItemTitle>
```

- [ ] **Step 3: Add the long-text mock to the WidgetGallery**

In `src/views/design-system/WidgetGallery.tsx`, in the `Section title="Plan tracker"`, insert this new `Example` between the existing `Example label="Mid-execution"` (ends line 500) and `Example label="Long plan (completed steps folded, pending capped)"` (starts line 501):

```tsx
<Example label="Long step text (truncated with ellipsis)">
  <PlanTrackerCard
    plan={{
      goal: "Ship the release",
      currentStepIndex: 1,
      steps: [
        { description: "Audit the changelog", done: true },
        {
          description:
            "Cross-check every model registry entry against the upstream capability matrix, then regenerate the tool grammar so the name-enum gate covers the plan tools and the search bound floors",
          done: false,
        },
        { description: "Tag and publish", done: false },
      ],
    }}
  />
</Example>
```

- [ ] **Step 4: Run the full unit test suite**

Run: `npm test`
Expected: PASS. PlanTracker tests query via `toHaveTextContent` / testids, which see through the span wrapper.

- [ ] **Step 5: Lint and format check**

Run: `npm run lint && npm run format:check`
Expected: both exit 0. If `format:check` fails, run `npm run format` and re-check.

- [ ] **Step 6: Commit**

```bash
git add src/views/workspace/PlanTracker.tsx src/views/design-system/WidgetGallery.tsx
git commit -m "fix(chat): truncate long todo step text with ellipsis"
```

---

### Task 3: Visual verification in the real app

**Files:**
- None modified — verification only.

**Interfaces:**
- Consumes: the "Long step text (truncated with ellipsis)" gallery example from Task 2.
- Produces: screenshots confirming the fix; go/no-go on the primitive change's blast radius.

**Note:** This task runs in the MAIN session via the project's `verify` skill (it drives the real Tauri app over WebDriver) — do not dispatch it to a subagent. Remember `DOCE_E2E_SKIP_WIPE` locally so the e2e build doesn't wipe real app data, and kill any orphaned `doce` binaries if the single-instance lock wedges.

- [ ] **Step 1: Invoke the `verify` skill** targeting the WidgetGallery (Cmd+D) Plan tracker section.

- [ ] **Step 2: Screenshot the long-text mock** — collapsed state: one line, ellipsis visible, `1/3` badge and chevron on the SAME line. Expanded state: the long step row is a single line ending in an ellipsis.

- [ ] **Step 3: Regression eyeball** — screenshot the Settings view (model rows with inline badges still wrap correctly) and the workspace topbar (title/path still truncate). Confirm no layout change.

- [ ] **Step 4: Report** — attach screenshots; if anything regressed, fix before closing out.
