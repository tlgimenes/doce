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
