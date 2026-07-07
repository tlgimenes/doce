# Empty State View Transition Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Switch from the empty-state composer into the workspace chat immediately after conversation creation, with a same-document View Transition API animation and a non-animated fallback.

**Architecture:** Move the first agent send out of `EmptyState` and into `Workspace` via a small pending initial-turn object owned by `App`. Wrap the `App` route-state update in a feature-detected `document.startViewTransition()` helper that uses React `flushSync`, then style only the main content pane and composer as named transition targets. Keep backend commands unchanged.

**Tech Stack:** React 19, TypeScript, Vitest, Testing Library, Tauri IPC wrappers, Tiptap `RichInput`, CSS View Transition API, Tailwind CSS v4 arbitrary properties.

---

## File Structure

- Create `src/lib/viewTransition.ts`
  - Owns the feature-detected `document.startViewTransition()` wrapper.
  - Uses `flushSync` so React commits the route swap during the transition callback.
- Create `src/lib/viewTransition.test.ts`
  - Unit tests for API-supported, unsupported, and throwing transition paths.
- Create `src/views/workspace/pendingInitialTurn.ts`
  - Shared type for the initial turn passed from `EmptyState` through `App` to `Workspace`.
- Modify `src/views/chat/EmptyState.tsx`
  - Stop calling `sendAgentMessage`.
  - Emit the created conversation plus pending initial turn after `createConversation`.
  - Add a composer wrapper test id and transition-name class.
- Modify `src/views/chat/EmptyState.test.tsx`
  - Assert the empty state does not wait for or call `sendAgentMessage`.
  - Assert rich content is forwarded through the pending initial turn.
- Modify `src/views/workspace/Workspace.tsx`
  - Accept and consume a pending initial turn once.
  - Reuse existing `send` behavior for the first turn.
  - Preserve optimistic user message and `Working...` state.
  - Add transition-name class/test id to the workspace composer shell.
- Modify `src/views/workspace/Workspace.test.tsx`
  - Assert pending first turns send once, show optimistic UI, preserve rich content, and surface errors.
- Modify `src/App.tsx`
  - Store pending initial turn state.
  - Wrap the empty-state route swap in `runViewTransition`.
  - Pass pending turn to `Workspace` and clear it after consumption.
  - Add transition-name class/test id to the content pane.
- Modify `src/App.test.tsx`
  - Assert route switch happens before `sendAgentMessage` resolves.
  - Assert `document.startViewTransition` is used when available.
- Modify `src/styles/theme.css`
  - Add named view-transition animation styles and reduced-motion pseudo-element handling.

Existing dirty worktree note: this repo currently has unrelated uncommitted changes in several of these files. During execution, inspect `git diff` before each commit. If a task touches a file with pre-existing unrelated hunks, do not commit that file blindly; report the commit as skipped for controller integration.

---

### Task 1: View Transition Helper

**Files:**

- Create: `src/lib/viewTransition.ts`
- Create: `src/lib/viewTransition.test.ts`

- [ ] **Step 1: Write the failing tests**

Create `src/lib/viewTransition.test.ts`:

```ts
import { afterEach, describe, expect, it, vi } from "vitest";
import { flushSync } from "react-dom";
import { runViewTransition } from "./viewTransition";

vi.mock("react-dom", () => ({
  flushSync: vi.fn((callback: () => void) => callback()),
}));

type TestDocument = Document & {
  startViewTransition?: (callback: () => void) => unknown;
};

const originalStartViewTransition = (document as TestDocument).startViewTransition;

afterEach(() => {
  Object.defineProperty(document, "startViewTransition", {
    configurable: true,
    value: originalStartViewTransition,
  });
  vi.clearAllMocks();
});

describe("runViewTransition", () => {
  it("updates immediately when the View Transition API is unavailable", () => {
    Object.defineProperty(document, "startViewTransition", {
      configurable: true,
      value: undefined,
    });
    const update = vi.fn();

    runViewTransition(update);

    expect(update).toHaveBeenCalledTimes(1);
    expect(flushSync).not.toHaveBeenCalled();
  });

  it("uses startViewTransition and flushSync when supported", () => {
    const update = vi.fn();
    const startViewTransition = vi.fn((callback: () => void) => {
      callback();
      return {};
    });
    Object.defineProperty(document, "startViewTransition", {
      configurable: true,
      value: startViewTransition,
    });

    runViewTransition(update);

    expect(startViewTransition).toHaveBeenCalledTimes(1);
    expect(flushSync).toHaveBeenCalledWith(update);
    expect(update).toHaveBeenCalledTimes(1);
  });

  it("falls back to one immediate update if starting the transition throws before update", () => {
    const update = vi.fn();
    Object.defineProperty(document, "startViewTransition", {
      configurable: true,
      value: vi.fn(() => {
        throw new Error("transition failed");
      }),
    });

    runViewTransition(update);

    expect(update).toHaveBeenCalledTimes(1);
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run:

```bash
npm test -- src/lib/viewTransition.test.ts
```

Expected: FAIL because `src/lib/viewTransition.ts` does not exist.

- [ ] **Step 3: Implement the helper**

Create `src/lib/viewTransition.ts`:

```ts
import { flushSync } from "react-dom";

type ViewTransitionDocument = Document & {
  startViewTransition?: (callback: () => void) => unknown;
};

export function runViewTransition(update: () => void) {
  const startViewTransition = (document as ViewTransitionDocument).startViewTransition;
  if (!startViewTransition) {
    update();
    return;
  }

  let didUpdate = false;
  try {
    startViewTransition.call(document, () => {
      didUpdate = true;
      flushSync(update);
    });
  } catch {
    if (!didUpdate) update();
  }
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run:

```bash
npm test -- src/lib/viewTransition.test.ts
```

Expected: PASS.

- [ ] **Step 5: Commit**

If `src/lib/viewTransition.ts` and `src/lib/viewTransition.test.ts` are the only staged changes:

```bash
git add src/lib/viewTransition.ts src/lib/viewTransition.test.ts
git commit -m "feat: add view transition helper"
```

Expected: commit contains only the helper and its tests.

---

### Task 2: Empty State Fast Handoff

**Files:**

- Create: `src/views/workspace/pendingInitialTurn.ts`
- Modify: `src/views/chat/EmptyState.tsx`
- Modify: `src/views/chat/EmptyState.test.tsx`

- [ ] **Step 1: Write failing EmptyState tests**

Update the main submit test in `src/views/chat/EmptyState.test.tsx` so `sendAgentMessage` never resolves and the test still expects `onConversationCreated`:

```ts
it("US1: submitting with the Home target untouched creates a workspace-scoped conversation and hands off the first turn without waiting for the agent", async () => {
  vi.mocked(commands.openWorkspace).mockResolvedValue({
    id: "ws-home",
    path: "/Users/tester",
    displayName: "tester",
    createdAt: 1,
    lastOpenedAt: 1,
  });
  vi.mocked(commands.createConversation).mockResolvedValue({
    id: "conv-1",
    workspaceId: "ws-home",
    title: "New conversation",
    createdAt: 1,
    updatedAt: 1,
    status: "done",
  });
  vi.mocked(commands.sendAgentMessage).mockReturnValue(new Promise(() => {}));
  const onConversationCreated = vi.fn();

  render(<EmptyState onConversationCreated={onConversationCreated} />);
  await waitFor(() =>
    expect(screen.getByTestId("folder-target-selector")).toHaveTextContent("Home"),
  );

  await userEvent.type(screen.getByTestId("empty-state-input"), "fix the login bug");
  await userEvent.click(screen.getByTestId("empty-state-submit"));

  await waitFor(() =>
    expect(onConversationCreated).toHaveBeenCalledWith(
      expect.objectContaining({ id: "conv-1", workspaceId: "ws-home" }),
      expect.objectContaining({
        conversationId: "conv-1",
        content: "fix the login bug",
        richContent: undefined,
      }),
    ),
  );

  expect(commands.openWorkspace).toHaveBeenCalledWith("/Users/tester");
  expect(commands.createConversation).toHaveBeenCalledWith("ws-home");
  expect(commands.sendAgentMessage).not.toHaveBeenCalled();

  const openOrder = vi.mocked(commands.openWorkspace).mock.invocationCallOrder[0];
  const createOrder = vi.mocked(commands.createConversation).mock.invocationCallOrder[0];
  const handoffOrder = onConversationCreated.mock.invocationCallOrder[0];
  expect(openOrder).toBeLessThan(createOrder);
  expect(createOrder).toBeLessThan(handoffOrder);
});
```

Update the rich-content regression test in the same file so it inspects the pending turn instead of `sendAgentMessage`:

```ts
it("009-rich-chat-input regression: a message containing a chip forwards richContent through the pending initial turn", async () => {
  vi.mocked(commands.openWorkspace).mockResolvedValue({
    id: "ws-home",
    path: "/Users/tester",
    displayName: "tester",
    createdAt: 1,
    lastOpenedAt: 1,
  });
  vi.mocked(commands.createConversation).mockResolvedValue({
    id: "conv-1",
    workspaceId: "ws-home",
    title: "New conversation",
    createdAt: 1,
    updatedAt: 1,
    status: "done",
  });
  const onConversationCreated = vi.fn();

  render(<EmptyState onConversationCreated={onConversationCreated} />);
  await waitFor(() =>
    expect(screen.getByTestId("folder-target-selector")).toHaveTextContent("Home"),
  );

  const input = screen.getByTestId("empty-state-input");
  const pastedBlock = Array.from({ length: 15 }, (_, i) => `line-${i}`).join("\n");
  fireEvent.paste(input, { clipboardData: { items: [], getData: () => pastedBlock } });
  await screen.findByTestId("pasted-text-chip");

  await userEvent.click(screen.getByTestId("empty-state-submit"));

  await waitFor(() => expect(onConversationCreated).toHaveBeenCalled());
  const [, pendingTurn] = onConversationCreated.mock.calls[0];
  expect(pendingTurn.richContent).toBeDefined();
  expect(
    pendingTurn.richContent.segments.some(
      (s: { type: string; text?: string }) => s.type === "pastedText" && s.text === pastedBlock,
    ),
  ).toBe(true);
  expect(commands.sendAgentMessage).not.toHaveBeenCalled();
});
```

Add this small structure test:

```ts
it("marks the empty-state composer as the chat composer view-transition target", async () => {
  render(<EmptyState onConversationCreated={vi.fn()} />);

  expect(await screen.findByTestId("empty-state-composer")).toHaveClass(
    "[view-transition-name:chat-composer]",
  );
});
```

- [ ] **Step 2: Run the EmptyState tests to verify they fail**

Run:

```bash
npm test -- src/views/chat/EmptyState.test.tsx
```

Expected: FAIL because `EmptyState` still calls and waits for `sendAgentMessage`, and it has no `empty-state-composer` test id.

- [ ] **Step 3: Create the shared pending turn type**

Create `src/views/workspace/pendingInitialTurn.ts`:

```ts
import type { RichMessageContent } from "@/lib/ipc";

export interface PendingInitialTurn {
  conversationId: string;
  content: string;
  richContent?: RichMessageContent;
}
```

- [ ] **Step 4: Update `EmptyState` implementation**

In `src/views/chat/EmptyState.tsx`, import the type:

```ts
import type { PendingInitialTurn } from "@/views/workspace/pendingInitialTurn";
```

Replace the props interface with:

```ts
interface EmptyStateProps {
  // Reports the full Conversation (not just its id) so App.tsx can route by
  // its workspaceId without a second lookup. The pending initial turn is
  // handed to Workspace so the view can switch before the full agent loop
  // resolves.
  onConversationCreated: (
    conversation: Conversation,
    pendingInitialTurn: PendingInitialTurn,
  ) => void;
}
```

Replace the `try` block inside `submit` with:

```ts
try {
  const workspace = await commands.openWorkspace(target.path);
  const conversation = await commands.createConversation(workspace.id);
  onConversationCreated(conversation, {
    conversationId: conversation.id,
    content,
    richContent,
  });
} catch (e) {
  setError(String(e));
} finally {
  setSubmitting(false);
}
```

Replace the composer wrapper `<div className="relative w-full max-w-xl space-y-3">` with:

```tsx
<div
  className="relative w-full max-w-xl space-y-3 [view-transition-name:chat-composer]"
  data-testid="empty-state-composer"
>
```

- [ ] **Step 5: Run the EmptyState tests to verify they pass**

Run:

```bash
npm test -- src/views/chat/EmptyState.test.tsx
```

Expected: PASS.

- [ ] **Step 6: Commit**

If these files have no unrelated staged hunks:

```bash
git add src/views/workspace/pendingInitialTurn.ts src/views/chat/EmptyState.tsx src/views/chat/EmptyState.test.tsx
git commit -m "feat: hand off empty state first turn"
```

Expected: commit contains only the pending-turn type and EmptyState changes.

---

### Task 3: Workspace Pending Initial Turn

**Files:**

- Modify: `src/views/workspace/Workspace.tsx`
- Modify: `src/views/workspace/Workspace.test.tsx`

- [ ] **Step 1: Write failing Workspace tests**

Add these tests to `src/views/workspace/Workspace.test.tsx`:

```ts
it("consumes a pending initial turn once, renders it optimistically, and shows Working", async () => {
  vi.mocked(commands.listMessages).mockResolvedValue([]);
  vi.mocked(commands.sendAgentMessage).mockReturnValue(new Promise(() => {}));
  const onConsumed = vi.fn();
  const pendingInitialTurn = {
    conversationId: "conv-1",
    content: "first task",
    richContent: undefined,
  };

  const { rerender } = render(
    <Workspace
      conversationId="conv-1"
      pendingInitialTurn={pendingInitialTurn}
      onPendingInitialTurnConsumed={onConsumed}
    />,
  );

  await waitFor(() =>
    expect(commands.sendAgentMessage).toHaveBeenCalledWith("conv-1", "first task", undefined),
  );
  expect(screen.getByText("first task")).toBeInTheDocument();
  expect(screen.getByTestId("agent-thinking")).toBeInTheDocument();
  expect(onConsumed).toHaveBeenCalledWith("conv-1");

  rerender(
    <Workspace
      conversationId="conv-1"
      pendingInitialTurn={pendingInitialTurn}
      onPendingInitialTurnConsumed={onConsumed}
    />,
  );

  expect(commands.sendAgentMessage).toHaveBeenCalledTimes(1);
});

it("forwards rich content from a pending initial turn to sendAgentMessage", async () => {
  vi.mocked(commands.listMessages).mockResolvedValue([]);
  vi.mocked(commands.sendAgentMessage).mockReturnValue(new Promise(() => {}));
  const richContent = {
    segments: [{ type: "pastedText" as const, text: "large paste" }],
  };

  render(
    <Workspace
      conversationId="conv-1"
      pendingInitialTurn={{
        conversationId: "conv-1",
        content: "large paste",
        richContent,
      }}
    />,
  );

  await waitFor(() =>
    expect(commands.sendAgentMessage).toHaveBeenCalledWith(
      "conv-1",
      "large paste",
      JSON.stringify(richContent),
    ),
  );
});

it("shows Workspace errors when the pending initial turn send fails after navigation", async () => {
  vi.mocked(commands.listMessages).mockResolvedValue([]);
  vi.mocked(commands.sendAgentMessage).mockRejectedValue(new Error("inference crashed"));

  render(
    <Workspace
      conversationId="conv-1"
      pendingInitialTurn={{
        conversationId: "conv-1",
        content: "first task",
      }}
    />,
  );

  await waitFor(() =>
    expect(screen.getByTestId("workspace-error")).toHaveTextContent("inference crashed"),
  );
});

it("marks the workspace composer as the chat composer view-transition target", async () => {
  vi.mocked(commands.listMessages).mockResolvedValue([]);

  render(<Workspace conversationId="conv-1" />);

  expect(await screen.findByTestId("workspace-composer-shell")).toHaveClass(
    "[view-transition-name:chat-composer]",
  );
});
```

- [ ] **Step 2: Run Workspace tests to verify they fail**

Run:

```bash
npm test -- src/views/workspace/Workspace.test.tsx
```

Expected: FAIL because `Workspace` does not accept pending-turn props and has no `workspace-composer-shell` test id.

- [ ] **Step 3: Update Workspace props and imports**

In `src/views/workspace/Workspace.tsx`, replace the React import with:

```ts
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
```

Add:

```ts
import type { PendingInitialTurn } from "@/views/workspace/pendingInitialTurn";
```

Replace the props interface with:

```ts
interface WorkspaceProps {
  conversationId: string;
  pendingInitialTurn?: PendingInitialTurn | null;
  onPendingInitialTurnConsumed?: (conversationId: string) => void;
}
```

Replace the component signature with:

```ts
export default function Workspace({
  conversationId,
  pendingInitialTurn = null,
  onPendingInitialTurnConsumed,
}: WorkspaceProps) {
```

Add refs after state declarations:

```ts
const pendingInitialTurnRef = useRef<PendingInitialTurn | null>(pendingInitialTurn);
pendingInitialTurnRef.current = pendingInitialTurn;
const consumedInitialTurnRef = useRef<string | null>(null);
```

- [ ] **Step 4: Keep the initial empty `listMessages` load from wiping the optimistic first turn**

Replace the initial `listMessages` effect with:

```ts
useEffect(() => {
  const skipEmptyInitialLoad = pendingInitialTurnRef.current?.conversationId === conversationId;
  setMessages([]);
  setError(null);
  commands.listMessages(conversationId).then((loaded) => {
    if (skipEmptyInitialLoad && loaded.length === 0) return;
    setMessages(loaded);
  });
}, [conversationId]);
```

- [ ] **Step 5: Make `send` reusable from an effect**

Replace:

```ts
const send = async (content: string, richContent?: RichMessageContent) => {
```

with:

```ts
const send = useCallback(async (content: string, richContent?: RichMessageContent) => {
```

Replace the closing `};` of the `send` function with:

```ts
}, [conversationId, pendingQuestion, thinking]);
```

- [ ] **Step 6: Consume the pending initial turn once**

Add this effect after `send`:

```ts
useEffect(() => {
  if (!pendingInitialTurn || pendingInitialTurn.conversationId !== conversationId) return;
  if (consumedInitialTurnRef.current === pendingInitialTurn.conversationId) return;

  consumedInitialTurnRef.current = pendingInitialTurn.conversationId;
  void send(pendingInitialTurn.content, pendingInitialTurn.richContent);
  onPendingInitialTurnConsumed?.(pendingInitialTurn.conversationId);
}, [conversationId, onPendingInitialTurnConsumed, pendingInitialTurn, send]);
```

- [ ] **Step 7: Mark the workspace composer shell**

Replace:

```tsx
<div className="border-t border-border p-4">
```

with:

```tsx
<div
  className="border-t border-border p-4 [view-transition-name:chat-composer]"
  data-testid="workspace-composer-shell"
>
```

- [ ] **Step 8: Run Workspace tests to verify they pass**

Run:

```bash
npm test -- src/views/workspace/Workspace.test.tsx
```

Expected: PASS.

- [ ] **Step 9: Commit**

If these files have no unrelated staged hunks:

```bash
git add src/views/workspace/Workspace.tsx src/views/workspace/Workspace.test.tsx
git commit -m "feat: consume pending initial workspace turn"
```

Expected: commit contains only Workspace pending-turn changes.

---

### Task 4: App View Transition Handoff

**Files:**

- Modify: `src/App.tsx`
- Modify: `src/App.test.tsx`

- [ ] **Step 1: Write failing App tests**

Add this type and helpers near the top of `src/App.test.tsx`:

```ts
type TestDocument = Document & {
  startViewTransition?: (callback: () => void) => unknown;
};

const originalStartViewTransition = (document as TestDocument).startViewTransition;

function setStartViewTransition(startViewTransition: TestDocument["startViewTransition"]) {
  Object.defineProperty(document, "startViewTransition", {
    configurable: true,
    value: startViewTransition,
  });
}
```

Update `afterEach` in the file, or add it if not present:

```ts
afterEach(() => {
  setStartViewTransition(originalStartViewTransition);
});
```

Add this test inside the main App keyboard shortcuts describe block:

```ts
it("routes into Workspace immediately after conversation creation without waiting for the first agent reply", async () => {
  vi.mocked(commands.sendAgentMessage).mockReturnValue(new Promise(() => {}));

  render(<App />);
  await waitForReady();

  await userEvent.type(await screen.findByTestId("empty-state-input"), "first task");
  await userEvent.click(screen.getByTestId("empty-state-submit"));

  await screen.findByTestId("agent-input");
  expect(screen.getByText("first task")).toBeInTheDocument();
  expect(screen.getByTestId("agent-thinking")).toBeInTheDocument();
  expect(commands.sendAgentMessage).toHaveBeenCalledWith("new-conv", "first task", undefined);
});
```

Add this test in the same describe block:

```ts
it("uses a same-document view transition for the empty-state to workspace route swap when supported", async () => {
  const startViewTransition = vi.fn((callback: () => void) => {
    callback();
    return {};
  });
  setStartViewTransition(startViewTransition);

  render(<App />);
  await waitForReady();

  await userEvent.type(await screen.findByTestId("empty-state-input"), "first task");
  await userEvent.click(screen.getByTestId("empty-state-submit"));

  await screen.findByTestId("agent-input");
  expect(startViewTransition).toHaveBeenCalledTimes(1);
});
```

Add this structure assertion to an existing ready-state test or as a new test:

```ts
it("marks only the main content pane as the chat surface transition target", async () => {
  render(<App />);
  await waitForReady();

  expect(screen.getByTestId("app-content-pane")).toHaveClass(
    "[view-transition-name:chat-surface]",
  );
});
```

- [ ] **Step 2: Run App tests to verify they fail**

Run:

```bash
npm test -- src/App.test.tsx
```

Expected: FAIL because `App` does not store pending turns, does not call `runViewTransition`, and does not mark the content pane.

- [ ] **Step 3: Update App imports and state**

In `src/App.tsx`, add:

```ts
import { runViewTransition } from "@/lib/viewTransition";
import type { PendingInitialTurn } from "@/views/workspace/pendingInitialTurn";
```

Add state after `activeConversation`:

```ts
const [pendingInitialTurn, setPendingInitialTurn] = useState<PendingInitialTurn | null>(null);
```

Add this helper before the `return`:

```ts
const activateConversation = (conversation: Conversation, initialTurn?: PendingInitialTurn) => {
  runViewTransition(() => {
    setShowSettings(false);
    setPendingInitialTurn(initialTurn ?? null);
    setActiveConversation(conversation);
  });
};
```

- [ ] **Step 4: Route selection and new-conversation paths through pending-turn cleanup**

Replace `onSelect` with:

```tsx
onSelect={(conversation) => {
  setShowSettings(false);
  setPendingInitialTurn(null);
  setActiveConversation(conversation);
}}
```

Replace `onNewConversation` with:

```tsx
onNewConversation={() => {
  setShowSettings(false);
  setPendingInitialTurn(null);
  setActiveConversation(null);
}}
```

- [ ] **Step 5: Pass pending turn into Workspace and wire EmptyState handoff**

Replace the content pane opening tag:

```tsx
<div className="flex-1">
```

with:

```tsx
<div className="flex-1 [view-transition-name:chat-surface]" data-testid="app-content-pane">
```

Replace the Workspace render with:

```tsx
<Workspace
  key={activeConversation.id}
  conversationId={activeConversation.id}
  pendingInitialTurn={
    pendingInitialTurn?.conversationId === activeConversation.id ? pendingInitialTurn : null
  }
  onPendingInitialTurnConsumed={(conversationId) => {
    setPendingInitialTurn((prev) => (prev?.conversationId === conversationId ? null : prev));
  }}
/>
```

Replace the EmptyState render with:

```tsx
<EmptyState onConversationCreated={activateConversation} />
```

- [ ] **Step 6: Run App tests to verify they pass**

Run:

```bash
npm test -- src/App.test.tsx
```

Expected: PASS.

- [ ] **Step 7: Commit**

If these files have no unrelated staged hunks:

```bash
git add src/App.tsx src/App.test.tsx
git commit -m "feat: transition from empty state into workspace"
```

Expected: commit contains only App handoff and view-transition integration changes.

---

### Task 5: View Transition Styling

**Files:**

- Modify: `src/styles/theme.css`
- Test: `src/App.test.tsx`
- Test: `src/views/chat/EmptyState.test.tsx`
- Test: `src/views/workspace/Workspace.test.tsx`

- [ ] **Step 1: Confirm structural tests pass before CSS**

Run:

```bash
npm test -- src/App.test.tsx src/views/chat/EmptyState.test.tsx src/views/workspace/Workspace.test.tsx
```

Expected: PASS for the class/test-id assertions added in Tasks 2-4.

- [ ] **Step 2: Add view-transition CSS**

Append this block to `src/styles/theme.css` after the existing `:focus-visible` rule and before the existing reduced-motion block:

```css
/* Empty-state -> workspace route transition. The root itself does not animate,
   so the sidebar remains visually stable while the main chat surface changes. */
::view-transition-old(root),
::view-transition-new(root) {
  animation: none;
}

::view-transition-group(chat-surface) {
  animation-duration: 180ms;
  animation-timing-function: cubic-bezier(0.2, 0, 0, 1);
}

::view-transition-old(chat-surface) {
  animation: doce-chat-surface-out 140ms cubic-bezier(0.2, 0, 0, 1) both;
}

::view-transition-new(chat-surface) {
  animation: doce-chat-surface-in 220ms cubic-bezier(0.2, 0, 0, 1) both;
}

::view-transition-group(chat-composer) {
  animation-duration: 220ms;
  animation-timing-function: cubic-bezier(0.2, 0, 0, 1);
}

@keyframes doce-chat-surface-out {
  from {
    opacity: 1;
    transform: translateY(0);
  }
  to {
    opacity: 0;
    transform: translateY(-6px);
  }
}

@keyframes doce-chat-surface-in {
  from {
    opacity: 0;
    transform: translateY(8px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}
```

Extend the existing `@media (prefers-reduced-motion: reduce)` block by adding these selectors inside it:

```css
::view-transition-group(*),
::view-transition-old(*),
::view-transition-new(*) {
  animation-duration: 0.01ms !important;
  animation-iteration-count: 1 !important;
}
```

- [ ] **Step 3: Run a formatting check for touched files**

Run:

```bash
./node_modules/.bin/oxfmt --check src/styles/theme.css src/App.tsx src/views/chat/EmptyState.tsx src/views/workspace/Workspace.tsx
```

Expected: PASS.

- [ ] **Step 4: Commit**

If `src/styles/theme.css` has no unrelated staged hunks:

```bash
git add src/styles/theme.css src/App.test.tsx src/views/chat/EmptyState.test.tsx src/views/workspace/Workspace.test.tsx
git commit -m "style: animate empty state workspace transition"
```

Expected: commit contains the view-transition styles and any class assertion tests not already committed.

---

### Task 6: Focused Verification

**Files:**

- Test: `src/lib/viewTransition.test.ts`
- Test: `src/views/chat/EmptyState.test.tsx`
- Test: `src/views/workspace/Workspace.test.tsx`
- Test: `src/App.test.tsx`
- Test: `src/views/chat/ConversationList.test.tsx`
- Test: `src/views/chat/sidebarConversationRow.test.ts`

- [ ] **Step 1: Run focused tests**

Run:

```bash
npm test -- src/lib/viewTransition.test.ts src/views/chat/EmptyState.test.tsx src/views/workspace/Workspace.test.tsx src/App.test.tsx src/views/chat/ConversationList.test.tsx src/views/chat/sidebarConversationRow.test.ts
```

Expected: PASS.

- [ ] **Step 2: Run scoped format check**

Run:

```bash
./node_modules/.bin/oxfmt --check src/lib/viewTransition.ts src/lib/viewTransition.test.ts src/views/workspace/pendingInitialTurn.ts src/views/chat/EmptyState.tsx src/views/chat/EmptyState.test.tsx src/views/workspace/Workspace.tsx src/views/workspace/Workspace.test.tsx src/App.tsx src/App.test.tsx src/styles/theme.css
```

Expected: PASS.

- [ ] **Step 3: Run lint**

Run:

```bash
npm run lint
```

Expected: PASS.

- [ ] **Step 4: Run build**

Run:

```bash
npm run build
```

Expected: PASS. The existing large chunk warning can be reported if it appears.

- [ ] **Step 5: Check worktree status**

Run:

```bash
git status --short
```

Expected: only intended files from this feature plus pre-existing unrelated dirty files remain. Do not revert unrelated dirty files.

---

## Self-Review

Spec coverage:

- Immediate empty-state exit after `createConversation`: Task 2 and Task 4.
- First `sendAgentMessage` owned by `Workspace`: Task 3 and Task 4.
- Same-document View Transition API with fallback: Task 1 and Task 4.
- Composer/content transition targets and sidebar stability: Task 4 and Task 5.
- Existing backend/event protocol unchanged: no backend files are in scope.
- Error handling: Task 2 covers setup failures; Task 3 covers post-navigation send failures.
- Reduced motion: Task 5.
- Tests for EmptyState, App, Workspace, and transition helper: Tasks 1-4 and Task 6.

Placeholder scan:

- No placeholders, incomplete sections, or deferred implementation steps are present.
- Every implementation step includes concrete code or exact commands.

Type consistency:

- `PendingInitialTurn` is created once in `src/views/workspace/pendingInitialTurn.ts`.
- `EmptyState`, `App`, and `Workspace` all use that same type.
- `runViewTransition` is created in Task 1 before `App` consumes it in Task 4.
