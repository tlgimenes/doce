import fs from "node:fs";
import path from "node:path";
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

  it("gives each option row a staggered entrance animation delay", () => {
    render(<UserAskWidget detail={MULTI} />);

    expect(screen.getByRole("checkbox", { name: /Option A/ })).toHaveStyle({
      animationDelay: "0ms",
    });
    expect(screen.getByRole("checkbox", { name: /Option B/ })).toHaveStyle({
      animationDelay: "18ms",
    });
  });

  it("keeps undefined gray theme variables out of src files", () => {
    const srcRoot = path.join(process.cwd(), "src");
    const pending = [srcRoot];
    const scannedFiles: string[] = [];
    const undefinedGrayToken = ["color", "gray"].join("-");

    while (pending.length > 0) {
      const current = pending.pop();
      if (!current) continue;

      for (const entry of fs.readdirSync(current, { withFileTypes: true })) {
        const nextPath = path.join(current, entry.name);
        if (entry.isDirectory()) {
          pending.push(nextPath);
          continue;
        }

        if (!/\.(ts|tsx|css)$/.test(entry.name) || /\.test\.(ts|tsx)$/.test(entry.name)) continue;
        scannedFiles.push(nextPath);
      }
    }

    const fileWithUndefinedToken = scannedFiles.find((filePath) =>
      fs.readFileSync(filePath, "utf8").includes(undefinedGrayToken),
    );

    expect(fileWithUndefinedToken).toBeUndefined();
  });
});
