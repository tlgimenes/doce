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

  it("submitting a whitespace-only free text answer does not answer the question", async () => {
    vi.mocked(commands.answerUserQuestion).mockResolvedValue(undefined);
    render(<UserAskWidget detail={SINGLE} />);

    await userEvent.click(screen.getByTestId("question-close"));
    const editable = screen.getByTestId("question-answer-input");
    await userEvent.click(editable);
    await userEvent.type(editable, "   {Enter}");

    expect(commands.answerUserQuestion).not.toHaveBeenCalled();
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
