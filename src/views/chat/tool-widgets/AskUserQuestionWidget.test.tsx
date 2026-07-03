import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import AskUserQuestionWidget from "./AskUserQuestionWidget";
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

describe("AskUserQuestionWidget (004-tool-call-widgets, US3)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders clickable options and indicates single-select", () => {
    render(<AskUserQuestionWidget detail={SINGLE} />);
    expect(screen.getByText("Which way should I go?")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Option A/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Option B/ })).toBeInTheDocument();
    expect(screen.queryByTestId("question-submit")).not.toBeInTheDocument();
  });

  it("clicking an option in a single-select question answers immediately (FR-008)", async () => {
    vi.mocked(commands.answerUserQuestion).mockResolvedValue(undefined);
    render(<AskUserQuestionWidget detail={SINGLE} />);

    await userEvent.click(screen.getByRole("button", { name: /Option A/ }));

    expect(commands.answerUserQuestion).toHaveBeenCalledWith("q1", ["Option A"]);
  });

  it("indicates multi-select and requires an explicit confirm before answering", async () => {
    vi.mocked(commands.answerUserQuestion).mockResolvedValue(undefined);
    render(<AskUserQuestionWidget detail={MULTI} />);

    expect(screen.getByTestId("multi-select-indicator")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /Option A/ }));
    await userEvent.click(screen.getByRole("button", { name: /Option B/ }));
    expect(commands.answerUserQuestion).not.toHaveBeenCalled();

    await userEvent.click(screen.getByTestId("question-submit"));
    expect(commands.answerUserQuestion).toHaveBeenCalledWith("q2", ["Option A", "Option B"]);
  });

  it("once answered, renders a read-only state showing the chosen option(s) and accepts no further input (FR-009)", () => {
    const answered: AskUserQuestionDetail = { ...SINGLE, answer: ["Option A"] };
    render(<AskUserQuestionWidget detail={answered} />);

    expect(screen.getByTestId("question-answered")).toHaveTextContent("Option A");
    expect(screen.queryByRole("button", { name: /Option A/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Option B/ })).not.toBeInTheDocument();
  });
});
