# UserAskWidget Grouped Form List Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace `UserAskWidget`'s pill-button options layout with real radio/checkbox rows in one bordered "module" card, unify single-select/multi-select/free-text into one "select, then press send" interaction using the same submit button `RichInput` already has, and give the composer arrival and options↔text mode switch the app's existing view-transition motion.

**Architecture:** Two tasks. Task 1 is the static redesign — shell unification (unboxed header + one bordered module), the options module (radio/checkbox rows + submit footer), and the interaction change (select-then-submit) — with zero motion changes, fully covered by an updated `UserAskWidget.test.tsx` and a small fix to `Workspace.test.tsx`. Task 2 layers motion on top: `theme.css` keyframes for a new `user-ask-module` view-transition group, and wiring `runViewTransition` (already used by `App.tsx` for conversation switches) around both the composer-level arrival (`Workspace.tsx`) and the mode switch (`UserAskWidget.tsx`).

**Tech Stack:** React + TypeScript, Vitest + `@testing-library/react` + `@testing-library/user-event`, Tailwind (v4, `@theme`-token-based), `@phosphor-icons/react` icons, the CSS View Transitions API via the existing `runViewTransition` helper.

## Global Constraints

- No backend/Rust changes anywhere in this plan.
- No change to `RichInput` itself, `AskUserQuestionWidget`'s answered/read-only rendering, or `answer_user_question`'s wire contract. `commands.answerUserQuestion(questionId, answer)` is called with exactly the same argument shapes as today (`[label]` single-select, `selected` multi-select, `[content]` free text) — only the trigger changes for single-select (a send-button press instead of a bare option click).
- The new submit button reuses `RichInput`'s own send-button classes verbatim (`h-8 w-8 shrink-0 rounded-full p-0 enabled:bg-gradient-to-r enabled:from-[var(--color-primary)] enabled:via-[var(--color-gray-2)] enabled:to-[var(--color-gray-1)] enabled:hover:from-[var(--color-gray-2)] enabled:hover:via-[var(--color-gray-1)] enabled:hover:to-[var(--color-foreground)]`) and the same `PaperPlaneRightIcon`.
- No new accent color anywhere — stay strictly within the existing monochrome gray scale (`--color-gray-1..6`, `--color-border`, `--color-muted`, etc.).
- Reuse the existing `runViewTransition` helper (`src/lib/viewTransition.ts`) for all motion in this plan — do not invent a new transition mechanism.
- No roving-tabindex arrow-key navigation for the `radiogroup`/`group` pattern in this pass (Tab-per-row is acceptable) — explicitly out of scope.
- `WidgetGallery.tsx` needs no code changes in this plan — `UserAskWidget`'s props (`{ detail, initialMode }`) are unchanged, only its internal rendering changes.
- Test command for a single file: `npx vitest run <path>`. Full suite: `npm run test`.

---

### Task 1: Redesign `UserAskWidget`'s shell, options module, and interaction (no motion yet)

**Files:**
- Modify: `src/views/chat/tool-widgets/UserAskWidget.tsx`
- Modify: `src/views/chat/tool-widgets/UserAskWidget.test.tsx`
- Modify: `src/views/workspace/Workspace.test.tsx:396-444` (the single-select pending-question test, which directly depends on this task's interaction change)

**Interfaces:**
- Consumes: `Button` (`@/components/ui/button`, variants `"primary"`/`"ghost"`, sizes `"icon-sm"`), `cn` (`@/lib/cn`), `commands.answerUserQuestion(questionId: string, answer: string[]): Promise<void>` and `type AskUserQuestionDetail`/`type QuestionOption` (`@/lib/ipc`), `RichInput` (unchanged), `ArrowLeftIcon`/`CheckIcon`/`PaperPlaneRightIcon`/`XIcon` (`@phosphor-icons/react`).
- Produces: `UserAskWidget`'s default export signature is unchanged (`{ detail: AskUserQuestionDetail; initialMode?: "options" | "text" }`) — `WidgetGallery.tsx` and `Workspace.tsx` need no changes. New/changed `data-testid`s: `user-ask-widget` (now on the unboxed root, not a bordered box), `question-close`, `question-back-to-options` (now an icon-only button), `question-submit` (now always rendered, both select modes, `aria-label="Send answer"`). Removed: `data-testid="multi-select-indicator"` (element deleted entirely). This is the interface Task 2 builds motion on top of.

- [ ] **Step 1: Write the failing test**

Replace the full contents of `src/views/chat/tool-widgets/UserAskWidget.test.tsx`:

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

  it("renders each option as a radio row and a disabled submit button until one is picked", () => {
    render(<UserAskWidget detail={SINGLE} />);
    expect(screen.getByText("Which way should I go?")).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: /Option A/ })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: /Option B/ })).toBeInTheDocument();
    expect(screen.getByTestId("question-submit")).toBeDisabled();
  });

  it("shows each option's description as visible text, not just a hover tooltip", () => {
    render(<UserAskWidget detail={SINGLE} />);
    expect(screen.getByText("the first way")).toBeInTheDocument();
    expect(screen.getByText("the second way")).toBeInTheDocument();
  });

  it("options are grouped with the correct ARIA role for the select mode", () => {
    const { unmount } = render(<UserAskWidget detail={SINGLE} />);
    expect(screen.getByRole("radiogroup")).toBeInTheDocument();
    unmount();

    render(<UserAskWidget detail={MULTI} />);
    expect(screen.getByRole("group")).toBeInTheDocument();
  });

  it("selecting a single-select option enables the submit button, and clicking it answers the question", async () => {
    vi.mocked(commands.answerUserQuestion).mockResolvedValue(undefined);
    render(<UserAskWidget detail={SINGLE} />);

    const submitButton = screen.getByTestId("question-submit");
    expect(submitButton).toBeDisabled();

    await userEvent.click(screen.getByRole("radio", { name: /Option A/ }));
    expect(commands.answerUserQuestion).not.toHaveBeenCalled();
    expect(submitButton).toBeEnabled();

    await userEvent.click(submitButton);
    expect(commands.answerUserQuestion).toHaveBeenCalledWith("q1", ["Option A"]);
  });

  it("never shows a selected count for single-select", async () => {
    render(<UserAskWidget detail={SINGLE} />);
    await userEvent.click(screen.getByRole("radio", { name: /Option A/ }));
    expect(screen.queryByText(/selected/)).not.toBeInTheDocument();
  });

  it("multi-select accumulates a selection, shows a live count, and requires an explicit submit", async () => {
    vi.mocked(commands.answerUserQuestion).mockResolvedValue(undefined);
    render(<UserAskWidget detail={MULTI} />);

    expect(screen.queryByText(/selected/)).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole("checkbox", { name: /Option A/ }));
    expect(screen.getByText("1 selected")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("checkbox", { name: /Option B/ }));
    expect(screen.getByText("2 selected")).toBeInTheDocument();
    expect(commands.answerUserQuestion).not.toHaveBeenCalled();

    await userEvent.click(screen.getByTestId("question-submit"));
    expect(commands.answerUserQuestion).toHaveBeenCalledWith("q2", ["Option A", "Option B"]);
  });

  it("closing the widget switches to a free-text answer input", async () => {
    render(<UserAskWidget detail={SINGLE} />);

    await userEvent.click(screen.getByTestId("question-close"));

    expect(screen.queryByRole("radio", { name: /Option A/ })).not.toBeInTheDocument();
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

  it("submitting a free-text answer that is entirely a collapsed paste chip (no text) does not answer the question", async () => {
    vi.mocked(commands.answerUserQuestion).mockResolvedValue(undefined);
    render(<UserAskWidget detail={SINGLE} />);

    await userEvent.click(screen.getByTestId("question-close"));
    const editable = screen.getByTestId("question-answer-input");
    const longText = Array.from({ length: 15 }, (_, i) => `line ${i}`).join("\n");

    await userEvent.click(editable);
    await userEvent.paste(longText);

    const chip = await screen.findByTestId("pasted-text-chip");
    expect(chip).toHaveTextContent("<pasted 15 lines>");

    await userEvent.keyboard("{Enter}");

    expect(commands.answerUserQuestion).not.toHaveBeenCalled();
  });

  it("'back to options' returns from the free-text input to the option rows", async () => {
    render(<UserAskWidget detail={SINGLE} />);

    await userEvent.click(screen.getByTestId("question-close"));
    await userEvent.click(screen.getByTestId("question-back-to-options"));

    expect(screen.getByRole("radio", { name: /Option A/ })).toBeInTheDocument();
    expect(screen.queryByTestId("question-answer-input")).not.toBeInTheDocument();
  });

  it("initialMode='text' starts directly in the free-text fallback (used by WidgetGallery)", () => {
    render(<UserAskWidget detail={SINGLE} initialMode="text" />);

    expect(screen.getByTestId("question-answer-input")).toBeInTheDocument();
    expect(screen.queryByRole("radio", { name: /Option A/ })).not.toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `npx vitest run src/views/chat/tool-widgets/UserAskWidget.test.tsx`
Expected: FAIL — most assertions fail against the current implementation (e.g. `getByRole("radio", ...)` finds nothing since today's options are plain `Button` elements with an implicit `"button"` role, not `role="radio"`; `question-submit` doesn't exist at all for single-select).

- [ ] **Step 3: Replace the component**

Replace the full contents of `src/views/chat/tool-widgets/UserAskWidget.tsx`:

```tsx
import { useId, useState } from "react";
import { ArrowLeftIcon, CheckIcon, PaperPlaneRightIcon, XIcon } from "@phosphor-icons/react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/cn";
import { commands, type AskUserQuestionDetail, type QuestionOption } from "@/lib/ipc";
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

// Identical to RichInput's own send button (RichInput.tsx) -- same size,
// same gradient sheen, same icon -- so single-select, multi-select, and
// free text all answer via one visually consistent affordance.
const SUBMIT_BUTTON_CLASSES =
  "h-8 w-8 shrink-0 rounded-full p-0 enabled:bg-gradient-to-r enabled:from-[var(--color-primary)] enabled:via-[var(--color-gray-2)] enabled:to-[var(--color-gray-1)] enabled:hover:from-[var(--color-gray-2)] enabled:hover:via-[var(--color-gray-1)] enabled:hover:to-[var(--color-foreground)]";

/**
 * One option row inside the options module -- a real radio/checkbox
 * control, not a Button pill: a glyph on the left (empty ring/square at
 * rest, filled on selection), the option's label and its description
 * stacked to the right. The description used to be reachable only via a
 * hover `title=` attribute; it's always-visible text now, so keyboard and
 * screen-reader users can read it too.
 */
function OptionRow({
  option,
  selected,
  multiSelect,
  disabled,
  onSelect,
}: {
  option: QuestionOption;
  selected: boolean;
  multiSelect: boolean;
  disabled: boolean;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      role={multiSelect ? "checkbox" : "radio"}
      aria-checked={selected}
      disabled={disabled}
      onClick={onSelect}
      className={cn(
        "flex w-full items-start gap-2.5 rounded-md px-2.5 py-2 text-left text-sm transition-colors",
        selected ? "bg-muted" : "hover:bg-muted",
      )}
    >
      <span
        className={cn(
          "mt-0.5 flex size-4 shrink-0 items-center justify-center border-[1.5px] border-[var(--color-gray-4)]",
          multiSelect ? "rounded-[4px]" : "rounded-full",
          selected && (multiSelect ? "border-primary bg-primary" : "border-foreground"),
        )}
      >
        {selected &&
          (multiSelect ? (
            <CheckIcon size={10} weight="bold" className="text-primary-foreground" />
          ) : (
            <span className="size-2 rounded-full bg-foreground" />
          ))}
      </span>
      <span className="flex min-w-0 flex-col gap-0.5">
        <span className={cn("text-foreground", selected && "font-semibold")}>{option.label}</span>
        {option.description && (
          <span className="text-xs leading-snug text-muted-foreground">{option.description}</span>
        )}
      </span>
    </button>
  );
}

/**
 * The live, still-unanswered `AskUserQuestion` prompt, rendered in the
 * chat composer slot in place of RichInput while a question is pending
 * (Workspace.tsx). One shared, unboxed header (eyebrow + question, one
 * icon button in the same slot in both modes) sits above a single
 * bordered "module": in options mode, a list of real radio/checkbox rows
 * plus a footer holding the one submit button also used by multi-select
 * and free text; in text mode, a bare RichInput (it already supplies its
 * own matching card -- wrapping it in a second border here would double
 * it up, which is exactly what the old implementation did). Picking an
 * option only selects it, single- or multi-select alike; answering
 * always requires pressing the submit button, which stays disabled until
 * at least one option is selected. The close (X) button swaps to free
 * text instead, whose submission answers the question with the raw
 * typed text -- for whenever the fixed option labels don't cover what
 * the user actually wants to say. Once answered, this component unmounts
 * on its own: Workspace.tsx stops rendering it as soon as the resolved
 * tool_result replaces the pending tool_call as the latest message.
 * (Compare AskUserQuestionWidget, which renders the read-only "already
 * answered" state in message history and never handles a live question.)
 */
export default function UserAskWidget({ detail, initialMode = "options" }: UserAskWidgetProps) {
  const [mode, setMode] = useState<Mode>(initialMode);
  const [selected, setSelected] = useState<string[]>([]);
  const [submitting, setSubmitting] = useState(false);
  const questionId = useId();

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
      setSelected([label]);
      return;
    }
    setSelected((prev) =>
      prev.includes(label) ? prev.filter((l) => l !== label) : [...prev, label],
    );
  };

  return (
    <div className="flex flex-col gap-1.5" data-testid="user-ask-widget">
      <div className="flex items-start gap-2">
        <div className="min-w-0 flex-1">
          {mode === "options" && detail.header && (
            <p className="mb-0.5 text-xs text-muted-foreground">{detail.header}</p>
          )}
          <p id={questionId} className="text-sm font-medium text-foreground">
            {mode === "options" ? detail.question : `Answering: ${detail.question}`}
          </p>
        </div>
        <Button
          type="button"
          variant="ghost"
          size="icon-sm"
          className="shrink-0 text-muted-foreground hover:bg-transparent"
          disabled={submitting}
          onClick={() => setMode(mode === "options" ? "text" : "options")}
          aria-label={mode === "options" ? "Close question" : "Back to options"}
          data-testid={mode === "options" ? "question-close" : "question-back-to-options"}
        >
          {mode === "options" ? <XIcon size={14} /> : <ArrowLeftIcon size={14} />}
        </Button>
      </div>

      {mode === "text" ? (
        <RichInput
          onSubmit={(content) => {
            if (content.trim()) submit([content]);
          }}
          skillsEnabled={true}
          disabled={submitting}
          placeholder="Type your answer…"
          inputTestId="question-answer-input"
          submitTestId="question-answer-send"
        />
      ) : (
        <div className="flex flex-col gap-2 rounded-lg border border-border bg-card px-3 py-2 shadow-xs transition-shadow focus-within:shadow-sm">
          <div
            className="flex flex-col gap-0.5"
            role={detail.multiSelect ? "group" : "radiogroup"}
            aria-labelledby={questionId}
          >
            {detail.options.map((option) => (
              <OptionRow
                key={option.label}
                option={option}
                selected={selected.includes(option.label)}
                multiSelect={detail.multiSelect}
                disabled={submitting}
                onSelect={() => toggleOption(option.label)}
              />
            ))}
          </div>
          <div className="flex items-center justify-between gap-2">
            <span className="text-xs text-muted-foreground">
              {detail.multiSelect && selected.length > 0 ? `${selected.length} selected` : ""}
            </span>
            <Button
              type="button"
              variant="primary"
              className={SUBMIT_BUTTON_CLASSES}
              disabled={selected.length === 0 || submitting}
              onClick={() => submit(selected)}
              aria-label="Send answer"
              data-testid="question-submit"
            >
              <PaperPlaneRightIcon size={16} />
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `npx vitest run src/views/chat/tool-widgets/UserAskWidget.test.tsx`
Expected: PASS (12 tests).

- [ ] **Step 5: Fix the now-broken Workspace.test.tsx assertion**

In `src/views/workspace/Workspace.test.tsx`, find (inside the test titled `'shows the pending question widget in the composer slot ...'`, around line 442):

```tsx
    await userEvent.click(screen.getByText("A"));
    expect(commands.answerUserQuestion).toHaveBeenCalledWith("q1", ["A"]);
  });
```

Replace with:

```tsx
    await userEvent.click(screen.getByRole("radio", { name: "A" }));
    expect(commands.answerUserQuestion).not.toHaveBeenCalled();

    await userEvent.click(screen.getByTestId("question-submit"));
    expect(commands.answerUserQuestion).toHaveBeenCalledWith("q1", ["A"]);
  });
```

- [ ] **Step 6: Run the full frontend test suite to check for regressions**

Run: `npm run test`
Expected: All tests pass. (The other pending-question test in `Workspace.test.tsx`, `"closing the pending question widget reveals a free-text composer..."`, is unaffected by this task — it only exercises the close→free-text path, not option selection.)

- [ ] **Step 7: Commit**

```bash
git add src/views/chat/tool-widgets/UserAskWidget.tsx src/views/chat/tool-widgets/UserAskWidget.test.tsx src/views/workspace/Workspace.test.tsx
git commit -m "feat(widgets): redesign UserAskWidget with real radio/checkbox rows and one submit button"
```

---

### Task 2: Add view-transition motion (composer arrival + mode switch)

**Files:**
- Modify: `src/styles/theme.css`
- Modify: `src/views/chat/tool-widgets/UserAskWidget.tsx`
- Modify: `src/views/chat/tool-widgets/UserAskWidget.test.tsx`
- Modify: `src/views/workspace/Workspace.tsx:145-152`
- Modify: `src/views/workspace/Workspace.test.tsx`

**Interfaces:**
- Consumes: `runViewTransition(update: () => void): void` from `src/lib/viewTransition.ts` (existing, already used by `App.tsx` for conversation switches; falls back to calling `update()` directly when `document.startViewTransition` is unsupported — this is already true in the jsdom test environment, so no test needs a real browser).
- Produces: no new exports. `UserAskWidget`'s and `Workspace.tsx`'s external behavior is unchanged except for *how* the composer swap and mode switch visually transition — same DOM content, same test ids, same props.

- [ ] **Step 1: Write the failing tests**

In `src/views/workspace/Workspace.test.tsx`, find the top of the file (imports) and the `describe` block's setup. Add the view-transition test scaffolding used identically in `App.test.tsx` — a module-level capture of the original `document.startViewTransition` plus an `afterEach` restore. Find:

```tsx
import { StrictMode } from "react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { act, render, screen, waitFor, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import Workspace from "./Workspace";
import { commands, events } from "@/lib/ipc";
import type { RichMessageContent } from "@/lib/ipc";
```

Replace with:

```tsx
import { StrictMode } from "react";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { act, render, screen, waitFor, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import Workspace from "./Workspace";
import { commands, events } from "@/lib/ipc";
import type { RichMessageContent } from "@/lib/ipc";

type TestDocument = Document & {
  startViewTransition?: (callback: () => void) => unknown;
};

const originalStartViewTransition = (document as TestDocument).startViewTransition;
```

Then find the file's `describe` block's `beforeEach` (the one that sets up `vi.clearAllMocks()` or similar at the top of the main `describe`) and add a matching `afterEach` immediately after it — if you're unsure exactly where the main `describe`'s setup block ends, add this as its own top-level statement right after the `describe("Workspace", () => {` opening line, before the first `it(...)`:

```tsx
  afterEach(() => {
    if (originalStartViewTransition) {
      Object.defineProperty(document, "startViewTransition", {
        configurable: true,
        writable: true,
        value: originalStartViewTransition,
      });
    } else {
      Object.defineProperty(document, "startViewTransition", {
        configurable: true,
        writable: true,
        value: undefined,
      });
    }
  });
```

Now add a new test. Find the test `"notifies when an agent-message-persisted event refreshes active messages"` and insert this new test immediately after it:

```tsx
  it("wraps the agent-message-persisted refresh in a view transition when the document supports it", async () => {
    const startViewTransition = vi.fn((callback: () => void) => {
      callback();
      return {};
    });
    Object.defineProperty(document, "startViewTransition", {
      configurable: true,
      writable: true,
      value: startViewTransition,
    });

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
    await screen.findByText("first message");
    startViewTransition.mockClear();

    firePersisted({ conversationId: "conv-1" });
    await screen.findByText("second message");

    expect(startViewTransition).toHaveBeenCalledTimes(1);
  });
```

In `src/views/chat/tool-widgets/UserAskWidget.test.tsx`, add the same view-transition scaffolding. Find:

```tsx
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import UserAskWidget from "./UserAskWidget";
import { commands } from "@/lib/ipc";
import type { AskUserQuestionDetail } from "@/lib/ipc";
```

Replace with:

```tsx
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import UserAskWidget from "./UserAskWidget";
import { commands } from "@/lib/ipc";
import type { AskUserQuestionDetail } from "@/lib/ipc";

type TestDocument = Document & {
  startViewTransition?: (callback: () => void) => unknown;
};

const originalStartViewTransition = (document as TestDocument).startViewTransition;
```

Find `beforeEach(() => { vi.clearAllMocks(); });` inside the `describe("UserAskWidget", ...)` block and add a matching `afterEach` right after it:

```tsx
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    if (originalStartViewTransition) {
      Object.defineProperty(document, "startViewTransition", {
        configurable: true,
        writable: true,
        value: originalStartViewTransition,
      });
    } else {
      Object.defineProperty(document, "startViewTransition", {
        configurable: true,
        writable: true,
        value: undefined,
      });
    }
  });
```

Then add a new test at the end of the `describe` block, just before its closing `});`:

```tsx
  it("starts a view transition when switching from options to free text, if supported", async () => {
    const startViewTransition = vi.fn((callback: () => void) => {
      callback();
      return {};
    });
    Object.defineProperty(document, "startViewTransition", {
      configurable: true,
      writable: true,
      value: startViewTransition,
    });

    render(<UserAskWidget detail={SINGLE} />);
    await userEvent.click(screen.getByTestId("question-close"));

    expect(startViewTransition).toHaveBeenCalledTimes(1);
  });
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `npx vitest run src/views/workspace/Workspace.test.tsx src/views/chat/tool-widgets/UserAskWidget.test.tsx`
Expected: FAIL — both new tests fail with `startViewTransition` not having been called (neither `refreshMessages` nor the mode switch route through `runViewTransition` yet).

- [ ] **Step 3: Add the `user-ask-module` view-transition keyframes**

In `src/styles/theme.css`, find the existing `chat-composer` group definition:

```css
::view-transition-group(chat-composer) {
  animation-duration: 220ms;
  animation-timing-function: cubic-bezier(0.2, 0, 0, 1);
}
```

Add immediately after it:

```css

::view-transition-group(user-ask-module) {
  animation-duration: 180ms;
  animation-timing-function: cubic-bezier(0.2, 0, 0, 1);
}

::view-transition-old(user-ask-module) {
  animation: doce-ask-module-out 120ms cubic-bezier(0.2, 0, 0, 1) both;
}

::view-transition-new(user-ask-module) {
  animation: doce-ask-module-in 180ms cubic-bezier(0.2, 0, 0, 1) both;
}

@keyframes doce-ask-module-out {
  from {
    opacity: 1;
    transform: translateY(0);
  }

  to {
    opacity: 0;
    transform: translateY(-3px);
  }
}

@keyframes doce-ask-module-in {
  from {
    opacity: 0;
    transform: translateY(3px);
  }

  to {
    opacity: 1;
    transform: translateY(0);
  }
}
```

(No change needed to the existing blanket `@media (prefers-reduced-motion: reduce)` rule — it already targets `::view-transition-group(*)`/`::view-transition-old(*)`/`::view-transition-new(*)` as wildcards, so it automatically covers this new named group too.)

- [ ] **Step 4: Wrap the composer-level arrival in `runViewTransition`**

In `src/views/workspace/Workspace.tsx`, find the top-of-file imports and add `runViewTransition`. Find:

```tsx
import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { ArrowDownIcon } from "@phosphor-icons/react";
import MessageContent from "@/components/MessageContent";
```

Replace with:

```tsx
import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { ArrowDownIcon } from "@phosphor-icons/react";
import MessageContent from "@/components/MessageContent";
import { runViewTransition } from "@/lib/viewTransition";
```

Then find `refreshMessages`'s definition:

```tsx
  const refreshMessages = useCallback(async () => {
    const targetConversationId = conversationId;
    const loadedMessages = await commands.listMessages(targetConversationId);
    if (!isMountedRef.current || currentConversationIdRef.current !== targetConversationId) return;

    setMessages(loadedMessages);
    onConversationSeenRef.current?.(targetConversationId);
  }, [conversationId]);
```

Replace with:

```tsx
  const refreshMessages = useCallback(async () => {
    const targetConversationId = conversationId;
    const loadedMessages = await commands.listMessages(targetConversationId);
    if (!isMountedRef.current || currentConversationIdRef.current !== targetConversationId) return;

    runViewTransition(() => {
      setMessages(loadedMessages);
      onConversationSeenRef.current?.(targetConversationId);
    });
  }, [conversationId]);
```

(`refreshMessages` is called from the `onAgentMessagePersisted` listener, the cross-tab `subscribeToConversationRefresh` listener, and after `/compact`/send resolves — every one of those is a moment `pendingQuestion` could flip, so this single wrap point covers the composer-level arrival in both directions, question appearing and question resolving. It is a *different* function from the raw `setMessages` calls in the conversation-switch effect further down the file — those are intentionally left alone; `App.tsx` already wraps conversation switching in its own, coarser-grained transition.)

- [ ] **Step 5: Wrap the mode switch in `runViewTransition` and add the shared view-transition-name**

Replace the full contents of `src/views/chat/tool-widgets/UserAskWidget.tsx` (identical to Task 1's version, with three changes: the new `runViewTransition` import, a `switchMode` helper wrapping `setMode`, the mode-switch button calling `switchMode` instead of `setMode` directly, and the module region wrapped in a `[view-transition-name:user-ask-module]` div):

```tsx
import { useId, useState } from "react";
import { ArrowLeftIcon, CheckIcon, PaperPlaneRightIcon, XIcon } from "@phosphor-icons/react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/cn";
import { commands, type AskUserQuestionDetail, type QuestionOption } from "@/lib/ipc";
import RichInput from "@/views/chat/rich-input/RichInput";
import { runViewTransition } from "@/lib/viewTransition";

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

// Identical to RichInput's own send button (RichInput.tsx) -- same size,
// same gradient sheen, same icon -- so single-select, multi-select, and
// free text all answer via one visually consistent affordance.
const SUBMIT_BUTTON_CLASSES =
  "h-8 w-8 shrink-0 rounded-full p-0 enabled:bg-gradient-to-r enabled:from-[var(--color-primary)] enabled:via-[var(--color-gray-2)] enabled:to-[var(--color-gray-1)] enabled:hover:from-[var(--color-gray-2)] enabled:hover:via-[var(--color-gray-1)] enabled:hover:to-[var(--color-foreground)]";

/**
 * One option row inside the options module -- a real radio/checkbox
 * control, not a Button pill: a glyph on the left (empty ring/square at
 * rest, filled on selection), the option's label and its description
 * stacked to the right. The description used to be reachable only via a
 * hover `title=` attribute; it's always-visible text now, so keyboard and
 * screen-reader users can read it too.
 */
function OptionRow({
  option,
  selected,
  multiSelect,
  disabled,
  onSelect,
}: {
  option: QuestionOption;
  selected: boolean;
  multiSelect: boolean;
  disabled: boolean;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      role={multiSelect ? "checkbox" : "radio"}
      aria-checked={selected}
      disabled={disabled}
      onClick={onSelect}
      className={cn(
        "flex w-full items-start gap-2.5 rounded-md px-2.5 py-2 text-left text-sm transition-colors",
        selected ? "bg-muted" : "hover:bg-muted",
      )}
    >
      <span
        className={cn(
          "mt-0.5 flex size-4 shrink-0 items-center justify-center border-[1.5px] border-[var(--color-gray-4)]",
          multiSelect ? "rounded-[4px]" : "rounded-full",
          selected && (multiSelect ? "border-primary bg-primary" : "border-foreground"),
        )}
      >
        {selected &&
          (multiSelect ? (
            <CheckIcon size={10} weight="bold" className="text-primary-foreground" />
          ) : (
            <span className="size-2 rounded-full bg-foreground" />
          ))}
      </span>
      <span className="flex min-w-0 flex-col gap-0.5">
        <span className={cn("text-foreground", selected && "font-semibold")}>{option.label}</span>
        {option.description && (
          <span className="text-xs leading-snug text-muted-foreground">{option.description}</span>
        )}
      </span>
    </button>
  );
}

/**
 * The live, still-unanswered `AskUserQuestion` prompt, rendered in the
 * chat composer slot in place of RichInput while a question is pending
 * (Workspace.tsx). One shared, unboxed header (eyebrow + question, one
 * icon button in the same slot in both modes) sits above a single
 * bordered "module": in options mode, a list of real radio/checkbox rows
 * plus a footer holding the one submit button also used by multi-select
 * and free text; in text mode, a bare RichInput (it already supplies its
 * own matching card -- wrapping it in a second border here would double
 * it up, which is exactly what the old implementation did). Picking an
 * option only selects it, single- or multi-select alike; answering
 * always requires pressing the submit button, which stays disabled until
 * at least one option is selected. The close (X) button swaps to free
 * text instead, whose submission answers the question with the raw
 * typed text -- for whenever the fixed option labels don't cover what
 * the user actually wants to say. Once answered, this component unmounts
 * on its own: Workspace.tsx stops rendering it as soon as the resolved
 * tool_result replaces the pending tool_call as the latest message.
 * (Compare AskUserQuestionWidget, which renders the read-only "already
 * answered" state in message history and never handles a live question.)
 *
 * Both the composer-level mount/unmount of this whole component
 * (Workspace.tsx) and the options<->text mode switch within it ride the
 * app's existing view-transition language (runViewTransition,
 * src/lib/viewTransition.ts) -- see switchMode below and the
 * [view-transition-name:user-ask-module] wrapper.
 */
export default function UserAskWidget({ detail, initialMode = "options" }: UserAskWidgetProps) {
  const [mode, setMode] = useState<Mode>(initialMode);
  const [selected, setSelected] = useState<string[]>([]);
  const [submitting, setSubmitting] = useState(false);
  const questionId = useId();

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
      setSelected([label]);
      return;
    }
    setSelected((prev) =>
      prev.includes(label) ? prev.filter((l) => l !== label) : [...prev, label],
    );
  };

  const switchMode = (next: Mode) => {
    runViewTransition(() => setMode(next));
  };

  return (
    <div className="flex flex-col gap-1.5" data-testid="user-ask-widget">
      <div className="flex items-start gap-2">
        <div className="min-w-0 flex-1">
          {mode === "options" && detail.header && (
            <p className="mb-0.5 text-xs text-muted-foreground">{detail.header}</p>
          )}
          <p id={questionId} className="text-sm font-medium text-foreground">
            {mode === "options" ? detail.question : `Answering: ${detail.question}`}
          </p>
        </div>
        <Button
          type="button"
          variant="ghost"
          size="icon-sm"
          className="shrink-0 text-muted-foreground hover:bg-transparent"
          disabled={submitting}
          onClick={() => switchMode(mode === "options" ? "text" : "options")}
          aria-label={mode === "options" ? "Close question" : "Back to options"}
          data-testid={mode === "options" ? "question-close" : "question-back-to-options"}
        >
          {mode === "options" ? <XIcon size={14} /> : <ArrowLeftIcon size={14} />}
        </Button>
      </div>

      <div className="[view-transition-name:user-ask-module]">
        {mode === "text" ? (
          <RichInput
            onSubmit={(content) => {
              if (content.trim()) submit([content]);
            }}
            skillsEnabled={true}
            disabled={submitting}
            placeholder="Type your answer…"
            inputTestId="question-answer-input"
            submitTestId="question-answer-send"
          />
        ) : (
          <div className="flex flex-col gap-2 rounded-lg border border-border bg-card px-3 py-2 shadow-xs transition-shadow focus-within:shadow-sm">
            <div
              className="flex flex-col gap-0.5"
              role={detail.multiSelect ? "group" : "radiogroup"}
              aria-labelledby={questionId}
            >
              {detail.options.map((option) => (
                <OptionRow
                  key={option.label}
                  option={option}
                  selected={selected.includes(option.label)}
                  multiSelect={detail.multiSelect}
                  disabled={submitting}
                  onSelect={() => toggleOption(option.label)}
                />
              ))}
            </div>
            <div className="flex items-center justify-between gap-2">
              <span className="text-xs text-muted-foreground">
                {detail.multiSelect && selected.length > 0 ? `${selected.length} selected` : ""}
              </span>
              <Button
                type="button"
                variant="primary"
                className={SUBMIT_BUTTON_CLASSES}
                disabled={selected.length === 0 || submitting}
                onClick={() => submit(selected)}
                aria-label="Send answer"
                data-testid="question-submit"
              >
                <PaperPlaneRightIcon size={16} />
              </Button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `npx vitest run src/views/workspace/Workspace.test.tsx src/views/chat/tool-widgets/UserAskWidget.test.tsx`
Expected: PASS (both new tests, plus all pre-existing tests in both files still pass unmodified — `runViewTransition` falls back to a synchronous plain call when `document.startViewTransition` is undefined, which is the default in every other test in these files).

- [ ] **Step 7: Format, lint, and run the full suite**

Run: `npx oxfmt src/views/chat/tool-widgets/UserAskWidget.tsx src/views/workspace/Workspace.tsx && npx oxlint src/views/chat/tool-widgets/UserAskWidget.tsx src/views/workspace/Workspace.tsx`
Expected: Clean (this also fixes the re-indentation from Step 5 mechanically).

Run: `npm run test`
Expected: All tests pass, no regressions.

- [ ] **Step 8: Commit**

```bash
git add src/styles/theme.css src/views/chat/tool-widgets/UserAskWidget.tsx src/views/chat/tool-widgets/UserAskWidget.test.tsx src/views/workspace/Workspace.tsx src/views/workspace/Workspace.test.tsx
git commit -m "feat(widgets): animate UserAskWidget's composer arrival and mode switch"
```

---

## Self-Review Notes

- **Spec coverage:** Section 1 (shell/header unification) → Task 1 Step 3. Section 2 (options module) → Task 1 Step 3. Section 3 (submit button + interaction change) → Task 1 Step 3. Section 4 (motion) → Task 2. Section 5 (accessibility: `role`, `aria-checked`, `aria-labelledby`, always-visible descriptions) → Task 1 Step 3, verified by Task 1's new tests. Testing section's specific call-outs (removed `multi-select-indicator`, updated `Workspace.test.tsx` assertions, no new unit tests for transition *mechanics* beyond asserting `startViewTransition` was invoked) → covered by both tasks' test steps.
- **Type consistency:** `UserAskWidgetProps` (`{ detail: AskUserQuestionDetail; initialMode?: Mode }`) is unchanged across both tasks. `commands.answerUserQuestion(questionId: string, answer: string[])` is called identically in Task 1's component and asserted identically in both tasks' tests. `runViewTransition(update: () => void): void`'s signature (from the existing, untouched `src/lib/viewTransition.ts`) matches its usage in both `Workspace.tsx` and `UserAskWidget.tsx` in Task 2.
- **No placeholders:** every step shows complete, runnable code — no "TBD" or "similar to Task N" shortcuts. Task 2 Step 5 replaces `UserAskWidget.tsx`'s full contents outright (rather than a fragile targeted diff around a reindented block) so there's no ambiguity about the file's final state.
