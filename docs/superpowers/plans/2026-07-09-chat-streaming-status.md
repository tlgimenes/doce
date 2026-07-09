# Chat Streaming Status Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move the generic active-turn status out of the transcript and render a bottom-of-chat `Thinking` row with a stable elapsed chron above the composer divider.

**Architecture:** Add a small presentational `StreamingStatus` component that owns animation and elapsed-time rendering. Keep `Workspace` responsible for deriving whether the generic status is shown and which user-message timestamp starts the active-turn chron. Preserve dedicated pending widgets for `AskUserQuestion`, `Bash`, and `Task`.

**Tech Stack:** React 19, TypeScript, Vitest + Testing Library + user-event, existing `Workspace` state, existing `RichInput`, Tailwind utility classes, and existing IPC mocks in `Workspace.test.tsx`.

## Global Constraints

- Move the generic active-turn indicator out of the transcript message list.
- Render a live status row above the chat input.
- Replace the static `Working...` text with a subtle animated `Thinking` indicator.
- Add a stable elapsed-time chron that starts when the user submits the message.
- Preserve existing pending widgets for `AskUserQuestion`, `Bash`, and `Task`.
- Preserve composer blocking behavior while a turn is active.
- Do not implement true token streaming.
- Do not change backend timing schema.
- Do not add per-tool progress details.
- Do not rework message persistence or active-generation semantics.
- Do not change the sidebar conversation state label.
- The status row is not rendered as a `chat-message`.
- The status row is not inside the rich input.
- The status row is above the input divider, not overlaid on top of it and not below it.
- The bottom edge of the status row touches the divider line.
- When the status row is visible, its bottom border is the divider between transcript/status and composer.
- When the status row is hidden, the composer keeps its normal top divider so idle layout remains unchanged.
- Use `Thinking`, not `Working...`, for the generic model-active state.
- Keep the animation decorative with `aria-hidden="true"`.
- Expose the live text as a status region for assistive technology.
- Use tabular numbers for the chron so the width does not jitter while ticking.
- On send, use the optimistic user message `createdAt` as the start timestamp.
- During a persisted active turn, derive the start timestamp from the latest user message in the conversation.
- Keep counting through the whole active turn.
- Do not reset when tool calls or tool results appear.
- Stop/hide when the turn becomes idle.
- If no user-message timestamp is available, fall back to the time the active status first appears in the current webview session.

---

## File Structure

- Create `src/views/workspace/StreamingStatus.tsx`: presentational status row with animated activity mark and elapsed chron.
- Create `src/views/workspace/StreamingStatus.test.tsx`: deterministic timer, accessibility, decorative animation, and fallback timestamp tests.
- Modify `src/views/workspace/Workspace.tsx`: derive active-turn timestamp, move generic thinking UI out of the transcript, render `StreamingStatus` above the composer, and adjust composer divider classes.
- Modify `src/views/workspace/Workspace.test.tsx`: update existing `agent-thinking` expectations for the new placement/text and add chron/layout regressions.

---

### Task 1: Add `StreamingStatus`

**Files:**
- Create: `src/views/workspace/StreamingStatus.tsx`
- Create: `src/views/workspace/StreamingStatus.test.tsx`

**Interfaces:**
- Produces: `StreamingStatus({ startedAt }: { startedAt: number | null }): JSX.Element`
- Produces test ids:
  - `agent-thinking`
  - `agent-thinking-dot`
  - `agent-thinking-timer`
- Later tasks import `StreamingStatus` into `Workspace`.

- [ ] **Step 1: Write failing component tests**

Create `src/views/workspace/StreamingStatus.test.tsx`:

```tsx
import { act, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import StreamingStatus from "./StreamingStatus";

describe("StreamingStatus", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("renders a quiet accessible thinking status with decorative animation", () => {
    vi.useFakeTimers();
    vi.setSystemTime(10_000);

    render(<StreamingStatus startedAt={8_750} />);

    expect(screen.getByRole("status", { name: "Thinking" })).toBeInTheDocument();
    expect(screen.getByTestId("agent-thinking")).toHaveTextContent("Thinking");
    expect(screen.getAllByTestId("agent-thinking-dot")).toHaveLength(3);
    expect(screen.getByTestId("agent-thinking-dots")).toHaveAttribute("aria-hidden", "true");
    expect(screen.getByTestId("agent-thinking-timer")).toHaveTextContent("1.3s");
    expect(screen.getByTestId("agent-thinking-timer")).toHaveClass("tabular-nums");
  });

  it("ticks from the provided start timestamp without changing layout width classes", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(10_000);

    render(<StreamingStatus startedAt={9_000} />);

    expect(screen.getByTestId("agent-thinking-timer")).toHaveTextContent("1.0s");

    vi.setSystemTime(12_400);
    await act(async () => {
      vi.advanceTimersByTime(100);
    });

    expect(screen.getByTestId("agent-thinking-timer")).toHaveTextContent("3.4s");
    expect(screen.getByTestId("agent-thinking-timer")).toHaveClass("min-w-[4.5ch]");
  });

  it("falls back to the mount time when no user-message timestamp is available", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(5_000);

    render(<StreamingStatus startedAt={null} />);

    expect(screen.getByTestId("agent-thinking-timer")).toHaveTextContent("0.0s");

    vi.setSystemTime(5_900);
    await act(async () => {
      vi.advanceTimersByTime(100);
    });

    expect(screen.getByTestId("agent-thinking-timer")).toHaveTextContent("0.9s");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
npx vitest run src/views/workspace/StreamingStatus.test.tsx
```

Expected: FAIL because `src/views/workspace/StreamingStatus.tsx` does not exist.

- [ ] **Step 3: Implement `StreamingStatus`**

Create `src/views/workspace/StreamingStatus.tsx`:

```tsx
import { useEffect, useRef, useState } from "react";

interface StreamingStatusProps {
  startedAt: number | null;
}

function formatElapsedMs(elapsedMs: number) {
  return `${(Math.max(0, elapsedMs) / 1000).toFixed(1)}s`;
}

export default function StreamingStatus({ startedAt }: StreamingStatusProps) {
  const fallbackStartedAtRef = useRef<number | null>(null);
  if (fallbackStartedAtRef.current == null) {
    fallbackStartedAtRef.current = Date.now();
  }

  const effectiveStartedAt = startedAt ?? fallbackStartedAtRef.current;
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    const intervalId = window.setInterval(() => setNow(Date.now()), 100);
    return () => window.clearInterval(intervalId);
  }, []);

  return (
    <div
      className="border-b border-border px-4"
      data-testid="agent-thinking"
      role="status"
      aria-label="Thinking"
    >
      <div className="mx-auto flex h-8 max-w-3xl items-center gap-2 text-xs text-muted-foreground">
        <span className="inline-flex gap-1" data-testid="agent-thinking-dots" aria-hidden="true">
          <span
            className="h-1 w-1 animate-pulse rounded-full bg-current [animation-delay:-300ms]"
            data-testid="agent-thinking-dot"
          />
          <span
            className="h-1 w-1 animate-pulse rounded-full bg-current [animation-delay:-150ms]"
            data-testid="agent-thinking-dot"
          />
          <span
            className="h-1 w-1 animate-pulse rounded-full bg-current"
            data-testid="agent-thinking-dot"
          />
        </span>
        <span>Thinking</span>
        <span
          className="min-w-[4.5ch] text-right font-mono tabular-nums"
          data-testid="agent-thinking-timer"
          aria-hidden="true"
        >
          {formatElapsedMs(now - effectiveStartedAt)}
        </span>
      </div>
    </div>
  );
}
```

- [ ] **Step 4: Run component tests**

Run:

```bash
npx vitest run src/views/workspace/StreamingStatus.test.tsx
```

Expected: PASS, 3 tests.

- [ ] **Step 5: Commit Task 1**

Run:

```bash
git add src/views/workspace/StreamingStatus.tsx src/views/workspace/StreamingStatus.test.tsx
git commit -m "feat(workspace): add streaming status component"
```

Expected: one commit containing only the new component and its test.

---

### Task 2: Move Generic Thinking Status Above Composer

**Files:**
- Modify: `src/views/workspace/Workspace.tsx`
- Modify: `src/views/workspace/Workspace.test.tsx`

**Interfaces:**
- Consumes: `StreamingStatus({ startedAt }: { startedAt: number | null })`
- Produces: `agent-thinking` outside transcript `chat-message` rows and immediately before `workspace-composer-shell`
- Keeps `workspace-composer-shell` and `agent-input` test ids unchanged.

- [ ] **Step 1: Add failing Workspace layout and chron tests**

In `src/views/workspace/Workspace.test.tsx`, add this helper after `setScrollMetrics`:

```tsx
function expectElementBefore(first: HTMLElement, second: HTMLElement) {
  expect(Boolean(first.compareDocumentPosition(second) & Node.DOCUMENT_POSITION_FOLLOWING)).toBe(
    true,
  );
}
```

In the existing test named `sends a task and shows a thinking state until the real (non-streamed) reply returns`, replace:

```tsx
    await waitFor(() =>
      expect(screen.getByTestId("agent-thinking")).toBeInTheDocument(),
    );
```

with:

```tsx
    const status = await screen.findByTestId("agent-thinking");
    const composerShell = screen.getByTestId("workspace-composer-shell");
    expect(status).toHaveTextContent("Thinking");
    expect(status).not.toHaveTextContent("Working");
    expect(status.closest('[data-testid="chat-message"]')).toBeNull();
    expectElementBefore(status, composerShell);
    expect(status).toHaveClass("border-b");
    expect(composerShell).not.toHaveClass("border-t");
```

After the existing test named `keeps the composer blocked after a reload while the backend reports the turn still active, even with no trailing tool_call (generation phase)`, add these tests:

```tsx
  it("starts the streaming chron from the latest persisted user message during a backend-active reload", async () => {
    const nowSpy = vi.spyOn(Date, "now").mockReturnValue(6_000);
    vi.mocked(commands.listMessages).mockResolvedValue([messageFixture("u1", "find the needle", 4_000)]);
    vi.mocked(commands.isGenerationActive).mockResolvedValue(true);

    render(<Workspace conversationId="conv-1" />);

    expect(await screen.findByTestId("agent-thinking-timer")).toHaveTextContent("2.0s");
    nowSpy.mockRestore();
  });

  it("does not reset the streaming chron when an unpaired non-dedicated tool call appears", async () => {
    const nowSpy = vi.spyOn(Date, "now").mockReturnValue(4_000);
    vi.mocked(commands.listMessages).mockResolvedValue([
      {
        id: "u1",
        conversationId: "conv-1",
        role: "user",
        contentType: "text",
        content: "find the needle",
        toolName: null,
        createdAt: 1_000,
        durationMs: null,
        tokenCount: null,
      },
      {
        id: "tc1",
        conversationId: "conv-1",
        role: "assistant",
        contentType: "tool_call",
        content: JSON.stringify({
          arguments: { pattern: "needle", path: "/tmp" },
        }),
        toolName: "Grep",
        createdAt: 3_000,
        durationMs: null,
        tokenCount: null,
      },
    ]);

    render(<Workspace conversationId="conv-1" />);

    expect(await screen.findByTestId("agent-thinking-timer")).toHaveTextContent("3.0s");
    nowSpy.mockRestore();
  });

  it("keeps the composer divider when the streaming status is hidden", async () => {
    vi.mocked(commands.listMessages).mockResolvedValue([]);

    render(<Workspace conversationId="conv-1" />);

    expect(await screen.findByTestId("workspace-composer-shell")).toHaveClass("border-t");
    expect(screen.queryByTestId("agent-thinking")).not.toBeInTheDocument();
  });
```

In the existing test named `blocks the composer and shows Working… when the latest message is an unfinished tool_call with no dedicated pending widget (e.g. Grep)`, change the test name to:

```tsx
  it("blocks the composer and shows Thinking when the latest message is an unfinished tool_call with no dedicated pending widget (e.g. Grep)", async () => {
```

and replace:

```tsx
    await screen.findByTestId("agent-thinking");
```

with:

```tsx
    expect(await screen.findByTestId("agent-thinking")).toHaveTextContent("Thinking");
```

- [ ] **Step 2: Run Workspace tests and verify they fail**

Run:

```bash
npx vitest run src/views/workspace/Workspace.test.tsx
```

Expected: FAIL because `agent-thinking` still renders inside the transcript content as `Working...`, the composer shell still owns the divider while the status is visible, and no `agent-thinking-timer` exists.

- [ ] **Step 3: Update imports and helper derivation in `Workspace.tsx`**

In `src/views/workspace/Workspace.tsx`, add these imports:

```tsx
import { cn } from "@/lib/cn";
import StreamingStatus from "@/views/workspace/StreamingStatus";
```

After `function isQuestionPending(messages: Message[]): boolean { ... }`, add:

```tsx
function getLatestUserMessageCreatedAt(messages: Message[]): number | null {
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    if (messages[i].role === "user") {
      return messages[i].createdAt;
    }
  }
  return null;
}
```

- [ ] **Step 4: Track the optimistic active-turn timestamp**

In `Workspace`, after:

```tsx
  const [thinking, setThinking] = useState(false);
```

add:

```tsx
  const [optimisticTurnStartedAt, setOptimisticTurnStartedAt] = useState<number | null>(null);
```

In the conversation-change effect that currently does:

```tsx
    setMessages([]);
    setThinking(false);
    setError(null);
    setBackendTurnActive(false);
```

change it to:

```tsx
    setMessages([]);
    setThinking(false);
    setOptimisticTurnStartedAt(null);
    setError(null);
    setBackendTurnActive(false);
```

In `send`, replace the optimistic message block:

```tsx
      setError(null);
      setMessages((prev) => [
        ...prev,
        {
          id: `u-${Date.now()}`,
          conversationId,
          role: "user",
          contentType: richContent ? "rich_text" : "text",
          content: richContent ? JSON.stringify(richContent) : content,
          toolName: null,
          createdAt: Date.now(),
          durationMs: null,
          // Not known until reload -- these are optimistic/synthetic
          // messages, not the real persisted row (which does get a real
          // token_count via a backend follow-up update).
          tokenCount: null,
        },
      ]);
      setThinking(true);
```

with:

```tsx
      const submittedAt = Date.now();
      setError(null);
      setMessages((prev) => [
        ...prev,
        {
          id: `u-${submittedAt}`,
          conversationId,
          role: "user",
          contentType: richContent ? "rich_text" : "text",
          content: richContent ? JSON.stringify(richContent) : content,
          toolName: null,
          createdAt: submittedAt,
          durationMs: null,
          // Not known until reload -- these are optimistic/synthetic
          // messages, not the real persisted row (which does get a real
          // token_count via a backend follow-up update).
          tokenCount: null,
        },
      ]);
      setOptimisticTurnStartedAt(submittedAt);
      setThinking(true);
```

In the `finally` block, replace:

```tsx
              setThinking(false);
              dispatchedInitialTurnRef.current = null;
```

with:

```tsx
              setThinking(false);
              setOptimisticTurnStartedAt(null);
              dispatchedInitialTurnRef.current = null;
```

- [ ] **Step 5: Derive the generic status visibility and start timestamp**

After:

```tsx
  const turnInFlight = sendInFlight || backendTurnActive;
  const showThinking = thinking || turnInFlight;
```

add:

```tsx
  const latestUserMessageCreatedAt = useMemo(
    () => getLatestUserMessageCreatedAt(messages),
    [messages],
  );
  const showGenericStreamingStatus =
    pendingToolCall?.kind === "other" || (!pendingToolCall && showThinking);
  const activeTurnStartedAt = optimisticTurnStartedAt ?? latestUserMessageCreatedAt;
```

- [ ] **Step 6: Move the generic status out of the transcript and above the composer**

In the `StickToBottom` transcript content, replace the full conditional that currently renders `agent-thinking`:

```tsx
                {pendingToolCall?.kind === "bash" ||
                pendingToolCall?.kind === "task" ? (
                  <div
                    className="mb-6"
                    data-testid="chat-message"
                    role="group"
                    aria-label="doce replied"
                  >
                    {pendingToolCall.kind === "bash" && (
                      <BashWidget detail={pendingToolCall.detail} />
                    )}
                    {pendingToolCall.kind === "task" && (
                      <TaskWidget detail={pendingToolCall.detail} />
                    )}
                  </div>
                ) : (
                  // "other" shows the indicator even when `thinking`/
                  // send-in-flight were wiped by a reload — the trailing
                  // unpaired tool_call itself is the proof a turn is running.
                  (pendingToolCall?.kind === "other" ||
                    (!pendingToolCall && showThinking)) && (
                    <p
                      className="text-sm text-muted-foreground"
                      data-testid="agent-thinking"
                    >
                      Working…
                    </p>
                  )
                )}
```

with:

```tsx
                {(pendingToolCall?.kind === "bash" || pendingToolCall?.kind === "task") && (
                  <div
                    className="mb-6"
                    data-testid="chat-message"
                    role="group"
                    aria-label="doce replied"
                  >
                    {pendingToolCall.kind === "bash" && (
                      <BashWidget detail={pendingToolCall.detail} />
                    )}
                    {pendingToolCall.kind === "task" && (
                      <TaskWidget detail={pendingToolCall.detail} />
                    )}
                  </div>
                )}
```

After the closing `</StickToBottom>` and before the composer shell `<div data-testid="workspace-composer-shell">`, add:

```tsx
      {showGenericStreamingStatus && <StreamingStatus startedAt={activeTurnStartedAt} />}
```

Then replace the composer shell opening:

```tsx
      <div
        className="border-t border-border p-4 [view-transition-name:chat-composer]"
        data-testid="workspace-composer-shell"
      >
```

with:

```tsx
      <div
        className={cn(
          "p-4 [view-transition-name:chat-composer]",
          showGenericStreamingStatus ? "" : "border-t border-border",
        )}
        data-testid="workspace-composer-shell"
      >
```

- [ ] **Step 7: Run Workspace tests**

Run:

```bash
npx vitest run src/views/workspace/Workspace.test.tsx
```

Expected: PASS.

- [ ] **Step 8: Run focused status/workspace tests**

Run:

```bash
npx vitest run src/views/workspace/StreamingStatus.test.tsx src/views/workspace/Workspace.test.tsx
```

Expected: PASS.

- [ ] **Step 9: Commit Task 2**

Run:

```bash
git add src/views/workspace/Workspace.tsx src/views/workspace/Workspace.test.tsx
git commit -m "feat(workspace): move streaming status above composer"
```

Expected: one commit containing only `Workspace` and its test.

---

### Task 3: Final Verification

**Files:**
- Verify: `src/views/workspace/StreamingStatus.tsx`
- Verify: `src/views/workspace/Workspace.tsx`
- Verify: frontend TypeScript project
- Verify: focused workspace tests
- Verify: full frontend suite

**Interfaces:**
- Consumes all prior task outputs.
- Produces verified implementation with no additional code changes unless a verification failure points to a task-owned defect.

- [ ] **Step 1: Run TypeScript build**

Run:

```bash
npx tsc -b
```

Expected: PASS with no TypeScript errors.

- [ ] **Step 2: Run focused workspace tests**

Run:

```bash
npx vitest run src/views/workspace/StreamingStatus.test.tsx src/views/workspace/Workspace.test.tsx
```

Expected: PASS.

- [ ] **Step 3: Run full frontend suite**

Run:

```bash
npx vitest run
```

Expected: PASS. The existing jsdom `Not implemented: navigation to another Document` line may appear; it is acceptable only if the command exits 0 and all test files pass.

- [ ] **Step 4: Confirm no unintended files are staged or modified**

Run:

```bash
git status --short
```

Expected: no uncommitted task files. If this command shows unrelated files, report them without staging or reverting them.

---

## Final Review Checklist

- [ ] Generic active-turn status no longer renders inside transcript `chat-message` rows.
- [ ] Generic active-turn status renders above `workspace-composer-shell`.
- [ ] Status row bottom border is the divider when visible.
- [ ] Composer shell keeps `border-t border-border` when status is hidden.
- [ ] Visible label is `Thinking`, not `Working...`.
- [ ] Activity mark is decorative.
- [ ] Chron uses tabular numbers.
- [ ] Chron starts from optimistic user message timestamp during local sends.
- [ ] Chron starts from latest persisted user message during backend-active reload state.
- [ ] Tool calls and tool results do not reset the chron.
- [ ] `AskUserQuestion`, `Bash`, and `Task` pending widgets suppress the generic status row.
- [ ] Composer remains disabled during active turns and pending tool calls.
- [ ] No backend timing schema changes were made.
- [ ] Sidebar conversation state label remains unchanged.

