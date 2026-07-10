# Plan Tracker Docked Above Composer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move the plan/todo tracker from its floating transcript-gutter overlay to a docked collapsible strip directly above the chat input (Claude Code style), deleting the card/rail split.

**Architecture:** `PlanTracker` becomes a `Collapsible` strip: a one-liner trigger (current-step icon + title + n/m Badge) with the full step list expanding upward (`CollapsibleContent` rendered before the trigger). `Workspace` mounts it between the transcript scroller and `StreamingStatus`. All chrome stays stock (Item/Badge/Progress/Spinner/Collapsible).

**Tech Stack:** React 19, shadcn base-nova on Base UI, Tailwind v4 tokens, Vitest + Testing Library.

**Spec:** `docs/superpowers/specs/2026-07-10-plan-tracker-docked-design.md` — read it first.

## Global Constraints

- Work on `main`, in place. Execute AFTER the tool-widgets plan (shared files: none, but sequencing keeps reviews clean).
- `PlanTracker.tsx` and `Workspace.tsx` may add only layout utilities. All visuals from ui-layer primitives. No new theme.css rules.
- Lifecycle logic in PlanTracker's effect (subscription, recovery, `sawEvent` race guards, immediate unmount on `plan: null`) is byte-frozen.
- Step-row semantics preserved: `stepState` helper, `data-state`/`data-current`, caps (`CARD_COLLAPSE_THRESHOLD = 6`, `CARD_MAX_PENDING = 4`), `plan-step`/`plan-done-collapsed`/`plan-more` testids. `RAIL_MAX_DOTS` and all rail/card code is deleted.
- New testid: `plan-current-step` on the one-liner trigger. Deleted testids: `plan-card`, `plan-rail`, `plan-dot`, `plan-collapse`, `plan-chip`.
- Decorative Spinners: `role="presentation" aria-label={undefined}`.
- Never run bare `npm run format`; format only task files with `npx oxfmt <paths>`.
- Commits end with: `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.

---

### Task 1: Recompose PlanTracker as the docked strip

**Files:**

- Modify: `src/views/workspace/PlanTracker.tsx`
- Test: `src/views/workspace/PlanTracker.test.tsx`

**Interfaces:**

- Consumes: `Collapsible/CollapsibleTrigger/CollapsibleContent` from `@/components/ui/collapsible`; `Item` family, `Badge`, `Progress`, `Spinner`, lucide `Check`/`ChevronDown`/`Circle`.
- Produces: `PlanTracker({ conversationId })` unchanged. DOM: root `Collapsible` `data-testid="plan-tracker"`; trigger `data-testid="plan-current-step"` (`aria-expanded` reflects state); list content with `plan-step` rows identical to today.

- [ ] **Step 1: Update tests**

In `PlanTracker.test.tsx`:

- Delete the card/rail tests ("renders the dot rail (with matching states) alongside the card, and a chip past 12 steps") and any `plan-card`/`plan-rail`/`plan-dot`/`plan-collapse` queries.
- Lifecycle tests (recovery, event filtering, unmount-on-null, both race tests) keep working untouched — they assert `plan-tracker` presence/absence only.
- Rewrite step-content assertions to go through expansion: the one-liner shows the CURRENT step (not all steps), so tests that assert `plan-step` rows must first `await userEvent.click(screen.getByTestId("plan-current-step"))`.
- New assertions:

```tsx
it("shows the current step and progress in the collapsed one-liner", async () => {
  vi.mocked(commands.getActivePlan).mockResolvedValue({
    goal: "Ship the feature",
    currentStepIndex: 1,
    steps: [
      { description: "Write tests", done: true },
      { description: "Implement", done: false },
      { description: "Verify", done: false },
    ],
  });
  render(<PlanTracker conversationId="conv-1" />);

  const trigger = await screen.findByTestId("plan-current-step");
  expect(trigger).toHaveTextContent("Implement");
  expect(trigger).toHaveTextContent("1/3");
  expect(trigger).toHaveAttribute("aria-expanded", "false");
  expect(trigger.querySelector('[data-slot="spinner"]')).not.toBeNull();
});

it("falls back to the goal while planning (currentStepIndex null)", async () => {
  vi.mocked(commands.getActivePlan).mockResolvedValue({
    goal: "Ship the feature",
    currentStepIndex: null,
    steps: [{ description: "Write tests", done: false }],
  });
  render(<PlanTracker conversationId="conv-1" />);

  expect(await screen.findByTestId("plan-current-step")).toHaveTextContent("Ship the feature");
});

it("expands upward into the full step list", async () => {
  // reuse the 3-step fixture above
  render(<PlanTracker conversationId="conv-1" />);
  await userEvent.click(await screen.findByTestId("plan-current-step"));

  const steps = screen.getAllByTestId("plan-step");
  expect(steps).toHaveLength(3);
  expect(steps[0]).toHaveAttribute("data-state", "done");
  expect(steps[1]).toHaveAttribute("data-state", "current");
  // The list renders BEFORE the trigger in DOM order (upward expansion).
  expect(
    steps[0].compareDocumentPosition(screen.getByTestId("plan-current-step")) &
      Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy();
});

it("shows a Check icon in the one-liner when every step is done", async () => {
  vi.mocked(commands.getActivePlan).mockResolvedValue({
    goal: "Ship the feature",
    currentStepIndex: null,
    steps: [{ description: "Write tests", done: true }],
  });
  render(<PlanTracker conversationId="conv-1" />);

  const trigger = await screen.findByTestId("plan-current-step");
  expect(trigger.querySelector('[data-slot="spinner"]')).toBeNull();
  expect(trigger).toHaveTextContent("1/1");
});
```

(Match the existing file's mock setup — it already mocks `commands`/`events`;
follow its fixture shape exactly, including any fields the `PlanSnapshot`
type requires beyond goal/steps/currentStepIndex.)

- [ ] **Step 2: Verify failures**

Run: `npx vitest run src/views/workspace/PlanTracker.test.tsx`
Expected: FAIL (`plan-current-step` not found).

- [ ] **Step 3: Rewrite the render**

Keep in `PlanTracker.tsx`: imports of `commands/events/PlanSnapshot`, the
caps constants minus `RAIL_MAX_DOTS`, the props interface, the entire effect,
the `if (!plan || plan.steps.length === 0) return null;` guard, `doneCount`,
and the `stepState` helper. Delete `PlanCard`, `PlanRail`, the `expanded`
state, and the `cn` import if unused. Replace the returned JSX and add a
derived current-step lookup:

```tsx
const doneCount = plan.steps.filter((s) => s.done).length;
const allDone = doneCount === plan.steps.length;
const currentStep = plan.currentStepIndex != null ? plan.steps[plan.currentStepIndex] : undefined;

const collapseDone = plan.steps.length > CARD_COLLAPSE_THRESHOLD;
const rows = plan.steps
  .map((step, index) => ({ step, index }))
  .filter(({ step, index }) => {
    if (!collapseDone) return true;
    // Keep the current step and pending ones; completed fold into the
    // "✓ n done" line.
    return !step.done || index === plan.currentStepIndex;
  });
const pendingVisible = collapseDone ? rows.slice(0, CARD_MAX_PENDING + 1) : rows;
const hiddenCount = rows.length - pendingVisible.length;

return (
  <div className="px-4">
    <Collapsible className="mx-auto max-w-3xl" data-testid="plan-tracker">
      {/* Content BEFORE the trigger: the list expands upward, Claude
            Code style — the one-liner stays anchored just above the
            composer. */}
      <CollapsibleContent>
        <Progress
          className="px-2 py-1"
          value={plan.steps.length > 0 ? (doneCount / plan.steps.length) * 100 : 0}
        />
        {collapseDone && doneCount > 0 && (
          <ItemDescription className="px-2" data-testid="plan-done-collapsed">
            ✓ {doneCount} done
          </ItemDescription>
        )}
        <ItemGroup>
          {pendingVisible.map(({ step, index }) => (
            <Item
              key={index}
              size="xs"
              data-state={stepState(step, index, plan.currentStepIndex)}
              data-current={index === plan.currentStepIndex ? "true" : undefined}
              data-testid="plan-step"
            >
              <ItemMedia variant="icon">
                {step.done ? (
                  <Check />
                ) : index === plan.currentStepIndex ? (
                  <Spinner role="presentation" aria-label={undefined} />
                ) : (
                  <Circle />
                )}
              </ItemMedia>
              <ItemContent>
                <ItemTitle className="truncate" title={step.description}>
                  {step.description}
                </ItemTitle>
              </ItemContent>
            </Item>
          ))}
        </ItemGroup>
        {hiddenCount > 0 && (
          <ItemDescription className="px-2" data-testid="plan-more">
            +{hiddenCount} more
          </ItemDescription>
        )}
      </CollapsibleContent>
      <CollapsibleTrigger
        render={
          <Item
            size="xs"
            variant="muted"
            className="group/plan w-full cursor-pointer"
            data-testid="plan-current-step"
          />
        }
      >
        <ItemMedia variant="icon">
          {allDone ? <Check /> : <Spinner role="presentation" aria-label={undefined} />}
        </ItemMedia>
        <ItemContent>
          <ItemTitle className="truncate" title={currentStep?.description ?? plan.goal}>
            {currentStep?.description ?? plan.goal}
          </ItemTitle>
        </ItemContent>
        <span className="flex items-center gap-2">
          <Badge variant="secondary">
            {doneCount}/{plan.steps.length}
          </Badge>
          <ChevronDown
            aria-hidden="true"
            className="size-4 shrink-0 text-muted-foreground transition-transform group-aria-expanded/plan:rotate-180"
          />
        </span>
      </CollapsibleTrigger>
    </Collapsible>
  </div>
);
```

New imports: `Check, ChevronDown, Circle` from lucide-react;
`Collapsible, CollapsibleContent, CollapsibleTrigger` from
`@/components/ui/collapsible`; add `ItemDescription`, `ItemGroup` to the item
import; DELETE `Card*`, `Button` imports and `cn` if now unused. Update the
component's doc comment: it no longer floats over the gutter — it docks
above the composer; card/rail split is gone (drop the container-query
sentences, keep the lifecycle sentences).

The same `render={<Item …/>}` trigger composition as `widget-frame.tsx` —
if Base UI rejects it, use the same fallback and note it.

- [ ] **Step 4: Verify pass**

Run: `npx vitest run src/views/workspace/PlanTracker.test.tsx`
Expected: PASS.

- [ ] **Step 5: Typecheck, format, commit**

```bash
npx tsc -b && npx oxfmt src/views/workspace/PlanTracker.tsx src/views/workspace/PlanTracker.test.tsx
git add src/views/workspace/PlanTracker.tsx src/views/workspace/PlanTracker.test.tsx
git commit -m "refactor(workspace): plan tracker as a docked collapsible strip

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: Relocate the mount in Workspace + gates

**Files:**

- Modify: `src/views/workspace/Workspace.tsx` (~lines 497–527)
- Test: `src/views/workspace/Workspace.test.tsx`

**Interfaces:**

- Consumes: Task 1's PlanTracker (block strip, no absolute positioning).
- Produces: DOM order transcript-scroller → plan-tracker → agent-thinking → composer-shell.

- [ ] **Step 1: Update tests**

In `Workspace.test.tsx`, find the PlanTracker placement test (~line 2016,
asserts `tracker.parentElement === screen.getByTestId("workspace-scroll-container").parentElement`)
and rewrite it (also fixing its stale StickToBottom comment):

```tsx
it("docks the plan tracker between the transcript and the composer", async () => {
  vi.mocked(commands.getActivePlan).mockResolvedValue({
    goal: "Ship it",
    currentStepIndex: 0,
    steps: [{ description: "Step one", done: false }],
  });
  vi.mocked(commands.listMessages).mockResolvedValue([]);
  render(<Workspace conversationId="conv-1" />);

  const tracker = await screen.findByTestId("plan-tracker");
  const scroller = screen.getByTestId("workspace-scroll-container");
  const composer = screen.getByTestId("workspace-composer-shell");
  // Not inside the scroller any more…
  expect(scroller.contains(tracker)).toBe(false);
  // …and between it and the composer in document order.
  expect(scroller.compareDocumentPosition(tracker) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(tracker.compareDocumentPosition(composer) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
});
```

(Match the fixture shape to the existing `getActivePlan` mocks in the file.)

- [ ] **Step 2: Verify failure**

Run: `npx vitest run src/views/workspace/Workspace.test.tsx`
Expected: FAIL (tracker still inside the scroller).

- [ ] **Step 3: Move the mount**

In `Workspace.tsx`: remove `<PlanTracker conversationId={conversationId} />`
from inside `<MessageScroller>` (line ~525) and remove ` @container` from the
MessageScroller className (PlanTracker was its only consumer):

```tsx
        <MessageScroller className="h-auto min-h-0 flex-1">
```

Insert the tracker between the Provider block and StreamingStatus:

```tsx
      </MessageScrollerProvider>
      <PlanTracker conversationId={conversationId} />
      {showGenericStreamingStatus && <StreamingStatus startedAt={activeTurnStartedAt} />}
```

- [ ] **Step 4: Verify pass + full suites**

Run: `npx vitest run src/views/workspace/ && npx tsc -b`
Expected: PASS.

- [ ] **Step 5: Compliance + gates + runtime**

```bash
grep -n "\[[a-z-]*:" src/views/workspace/PlanTracker.tsx    # expected: EMPTY
npm run build && npm test && npm run lint                    # expected: green
```

Runtime: drive a planned turn in the real app (or eyeball during normal
use): strip appears above the input with the running step, expands upward,
disappears when the turn ends.

- [ ] **Step 6: Format, commit**

```bash
npx oxfmt src/views/workspace/Workspace.tsx src/views/workspace/Workspace.test.tsx
git add src/views/workspace/Workspace.tsx src/views/workspace/Workspace.test.tsx
git commit -m "refactor(workspace): dock the plan tracker above the composer

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```
