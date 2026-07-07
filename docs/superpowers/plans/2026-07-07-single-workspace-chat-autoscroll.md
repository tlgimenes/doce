# Single Workspace Chat Autoscroll Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove the legacy plain chat frontend path and add pinned autoscroll to the single remaining workspace chat view.

**Architecture:** `App.tsx` will render `Workspace` for every selected conversation and focus only `empty-state-input` or `agent-input`. The legacy `Chat.tsx`/plain streaming frontend will be deleted, while backend IPC wrappers remain unless unused removal is mechanical and covered by existing checks. `Workspace` will own pinned autoscroll with a scroll container ref and a small near-bottom state machine.

**Tech Stack:** React 19, TypeScript, Vitest + Testing Library, WDIO e2e specs, Tauri IPC wrappers, Tailwind CSS.

---

## File Structure

- Modify `src/App.tsx`
  - Remove `Chat` import and render branch.
  - Remove `wireConversationStreamEvents` import/call once no frontend code needs streamed plain-chat tokens.
  - Simplify Cmd+L focus target to `empty-state-input` or `agent-input`.
- Modify `src/App.test.tsx`
  - Remove legacy `chat-input` assertions.
  - Prove selected conversations render `Workspace` even when fixture `workspaceId` is `null`.
  - Keep existing empty-state pending-turn and view-transition tests.
- Delete `src/views/chat/Chat.tsx`
  - Removes legacy plain chat UI, plain streamed placeholder, and cancel-generation UI.
- Delete `src/views/chat/Chat.test.tsx`
  - Its behavior no longer exists in frontend product scope.
- Delete `src/state/conversationStreamStore.ts`
  - It is only used by the legacy `Chat` view.
- Modify `src/views/workspace/Workspace.tsx`
  - Add pinned autoscroll to the existing scroll container.
  - Add `data-testid="workspace-scroll-container"` to the transcript scroll element.
- Modify `src/views/workspace/Workspace.test.tsx`
  - Add deterministic jsdom scroll metric helpers.
  - Add autoscroll behavior tests.
- Modify active source comments that mention `Chat.tsx`
  - `src/App.test.tsx`
  - `src/lib/compactCommand.ts`
  - `src/components/ContextUsageGauge.tsx`
  - `src/components/MessageContent.tsx`
  - `src/state/contextUsageStore.ts`
  - `src/views/workspace/Workspace.tsx`
  - `src/views/chat/rich-input/RichInput.tsx`
  - `src/views/chat/rich-input/RichInput.test.tsx`
  - `src/views/chat/rich-input/extensions/skill-mention.tsx`
- Modify e2e specs that reference `chat-input`
  - Delete `tests/e2e/specs/chat.spec.ts` and replace it with `tests/e2e/specs/workspace-chat.spec.ts`.
  - Update `tests/e2e/specs/keyboard-shortcuts.spec.ts` to use EmptyState/Workspace behavior.

Existing dirty worktree note: this repo has unrelated uncommitted work in several files. For every task, inspect `git diff` before staging. If a touched file already contains unrelated hunks, stage only the hunks owned by the task.

---

### Task 1: App Always Renders Workspace

**Files:**

- Modify: `src/App.tsx`
- Modify: `src/App.test.tsx`

- [ ] **Step 1: Write the failing App tests**

In `src/App.test.tsx`, replace the legacy test named:

```ts
it("Cmd+L focuses the plain chat input for a pre-existing, non-workspace conversation (US1, FR-012 regression guard)", async () => {
```

with this test:

```ts
it("renders Workspace and focuses the agent input for any selected conversation", async () => {
  vi.mocked(commands.listConversations).mockResolvedValue([
    {
      id: "legacy-1",
      workspaceId: null,
      title: "Before 006",
      createdAt: 1,
      updatedAt: 1,
      status: "done",
    },
  ]);

  render(<App />);
  await waitForReady();

  await userEvent.click(await screen.findByText("Before 006"));
  const agentInput = await screen.findByTestId("agent-input");

  document.body.focus();
  expect(document.activeElement).not.toBe(agentInput);
  pressCmd("l");
  expect(document.activeElement).toBe(agentInput);
});
```

Keep the existing test:

```ts
it("Cmd+L focuses the agent task input for a workspace-scoped conversation, not the chat input (US1)", async () => {
```

but rename it to:

```ts
it("Cmd+L focuses the agent task input after creating a conversation", async () => {
```

and keep its existing assertions.

In the Settings shortcut test, remove this assertion:

```ts
expect(screen.queryByTestId("chat-input")).not.toBeInTheDocument();
```

At the top of `src/App.test.tsx`, replace:

```ts
// child view's IPC surface mocked (matching Chat.test.tsx/ConversationList.
// test.tsx/Workspace.test.tsx/Settings.test.tsx's existing mock shapes).
```

with:

```ts
// child view's IPC surface mocked (matching ConversationList.test.tsx,
// Workspace.test.tsx, and Settings.test.tsx's existing mock shapes).
```

- [ ] **Step 2: Run App tests to verify they fail**

Run:

```bash
npm test -- src/App.test.tsx
```

Expected: FAIL because selecting a `workspaceId: null` conversation still renders `Chat`, so `agent-input` is not found.

- [ ] **Step 3: Simplify `App.tsx` routing and focus**

In `src/App.tsx`, remove these imports:

```ts
import Chat from "@/views/chat/Chat";
import { wireConversationStreamEvents } from "@/state/conversationStreamStore";
```

In the startup effect, remove this call:

```ts
wireConversationStreamEvents();
```

Replace the Cmd+L selector block inside `buildShortcuts({ focusInput })`:

```ts
const selector = !activeConversation
  ? '[data-testid="empty-state-input"]'
  : activeConversation.workspaceId != null
    ? '[data-testid="agent-input"]'
    : '[data-testid="chat-input"]';
document.querySelector<HTMLElement>(selector)?.focus();
```

with:

```ts
const selector = activeConversation
  ? '[data-testid="agent-input"]'
  : '[data-testid="empty-state-input"]';
document.querySelector<HTMLElement>(selector)?.focus();
```

Replace the active conversation render branch:

```tsx
) : activeConversation ? (
  activeConversation.workspaceId != null ? (
    <Workspace
      key={activeConversation.id}
      conversationId={activeConversation.id}
      pendingInitialTurn={
        pendingInitialTurn?.conversationId === activeConversation.id
          ? pendingInitialTurn
          : null
      }
      onPendingInitialTurnConsumed={(conversationId) =>
        setPendingInitialTurn((prev) =>
          prev?.conversationId === conversationId ? null : prev,
        )
      }
    />
  ) : (
    <Chat key={activeConversation.id} conversationId={activeConversation.id} />
  )
) : (
```

with:

```tsx
) : activeConversation ? (
  <Workspace
    key={activeConversation.id}
    conversationId={activeConversation.id}
    pendingInitialTurn={
      pendingInitialTurn?.conversationId === activeConversation.id ? pendingInitialTurn : null
    }
    onPendingInitialTurnConsumed={(conversationId) =>
      setPendingInitialTurn((prev) => (prev?.conversationId === conversationId ? null : prev))
    }
  />
) : (
```

- [ ] **Step 4: Run App tests to verify they pass**

Run:

```bash
npm test -- src/App.test.tsx
```

Expected: PASS.

- [ ] **Step 5: Commit**

Before staging, run:

```bash
git diff -- src/App.tsx src/App.test.tsx
```

Then stage only App routing/focus/test changes:

```bash
git add src/App.tsx src/App.test.tsx
git commit -m "refactor: render all conversations in workspace"
```

Expected: commit includes only `src/App.tsx` and `src/App.test.tsx`.

---

### Task 2: Remove Legacy Plain Chat Frontend

**Files:**

- Delete: `src/views/chat/Chat.tsx`
- Delete: `src/views/chat/Chat.test.tsx`
- Delete: `src/state/conversationStreamStore.ts`
- Modify: `src/lib/compactCommand.ts`
- Modify: `src/components/ContextUsageGauge.tsx`
- Modify: `src/components/MessageContent.tsx`
- Modify: `src/state/contextUsageStore.ts`
- Modify: `src/views/workspace/Workspace.tsx`
- Modify: `src/views/chat/rich-input/RichInput.tsx`
- Modify: `src/views/chat/rich-input/RichInput.test.tsx`
- Modify: `src/views/chat/rich-input/extensions/skill-mention.tsx`

- [ ] **Step 1: Confirm the legacy files are unreachable**

Run:

```bash
rg -n "from \"@/views/chat/Chat\"|from \"./Chat\"|wireConversationStreamEvents|useConversationStreamStore" src
```

Expected before deletion: either no `Chat` import remains after Task 1, or only `Chat.tsx`/`Chat.test.tsx`/`conversationStreamStore.ts` self-references remain.

- [ ] **Step 2: Delete legacy files**

Delete these files:

```bash
git rm src/views/chat/Chat.tsx src/views/chat/Chat.test.tsx src/state/conversationStreamStore.ts
```

Expected: the three files are staged as deletions.

- [ ] **Step 3: Clean active source comments**

In `src/lib/compactCommand.ts`, replace the file comment with:

```ts
// 010-context-window-management (UI refactor): the `/compact` slash
// command, mirroring Claude Code's own `/compact` convention — typing it in
// the workspace composer triggers compaction directly (via
// `commands.compactConversation`) instead of being sent as a normal agent
// message.
```

In `src/components/ContextUsageGauge.tsx`, replace the sentence:

```ts
 * typing `/compact` in the composer (see Chat.tsx/Workspace.tsx), mirroring
```

with:

```ts
 * typing `/compact` in the workspace composer, mirroring
```

In `src/components/MessageContent.tsx`, replace the `showTimer` prop comment:

```ts
  // Chat.tsx's `send_message` is streamed (real queued/generating latency
  // worth showing); Workspace.tsx's `send_agent_message` runs synchronously
  // to completion server-side with no per-message duration captured, so it
  // opts out rather than showing a meaningless "0s" (matches its pre-004
  // behavior exactly).
  showTimer?: boolean;
```

with:

```ts
  // Historical message rows may include duration metadata. Workspace keeps
  // this off by default because `send_agent_message` has no useful
  // per-message duration for the optimistic in-progress turn.
  showTimer?: boolean;
```

Replace the component-level comment:

```ts
 * 004-tool-call-widgets (FR-013): the one place both `Chat.tsx` and
 * `Workspace.tsx` render a message's content from — a `tool_result` row
 * dispatches to its matching widget by `toolName`, everything else renders
 * exactly as it always has. Having one function, not two independently
 * maintained copies, is what actually satisfies FR-013 (SC-006) — it isn't
 * possible for the two views to drift since there's only one rendering
 * path to edit.
```

with:

```ts
 * 004-tool-call-widgets (FR-013): the single transcript renderer. A
 * `tool_result` row dispatches to its matching widget by `toolName`, while
 * ordinary text/rich-text/context rows render through the shared message
 * components below.
```

In `src/views/chat/rich-input/RichInput.tsx`, replace the `skillsEnabled` comment sentence:

```ts
   * inert — no picker, no `commands.listSkills()` call — matching
   * `Chat.tsx`'s plain-mode composer (FR-011).
```

with:

```ts
   * inert — no picker, no `commands.listSkills()` call.
```

Replace the `inputTestId` comment:

```ts
   * existing testid (`empty-state-input`/`chat-input`/`agent-input`).
```

with:

```ts
   * existing testid (`empty-state-input`/`agent-input`).
```

Replace the `contextGauge` comment sentence:

```ts
   * for (`Chat.tsx`/`Workspace.tsx`; omitted by `EmptyState.tsx`, which has
```

with:

```ts
   * for (`Workspace.tsx`; omitted by `EmptyState.tsx`, which has
```

Replace the component-level sentence:

```ts
 * separate raw <textarea>s in EmptyState.tsx/Chat.tsx/Workspace.tsx. A
```

with:

```ts
 * separate raw inputs in EmptyState.tsx and Workspace.tsx. A
```

In `src/state/contextUsageStore.ts`, replace:

```ts
// state, mirroring conversationStreamStore.ts's shape/conventions. Keyed by
```

with:

```ts
// state keyed by conversationId since more than one conversation's usage may
// be known at once (e.g. after switching away and back).
```

Then remove the now-duplicated next two comment lines:

```ts
// conversationId since more than one conversation's usage may be known at
// once (e.g. after switching away and back).
```

In `src/views/workspace/Workspace.tsx`, replace the component-level comment:

```ts
/**
 * 006-chat-empty-state: restructured from a self-contained "pick a folder,
 * then chat" component into a `conversationId`-driven message view, the
 * same shape as `Chat.tsx` — folder selection now happens once, up front,
 * in `EmptyState.tsx`/`FolderPicker.tsx`.
 *
 * Streaming (UI refactor): unlike `Chat.tsx`'s token-level streaming,
 * `send_agent_message`'s single promise doesn't resolve until the whole
 * (up to 200-turn) tool-use loop finishes — so instead, every tool_call/
 * tool_result/final-answer row persisted *during* that loop fires an
 * `agent-message-persisted` event, and this view just re-fetches
 * `list_messages` each time and re-renders. Simplified streaming, not
 * token deltas: the transcript grows message-by-message as the loop
 * actually progresses, rather than appearing all at once at the end.
 */
```

with:

```ts
/**
 * 006-chat-empty-state: message view for a selected conversation. Folder
 * selection happens once, up front, in `EmptyState.tsx`/`FolderPicker.tsx`.
 *
 * Streaming (UI refactor): `send_agent_message`'s single promise does not
 * resolve until the whole tool-use loop finishes. During that loop, every
 * persisted tool_call/tool_result/final-answer row fires an
 * `agent-message-persisted` event, and this view re-fetches `list_messages`
 * each time so the transcript grows message-by-message.
 */
```

In `src/views/chat/rich-input/extensions/skill-mention.tsx`, replace:

```ts
        // Chat.tsx/Workspace.tsx's `isCompactCommand`), not a skill mention
```

with:

```ts
        // Workspace's `isCompactCommand`), not a skill mention
```

In `src/views/chat/rich-input/RichInput.tsx`, replace the extension-registration comment:

```ts
      // `skillsEnabled` is effectively static per mounted `RichInput`
      // instance in this app (`EmptyState`/`Workspace` always pass `true`,
      // `Chat` always passes `false` — none of the three call sites flips
      // it after mounting), so there is no real prop-change case to handle,
      // and omitting the extension entirely when disabled is strictly
      // stronger than a runtime no-op: `commands.listSkills()` (FR-011's
      // "no picker, no request") is *structurally* unreachable rather than
      // reachable-but-gated, and `/` in `Chat.tsx`'s composer never even
      // registers a suggestion plugin to intercept it.
```

with:

```ts
      // `skillsEnabled` is effectively static per mounted `RichInput`
      // instance in this app (`EmptyState` and `Workspace` pass `true`), so
      // there is no real prop-change case to handle. Omitting the extension
      // entirely when disabled is stronger than a runtime no-op:
      // `commands.listSkills()` is structurally unreachable when skills are
      // disabled.
```

In `src/views/chat/rich-input/RichInput.test.tsx`, replace the file comment:

```ts
/**
 * 009-rich-chat-input, User Story 1 (T004): the shared rich-text input that
 * replaces the three raw <textarea>s in EmptyState.tsx/Chat.tsx/
 * Workspace.tsx. Tier-2 jsdom component tests per research.md's Testing
 * strategy — structural/rendering correctness only, driven via
 * userEvent.type()/userEvent.keyboard() on an empty/focused editor. No
 * pixel-geometry assertions.
 */
```

with:

```ts
/**
 * 009-rich-chat-input, User Story 1 (T004): the shared rich-text input used
 * by EmptyState.tsx and Workspace.tsx. Tier-2 jsdom component tests per
 * research.md's Testing strategy — structural/rendering correctness only,
 * driven via userEvent.type()/userEvent.keyboard() on an empty/focused
 * editor. No pixel-geometry assertions.
 */
```

- [ ] **Step 4: Verify no active frontend references remain**

Run:

```bash
rg -n "Chat\\.tsx|Chat\\.test\\.tsx|conversationStreamStore|wireConversationStreamEvents|useConversationStreamStore" src
```

Expected: no matches in active `src` files.

Run:

```bash
rg -n 'data-testid=.chat-(input|send)|inputTestId=.chat-input|submitTestId=.chat-send|getByTestId\(.chat-(input|send)|findByTestId\(.chat-(input|send)|queryByTestId\(.chat-(input|send)|\[data-testid=.chat-(input|send)' src
```

Expected: no matches. This intentionally ignores feature names such as `009-rich-chat-input`; it checks only the removed legacy selectors.

- [ ] **Step 5: Run source checks**

Run:

```bash
npm test -- src/App.test.tsx src/views/workspace/Workspace.test.tsx src/views/chat/EmptyState.test.tsx
npm run lint
npm run build
```

Expected: all commands exit 0. `npm run build` may print the existing Vite chunk-size warning.

- [ ] **Step 6: Commit**

Before staging, inspect:

```bash
git diff -- src/views/chat/Chat.tsx src/views/chat/Chat.test.tsx src/state/conversationStreamStore.ts src/lib/compactCommand.ts src/components/ContextUsageGauge.tsx src/components/MessageContent.tsx src/state/contextUsageStore.ts src/views/workspace/Workspace.tsx src/views/chat/rich-input/RichInput.tsx src/views/chat/rich-input/RichInput.test.tsx src/views/chat/rich-input/extensions/skill-mention.tsx
```

Then commit:

```bash
git add src/views/chat/Chat.tsx src/views/chat/Chat.test.tsx src/state/conversationStreamStore.ts src/lib/compactCommand.ts src/components/ContextUsageGauge.tsx src/components/MessageContent.tsx src/state/contextUsageStore.ts src/views/workspace/Workspace.tsx src/views/chat/rich-input/RichInput.tsx src/views/chat/rich-input/RichInput.test.tsx src/views/chat/rich-input/extensions/skill-mention.tsx
git commit -m "refactor: remove legacy plain chat frontend"
```

Expected: commit removes the legacy frontend files and updates only active source comments.

---

### Task 3: Update E2E Specs For Workspace Chat

**Files:**

- Delete: `tests/e2e/specs/chat.spec.ts`
- Create: `tests/e2e/specs/workspace-chat.spec.ts`
- Modify: `tests/e2e/specs/keyboard-shortcuts.spec.ts`

- [ ] **Step 1: Replace the legacy chat e2e spec**

Delete `tests/e2e/specs/chat.spec.ts`.

Create `tests/e2e/specs/workspace-chat.spec.ts`:

```ts
import { expect } from "@wdio/globals";
import { Key } from "webdriverio";

const MARKER_ONE = "DOCE_E2E_WORKSPACE_MARKER_ONE say hello in exactly three words";
const MARKER_TWO = "DOCE_E2E_WORKSPACE_MARKER_TWO what's 2+2";

async function bubbleTexts(): Promise<string[]> {
  const bubbles = await browser.$$("[data-testid='chat-message']");
  const texts: string[] = [];
  for (let i = 0; i < bubbles.length; i++) {
    texts.push(await bubbles[i].getText());
  }
  return texts;
}

async function openEmptyState() {
  await browser.keys([Key.Command, "n"]);
  const input = await browser.$("[data-testid='empty-state-input']");
  await input.waitForExist({ timeout: 60000 });
  return input;
}

async function submitInitialWorkspaceTurn(text: string) {
  const input = await openEmptyState();
  await input.setValue(text);
  await (await browser.$("[data-testid='empty-state-submit']")).click();
  const agentInput = await browser.$("[data-testid='agent-input']");
  await agentInput.waitForExist({ timeout: 60000 });
  return agentInput;
}

async function waitForMessageFollowedByAnotherBubble(marker: string) {
  await browser.waitUntil(
    async () => {
      const texts = await bubbleTexts();
      const idx = texts.findIndex((t) => t.includes(marker));
      return idx !== -1 && idx + 1 < texts.length;
    },
    {
      timeout: 60000,
      timeoutMsg: `no response bubble appeared after ${marker}`,
    },
  );
}

describe("Workspace chat", () => {
  it("sends an initial task from the empty state and renders follow-up output", async () => {
    await submitInitialWorkspaceTurn(MARKER_ONE);
    await waitForMessageFollowedByAnotherBubble(MARKER_ONE);

    const texts = await bubbleTexts();
    const idx = texts.findIndex((t) => t.includes(MARKER_ONE));
    const nextBubble = texts[idx + 1];
    expect(nextBubble.trim().length).toBeGreaterThan(0);
    expect(nextBubble).not.toContain(MARKER_ONE);
  });

  it("keeps later turns ordered after their user message", async () => {
    const input = await browser.$("[data-testid='agent-input']");
    await input.waitForExist({ timeout: 60000 });
    await input.setValue(MARKER_TWO);
    await (await browser.$("[data-testid='agent-send']")).click();

    await waitForMessageFollowedByAnotherBubble(MARKER_TWO);

    const texts = await bubbleTexts();
    const idxOne = texts.findIndex((t) => t.includes(MARKER_ONE));
    const idxTwo = texts.findIndex((t) => t.includes(MARKER_TWO));
    expect(idxOne).toBeGreaterThanOrEqual(0);
    expect(idxTwo).toBeGreaterThan(idxOne);
    expect(texts[idxTwo + 1].trim().length).toBeGreaterThan(0);
    expect(texts[idxTwo + 1]).not.toContain(MARKER_TWO);
  });
});
```

- [ ] **Step 2: Update keyboard shortcut e2e**

Replace the first test in `tests/e2e/specs/keyboard-shortcuts.spec.ts`:

```ts
it("Cmd+N creates a new conversation and switches to it (US2)", async () => {
  const before = (await browser.$$("[data-testid='conversation-item']")).length;

  await browser.keys([Key.Command, "n"]);

  await browser.waitUntil(
    async () => (await browser.$$("[data-testid='conversation-item']")).length > before,
    { timeout: 15000, timeoutMsg: "Cmd+N never created a new conversation" },
  );
  const input = await browser.$("[data-testid='chat-input']");
  await input.waitForExist({ timeout: 10000 });
});
```

with:

```ts
it("Cmd+N opens the empty-state composer", async () => {
  await browser.keys([Key.Command, "n"]);

  const input = await browser.$("[data-testid='empty-state-input']");
  await input.waitForExist({ timeout: 10000 });
  expect(await input.isExisting()).toBe(true);
});
```

Replace the second test:

```ts
it("Cmd+L focuses the chat input from elsewhere on the page (US1)", async () => {
  const sidebar = await browser.$("[data-testid='conversation-list']");
  await sidebar.click();

  const input = await browser.$("[data-testid='chat-input']");
  await input.waitForExist({ timeout: 10000 });
  expect(await input.isFocused()).toBe(false);

  await browser.keys([Key.Command, "l"]);

  await browser.waitUntil(async () => input.isFocused(), {
    timeout: 5000,
    timeoutMsg: "Cmd+L never focused the chat input",
  });
});
```

with:

```ts
it("Cmd+L focuses the workspace input from elsewhere on the page (US1)", async () => {
  await browser.keys([Key.Command, "n"]);
  const emptyInput = await browser.$("[data-testid='empty-state-input']");
  await emptyInput.waitForExist({ timeout: 10000 });
  await emptyInput.setValue("DOCE_E2E_SHORTCUT_FOCUS create a workspace conversation");
  await (await browser.$("[data-testid='empty-state-submit']")).click();

  const input = await browser.$("[data-testid='agent-input']");
  await input.waitForExist({ timeout: 60000 });

  const sidebar = await browser.$("[data-testid='conversation-list']");
  await sidebar.click();
  expect(await input.isFocused()).toBe(false);

  await browser.keys([Key.Command, "l"]);

  await browser.waitUntil(async () => input.isFocused(), {
    timeout: 5000,
    timeoutMsg: "Cmd+L never focused the workspace input",
  });
});
```

In the Cmd+K test, replace:

```ts
expect(rows.length).toBe(3);
```

with:

```ts
expect(rows.length).toBe(4);
```

- [ ] **Step 3: Verify no e2e references to legacy selectors remain**

Run:

```bash
rg -n 'data-testid=.chat-(input|send)|getByTestId\(.chat-(input|send)|findByTestId\(.chat-(input|send)|queryByTestId\(.chat-(input|send)|\[data-testid=.chat-(input|send)|send_message|Chat\.tsx' tests/e2e
```

Expected: no matches.

- [ ] **Step 4: Run static verification**

Run:

```bash
npm run lint
npm run build
```

Expected: both exit 0.

- [ ] **Step 5: Commit**

Before staging, inspect:

```bash
git diff -- tests/e2e/specs/chat.spec.ts tests/e2e/specs/workspace-chat.spec.ts tests/e2e/specs/keyboard-shortcuts.spec.ts
```

Then commit:

```bash
git add tests/e2e/specs/chat.spec.ts tests/e2e/specs/workspace-chat.spec.ts tests/e2e/specs/keyboard-shortcuts.spec.ts
git commit -m "test: move e2e chat coverage to workspace"
```

Expected: commit deletes the legacy chat e2e spec, adds workspace chat coverage, and updates keyboard shortcut selectors.

---

### Task 4: Workspace Pinned Autoscroll

**Files:**

- Modify: `src/views/workspace/Workspace.tsx`
- Modify: `src/views/workspace/Workspace.test.tsx`

- [ ] **Step 1: Add jsdom scroll helpers and failing tests**

In `src/views/workspace/Workspace.test.tsx`, add these helper functions near the top of the file after the `vi.mock(...)` block:

```ts
function messageFixture(id: string, content: string, createdAt = 1) {
  return {
    id,
    conversationId: "conv-1",
    role: "user" as const,
    contentType: "text" as const,
    content,
    toolName: null,
    createdAt,
    durationMs: null,
    tokenCount: null,
  };
}

function setScrollMetrics(
  element: HTMLElement,
  metrics: { scrollHeight: number; clientHeight: number; scrollTop: number },
) {
  let currentScrollTop = metrics.scrollTop;
  Object.defineProperty(element, "scrollHeight", {
    configurable: true,
    value: metrics.scrollHeight,
  });
  Object.defineProperty(element, "clientHeight", {
    configurable: true,
    value: metrics.clientHeight,
  });
  Object.defineProperty(element, "scrollTop", {
    configurable: true,
    get: () => currentScrollTop,
    set: (value: number) => {
      currentScrollTop = value;
    },
  });
}
```

Add these tests before the existing `/compact` test section:

```ts
it("starts pinned and scrolls to the bottom after messages render", async () => {
  let resolveMessages!: (messages: Awaited<ReturnType<typeof commands.listMessages>>) => void;
  vi.mocked(commands.listMessages).mockReturnValueOnce(
    new Promise((resolve) => {
      resolveMessages = resolve;
    }),
  );

  render(<Workspace conversationId="conv-1" />);
  const scrollContainer = await screen.findByTestId("workspace-scroll-container");
  setScrollMetrics(scrollContainer, { scrollHeight: 1000, clientHeight: 300, scrollTop: 0 });

  resolveMessages([messageFixture("m1", "first message")]);

  await screen.findByText("first message");
  await waitFor(() => expect(scrollContainer.scrollTop).toBe(700));
});

it("keeps following new messages while pinned near the bottom", async () => {
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
  setScrollMetrics(scrollContainer, { scrollHeight: 1000, clientHeight: 300, scrollTop: 690 });
  fireEvent.scroll(scrollContainer);

  await screen.findByText("first message");
  setScrollMetrics(scrollContainer, { scrollHeight: 1400, clientHeight: 300, scrollTop: 690 });
  firePersisted({ conversationId: "conv-1" });

  await screen.findByText("second message");
  await waitFor(() => expect(scrollContainer.scrollTop).toBe(1100));
});

it("does not autoscroll new messages after the user scrolls up", async () => {
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
  setScrollMetrics(scrollContainer, { scrollHeight: 1400, clientHeight: 300, scrollTop: 200 });

  firePersisted({ conversationId: "conv-1" });

  await screen.findByText("second message");
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(scrollContainer.scrollTop).toBe(200);
});

it("resumes autoscroll after the user scrolls back near the bottom", async () => {
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
  setScrollMetrics(scrollContainer, { scrollHeight: 1000, clientHeight: 300, scrollTop: 680 });
  fireEvent.scroll(scrollContainer);
  setScrollMetrics(scrollContainer, { scrollHeight: 1400, clientHeight: 300, scrollTop: 680 });

  firePersisted({ conversationId: "conv-1" });

  await screen.findByText("second message");
  await waitFor(() => expect(scrollContainer.scrollTop).toBe(1100));
});

it("resets autoscroll pinning when switching conversations", async () => {
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

  rerender(<Workspace conversationId="conv-2" />);
  setScrollMetrics(scrollContainer, { scrollHeight: 900, clientHeight: 300, scrollTop: 0 });

  await screen.findByText("second workspace");
  await waitFor(() => expect(scrollContainer.scrollTop).toBe(600));
});
```

- [ ] **Step 2: Run Workspace tests to verify they fail**

Run:

```bash
npm test -- src/views/workspace/Workspace.test.tsx
```

Expected: FAIL because `workspace-scroll-container` does not exist and no autoscroll behavior is implemented.

- [ ] **Step 3: Implement autoscroll in `Workspace.tsx`**

In `src/views/workspace/Workspace.tsx`, update the React import:

```ts
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
```

Add these helpers above `interface WorkspaceProps`:

```ts
const AUTOSCROLL_BOTTOM_THRESHOLD_PX = 48;

function isNearScrollBottom(element: HTMLElement): boolean {
  return (
    element.scrollHeight - element.scrollTop - element.clientHeight <=
    AUTOSCROLL_BOTTOM_THRESHOLD_PX
  );
}

function scrollElementToBottom(element: HTMLElement) {
  element.scrollTop = Math.max(0, element.scrollHeight - element.clientHeight);
}
```

Inside `Workspace`, after `const [error, setError] = useState<string | null>(null);`, add:

```ts
const scrollContainerRef = useRef<HTMLDivElement | null>(null);
const autoscrollPinnedRef = useRef(true);
```

After `const showThinking = thinking || sendInFlight;`, add:

```ts
const scrollToTranscriptBottom = useCallback(() => {
  const element = scrollContainerRef.current;
  if (!element) return;
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

useEffect(() => {
  autoscrollPinnedRef.current = true;
  return scheduleScrollToTranscriptBottom();
}, [conversationId, scheduleScrollToTranscriptBottom]);

useEffect(() => {
  if (!autoscrollPinnedRef.current) return;
  return scheduleScrollToTranscriptBottom();
}, [messages, pendingQuestion, scheduleScrollToTranscriptBottom, showThinking]);
```

Update the scroll container:

```tsx
<div className="flex-1 overflow-y-auto p-4">
```

to:

```tsx
<div
  ref={scrollContainerRef}
  className="flex-1 overflow-y-auto p-4"
  data-testid="workspace-scroll-container"
  onScroll={updateAutoscrollPinned}
>
```

- [ ] **Step 4: Run Workspace tests to verify they pass**

Run:

```bash
npm test -- src/views/workspace/Workspace.test.tsx
```

Expected: PASS.

- [ ] **Step 5: Run focused format check**

Run:

```bash
./node_modules/.bin/oxfmt --check src/views/workspace/Workspace.tsx src/views/workspace/Workspace.test.tsx
```

Expected: PASS. If it fails, run:

```bash
./node_modules/.bin/oxfmt src/views/workspace/Workspace.tsx src/views/workspace/Workspace.test.tsx
npm test -- src/views/workspace/Workspace.test.tsx
```

Expected after formatting: tests PASS.

- [ ] **Step 6: Commit**

Before staging, inspect:

```bash
git diff -- src/views/workspace/Workspace.tsx src/views/workspace/Workspace.test.tsx
```

Then commit:

```bash
git add src/views/workspace/Workspace.tsx src/views/workspace/Workspace.test.tsx
git commit -m "feat: add workspace chat autoscroll"
```

Expected: commit contains only `Workspace` autoscroll code and tests.

---

### Task 5: Final Cleanup And Verification

**Files:**

- Verify only; do not edit files unless a command exposes a concrete failure.

- [ ] **Step 1: Check for stale active references**

Run:

```bash
rg -n "from \"@/views/chat/Chat\"|from \"./Chat\"|conversationStreamStore|wireConversationStreamEvents|useConversationStreamStore" src tests/e2e
```

Expected: no matches.

Run:

```bash
rg -n "Chat\\.tsx|Chat\\.test\\.tsx" src
```

Expected: no matches in active source. Historical docs/specs outside `src` may still mention `Chat.tsx`.

Run:

```bash
rg -n 'data-testid=.chat-(input|send)|inputTestId=.chat-input|submitTestId=.chat-send|getByTestId\(.chat-(input|send)|findByTestId\(.chat-(input|send)|queryByTestId\(.chat-(input|send)|\[data-testid=.chat-(input|send)' src tests/e2e
```

Expected: no matches. Feature names like `009-rich-chat-input` are allowed because they do not reference the removed legacy chat selectors.

- [ ] **Step 2: Run focused unit tests**

Run:

```bash
npm test -- src/App.test.tsx src/views/workspace/Workspace.test.tsx src/views/chat/EmptyState.test.tsx src/views/chat/ConversationList.test.tsx src/views/chat/sidebarConversationRow.test.ts
```

Expected: all listed test files pass.

- [ ] **Step 3: Run full frontend unit tests**

Run:

```bash
npm test
```

Expected: all Vitest test files pass.

- [ ] **Step 4: Run formatting check on touched files**

Run:

```bash
./node_modules/.bin/oxfmt --check src/App.tsx src/App.test.tsx src/views/workspace/Workspace.tsx src/views/workspace/Workspace.test.tsx src/lib/compactCommand.ts src/components/ContextUsageGauge.tsx src/components/MessageContent.tsx src/state/contextUsageStore.ts src/views/chat/rich-input/RichInput.tsx src/views/chat/rich-input/RichInput.test.tsx src/views/chat/rich-input/extensions/skill-mention.tsx tests/e2e/specs/keyboard-shortcuts.spec.ts tests/e2e/specs/workspace-chat.spec.ts
```

Expected: PASS.

- [ ] **Step 5: Run lint and build**

Run:

```bash
npm run lint
npm run build
```

Expected: both exit 0. `npm run build` may print the existing Vite chunk-size warning.

- [ ] **Step 6: Inspect status**

Run:

```bash
git status --short
```

Expected: only unrelated pre-existing dirty files remain. No feature-owned files are unstaged.

- [ ] **Step 7: Commit any verification-only formatting fixes**

If Step 4 required formatter changes, inspect and commit only those feature-owned files:

```bash
git diff -- src/App.tsx src/App.test.tsx src/views/workspace/Workspace.tsx src/views/workspace/Workspace.test.tsx src/lib/compactCommand.ts src/components/ContextUsageGauge.tsx src/components/MessageContent.tsx src/state/contextUsageStore.ts src/views/chat/rich-input/RichInput.tsx src/views/chat/rich-input/RichInput.test.tsx src/views/chat/rich-input/extensions/skill-mention.tsx tests/e2e/specs/keyboard-shortcuts.spec.ts tests/e2e/specs/workspace-chat.spec.ts
git add src/App.tsx src/App.test.tsx src/views/workspace/Workspace.tsx src/views/workspace/Workspace.test.tsx src/lib/compactCommand.ts src/components/ContextUsageGauge.tsx src/components/MessageContent.tsx src/state/contextUsageStore.ts src/views/chat/rich-input/RichInput.tsx src/views/chat/rich-input/RichInput.test.tsx src/views/chat/rich-input/extensions/skill-mention.tsx tests/e2e/specs/keyboard-shortcuts.spec.ts tests/e2e/specs/workspace-chat.spec.ts
git commit -m "style: format single workspace chat changes"
```

Expected: skip this commit if no formatter changes were needed.
