# Sticky User Message Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Render each user request as a Mesh-style sticky, clipped turn anchor at the top of the workspace chat while its assistant/tool output scrolls underneath.

**Architecture:** Keep backend data and IPC unchanged. Add a render-only transcript grouping helper, extract the existing user-message bubble into a reusable component, then render grouped turns through a `TranscriptTurn` component that owns the sticky user header and latest pending `Bash`/`Task` widgets. Preserve `StickToBottom` as the only autoscroll owner and rely on CSS sticky, not scroll listeners, for user-message replacement.

**Tech Stack:** React 19 + TypeScript, Tailwind v4, `use-stick-to-bottom`, vitest + Testing Library, Vite.

**Spec:** `docs/superpowers/specs/2026-07-09-sticky-user-message-design.md`

## Global Constraints

- No backend schema changes.
- Do not change how messages are persisted, loaded, or typed in `src/lib/ipc.ts`.
- `groupTranscriptTurns(messages)` is render-only and exported for tests.
- A turn starts at each `role === "user"` row and owns following rows until the next user row.
- Assistant-only rows before the first user row render as standalone assistant turns.
- Sticky user message replacement is pure CSS, with no scroll listener or active-turn observer.
- Default user bubble clipped height is `84px`.
- Expanded user bubble max height is `50vh` with internal scrolling.
- Expanded/collapsed state is local only and is not persisted.
- Use `overflow-x-clip`, not `overflow-x-hidden`, on the transcript content wrapper so sticky resolves against the real scroll container.
- The workspace scroller remains owned by `StickToBottom`.
- Generic `Working` status remains outside the transcript, above the composer.
- Pending `AskUserQuestion` remains composer-only.
- Pending `Bash` and `Task` widgets render inside the latest transcript turn.

---

### Task 1: Transcript turn grouping helper

**Files:**

- Create: `src/views/workspace/transcriptTurns.ts`
- Create: `src/views/workspace/transcriptTurns.test.ts`

**Interfaces:**

- Consumes: `Message` from `src/lib/ipc.ts`.
- Produces:
  - `export interface TranscriptTurn { id: string; user: Message | null; rows: Message[] }`
  - `export function groupTranscriptTurns(messages: Message[]): TranscriptTurn[]`

- [ ] **Step 1: Write the failing test**

Create `src/views/workspace/transcriptTurns.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import type { Message } from "@/lib/ipc";
import { groupTranscriptTurns } from "./transcriptTurns";

function message(overrides: Partial<Message> & { id: string }): Message {
  return {
    id: overrides.id,
    conversationId: "conv-1",
    role: "assistant",
    contentType: "text",
    content: overrides.id,
    toolName: null,
    createdAt: 1,
    durationMs: null,
    tokenCount: null,
    ...overrides,
  };
}

describe("groupTranscriptTurns", () => {
  it("groups each user message with following rows until the next user message", () => {
    const u1 = message({ id: "u1", role: "user", content: "first request" });
    const a1 = message({ id: "a1", role: "assistant", content: "first answer" });
    const tool = message({
      id: "tr1",
      role: "assistant",
      contentType: "tool_result",
      toolName: "Read",
      content: JSON.stringify({
        toolName: "Read",
        filePath: "notes.txt",
        offset: null,
        limit: null,
        outcome: { ok: true, content: "hello", truncated: false },
      }),
    });
    const u2 = message({ id: "u2", role: "user", content: "second request" });
    const a2 = message({ id: "a2", role: "assistant", content: "second answer" });

    const turns = groupTranscriptTurns([u1, a1, tool, u2, a2]);

    expect(turns).toHaveLength(2);
    expect(turns[0]).toEqual({ id: "u1", user: u1, rows: [a1, tool] });
    expect(turns[1]).toEqual({ id: "u2", user: u2, rows: [a2] });
  });

  it("keeps assistant-only rows before the first user message in a standalone turn", () => {
    const intro = message({ id: "a0", role: "assistant", content: "welcome" });
    const u1 = message({ id: "u1", role: "user", content: "request" });
    const a1 = message({ id: "a1", role: "assistant", content: "answer" });

    const turns = groupTranscriptTurns([intro, u1, a1]);

    expect(turns).toHaveLength(2);
    expect(turns[0]).toEqual({ id: "a0", user: null, rows: [intro] });
    expect(turns[1]).toEqual({ id: "u1", user: u1, rows: [a1] });
  });

  it("keeps plan-machine rows in their owning turn so MessageContent can filter them", () => {
    const u1 = message({ id: "u1", role: "user", content: "make a plan" });
    const planTool = message({
      id: "tc1",
      role: "assistant",
      contentType: "tool_call",
      toolName: "CreatePlan",
      content: JSON.stringify({ plan: true }),
    });

    const turns = groupTranscriptTurns([u1, planTool]);

    expect(turns).toEqual([{ id: "u1", user: u1, rows: [planTool] }]);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run src/views/workspace/transcriptTurns.test.ts`

Expected: FAIL with an import error containing `Failed to resolve import "./transcriptTurns"`.

- [ ] **Step 3: Write minimal implementation**

Create `src/views/workspace/transcriptTurns.ts`:

```ts
import type { Message } from "@/lib/ipc";

export interface TranscriptTurn {
  id: string;
  user: Message | null;
  rows: Message[];
}

export function groupTranscriptTurns(messages: Message[]): TranscriptTurn[] {
  const turns: TranscriptTurn[] = [];
  let current: TranscriptTurn | null = null;

  for (const message of messages) {
    if (message.role === "user") {
      current = {
        id: message.id,
        user: message,
        rows: [],
      };
      turns.push(current);
      continue;
    }

    if (!current) {
      current = {
        id: message.id,
        user: null,
        rows: [],
      };
      turns.push(current);
    }

    current.rows.push(message);
  }

  return turns;
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run src/views/workspace/transcriptTurns.test.ts`

Expected: PASS, 3 tests passed.

- [ ] **Step 5: Commit**

```bash
git add src/views/workspace/transcriptTurns.ts src/views/workspace/transcriptTurns.test.ts
git commit -m "feat(workspace): group transcript rows into turns"
```

---

### Task 2: Reusable user message bubble

**Files:**

- Create: `src/components/UserMessageBubble.tsx`
- Create: `src/components/UserMessageBubble.test.tsx`
- Modify: `src/components/MarkdownPreview.tsx`
- Modify: `src/components/MessageContent.tsx`
- Test: `src/components/MessageContent.test.tsx`

**Interfaces:**

- Consumes: `Message`, `UserMessageContent`, `MarkdownPreview`, `formatTokenCount`, `cn`.
- Produces:
  - `MarkdownPreview` accepts an optional `testId?: string` prop and maps it to `data-testid`
  - `export interface UserMessageBubbleProps { message: Message; bubbleClassName?: string; tokenMeterClassName?: string }`
  - `export default function UserMessageBubble(props: UserMessageBubbleProps): JSX.Element`

- [ ] **Step 1: Write the failing component test**

Create `src/components/UserMessageBubble.test.tsx`:

```tsx
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { Message } from "@/lib/ipc";
import UserMessageBubble from "./UserMessageBubble";

function userMessage(overrides: Partial<Message> = {}): Message {
  return {
    id: "u1",
    conversationId: "conv-1",
    role: "user",
    contentType: "text",
    content: "hello **there**",
    toolName: null,
    createdAt: 1,
    durationMs: null,
    tokenCount: null,
    ...overrides,
  };
}

describe("UserMessageBubble", () => {
  it("renders text user content through the markdown bubble", () => {
    render(<UserMessageBubble message={userMessage()} />);

    const bubble = screen.getByTestId("user-message-bubble");
    expect(bubble).toHaveTextContent("hello");
    expect(bubble).toHaveTextContent("there");
    expect(bubble).toHaveClass("rounded-lg", "bg-muted", "p-3", "text-foreground");
  });

  it("applies caller classes to the visual bubble without moving the token meter", () => {
    render(
      <UserMessageBubble
        message={userMessage({ tokenCount: 4200 })}
        bubbleClassName="max-h-[84px] overflow-hidden"
      />,
    );

    expect(screen.getByTestId("user-message-bubble")).toHaveClass(
      "max-h-[84px]",
      "overflow-hidden",
    );
    expect(screen.getByTestId("token-meter")).toHaveTextContent("↑ 4.2k tokens");
    expect(screen.getByTestId("token-meter")).not.toHaveClass("max-h-[84px]");
  });

  it("renders rich user content with the same bubble test id", () => {
    render(
      <UserMessageBubble
        message={userMessage({
          contentType: "rich_text",
          content: JSON.stringify({
            segments: [{ type: "text", text: "rich hello" }],
          }),
        })}
      />,
    );

    expect(screen.getByTestId("user-message-bubble")).toHaveTextContent("rich hello");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run src/components/UserMessageBubble.test.tsx`

Expected: FAIL with an import error containing `Failed to resolve import "./UserMessageBubble"`.

- [ ] **Step 3: Add test-id support to `MarkdownPreview`**

In `src/components/MarkdownPreview.tsx`, replace the file with:

```tsx
import ReactMarkdown from "react-markdown";
import { cn } from "@/lib/cn";

interface MarkdownPreviewProps {
  children: string;
  className?: string;
  testId?: string;
}

export default function MarkdownPreview({ children, className, testId }: MarkdownPreviewProps) {
  return (
    <div
      className={cn("prose prose-sm dark:prose-invert max-w-none", className)}
      data-testid={testId}
    >
      <ReactMarkdown>{children}</ReactMarkdown>
    </div>
  );
}
```

- [ ] **Step 4: Create the reusable component**

Create `src/components/UserMessageBubble.tsx`:

```tsx
import MarkdownPreview from "@/components/MarkdownPreview";
import { formatTokenCount } from "@/lib/formatTokenCount";
import { cn } from "@/lib/cn";
import type { Message } from "@/lib/ipc";
import UserMessageContent from "@/views/chat/rich-input/UserMessageContent";

export interface UserMessageBubbleProps {
  message: Message;
  bubbleClassName?: string;
  tokenMeterClassName?: string;
}

export default function UserMessageBubble({
  message,
  bubbleClassName,
  tokenMeterClassName,
}: UserMessageBubbleProps) {
  return (
    <>
      {message.contentType === "rich_text" ? (
        <div
          className={cn(
            "prose prose-sm dark:prose-invert max-w-none rounded-lg bg-muted p-3 text-foreground",
            bubbleClassName,
          )}
          data-testid="user-message-bubble"
        >
          <UserMessageContent content={message.content} />
        </div>
      ) : (
        <MarkdownPreview
          className={cn("rounded-lg bg-muted p-3 text-foreground", bubbleClassName)}
          testId="user-message-bubble"
        >
          {message.content}
        </MarkdownPreview>
      )}
      {message.tokenCount != null && (
        <p
          className={cn("mt-1 text-xs text-muted-foreground", tokenMeterClassName)}
          data-testid="token-meter"
        >
          ↑ {formatTokenCount(message.tokenCount)} tokens
        </p>
      )}
    </>
  );
}
```

- [ ] **Step 5: Update `MessageContent` to use `UserMessageBubble`**

In `src/components/MessageContent.tsx`, remove this import:

```tsx
import UserMessageContent from "@/views/chat/rich-input/UserMessageContent";
```

Then add this import:

```tsx
import UserMessageBubble from "@/components/UserMessageBubble";
```

Replace the entire `if (m.role === "user")` branch with:

```tsx
if (m.role === "user") {
  return (
    <div className="mb-6" data-testid="chat-message" role="group" aria-label="You said">
      <UserMessageBubble message={m} />
    </div>
  );
}
```

- [ ] **Step 6: Run focused tests**

Run: `npx vitest run src/components/UserMessageBubble.test.tsx src/components/MessageContent.test.tsx`

Expected: PASS. `UserMessageBubble.test.tsx` has 3 passing tests, and `MessageContent.test.tsx` continues passing its full file.

- [ ] **Step 7: Commit**

```bash
git add src/components/MarkdownPreview.tsx src/components/UserMessageBubble.tsx src/components/UserMessageBubble.test.tsx src/components/MessageContent.tsx src/components/MessageContent.test.tsx
git commit -m "refactor(chat): extract user message bubble"
```

---

### Task 3: Sticky user message component

**Files:**

- Create: `src/views/workspace/StickyUserMessage.tsx`
- Create: `src/views/workspace/StickyUserMessage.test.tsx`

**Interfaces:**

- Consumes: `UserMessageBubble`, `Message`, `cn`.
- Produces:
  - `export interface StickyUserMessageProps { message: Message; onScrollToTurn?: () => void }`
  - `export default function StickyUserMessage(props: StickyUserMessageProps): JSX.Element`

- [ ] **Step 1: Write the failing sticky component test**

Create `src/views/workspace/StickyUserMessage.test.tsx`:

```tsx
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { Message } from "@/lib/ipc";
import StickyUserMessage from "./StickyUserMessage";

function userMessage(content = "line one\n\nline two"): Message {
  return {
    id: "u1",
    conversationId: "conv-1",
    role: "user",
    contentType: "text",
    content,
    toolName: null,
    createdAt: 1,
    durationMs: null,
    tokenCount: 12,
  };
}

describe("StickyUserMessage", () => {
  it("renders as a sticky chat message with a clipped user bubble by default", () => {
    render(<StickyUserMessage message={userMessage()} />);

    const shell = screen.getByTestId("chat-message");
    const focusTarget = screen.getByTestId("sticky-user-message-bubble");
    const bubble = screen.getByTestId("user-message-bubble");

    expect(shell).toHaveAttribute("data-sticky-user-message", "true");
    expect(shell).toHaveClass("sticky", "top-4", "z-40");
    expect(shell).toHaveAttribute("aria-label", "You said");
    expect(focusTarget).toHaveAttribute("tabindex", "0");
    expect(bubble).toHaveClass("max-h-[84px]", "overflow-hidden");
    expect(bubble.className).toContain("[mask-image:linear-gradient");
    expect(screen.getByTestId("token-meter")).toHaveTextContent("↑ 12 tokens");
  });

  it("expands on click, calls onScrollToTurn, and collapses on blur", async () => {
    const onScrollToTurn = vi.fn();
    render(<StickyUserMessage message={userMessage()} onScrollToTurn={onScrollToTurn} />);

    const user = userEvent.setup();
    const focusTarget = screen.getByTestId("sticky-user-message-bubble");

    await user.click(focusTarget);

    expect(onScrollToTurn).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId("user-message-bubble")).toHaveClass("max-h-[50vh]", "overflow-auto");

    fireEvent.blur(focusTarget, { relatedTarget: null });

    expect(screen.getByTestId("user-message-bubble")).toHaveClass(
      "max-h-[84px]",
      "overflow-hidden",
    );
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run src/views/workspace/StickyUserMessage.test.tsx`

Expected: FAIL with an import error containing `Failed to resolve import "./StickyUserMessage"`.

- [ ] **Step 3: Implement the sticky component**

Create `src/views/workspace/StickyUserMessage.tsx`:

```tsx
import { useState, type FocusEvent } from "react";
import UserMessageBubble from "@/components/UserMessageBubble";
import { cn } from "@/lib/cn";
import type { Message } from "@/lib/ipc";

export interface StickyUserMessageProps {
  message: Message;
  onScrollToTurn?: () => void;
}

export default function StickyUserMessage({ message, onScrollToTurn }: StickyUserMessageProps) {
  const [expanded, setExpanded] = useState(false);

  const expandAndAnchor = () => {
    setExpanded(true);
    onScrollToTurn?.();
  };

  const collapseWhenFocusLeaves = (event: FocusEvent<HTMLDivElement>) => {
    const nextTarget = event.relatedTarget;
    if (nextTarget instanceof Node && event.currentTarget.contains(nextTarget)) return;
    setExpanded(false);
  };

  return (
    <div
      className="sticky top-4 z-40 mb-8 sm:mb-6"
      data-testid="chat-message"
      data-sticky-user-message="true"
      role="group"
      aria-label="You said"
    >
      <div
        tabIndex={0}
        className="outline-none"
        data-testid="sticky-user-message-bubble"
        onClick={expandAndAnchor}
        onFocus={expandAndAnchor}
        onBlur={collapseWhenFocusLeaves}
      >
        <UserMessageBubble
          message={message}
          bubbleClassName={cn(
            "cursor-pointer transition-[max-height,opacity] duration-300 ease-out",
            expanded
              ? "max-h-[50vh] overflow-auto opacity-100"
              : "max-h-[84px] overflow-hidden opacity-99 [mask-image:linear-gradient(to_bottom,black_calc(100%-16px),transparent)]",
          )}
        />
      </div>
    </div>
  );
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run src/views/workspace/StickyUserMessage.test.tsx`

Expected: PASS, 2 tests passed.

- [ ] **Step 5: Commit**

```bash
git add src/views/workspace/StickyUserMessage.tsx src/views/workspace/StickyUserMessage.test.tsx
git commit -m "feat(workspace): add sticky user message"
```

---

### Task 4: Transcript turn renderer

**Files:**

- Create: `src/views/workspace/TranscriptTurn.tsx`
- Create: `src/views/workspace/TranscriptTurn.test.tsx`

**Interfaces:**

- Consumes:
  - `TranscriptTurn` from `src/views/workspace/transcriptTurns.ts`
  - `StickyUserMessage`
  - `MessageContent`
  - `BashWidget`
  - `TaskWidget`
  - `BashDetail` and `TaskDetail` from `src/lib/ipc.ts`
- Produces:
  - `export type PendingTurnWidget = { kind: "bash"; detail: BashDetail } | { kind: "task"; detail: TaskDetail }`
  - `export interface TranscriptTurnProps { turn: TranscriptTurn; isLastTurn?: boolean; pendingWidget?: PendingTurnWidget | null; error?: string | null }`
  - `export default function TranscriptTurn(props: TranscriptTurnProps): JSX.Element`

- [ ] **Step 1: Write the failing turn renderer test**

Create `src/views/workspace/TranscriptTurn.test.tsx`:

```tsx
import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { BashDetail, Message } from "@/lib/ipc";
import type { TranscriptTurn as TranscriptTurnModel } from "./transcriptTurns";
import TranscriptTurn from "./TranscriptTurn";

function message(overrides: Partial<Message> & { id: string }): Message {
  return {
    id: overrides.id,
    conversationId: "conv-1",
    role: "assistant",
    contentType: "text",
    content: overrides.id,
    toolName: null,
    createdAt: 1,
    durationMs: null,
    tokenCount: null,
    ...overrides,
  };
}

function turn(overrides: Partial<TranscriptTurnModel>): TranscriptTurnModel {
  return {
    id: "u1",
    user: message({ id: "u1", role: "user", content: "run the tests" }),
    rows: [message({ id: "a1", role: "assistant", content: "done" })],
    ...overrides,
  };
}

describe("TranscriptTurn", () => {
  it("renders a sticky user header above the owning assistant rows", () => {
    render(<TranscriptTurn turn={turn({})} />);

    const transcriptTurn = screen.getByTestId("transcript-turn");
    const stickyBackground = screen.getByTestId("sticky-user-background");
    const stickyMessage = transcriptTurn.querySelector('[data-sticky-user-message="true"]');

    expect(stickyBackground).toHaveClass("sticky", "top-0", "bg-background");
    expect(stickyMessage).not.toBeNull();
    expect(stickyMessage).toHaveTextContent("run the tests");
    expect(within(transcriptTurn).getByText("done")).toBeInTheDocument();
  });

  it("renders assistant-only turns without sticky user chrome", () => {
    render(
      <TranscriptTurn
        turn={turn({
          id: "a0",
          user: null,
          rows: [message({ id: "a0", role: "assistant", content: "welcome" })],
        })}
      />,
    );

    expect(screen.getByTestId("transcript-turn")).toHaveTextContent("welcome");
    expect(screen.queryByTestId("sticky-user-background")).not.toBeInTheDocument();
    expect(screen.queryByTestId("sticky-user-message-bubble")).not.toBeInTheDocument();
  });

  it("renders pending Bash and local error content inside the turn body", () => {
    const pendingBash: BashDetail = {
      toolName: "Bash",
      command: "cargo test --lib",
      timeoutMs: null,
    };

    render(
      <TranscriptTurn
        turn={turn({})}
        pendingWidget={{ kind: "bash", detail: pendingBash }}
        error="send failed"
      />,
    );

    const body = screen.getByTestId("transcript-turn-body");
    expect(within(body).getByTestId("bash-widget")).toBeInTheDocument();
    expect(within(body).getByText("send failed")).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run src/views/workspace/TranscriptTurn.test.tsx`

Expected: FAIL with an import error containing `Failed to resolve import "./TranscriptTurn"`.

- [ ] **Step 3: Implement `TranscriptTurn`**

Create `src/views/workspace/TranscriptTurn.tsx`:

```tsx
import { useRef } from "react";
import MessageContent from "@/components/MessageContent";
import BashWidget from "@/views/chat/tool-widgets/BashWidget";
import TaskWidget from "@/views/chat/tool-widgets/TaskWidget";
import StickyUserMessage from "@/views/workspace/StickyUserMessage";
import { cn } from "@/lib/cn";
import type { BashDetail, TaskDetail } from "@/lib/ipc";
import type { TranscriptTurn as TranscriptTurnModel } from "./transcriptTurns";

export type PendingTurnWidget =
  | { kind: "bash"; detail: BashDetail }
  | { kind: "task"; detail: TaskDetail };

export interface TranscriptTurnProps {
  turn: TranscriptTurnModel;
  isLastTurn?: boolean;
  pendingWidget?: PendingTurnWidget | null;
  error?: string | null;
}

export default function TranscriptTurn({
  turn,
  isLastTurn = false,
  pendingWidget = null,
  error = null,
}: TranscriptTurnProps) {
  const turnRef = useRef<HTMLDivElement | null>(null);

  const scrollToTurn = () => {
    turnRef.current?.scrollIntoView({ behavior: "smooth", block: "start" });
  };

  return (
    <div
      ref={turnRef}
      className={cn("flex flex-col pb-2 sm:pb-2", !turn.user && "pt-8 sm:pt-6")}
      data-testid="transcript-turn"
      data-turn-id={turn.id}
      data-last-turn={isLastTurn ? "true" : "false"}
    >
      {turn.user && (
        <>
          <div
            className="sticky top-0 z-40 h-4 w-full bg-background"
            data-testid="sticky-user-background"
            aria-hidden="true"
          />
          <StickyUserMessage message={turn.user} onScrollToTurn={scrollToTurn} />
        </>
      )}
      <div className="min-w-0" data-testid="transcript-turn-body">
        {turn.rows.map((message) => (
          <MessageContent
            key={message.id}
            message={message}
            showTimer={
              message.role === "assistant" &&
              message.contentType === "text" &&
              message.durationMs != null
            }
          />
        ))}
        {pendingWidget && (
          <div className="mb-6" data-testid="chat-message" role="group" aria-label="doce replied">
            {pendingWidget.kind === "bash" && <BashWidget detail={pendingWidget.detail} />}
            {pendingWidget.kind === "task" && <TaskWidget detail={pendingWidget.detail} />}
          </div>
        )}
        {error && (
          <div
            className="mb-6 rounded-lg bg-destructive/10 p-3 text-sm text-destructive"
            data-testid="workspace-error"
          >
            {error}
          </div>
        )}
      </div>
    </div>
  );
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run src/views/workspace/TranscriptTurn.test.tsx`

Expected: PASS, 3 tests passed.

- [ ] **Step 5: Commit**

```bash
git add src/views/workspace/TranscriptTurn.tsx src/views/workspace/TranscriptTurn.test.tsx
git commit -m "feat(workspace): render transcript turns"
```

---

### Task 5: Workspace transcript integration

**Files:**

- Modify: `src/views/workspace/Workspace.tsx`
- Modify: `src/views/workspace/Workspace.test.tsx`
- Test: all new workspace/component tests from Tasks 1-4

**Interfaces:**

- Consumes:
  - `groupTranscriptTurns(messages: Message[]): TranscriptTurn[]`
  - `TranscriptTurn`
  - `PendingTurnWidget`
  - existing `pendingToolCall` derivation in `Workspace.tsx`
- Produces:
  - `data-testid="workspace-transcript-content"` on the `StickToBottom` observed content wrapper
  - `data-testid="last-transcript-turn-viewport"` on the last-turn viewport-height wrapper
  - Pending `Bash`/`Task` widgets rendered inside the latest `TranscriptTurn`

- [ ] **Step 1: Write failing workspace tests**

Append these tests inside the existing `describe("Workspace (006-chat-empty-state: conversationId-driven agent view)")` block in `src/views/workspace/Workspace.test.tsx`:

```tsx
it("renders user messages as sticky turn anchors that own following assistant rows", async () => {
  vi.mocked(commands.listMessages).mockResolvedValue([
    messageFixture("u1", "first request", 1),
    {
      id: "a1",
      conversationId: "conv-1",
      role: "assistant",
      contentType: "text",
      content: "first answer",
      toolName: null,
      createdAt: 2,
      durationMs: null,
      tokenCount: null,
    },
    messageFixture("u2", "second request", 3),
    {
      id: "a2",
      conversationId: "conv-1",
      role: "assistant",
      contentType: "text",
      content: "second answer",
      toolName: null,
      createdAt: 4,
      durationMs: null,
      tokenCount: null,
    },
  ]);

  render(<Workspace conversationId="conv-1" />);

  await screen.findByText("second answer");

  const turns = screen.getAllByTestId("transcript-turn");
  expect(turns).toHaveLength(2);
  expect(turns[0]).toHaveTextContent("first request");
  expect(turns[0]).toHaveTextContent("first answer");
  expect(turns[0]).not.toHaveTextContent("second request");
  expect(turns[1]).toHaveTextContent("second request");
  expect(turns[1]).toHaveTextContent("second answer");
  expect(document.querySelectorAll('[data-sticky-user-message="true"]')).toHaveLength(2);
});

it("keeps the transcript content wrapper sticky-safe and makes the latest turn viewport-height", async () => {
  vi.mocked(commands.listMessages).mockResolvedValue([messageFixture("u1", "latest request", 1)]);

  render(<Workspace conversationId="conv-1" />);

  await screen.findByText("latest request");

  expect(screen.getByTestId("workspace-scroll-container")).toHaveClass("[container-type:size]");
  expect(screen.getByTestId("workspace-transcript-content")).toHaveClass("overflow-x-clip");
  expect(screen.getByTestId("last-transcript-turn-viewport")).toHaveClass("min-h-[100cqh]");
});
```

Then update the existing pending Bash test by adding this assertion after `expect(screen.getByTestId("bash-command")).toHaveTextContent("cargo test --lib");`:

```tsx
expect(status.closest('[data-testid="transcript-turn"]')).toHaveTextContent("run the tests");
```

Update the existing pending Task test by adding this assertion after `expect(status).toHaveTextContent(/running/i);`:

```tsx
expect(status.closest('[data-testid="transcript-turn"]')).toHaveTextContent("investigate the bug");
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run src/views/workspace/Workspace.test.tsx --testNamePattern "sticky turn anchors|sticky-safe|pending Bash|pending Task"`

Expected: FAIL because `transcript-turn`, `workspace-transcript-content`, and `last-transcript-turn-viewport` are not rendered yet, and pending widgets are not inside a turn.

- [ ] **Step 3: Update Workspace imports**

In `src/views/workspace/Workspace.tsx`, remove these imports:

```tsx
import BashWidget from "@/views/chat/tool-widgets/BashWidget";
import TaskWidget from "@/views/chat/tool-widgets/TaskWidget";
```

Add these imports:

```tsx
import TranscriptTurn, { type PendingTurnWidget } from "@/views/workspace/TranscriptTurn";
import { groupTranscriptTurns } from "@/views/workspace/transcriptTurns";
```

- [ ] **Step 4: Derive transcript turns and pending turn widget**

In `Workspace.tsx`, after the declaration whose left side is `const activeTurnStartedAt =`, add:

```tsx
const transcriptTurns = useMemo(() => groupTranscriptTurns(messages), [messages]);
const previousTurns = transcriptTurns.slice(0, -1);
const lastTurn = transcriptTurns.at(-1) ?? null;
const pendingTurnWidget: PendingTurnWidget | null =
  pendingToolCall?.kind === "bash" || pendingToolCall?.kind === "task" ? pendingToolCall : null;
```

- [ ] **Step 5: Replace the flat transcript map**

In `Workspace.tsx`, replace the current `StickToBottom` scroll body content:

```tsx
<div ref={contentRef} className="mx-auto max-w-3xl">
  {messages.map((m) => (
    <MessageContent
      key={m.id}
      message={m}
      showTimer={m.role === "assistant" && m.contentType === "text" && m.durationMs != null}
    />
  ))}
  {(pendingToolCall?.kind === "bash" || pendingToolCall?.kind === "task") && (
    <div className="mb-6" data-testid="chat-message" role="group" aria-label="doce replied">
      {pendingToolCall.kind === "bash" && <BashWidget detail={pendingToolCall.detail} />}
      {pendingToolCall.kind === "task" && <TaskWidget detail={pendingToolCall.detail} />}
    </div>
  )}
  {error && (
    <div
      className="mb-6 rounded-lg bg-destructive/10 p-3 text-sm text-destructive"
      data-testid="workspace-error"
    >
      {error}
    </div>
  )}
</div>
```

with:

```tsx
<div ref={contentRef} className="overflow-x-clip" data-testid="workspace-transcript-content">
  <div className="mx-auto max-w-3xl">
    {previousTurns.map((turn) => (
      <TranscriptTurn key={turn.id} turn={turn} />
    ))}
  </div>
  {lastTurn ? (
    <div className="mx-auto min-h-[100cqh] max-w-3xl" data-testid="last-transcript-turn-viewport">
      <TranscriptTurn
        key={lastTurn.id}
        turn={lastTurn}
        isLastTurn
        pendingWidget={pendingTurnWidget}
        error={error}
      />
    </div>
  ) : (
    error && (
      <div className="mx-auto max-w-3xl">
        <div
          className="mb-6 rounded-lg bg-destructive/10 p-3 text-sm text-destructive"
          data-testid="workspace-error"
        >
          {error}
        </div>
      </div>
    )
  )}
</div>
```

- [ ] **Step 6: Add size-container support to the scroll container**

In the same file, change the scroll container class from:

```tsx
className = "h-full overflow-y-auto p-4";
```

to:

```tsx
className = "h-full overflow-y-auto p-4 [container-type:size]";
```

- [ ] **Step 7: Remove unused imports**

Run: `npx oxlint src/views/workspace/Workspace.tsx`

Expected: PASS. If it reports unused imports for `MessageContent`, `BashWidget`, or `TaskWidget`, remove those imports from `Workspace.tsx`.

- [ ] **Step 8: Run focused tests**

Run: `npx vitest run src/views/workspace/Workspace.test.tsx --testNamePattern "sticky turn anchors|sticky-safe|pending Bash|pending Task"`

Expected: PASS for the selected tests.

- [ ] **Step 9: Run affected workspace/component tests**

Run:

```bash
npx vitest run \
  src/views/workspace/transcriptTurns.test.ts \
  src/components/UserMessageBubble.test.tsx \
  src/views/workspace/StickyUserMessage.test.tsx \
  src/views/workspace/TranscriptTurn.test.tsx \
  src/components/MessageContent.test.tsx \
  src/views/workspace/Workspace.test.tsx
```

Expected: PASS for all listed files.

- [ ] **Step 10: Commit**

```bash
git add src/views/workspace/Workspace.tsx src/views/workspace/Workspace.test.tsx
git commit -m "feat(workspace): render sticky transcript turns"
```

---

### Task 6: Final verification

**Files:**

- No planned file edits.

**Interfaces:**

- Consumes the complete implementation from Tasks 1-5.
- Produces final evidence that the sticky transcript feature is integrated without breaking the chat UI.

- [ ] **Step 1: Run all affected frontend tests**

Run:

```bash
npx vitest run \
  src/views/workspace/transcriptTurns.test.ts \
  src/components/UserMessageBubble.test.tsx \
  src/views/workspace/StickyUserMessage.test.tsx \
  src/views/workspace/TranscriptTurn.test.tsx \
  src/components/MessageContent.test.tsx \
  src/views/workspace/Workspace.test.tsx \
  src/views/workspace/StreamingStatus.test.tsx
```

Expected: PASS for all listed files.

- [ ] **Step 2: Run the frontend build**

Run: `npm run build`

Expected: PASS with `tsc -b` and `vite build` both completing successfully.

- [ ] **Step 3: Run browser verification in the Tauri app**

Run: `npm run tauri dev`

Manual verification steps:

1. Open or create a conversation with at least three user turns.
2. Ensure the first two turns have enough assistant/tool output to require vertical scrolling.
3. Scroll down through the transcript.
4. Confirm the current user message sticks near the top of the transcript.
5. Continue scrolling until the next user message reaches the top.
6. Confirm the next user message replaces the previous sticky message.
7. Click a long sticky user message.
8. Confirm it expands inline up to a bounded height and scrolls internally when needed.
9. Blur the sticky message.
10. Confirm it returns to the compact clipped state.
11. Start a turn that shows the generic active state.
12. Confirm `Working` still appears above the composer, not inside the transcript.

Expected: all 12 checks match the stated behavior.

- [ ] **Step 4: Check final git state**

Run: `git status -sb`

Expected: no uncommitted changes.
