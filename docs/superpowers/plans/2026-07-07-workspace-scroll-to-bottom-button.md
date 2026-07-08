# Workspace Scroll To Bottom Button Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show a bottom-right floating arrow-down button when Workspace autoscroll detaches, and make it scroll to bottom while reactivating autoscroll.

**Architecture:** `Workspace` will keep the existing ref-based autoscroll source of truth and add React state only for rendering the detached affordance. A helper will update the ref and state together, while the button click will force an immediate bottom scroll and set autoscroll pinned again.

**Tech Stack:** React 19, TypeScript, Vitest + Testing Library, Phosphor icons, shared `Button` component, Tailwind CSS.

---

## File Structure

- Modify `src/views/workspace/Workspace.tsx`
  - Import `ArrowDownIcon` from `@phosphor-icons/react`.
  - Import `Button` from `@/components/ui/button`.
  - Add `isAutoscrollPinned` state for button rendering.
  - Add a helper that keeps `autoscrollPinnedRef.current` and state synchronized.
  - Add a click handler that pins and immediately scrolls to bottom.
  - Wrap the scroll container in a relative transcript area and render the floating button bottom-right.
- Modify `src/views/workspace/Workspace.test.tsx`
  - Add tests for button show/hide behavior.
  - Add tests for click-to-bottom and resumed following after click.
  - Reuse existing scroll metric and animation frame helpers.

Existing note: this repository currently has a named stash preserving unrelated Rust agent-planning edits. Do not pop or modify that stash while implementing this frontend task.

---

### Task 1: Floating Scroll-To-Bottom Button

**Files:**

- Modify: `src/views/workspace/Workspace.tsx`
- Modify: `src/views/workspace/Workspace.test.tsx`

- [ ] **Step 1: Add failing button tests**

In `src/views/workspace/Workspace.test.tsx`, add these tests after the existing test named:

```ts
it("does not autoscroll new messages after the user scrolls up", async () => {
```

and before the existing test named:

```ts
it("does not run a scheduled autoscroll after the user scrolls up before the animation frame", async () => {
```

Add:

```ts
it("shows the scroll-to-bottom button when autoscroll detaches and hides it near the bottom", async () => {
  vi.mocked(commands.listMessages).mockResolvedValueOnce([
    messageFixture("m1", "first message"),
  ]);

  render(<Workspace conversationId="conv-1" />);
  const scrollContainer = await screen.findByTestId("workspace-scroll-container");
  await screen.findByText("first message");

  expect(screen.queryByTestId("scroll-to-bottom")).not.toBeInTheDocument();

  setScrollMetrics(scrollContainer, { scrollHeight: 1000, clientHeight: 300, scrollTop: 200 });
  fireEvent.scroll(scrollContainer);

  expect(screen.getByTestId("scroll-to-bottom")).toBeInTheDocument();

  setScrollMetrics(scrollContainer, { scrollHeight: 1000, clientHeight: 300, scrollTop: 680 });
  fireEvent.scroll(scrollContainer);

  expect(screen.queryByTestId("scroll-to-bottom")).not.toBeInTheDocument();
});

it("scrolls to bottom and hides the scroll-to-bottom button when clicked", async () => {
  vi.mocked(commands.listMessages).mockResolvedValueOnce([
    messageFixture("m1", "first message"),
  ]);

  render(<Workspace conversationId="conv-1" />);
  const scrollContainer = await screen.findByTestId("workspace-scroll-container");
  await screen.findByText("first message");

  setScrollMetrics(scrollContainer, { scrollHeight: 1000, clientHeight: 300, scrollTop: 200 });
  fireEvent.scroll(scrollContainer);

  await userEvent.click(screen.getByTestId("scroll-to-bottom"));

  expect(scrollContainer.scrollTop).toBe(700);
  expect(screen.queryByTestId("scroll-to-bottom")).not.toBeInTheDocument();
});

it("keeps following new messages after the scroll-to-bottom button is clicked", async () => {
  let firePersisted!: (p: { conversationId: string }) => void;
  vi.mocked(events.onAgentMessagePersisted).mockImplementation(async (cb) => {
    firePersisted = cb;
    return () => {};
  });
  vi.mocked(commands.listMessages)
    .mockResolvedValueOnce([messageFixture("m1", "first message")])
    .mockResolvedValueOnce([
      messageFixture("m1", "first message"),
      messageFixture("m2", "second message", 2),
    ]);

  render(<Workspace conversationId="conv-1" />);
  const scrollContainer = await screen.findByTestId("workspace-scroll-container");
  await screen.findByText("first message");

  setScrollMetrics(scrollContainer, { scrollHeight: 1000, clientHeight: 300, scrollTop: 200 });
  fireEvent.scroll(scrollContainer);
  await userEvent.click(screen.getByTestId("scroll-to-bottom"));

  setScrollMetrics(scrollContainer, { scrollHeight: 1400, clientHeight: 300, scrollTop: 700 });
  firePersisted({ conversationId: "conv-1" });

  await screen.findByText("second message");
  await waitFor(() => expect(scrollContainer.scrollTop).toBe(1100));
});

it("hides the scroll-to-bottom button when switching conversations", async () => {
  vi.mocked(commands.listMessages)
    .mockResolvedValueOnce([messageFixture("m1", "first workspace")])
    .mockResolvedValueOnce([
      {
        ...messageFixture("m2", "second workspace"),
        conversationId: "conv-2",
      },
    ]);

  const { rerender } = render(<Workspace conversationId="conv-1" />);
  const scrollContainer = await screen.findByTestId("workspace-scroll-container");
  await screen.findByText("first workspace");

  setScrollMetrics(scrollContainer, { scrollHeight: 1000, clientHeight: 300, scrollTop: 200 });
  fireEvent.scroll(scrollContainer);
  expect(screen.getByTestId("scroll-to-bottom")).toBeInTheDocument();

  rerender(<Workspace conversationId="conv-2" />);

  expect(screen.queryByTestId("scroll-to-bottom")).not.toBeInTheDocument();
  await screen.findByText("second workspace");
});
```

- [ ] **Step 2: Run Workspace tests to verify they fail**

Run:

```bash
npm test -- src/views/workspace/Workspace.test.tsx
```

Expected: FAIL because `scroll-to-bottom` is not rendered yet.

- [ ] **Step 3: Add imports in `Workspace.tsx`**

In `src/views/workspace/Workspace.tsx`, add these imports near the top:

```ts
import { ArrowDownIcon } from "@phosphor-icons/react";
import { Button } from "@/components/ui/button";
```

The import block should start like:

```ts
import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { ArrowDownIcon } from "@phosphor-icons/react";
import MessageContent from "@/components/MessageContent";
import ContextUsageGauge from "@/components/ContextUsageGauge";
import { Button } from "@/components/ui/button";
```

- [ ] **Step 4: Add synchronized pinned state**

In `src/views/workspace/Workspace.tsx`, after:

```ts
const [error, setError] = useState<string | null>(null);
const scrollContainerRef = useRef<HTMLDivElement | null>(null);
const autoscrollPinnedRef = useRef(true);
```

replace that block with:

```ts
const [error, setError] = useState<string | null>(null);
const [isAutoscrollPinned, setIsAutoscrollPinned] = useState(true);
const scrollContainerRef = useRef<HTMLDivElement | null>(null);
const autoscrollPinnedRef = useRef(true);
```

After:

```ts
const showThinking = thinking || sendInFlight;
```

add:

```ts
const setAutoscrollPinned = useCallback((pinned: boolean) => {
  autoscrollPinnedRef.current = pinned;
  setIsAutoscrollPinned(pinned);
}, []);
```

- [ ] **Step 5: Update scroll callbacks**

In `src/views/workspace/Workspace.tsx`, replace the existing scroll callback block:

```ts
const scrollToTranscriptBottom = useCallback(() => {
  const element = scrollContainerRef.current;
  if (!element) return;
  if (!autoscrollPinnedRef.current) return;
  scrollElementToBottom(element);
}, []);

const scheduleScrollToTranscriptBottom = useCallback(() => {
  const frame = window.requestAnimationFrame(scrollToTranscriptBottom);
  return () => window.cancelAnimationFrame(frame);
}, [scrollToTranscriptBottom]);

const updateAutoscrollPinned = useCallback(() => {
  const element = scrollContainerRef.current;
  if (!element) return;
  autoscrollPinnedRef.current = isNearScrollBottom(element);
}, []);
```

with:

```ts
const scrollToTranscriptBottom = useCallback((force = false) => {
  const element = scrollContainerRef.current;
  if (!element) return;
  if (!force && !autoscrollPinnedRef.current) return;
  scrollElementToBottom(element);
}, []);

const scheduleScrollToTranscriptBottom = useCallback(() => {
  const frame = window.requestAnimationFrame(() => {
    scrollToTranscriptBottom();
  });
  return () => window.cancelAnimationFrame(frame);
}, [scrollToTranscriptBottom]);

const updateAutoscrollPinned = useCallback(() => {
  const element = scrollContainerRef.current;
  if (!element) return;
  setAutoscrollPinned(isNearScrollBottom(element));
}, [setAutoscrollPinned]);

const scrollToBottomAndPin = useCallback(() => {
  setAutoscrollPinned(true);
  scrollToTranscriptBottom(true);
}, [scrollToTranscriptBottom, setAutoscrollPinned]);
```

- [ ] **Step 6: Reset visible pinned state on conversation switch**

In `src/views/workspace/Workspace.tsx`, replace:

```ts
useEffect(() => {
  autoscrollPinnedRef.current = true;
  return scheduleScrollToTranscriptBottom();
}, [conversationId, scheduleScrollToTranscriptBottom]);
```

with:

```ts
useEffect(() => {
  setAutoscrollPinned(true);
  return scheduleScrollToTranscriptBottom();
}, [conversationId, scheduleScrollToTranscriptBottom, setAutoscrollPinned]);
```

- [ ] **Step 7: Render the bottom-right floating button**

In `src/views/workspace/Workspace.tsx`, replace the current transcript container render:

```tsx
<div
  ref={scrollContainerRef}
  className="flex-1 overflow-y-auto p-4"
  data-testid="workspace-scroll-container"
  onScroll={updateAutoscrollPinned}
>
  <div className="mx-auto max-w-3xl">
```

with:

```tsx
<div className="relative min-h-0 flex-1">
  <div
    ref={scrollContainerRef}
    className="h-full overflow-y-auto p-4"
    data-testid="workspace-scroll-container"
    onScroll={updateAutoscrollPinned}
  >
    <div className="mx-auto max-w-3xl">
```

Then, after the closing `</div>` for the scroll container and before the composer shell starts, add:

```tsx
  {!isAutoscrollPinned && (
    <Button
      type="button"
      variant="secondary"
      size="icon"
      className="absolute bottom-4 right-4 z-10 rounded-full bg-card/95 shadow-sm backdrop-blur supports-[backdrop-filter]:bg-card/80"
      onClick={scrollToBottomAndPin}
      aria-label="Scroll to bottom"
      data-testid="scroll-to-bottom"
    >
      <ArrowDownIcon size={16} />
    </Button>
  )}
</div>
```

After this change, the render structure should be:

```tsx
return (
  <div className="flex h-dvh flex-col bg-background text-foreground">
    <div className="relative min-h-0 flex-1">
      <div
        ref={scrollContainerRef}
        className="h-full overflow-y-auto p-4"
        data-testid="workspace-scroll-container"
        onScroll={updateAutoscrollPinned}
      >
        <div className="mx-auto max-w-3xl">
          {/* existing message/thinking/error content */}
        </div>
      </div>
      {!isAutoscrollPinned && (
        <Button
          type="button"
          variant="secondary"
          size="icon"
          className="absolute bottom-4 right-4 z-10 rounded-full bg-card/95 shadow-sm backdrop-blur supports-[backdrop-filter]:bg-card/80"
          onClick={scrollToBottomAndPin}
          aria-label="Scroll to bottom"
          data-testid="scroll-to-bottom"
        >
          <ArrowDownIcon size={16} />
        </Button>
      )}
    </div>
    <div
      className="border-t border-border p-4 [view-transition-name:chat-composer]"
      data-testid="workspace-composer-shell"
    >
      {/* existing RichInput */}
    </div>
  </div>
);
```

- [ ] **Step 8: Run Workspace tests to verify they pass**

Run:

```bash
npm test -- src/views/workspace/Workspace.test.tsx
```

Expected: PASS.

- [ ] **Step 9: Run focused formatting check**

Run:

```bash
./node_modules/.bin/oxfmt --check src/views/workspace/Workspace.tsx src/views/workspace/Workspace.test.tsx
```

Expected: PASS.

If formatting fails, run:

```bash
./node_modules/.bin/oxfmt src/views/workspace/Workspace.tsx src/views/workspace/Workspace.test.tsx
npm test -- src/views/workspace/Workspace.test.tsx
```

Expected after formatting: tests PASS.

- [ ] **Step 10: Commit**

Before staging, inspect:

```bash
git diff -- src/views/workspace/Workspace.tsx src/views/workspace/Workspace.test.tsx
```

Then commit:

```bash
git add src/views/workspace/Workspace.tsx src/views/workspace/Workspace.test.tsx
git commit -m "feat: add workspace scroll to bottom button"
```

Expected: commit contains only the Workspace scroll-to-bottom button code and tests.

---

### Task 2: Final Verification

**Files:**

- Verify only; do not edit files unless a command exposes a concrete failure.

- [ ] **Step 1: Run focused tests**

Run:

```bash
npm test -- src/views/workspace/Workspace.test.tsx
```

Expected: PASS.

- [ ] **Step 2: Run full frontend tests**

Run:

```bash
npm test
```

Expected: PASS.

- [ ] **Step 3: Run format, lint, and build**

Run:

```bash
./node_modules/.bin/oxfmt --check src/views/workspace/Workspace.tsx src/views/workspace/Workspace.test.tsx
npm run lint
npm run build
```

Expected: all commands exit 0. `npm run build` may print the existing Vite chunk-size warning.

- [ ] **Step 4: Inspect status**

Run:

```bash
git status --short
```

Expected: no unstaged feature-owned files. A named stash for unrelated Rust agent-planning edits may still exist; do not pop it.

