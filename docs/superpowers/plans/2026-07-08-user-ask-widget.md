# UserAskWidget Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move the live, unanswered `AskUserQuestion` prompt out of the message scroll list and into the chat composer slot (replacing `RichInput` while pending), with a close (✕) affordance that reveals a free-text fallback whose typed answer resolves the question.

**Architecture:** Split the current `AskUserQuestionWidget` by concern. `UserAskWidget` (new) owns the live, interactive question — option buttons, multi-select confirm, close, and the `RichInput`-backed free-text fallback — and renders in `Workspace.tsx`'s composer shell. `AskUserQuestionWidget` (existing) shrinks to only the read-only "already answered" rendering used in message history, plus a client-side "You chose" vs. "You replied" wording heuristic. No backend changes: `answer_user_question(questionId, answer: string[])` already accepts arbitrary strings.

**Tech Stack:** React + TypeScript, Vitest + `@testing-library/react` + `@testing-library/user-event`, Tailwind classes, `@phosphor-icons/react` for icons.

## Global Constraints

- No backend/Rust changes anywhere in this plan — `answer_user_question`'s signature, `PendingQuestions`, and the persisted `tool_result` shape are untouched.
- A free-text answer calls `commands.answerUserQuestion(questionId, [content])` — only the flat `content` string; any `richContent` (attachments/skill chips) from `RichInput`'s `onSubmit` is ignored.
- The answered-wording heuristic (`AskUserQuestionWidget`) is a pure client-side computation over `detail.answer` vs. `detail.options` — no new field on `AskUserQuestionDetail`, no backend change.
- Follow this codebase's existing conventions: Vitest + Testing Library, `data-testid` for element targeting, `vi.mock("@/lib/ipc", ...)` replacing the whole module in widget-level tests.
- Test command for a single file: `npx vitest run <path>`. Full suite: `npm run test`.

---

### Task 1: Create `UserAskWidget`

**Files:**
- Create: `src/views/chat/tool-widgets/UserAskWidget.tsx`
- Create: `src/views/chat/tool-widgets/UserAskWidget.test.tsx`

**Interfaces:**
- Consumes: `commands.answerUserQuestion(questionId: string, answer: string[]): Promise<void>` and `type AskUserQuestionDetail` from `@/lib/ipc`; default-exported `RichInput` from `@/views/chat/rich-input/RichInput` (props used: `onSubmit: (content: string, richContent?: RichMessageContent) => void`, `skillsEnabled: boolean`, `disabled: boolean`, `placeholder: string`, `inputTestId?: string`, `submitTestId?: string`); `Button` from `@/components/ui/button` (`variant`, `size`, `disabled`, `onClick`, `title`, `data-testid`, `aria-label`, `className`); `XIcon` from `@phosphor-icons/react`.
- Produces: default-exported `UserAskWidget({ detail, initialMode }: { detail: AskUserQuestionDetail; initialMode?: "options" | "text" })`. `data-testid`s: `user-ask-widget` (outer container, either mode), `question-close`, `multi-select-indicator`, `question-submit`, `question-back-to-options`, `question-answer-input`, `question-answer-send`. This is the component Task 3 renders in `Workspace.tsx`'s composer slot, and the component Task 4 previews (via `initialMode`) in `WidgetGallery.tsx`.

- [ ] **Step 1: Write the failing test**

Create `src/views/chat/tool-widgets/UserAskWidget.test.tsx`:

```tsx
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import UserAskWidget from "./UserAskWidget";
import { commands } from "@/lib/ipc";
import type { AskUserQuestionDetail } from "@/lib/ipc";

vi.mock("@/lib/ipc", () => ({
  commands: {
    answerUserQuestion: vi.fn(),
  },
}));

const SINGLE: AskUserQuestionDetail = {
  toolName: "AskUserQuestion",
  questionId: "q1",
  header: "Pick a direction",
  question: "Which way should I go?",
  options: [
    { label: "Option A", description: "the first way" },
    { label: "Option B", description: "the second way" },
  ],
  multiSelect: false,
  answer: null,
};

const MULTI: AskUserQuestionDetail = { ...SINGLE, multiSelect: true, questionId: "q2" };

describe("UserAskWidget", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders clickable options and indicates single-select", () => {
    render(<UserAskWidget detail={SINGLE} />);
    expect(screen.getByText("Which way should I go?")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Option A/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Option B/ })).toBeInTheDocument();
    expect(screen.queryByTestId("question-submit")).not.toBeInTheDocument();
  });

  it("clicking an option in a single-select question answers immediately", async () => {
    vi.mocked(commands.answerUserQuestion).mockResolvedValue(undefined);
    render(<UserAskWidget detail={SINGLE} />);

    await userEvent.click(screen.getByRole("button", { name: /Option A/ }));

    expect(commands.answerUserQuestion).toHaveBeenCalledWith("q1", ["Option A"]);
  });

  it("indicates multi-select and requires an explicit confirm before answering", async () => {
    vi.mocked(commands.answerUserQuestion).mockResolvedValue(undefined);
    render(<UserAskWidget detail={MULTI} />);

    expect(screen.getByTestId("multi-select-indicator")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /Option A/ }));
    await userEvent.click(screen.getByRole("button", { name: /Option B/ }));
    expect(commands.answerUserQuestion).not.toHaveBeenCalled();

    await userEvent.click(screen.getByTestId("question-submit"));
    expect(commands.answerUserQuestion).toHaveBeenCalledWith("q2", ["Option A", "Option B"]);
  });

  it("closing the widget switches to a free-text answer input", async () => {
    render(<UserAskWidget detail={SINGLE} />);

    await userEvent.click(screen.getByTestId("question-close"));

    expect(screen.queryByRole("button", { name: /Option A/ })).not.toBeInTheDocument();
    expect(screen.getByTestId("question-answer-input")).toBeInTheDocument();
  });

  it("submitting free text answers the question with the full typed text", async () => {
    vi.mocked(commands.answerUserQuestion).mockResolvedValue(undefined);
    render(<UserAskWidget detail={SINGLE} />);

    await userEvent.click(screen.getByTestId("question-close"));
    const editable = screen.getByTestId("question-answer-input");
    await userEvent.click(editable);
    await userEvent.type(editable, "actually, do both{Enter}");

    expect(commands.answerUserQuestion).toHaveBeenCalledWith("q1", ["actually, do both"]);
  });

  it("'back to options' returns from the free-text input to the option buttons", async () => {
    render(<UserAskWidget detail={SINGLE} />);

    await userEvent.click(screen.getByTestId("question-close"));
    await userEvent.click(screen.getByTestId("question-back-to-options"));

    expect(screen.getByRole("button", { name: /Option A/ })).toBeInTheDocument();
    expect(screen.queryByTestId("question-answer-input")).not.toBeInTheDocument();
  });

  it("initialMode='text' starts directly in the free-text fallback (used by WidgetGallery)", () => {
    render(<UserAskWidget detail={SINGLE} initialMode="text" />);

    expect(screen.getByTestId("question-answer-input")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Option A/ })).not.toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `npx vitest run src/views/chat/tool-widgets/UserAskWidget.test.tsx`
Expected: FAIL — `Cannot find module './UserAskWidget'` (the component doesn't exist yet).

- [ ] **Step 3: Write the component**

Create `src/views/chat/tool-widgets/UserAskWidget.tsx`:

```tsx
import { useState } from "react";
import { XIcon } from "@phosphor-icons/react";
import { Button } from "@/components/ui/button";
import { commands, type AskUserQuestionDetail } from "@/lib/ipc";
import RichInput from "@/views/chat/rich-input/RichInput";

type Mode = "options" | "text";

interface UserAskWidgetProps {
  detail: AskUserQuestionDetail;
  /**
   * Seeds which mode the widget starts in. Always omitted (defaults to
   * "options") by the real caller, Workspace.tsx -- only WidgetGallery.tsx
   * passes "text", to preview the free-text fallback state without
   * requiring a click first.
   */
  initialMode?: Mode;
}

/**
 * The live, still-unanswered `AskUserQuestion` prompt (contracts/
 * tool-widgets.md), rendered in the chat composer slot in place of
 * RichInput while a question is pending (Workspace.tsx). Single-select
 * answers immediately on click; multi-select accumulates a selection and
 * requires an explicit confirm. The close (X) button swaps to a full
 * RichInput instead, whose submission answers the question with the raw
 * typed text -- for whenever the fixed option labels don't cover what the
 * user actually wants to say. Once answered, this component unmounts on
 * its own: Workspace.tsx stops rendering it as soon as the resolved
 * tool_result replaces the pending tool_call as the latest message.
 * (Compare AskUserQuestionWidget, which renders the read-only "already
 * answered" state in message history and never handles a live question.)
 */
export default function UserAskWidget({ detail, initialMode = "options" }: UserAskWidgetProps) {
  const [mode, setMode] = useState<Mode>(initialMode);
  const [selected, setSelected] = useState<string[]>([]);
  const [submitting, setSubmitting] = useState(false);

  const submit = async (answer: string[]) => {
    if (answer.length === 0 || submitting) return;
    setSubmitting(true);
    try {
      await commands.answerUserQuestion(detail.questionId, answer);
    } finally {
      setSubmitting(false);
    }
  };

  const toggleOption = (label: string) => {
    if (!detail.multiSelect) {
      submit([label]);
      return;
    }
    setSelected((prev) =>
      prev.includes(label) ? prev.filter((l) => l !== label) : [...prev, label],
    );
  };

  if (mode === "text") {
    return (
      <div
        className="rounded-lg border border-border bg-card p-3 text-sm"
        data-testid="user-ask-widget"
      >
        <div className="mb-2 flex items-center justify-between gap-2">
          <p className="text-xs text-muted-foreground">Answering: {detail.question}</p>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            disabled={submitting}
            onClick={() => setMode("options")}
            data-testid="question-back-to-options"
          >
            Back to options
          </Button>
        </div>
        <RichInput
          onSubmit={(content) => submit([content])}
          skillsEnabled={true}
          disabled={submitting}
          placeholder="Type your answer…"
          inputTestId="question-answer-input"
          submitTestId="question-answer-send"
        />
      </div>
    );
  }

  return (
    <div
      className="rounded-lg border border-border bg-card p-3 text-sm"
      data-testid="user-ask-widget"
    >
      <div className="mb-1 flex items-start gap-2">
        {detail.header && <p className="text-xs text-muted-foreground">{detail.header}</p>}
        <Button
          type="button"
          variant="ghost"
          size="icon-sm"
          className="ml-auto text-muted-foreground hover:bg-transparent"
          disabled={submitting}
          onClick={() => setMode("text")}
          aria-label="Close question"
          data-testid="question-close"
        >
          <XIcon size={14} />
        </Button>
      </div>
      <p className="mb-2 font-medium">{detail.question}</p>
      {detail.multiSelect && (
        <p className="mb-2 text-xs text-muted-foreground" data-testid="multi-select-indicator">
          Select all that apply
        </p>
      )}
      <div className="flex flex-wrap gap-2">
        {detail.options.map((option) => (
          <Button
            key={option.label}
            type="button"
            variant={selected.includes(option.label) ? "primary" : "secondary"}
            size="sm"
            disabled={submitting}
            onClick={() => toggleOption(option.label)}
            title={option.description}
          >
            {option.label}
          </Button>
        ))}
      </div>
      {detail.multiSelect && (
        <Button
          type="button"
          variant="primary"
          size="sm"
          className="mt-2"
          disabled={selected.length === 0 || submitting}
          onClick={() => submit(selected)}
          data-testid="question-submit"
        >
          Submit
        </Button>
      )}
    </div>
  );
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `npx vitest run src/views/chat/tool-widgets/UserAskWidget.test.tsx`
Expected: PASS (7 tests).

- [ ] **Step 5: Commit**

```bash
git add src/views/chat/tool-widgets/UserAskWidget.tsx src/views/chat/tool-widgets/UserAskWidget.test.tsx
git commit -m "feat(widgets): add UserAskWidget for live AskUserQuestion prompts"
```

---

### Task 2: Simplify `AskUserQuestionWidget` to answered-only, add wording heuristic

**Files:**
- Modify: `src/views/chat/tool-widgets/AskUserQuestionWidget.tsx`
- Modify: `src/views/chat/tool-widgets/AskUserQuestionWidget.test.tsx`

**Interfaces:**
- Consumes: `type AskUserQuestionDetail` from `@/lib/ipc` only (no `commands`, no `Button`, no local state — this component becomes a pure read-only render).
- Produces: default-exported `AskUserQuestionWidget({ detail }: { detail: AskUserQuestionDetail })`, unchanged signature, now rendering only the "already answered" state (`data-testid="question-answered"`) with `"You chose: ..."` or `"You replied: ..."` text. `MessageContent.tsx` (`src/components/MessageContent.tsx:169`) is the only caller and needs no changes — it already only invokes this with a resolved (non-null `answer`) detail.

- [ ] **Step 1: Write the failing test**

Replace the full contents of `src/views/chat/tool-widgets/AskUserQuestionWidget.test.tsx`:

```tsx
import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import AskUserQuestionWidget from "./AskUserQuestionWidget";
import type { AskUserQuestionDetail } from "@/lib/ipc";

const SINGLE: AskUserQuestionDetail = {
  toolName: "AskUserQuestion",
  questionId: "q1",
  header: "Pick a direction",
  question: "Which way should I go?",
  options: [
    { label: "Option A", description: "the first way" },
    { label: "Option B", description: "the second way" },
  ],
  multiSelect: false,
  answer: null,
};

describe("AskUserQuestionWidget", () => {
  it("renders the question and the chosen option when the answer matches a known option", () => {
    const answered: AskUserQuestionDetail = { ...SINGLE, answer: ["Option A"] };
    render(<AskUserQuestionWidget detail={answered} />);

    const widget = screen.getByTestId("question-answered");
    expect(widget).toHaveTextContent("Which way should I go?");
    expect(widget).toHaveTextContent("You chose: Option A");
  });

  it("joins a multi-select answer with commas and still reads as 'You chose'", () => {
    const answered: AskUserQuestionDetail = {
      ...SINGLE,
      multiSelect: true,
      answer: ["Option A", "Option B"],
    };
    render(<AskUserQuestionWidget detail={answered} />);

    expect(screen.getByTestId("question-answered")).toHaveTextContent(
      "You chose: Option A, Option B",
    );
  });

  it("renders 'You replied' when the answer doesn't match any known option (a free-text answer)", () => {
    const answered: AskUserQuestionDetail = { ...SINGLE, answer: ["actually, do both"] };
    render(<AskUserQuestionWidget detail={answered} />);

    expect(screen.getByTestId("question-answered")).toHaveTextContent(
      "You replied: actually, do both",
    );
  });

  it("accepts no further input (FR-009) -- no option buttons render", () => {
    const answered: AskUserQuestionDetail = { ...SINGLE, answer: ["Option A"] };
    render(<AskUserQuestionWidget detail={answered} />);

    expect(screen.queryByRole("button", { name: /Option A/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Option B/ })).not.toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `npx vitest run src/views/chat/tool-widgets/AskUserQuestionWidget.test.tsx`
Expected: FAIL — the "You replied" test fails (current component always renders "You chose"), since the component still has its old interactive behavior at this point.

- [ ] **Step 3: Simplify the component**

Replace the full contents of `src/views/chat/tool-widgets/AskUserQuestionWidget.tsx`:

```tsx
import type { AskUserQuestionDetail } from "@/lib/ipc";

interface AskUserQuestionWidgetProps {
  detail: AskUserQuestionDetail;
}

/**
 * Read-only "already answered" rendering for a resolved AskUserQuestion
 * tool_result (data-model.md) -- the only caller is MessageContent.tsx,
 * rendering a historical, resolved message. The live, still-pending
 * interaction (option buttons, free-text fallback) lives in UserAskWidget
 * instead, rendered in the composer slot by Workspace.tsx.
 *
 * `answer` can come from either a button click (every entry matches a
 * known option label) or typed free text (it won't) -- there's no backend
 * field recording which, so this is a client-side heuristic computed at
 * render time, not a stored fact.
 */
export default function AskUserQuestionWidget({ detail }: AskUserQuestionWidgetProps) {
  const answer = detail.answer ?? [];
  const isFreeText = !answer.every((a) => detail.options.some((o) => o.label === a));

  return (
    <div
      className="rounded-lg border border-border bg-card p-3 text-sm"
      data-testid="question-answered"
    >
      <p className="mb-1 text-muted-foreground">{detail.question}</p>
      <p className="font-medium">
        {isFreeText ? "You replied" : "You chose"}: {answer.join(", ")}
      </p>
    </div>
  );
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `npx vitest run src/views/chat/tool-widgets/AskUserQuestionWidget.test.tsx`
Expected: PASS (4 tests).

- [ ] **Step 5: Run the full frontend test suite to check for regressions**

Run: `npm run test`
Expected: All tests pass except (at this point in the plan) `Workspace.test.tsx`'s pending-question test, which still asserts the old message-list location and `question-widget` testid — Task 3 fixes this. Confirm no other test references `AskUserQuestionWidget`'s removed interactive behavior (`grep -rn "AskUserQuestionWidget" src --include="*.test.tsx"` should only show `AskUserQuestionWidget.test.tsx` itself and `Workspace.test.tsx`).

- [ ] **Step 6: Commit**

```bash
git add src/views/chat/tool-widgets/AskUserQuestionWidget.tsx src/views/chat/tool-widgets/AskUserQuestionWidget.test.tsx
git commit -m "refactor(widgets): shrink AskUserQuestionWidget to the read-only answered state"
```

---

### Task 3: Wire `UserAskWidget` into `Workspace.tsx`'s composer slot

**Files:**
- Modify: `src/views/workspace/Workspace.tsx`
- Modify: `src/views/workspace/Workspace.test.tsx`

**Interfaces:**
- Consumes: `UserAskWidget` (Task 1) default export, `{ detail: AskUserQuestionDetail }`. Removes the `AskUserQuestionWidget` import (Workspace.tsx no longer renders it anywhere).
- Produces: `Workspace.tsx`'s composer shell (`data-testid="workspace-composer-shell"`) renders `UserAskWidget` in place of `RichInput` while `pendingQuestion` is truthy; the message-list pending block (`data-testid="chat-message"`) no longer renders anything for a pending question (only Bash/Task).

- [ ] **Step 1: Write the failing test**

In `src/views/workspace/Workspace.test.tsx`, replace the existing test (currently at line 396) with two tests. Find:

```tsx
  it('shows the pending question widget (not "Working…") when the latest message is an unanswered AskUserQuestion tool_call, and answering it calls answerUserQuestion', async () => {
    vi.mocked(commands.listMessages).mockResolvedValue([
      {
        id: "u1",
        conversationId: "conv-1",
        role: "user",
        contentType: "text",
        content: "ask me something",
        toolName: null,
        createdAt: 1,
        durationMs: null,
        tokenCount: null,
      },
      {
        id: "tc1",
        conversationId: "conv-1",
        role: "assistant",
        contentType: "tool_call",
        content: JSON.stringify({
          arguments: {
            header: "Quick check",
            question: "What would you like to do?",
            options: [{ label: "A" }, { label: "B" }],
            multiSelect: false,
            questionId: "q1",
          },
        }),
        toolName: "AskUserQuestion",
        createdAt: 2,
        durationMs: null,
        tokenCount: null,
      },
    ]);
    // send_agent_message's own promise never resolves in this test -- it's
    // genuinely still blocked server-side, exactly like the real bug.
    vi.mocked(commands.sendAgentMessage).mockReturnValue(new Promise(() => {}));

    render(<Workspace conversationId="conv-1" />);

    const widget = await screen.findByTestId("question-widget");
    expect(widget).toHaveTextContent("What would you like to do?");
    expect(screen.queryByTestId("agent-thinking")).not.toBeInTheDocument();
    // The composer must not accept a new message while this is pending --
    // that's exactly how a second message ("e?") got queued up behind the
    // same stuck lock in the real incident.
    expect(screen.getByTestId("agent-input")).toHaveAttribute("contenteditable", "false");

    await userEvent.click(screen.getByText("A"));
    expect(commands.answerUserQuestion).toHaveBeenCalledWith("q1", ["A"]);
  });
```

Replace it with:

```tsx
  it('shows the pending question widget in the composer slot (not "Working…", not the normal chat input) when the latest message is an unanswered AskUserQuestion tool_call, and answering it calls answerUserQuestion', async () => {
    vi.mocked(commands.listMessages).mockResolvedValue([
      {
        id: "u1",
        conversationId: "conv-1",
        role: "user",
        contentType: "text",
        content: "ask me something",
        toolName: null,
        createdAt: 1,
        durationMs: null,
        tokenCount: null,
      },
      {
        id: "tc1",
        conversationId: "conv-1",
        role: "assistant",
        contentType: "tool_call",
        content: JSON.stringify({
          arguments: {
            header: "Quick check",
            question: "What would you like to do?",
            options: [{ label: "A" }, { label: "B" }],
            multiSelect: false,
            questionId: "q1",
          },
        }),
        toolName: "AskUserQuestion",
        createdAt: 2,
        durationMs: null,
        tokenCount: null,
      },
    ]);
    // send_agent_message's own promise never resolves in this test -- it's
    // genuinely still blocked server-side, exactly like the real bug.
    vi.mocked(commands.sendAgentMessage).mockReturnValue(new Promise(() => {}));

    render(<Workspace conversationId="conv-1" />);

    const widget = await screen.findByTestId("user-ask-widget");
    expect(widget).toHaveTextContent("What would you like to do?");
    expect(screen.queryByTestId("agent-thinking")).not.toBeInTheDocument();
    // The normal composer is replaced entirely, not merely disabled -- the
    // question widget sits in its place instead.
    expect(screen.queryByTestId("agent-input")).not.toBeInTheDocument();

    await userEvent.click(screen.getByText("A"));
    expect(commands.answerUserQuestion).toHaveBeenCalledWith("q1", ["A"]);
  });

  it("closing the pending question widget reveals a free-text composer whose submission answers the question", async () => {
    vi.mocked(commands.listMessages).mockResolvedValue([
      {
        id: "u1",
        conversationId: "conv-1",
        role: "user",
        contentType: "text",
        content: "ask me something",
        toolName: null,
        createdAt: 1,
        durationMs: null,
        tokenCount: null,
      },
      {
        id: "tc1",
        conversationId: "conv-1",
        role: "assistant",
        contentType: "tool_call",
        content: JSON.stringify({
          arguments: {
            header: "Quick check",
            question: "What would you like to do?",
            options: [{ label: "A" }, { label: "B" }],
            multiSelect: false,
            questionId: "q1",
          },
        }),
        toolName: "AskUserQuestion",
        createdAt: 2,
        durationMs: null,
        tokenCount: null,
      },
    ]);
    vi.mocked(commands.sendAgentMessage).mockReturnValue(new Promise(() => {}));
    vi.mocked(commands.answerUserQuestion).mockResolvedValue(undefined);

    render(<Workspace conversationId="conv-1" />);

    await screen.findByTestId("user-ask-widget");
    await userEvent.click(screen.getByTestId("question-close"));

    const editable = screen.getByTestId("question-answer-input");
    await userEvent.click(editable);
    await userEvent.type(editable, "actually, do both{Enter}");

    expect(commands.answerUserQuestion).toHaveBeenCalledWith("q1", ["actually, do both"]);
  });
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `npx vitest run src/views/workspace/Workspace.test.tsx`
Expected: FAIL — `Unable to find an element by: [data-testid="user-ask-widget"]` (Workspace.tsx still renders the old `AskUserQuestionWidget` in the message list and still shows `RichInput` in the composer).

- [ ] **Step 3: Update the import**

In `src/views/workspace/Workspace.tsx`, find:

```tsx
import AskUserQuestionWidget from "@/views/chat/tool-widgets/AskUserQuestionWidget";
```

Replace with:

```tsx
import UserAskWidget from "@/views/chat/tool-widgets/UserAskWidget";
```

- [ ] **Step 4: Stop rendering the question widget in the message list**

Find:

```tsx
            {pendingToolCall ? (
              <div
                className="mb-6"
                data-testid="chat-message"
                role="group"
                aria-label="doce replied"
              >
                {pendingQuestion && <AskUserQuestionWidget detail={pendingQuestion} />}
                {pendingToolCall.kind === "bash" && <BashWidget detail={pendingToolCall.detail} />}
                {pendingToolCall.kind === "task" && <TaskWidget detail={pendingToolCall.detail} />}
              </div>
            ) : (
              showThinking && (
                <p className="text-sm text-muted-foreground" data-testid="agent-thinking">
                  Working…
                </p>
              )
            )}
```

Replace with:

```tsx
            {pendingToolCall && pendingToolCall.kind !== "question" ? (
              <div
                className="mb-6"
                data-testid="chat-message"
                role="group"
                aria-label="doce replied"
              >
                {pendingToolCall.kind === "bash" && <BashWidget detail={pendingToolCall.detail} />}
                {pendingToolCall.kind === "task" && <TaskWidget detail={pendingToolCall.detail} />}
              </div>
            ) : (
              !pendingToolCall &&
              showThinking && (
                <p className="text-sm text-muted-foreground" data-testid="agent-thinking">
                  Working…
                </p>
              )
            )}
```

(A pending question now renders nothing in the message list at all — Step 5 moves it to the composer. Without the added `!pendingToolCall &&` guard, a pending question would incorrectly fall into the `showThinking` branch and show "Working…".)

- [ ] **Step 5: Render `UserAskWidget` in the composer slot**

Find:

```tsx
        <RichInput
          onSubmit={(content, richContent) => {
            send(content, richContent);
          }}
          skillsEnabled={true}
          disabled={sendInFlight || pendingToolCall !== null}
          placeholder="Describe a task…"
          inputTestId="agent-input"
          submitTestId="agent-send"
          contextGauge={<ContextUsageGauge conversationId={conversationId} />}
        />
```

Replace with:

```tsx
        {pendingQuestion ? (
          <UserAskWidget detail={pendingQuestion} />
        ) : (
          <RichInput
            onSubmit={(content, richContent) => {
              send(content, richContent);
            }}
            skillsEnabled={true}
            disabled={sendInFlight || pendingToolCall !== null}
            placeholder="Describe a task…"
            inputTestId="agent-input"
            submitTestId="agent-send"
            contextGauge={<ContextUsageGauge conversationId={conversationId} />}
          />
        )}
```

- [ ] **Step 6: Run the test to verify it passes**

Run: `npx vitest run src/views/workspace/Workspace.test.tsx`
Expected: PASS (all tests in this file, including the two from Step 1).

- [ ] **Step 7: Run the full frontend test suite to check for regressions**

Run: `npm run test`
Expected: All tests pass.

- [ ] **Step 8: Commit**

```bash
git add src/views/workspace/Workspace.tsx src/views/workspace/Workspace.test.tsx
git commit -m "feat(workspace): render UserAskWidget in the composer slot for pending questions"
```

---

### Task 4: Update `WidgetGallery` examples

**Files:**
- Modify: `src/views/design-system/WidgetGallery.tsx`

**Interfaces:**
- Consumes: `UserAskWidget` (Task 1, including its `initialMode` prop) and `AskUserQuestionWidget` (Task 2, unchanged answered-state usage).
- Produces: no new exports — this is a visual catalog page with no dedicated test file (confirmed: no `WidgetGallery.test.tsx` exists in this repo).

- [ ] **Step 1: Add the `UserAskWidget` import**

In `src/views/design-system/WidgetGallery.tsx`, find:

```tsx
import AskUserQuestionWidget from "@/views/chat/tool-widgets/AskUserQuestionWidget";
```

Replace with:

```tsx
import AskUserQuestionWidget from "@/views/chat/tool-widgets/AskUserQuestionWidget";
import UserAskWidget from "@/views/chat/tool-widgets/UserAskWidget";
```

- [ ] **Step 2: Switch the pending examples to `UserAskWidget`, add a free-text fallback example**

Find:

```tsx
        <Section
          title="AskUserQuestion"
          description="An interactive pause/resume prompt. Single-select answers on click; multi-select accumulates + confirms. Read-only once answered."
        >
          <Example label="Pending, single-select">
            <AskUserQuestionWidget
              detail={{
                toolName: "AskUserQuestion",
                questionId: "design-system-preview-1",
                header: "Ambiguous request",
                question: "Which config file should this apply to?",
                options: [
                  { label: "tauri.conf.json", description: "The app's own config" },
                  { label: "vite.config.ts", description: "The dev server config" },
                ],
                multiSelect: false,
                answer: null,
              }}
            />
          </Example>
          <Example label="Pending, multi-select">
            <AskUserQuestionWidget
              detail={{
                toolName: "AskUserQuestion",
                questionId: "design-system-preview-2",
                header: "",
                question: "Which tiers should the rerun cover?",
                options: [
                  { label: "Tier 1", description: "" },
                  { label: "Tier 4", description: "" },
                  { label: "Tier 4 planned", description: "" },
                ],
                multiSelect: true,
                answer: null,
              }}
            />
          </Example>
          <Example label="Answered">
            <AskUserQuestionWidget
              detail={{
                toolName: "AskUserQuestion",
                questionId: "design-system-preview-3",
                header: "",
                question: "Rerun now or wait?",
                options: [
                  { label: "Rerun now", description: "" },
                  { label: "Wait", description: "" },
                ],
                multiSelect: false,
                answer: ["Rerun now"],
              }}
            />
          </Example>
        </Section>
```

Replace with:

```tsx
        <Section
          title="AskUserQuestion"
          description="An interactive pause/resume prompt, rendered in the composer slot while pending. Single-select answers on click; multi-select accumulates + confirms; closing it (✕) reveals a free-text fallback. Read-only once answered."
        >
          <Example label="Pending, single-select">
            <UserAskWidget
              detail={{
                toolName: "AskUserQuestion",
                questionId: "design-system-preview-1",
                header: "Ambiguous request",
                question: "Which config file should this apply to?",
                options: [
                  { label: "tauri.conf.json", description: "The app's own config" },
                  { label: "vite.config.ts", description: "The dev server config" },
                ],
                multiSelect: false,
                answer: null,
              }}
            />
          </Example>
          <Example label="Pending, multi-select">
            <UserAskWidget
              detail={{
                toolName: "AskUserQuestion",
                questionId: "design-system-preview-2",
                header: "",
                question: "Which tiers should the rerun cover?",
                options: [
                  { label: "Tier 1", description: "" },
                  { label: "Tier 4", description: "" },
                  { label: "Tier 4 planned", description: "" },
                ],
                multiSelect: true,
                answer: null,
              }}
            />
          </Example>
          <Example label="Pending, free-text fallback">
            <UserAskWidget
              initialMode="text"
              detail={{
                toolName: "AskUserQuestion",
                questionId: "design-system-preview-4",
                header: "",
                question: "Rerun now or wait?",
                options: [
                  { label: "Rerun now", description: "" },
                  { label: "Wait", description: "" },
                ],
                multiSelect: false,
                answer: null,
              }}
            />
          </Example>
          <Example label="Answered">
            <AskUserQuestionWidget
              detail={{
                toolName: "AskUserQuestion",
                questionId: "design-system-preview-3",
                header: "",
                question: "Rerun now or wait?",
                options: [
                  { label: "Rerun now", description: "" },
                  { label: "Wait", description: "" },
                ],
                multiSelect: false,
                answer: ["Rerun now"],
              }}
            />
          </Example>
        </Section>
```

- [ ] **Step 3: Run the full frontend test suite to check for regressions**

Run: `npm run test`
Expected: All tests pass (there is no `WidgetGallery.test.tsx`, so this step is a regression check on the rest of the suite, e.g. confirming the changed imports don't break any snapshot or type check elsewhere).

- [ ] **Step 4: Type-check and lint**

Run: `npx tsc -b --noEmit && npm run lint`
Expected: No new errors.

- [ ] **Step 5: Commit**

```bash
git add src/views/design-system/WidgetGallery.tsx
git commit -m "feat(design-system): preview UserAskWidget's pending and free-text states"
```

---

### Task 5: Update the `tool-call-widgets` e2e spec

**Files:**
- Modify: `tests/e2e/specs/tool-call-widgets.spec.ts`

**Interfaces:**
- Consumes: the `data-testid="user-ask-widget"` and `data-testid="agent-input"` contracts established in Tasks 1 and 3.
- Produces: no exports — an assertion-only update.

This spec drives a real built Tauri app against a real model (`tests/e2e/run-e2e.sh` serves the built `dist/` via `vite preview`, then runs `wdio`) — a slow (multi-minute), separate test cycle from Tasks 1-4's Vitest suite, and not run by `npm run test`. Treat this task as a distinct review/verification gate; don't fold its verification into the earlier tasks' `npm run test` runs.

- [ ] **Step 1: Update the testid and the stale-composer assertion**

In `tests/e2e/specs/tool-call-widgets.spec.ts`, find:

```ts
    const widget = await browser.$("[data-testid='question-widget']");
    await widget.waitForExist({ timeout: 60000 });
    expect(await widget.getText()).toContain("Which color do you prefer?");

    // The regression itself: the composer must refuse a new message while
    // genuinely paused here, since typing one would just queue up stuck
    // behind the same lock rather than doing anything.
    expect(await agentInput.getAttribute("contenteditable")).toBe("false");

    await (await widget.$("button=Red")).click();
```

Replace with:

```ts
    const widget = await browser.$("[data-testid='user-ask-widget']");
    await widget.waitForExist({ timeout: 60000 });
    expect(await widget.getText()).toContain("Which color do you prefer?");

    // The regression itself: the composer must be fully replaced by the
    // question widget while genuinely paused here, not merely disabled --
    // typing into a still-present-but-disabled input would queue a message
    // up stuck behind the same lock rather than doing anything.
    const composerInputWhilePending = await browser.$("[data-testid='agent-input']");
    expect(await composerInputWhilePending.isExisting()).toBe(false);

    await (await widget.$("button=Red")).click();
```

(`agentInput`, captured earlier in the test from `startWorkspaceConversationViaComposer`'s return value, is still used for its one remaining call — `await agentInput.setValue(...)` a few lines above this block, sending the message that triggers the `AskUserQuestion` tool call — so it's not left unused.)

- [ ] **Step 2: Run the e2e suite**

Run: `npm run test:e2e`
Expected: PASS, including `"a real AskUserQuestion pauses the loop with a visible, answerable prompt, and answering it resumes and completes the turn"`. This requires whatever this repo's e2e setup already needs to talk to a real model (check `tests/e2e/wdio.conf.ts` / CI config if it fails locally for environment reasons unrelated to this change).

- [ ] **Step 3: Commit**

```bash
git add tests/e2e/specs/tool-call-widgets.spec.ts
git commit -m "test(e2e): update AskUserQuestion e2e spec for the composer-slot widget"
```

## Self-Review Notes

- **Spec coverage:** Section 1 (component split) → Tasks 1–2. Section 2 (`UserAskWidget` behavior/modes) → Task 1. Section 3 (wording heuristic) → Task 2. Section 4 (gallery + test updates) → Tasks 3–4. The e2e spec referencing the old testid/disabled-input assertion (found via a repo-wide grep for `question-widget`/`AskUserQuestionWidget`, not called out in the original design doc) → Task 5. All spec sections, plus this one discovered gap, have a corresponding task.
- **Type consistency:** `UserAskWidget`'s props (`detail: AskUserQuestionDetail`, `initialMode?: "options" | "text"`) are the same across Task 1's definition, Task 3's `Workspace.tsx` call site (`<UserAskWidget detail={pendingQuestion} />`, no `initialMode`), and Task 4's gallery call sites (one passing `initialMode="text"`). `commands.answerUserQuestion(questionId: string, answer: string[])` is called identically in Task 1 (component) and asserted identically in Task 1/Task 3/Task 5's tests.
- **No placeholders:** every step above shows complete, runnable code — no "TBD" or "similar to Task N" shortcuts.
