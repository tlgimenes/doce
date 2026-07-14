# Plan Tracker Flat List Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the collapsible plan tracker with an ordered Marker-based task list that scrolls after three rows and reports completion status above the list.

**Architecture:** Keep `PlanTracker`'s recovery and event lifecycle unchanged. Refactor only `PlanTrackerCard` to map `plan.steps` directly into Marker rows inside Shadcn's `MessageScroller`, deriving emphasis from `currentStepIndex`, completion styling from `step.done`, and status counts from completed versus unfinished steps.

**Tech Stack:** React 19, TypeScript, Tailwind CSS, Base UI Checkbox, Shadcn MessageScroller, Vitest, Testing Library

## Global Constraints

- Render `plan.steps` directly without sorting, filtering, grouping, or truncating the data set.
- Use `Marker`, `MarkerIcon`, and `MarkerContent` for every task row.
- Use a disabled checkbox for every task; completed tasks are checked and have line-through text.
- Render the current task in the normal foreground color and every other task in the muted foreground color.
- Use a one-line task description and cap the `overflow-y-auto` viewport at exactly three rows.
- Preserve Shadcn's standard bottom scroll fade and keep all rows mounted.
- Remove all accordion, progress, count, collapsed-completion, and hidden-task summary UI.
- Preserve plan recovery, event subscription, conversation filtering, and turn-end unmount behavior.
- Render a static, left-aligned status header above the scroller as `{doneCount} done · {queuedCount} queued` while unfinished tasks remain, or `{doneCount} completed` when all tasks are done.

---

### Task 1: Replace the collapsible tracker with a three-row Marker list

**Files:**
- Modify: `src/views/workspace/PlanTracker.test.tsx`
- Modify: `src/views/workspace/PlanTracker.tsx`

**Interfaces:**
- Consumes: `PlanSnapshot` with `steps: Array<{ description: string; done: boolean }>` and `currentStepIndex: number | null`.
- Consumes: `Marker`, `MarkerIcon`, `MarkerContent`, `Checkbox`, and the Shadcn `MessageScroller` primitives.
- Produces: `PlanTrackerCard({ plan }: { plan: PlanSnapshot })`, retaining `data-testid="plan-tracker"` and one `data-testid="plan-step"` per source step.

- [ ] **Step 1: Replace accordion-oriented assertions with failing flat-list tests**

Remove the unused `userEvent` import. In the event update test, replace the old progress-summary assertion with state assertions:

```tsx
const updatedRows = screen.getAllByTestId("plan-step");
expect(updatedRows).toHaveLength(3);
expect(updatedRows[2]).toHaveAttribute("data-current", "true");
expect(screen.getAllByRole("checkbox")[1]).toBeChecked();
```

In the stale-recovery test, replace both `2/3` assertions with an assertion that distinguishes the fresher snapshot from the stale one:

```tsx
expect(screen.getByText("Fix bug_01.txt")).toHaveClass("line-through");
```

Delete the tests for collapsed completed steps, collapsed one-line progress, goal fallback, upward expansion, and the all-done trigger icon. Add these two presentation tests:

```tsx
it("renders every step in source order with Marker completion and current-state styling", async () => {
  vi.mocked(commands.getActivePlan).mockResolvedValue(snapshot());
  render(<PlanTracker conversationId="c1" />);

  const rows = await screen.findAllByTestId("plan-step");
  expect(rows).toHaveLength(3);
  expect(rows.map((row) => row.textContent)).toEqual([
    "Find all bug markers",
    "Fix bug_01.txt",
    "Fix bug_02.txt",
  ]);

  const checkboxes = screen.getAllByRole("checkbox");
  expect(checkboxes).toHaveLength(3);
  expect(checkboxes[0]).toBeChecked();
  expect(checkboxes[1]).not.toBeChecked();
  expect(checkboxes[2]).not.toBeChecked();
  checkboxes.forEach((checkbox) => expect(checkbox).toBeDisabled());

  expect(screen.getByText("Find all bug markers")).toHaveClass("line-through");
  expect(rows[0]).toHaveClass("text-muted-foreground");
  expect(rows[1]).toHaveClass("text-foreground");
  expect(rows[2]).toHaveClass("text-muted-foreground");
  expect(rows[1]).toHaveAttribute("data-current", "true");
  expect(rows[0]).not.toHaveAttribute("data-current");
});

it("uses a three-row scroll viewport with Shadcn's bottom fade", async () => {
  vi.mocked(commands.getActivePlan).mockResolvedValue(
    snapshot({
      steps: [
        { description: "s0", done: true },
        { description: "s1", done: false },
        { description: "s2", done: false },
        { description: "s3", done: false },
      ],
      currentStepIndex: 1,
    }),
  );
  render(<PlanTracker conversationId="c1" />);

  const scroller = await screen.findByTestId("plan-task-scroller");
  expect(scroller).toHaveStyle({ maxHeight: "3.75rem" });
  expect(screen.getAllByTestId("plan-step")).toHaveLength(4);

  const viewport = screen.getByTestId("plan-task-viewport");
  expect(viewport).toHaveClass("overflow-y-auto", "scroll-fade-b");
});
```

- [ ] **Step 2: Run the focused test and verify the new assertions fail**

Run:

```bash
npm test -- src/views/workspace/PlanTracker.test.tsx
```

Expected: FAIL because the restored component still renders a collapsible trigger, does not render checkboxes or Marker rows until expanded, and has no `plan-task-scroller` test ID.

- [ ] **Step 3: Replace `PlanTrackerCard` with the minimal Marker/MessageScroller implementation**

Replace the presentation imports with:

```tsx
import { Checkbox } from "@/components/ui/checkbox";
import { Marker, MarkerContent, MarkerIcon } from "@/components/ui/marker";
import {
  MessageScroller,
  MessageScrollerContent,
  MessageScrollerItem,
  MessageScrollerProvider,
  MessageScrollerViewport,
} from "@/components/ui/message-scroller";
```

Add stable sizing constants below the imports:

```tsx
const PLAN_VISIBLE_ROWS = 3;
const PLAN_ROW_HEIGHT_REM = 1.25;
```

Replace `PlanTrackerCard` and remove `stepState`, `CARD_COLLAPSE_THRESHOLD`, and `CARD_MAX_PENDING`:

```tsx
export function PlanTrackerCard({ plan }: { plan: PlanSnapshot }) {
  const maxListHeight = `${PLAN_VISIBLE_ROWS * PLAN_ROW_HEIGHT_REM}rem`;

  return (
    <div className="mx-auto w-full max-w-xl" data-testid="plan-tracker">
      <MessageScrollerProvider>
        <MessageScroller
          className="w-full"
          data-testid="plan-task-scroller"
          style={{ maxHeight: maxListHeight }}
        >
          <MessageScrollerViewport data-testid="plan-task-viewport">
            <MessageScrollerContent className="min-h-0 gap-0">
              {plan.steps.map((step, index) => {
                const isCurrent = index === plan.currentStepIndex;

                return (
                  <MessageScrollerItem key={index}>
                    <Marker
                      className={
                        isCurrent
                          ? "gap-1.5 px-2.5 py-0 text-foreground"
                          : "gap-1.5 px-2.5 py-0 text-muted-foreground"
                      }
                      data-current={isCurrent ? "true" : undefined}
                      data-state={step.done ? "done" : "todo"}
                      data-testid="plan-step"
                    >
                      <MarkerIcon>
                        <Checkbox
                          checked={step.done}
                          className="size-3.5 shrink-0"
                          disabled
                        />
                      </MarkerIcon>
                      <MarkerContent className="min-w-0">
                        <span
                          className={
                            step.done
                              ? "block min-w-0 truncate line-through"
                              : "block min-w-0 truncate"
                          }
                          title={step.description}
                        >
                          {step.description}
                        </span>
                      </MarkerContent>
                    </Marker>
                  </MessageScrollerItem>
                );
              })}
            </MessageScrollerContent>
          </MessageScrollerViewport>
        </MessageScroller>
      </MessageScrollerProvider>
    </div>
  );
}
```

Update the component comments so they describe a flat, three-row scroll viewport rather than an upward-expanding collapsible strip.

- [ ] **Step 4: Run the focused test and verify it passes**

Run:

```bash
npm test -- src/views/workspace/PlanTracker.test.tsx
```

Expected: all `PlanTracker.test.tsx` tests PASS.

- [ ] **Step 5: Run static validation**

Run:

```bash
npm run build
```

Expected: TypeScript and Vite build complete successfully with no errors.

- [ ] **Step 6: Commit the completed refactor**

```bash
git add src/views/workspace/PlanTracker.tsx src/views/workspace/PlanTracker.test.tsx docs/superpowers/specs/2026-07-14-plan-tracker-flat-list-design.md docs/superpowers/plans/2026-07-14-plan-tracker-flat-list.md
git commit -m "refactor(chat): flatten plan tracker task list"
```

### Task 2: Add the plan status header

**Files:**
- Modify: `src/views/workspace/PlanTracker.test.tsx`
- Modify: `src/views/workspace/PlanTracker.tsx`
- Modify: `docs/superpowers/specs/2026-07-14-plan-tracker-flat-list-design.md`
- Modify: `docs/superpowers/plans/2026-07-14-plan-tracker-flat-list.md`

**Interfaces:**
- Consumes: `plan.steps`, where each step has a `done: boolean` field.
- Produces: a `data-testid="plan-status"` header before `data-testid="plan-task-scroller"`.
- Produces: `{doneCount} done · {queuedCount} queued` while unfinished tasks remain, or `{doneCount} completed` when none remain; no separate overall-status label is rendered.

- [ ] **Step 1: Write failing status-header tests**

Add these presentation tests to `PlanTracker.test.tsx`:

```tsx
it("renders done and queued counts on the left above the task list", async () => {
  vi.mocked(commands.getActivePlan).mockResolvedValue(snapshot());
  render(<PlanTracker conversationId="c1" />);

  const status = await screen.findByTestId("plan-status");
  expect(status).toHaveTextContent("1 done");
  expect(status).toHaveTextContent("2 queued");
  expect(status).not.toHaveTextContent("In progress");
  expect(status.children).toHaveLength(1);
  expect(status.firstElementChild).not.toHaveClass("ml-auto");

  const scroller = screen.getByTestId("plan-task-scroller");
  expect(
    status.compareDocumentPosition(scroller) & Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy();
});

it("omits the queued count when every task is done", async () => {
  vi.mocked(commands.getActivePlan).mockResolvedValue(
    snapshot({
      steps: [
        { description: "s0", done: true },
        { description: "s1", done: true },
      ],
      currentStepIndex: null,
    }),
  );
  render(<PlanTracker conversationId="c1" />);

  const status = await screen.findByTestId("plan-status");
  expect(status).toHaveTextContent("2 completed");
  expect(status).not.toHaveTextContent("2 done");
  expect(status).not.toHaveTextContent("queued");
  expect(status).not.toHaveTextContent("Complete");
});
```

- [ ] **Step 2: Run the focused test and verify the new assertions fail**

Run:

```bash
npm test -- src/views/workspace/PlanTracker.test.tsx --reporter=verbose
```

Expected: the two new tests FAIL because `plan-status` does not exist.

- [ ] **Step 3: Implement the status header above the scroller**

At the start of `PlanTrackerCard`, derive the counts:

```tsx
const doneCount = plan.steps.filter((step) => step.done).length;
const queuedCount = plan.steps.length - doneCount;
```

Inside the existing `data-testid="plan-tracker"` container, immediately before `MessageScrollerProvider`, add:

```tsx
<div
  className="px-2.5 py-0 text-xs tabular-nums text-muted-foreground"
  data-testid="plan-status"
>
  <span>
    {queuedCount > 0 ? (
      <>{doneCount} done · {queuedCount} queued</>
    ) : (
      <>{doneCount} completed</>
    )}
  </span>
</div>
```

The header remains outside `MessageScrollerProvider`, uses the same `px-2.5` as task rows, and adds no margin or parent gap.

- [ ] **Step 4: Run the focused test and verify it passes**

Run:

```bash
npm test -- src/views/workspace/PlanTracker.test.tsx --reporter=verbose
```

Expected: all `PlanTracker.test.tsx` tests PASS.

- [ ] **Step 5: Run production validation**

Run:

```bash
npm run build
```

Expected: TypeScript and Vite build complete successfully with no errors.

- [ ] **Step 6: Commit the status header**

```bash
git add src/views/workspace/PlanTracker.tsx src/views/workspace/PlanTracker.test.tsx docs/superpowers/specs/2026-07-14-plan-tracker-flat-list-design.md docs/superpowers/plans/2026-07-14-plan-tracker-flat-list.md
git commit -m "feat(chat): add plan tracker status summary"
```
