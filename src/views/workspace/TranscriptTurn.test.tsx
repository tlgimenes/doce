import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { BashDetail, Message, TaskDetail } from "@/lib/ipc";
import type { TranscriptTurn as TranscriptTurnModel } from "./transcriptTurns";
import TranscriptTurn, { type PendingTurnWidget } from "./TranscriptTurn";

function message({ id, ...overrides }: Partial<Message> & { id: string }): Message {
  return {
    id,
    conversationId: "conv-1",
    role: "assistant",
    contentType: "text",
    content: id,
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
  it("renders a sticky user header above assistant rows", () => {
    render(<TranscriptTurn turn={turn({})} />);

    const transcriptTurn = screen.getByTestId("transcript-turn");
    const body = screen.getByTestId("transcript-turn-body");

    expect(screen.getByTestId("sticky-user-background")).toHaveClass(
      "sticky",
      "top-0",
      "z-40",
      "h-4",
      "w-full",
      "bg-background",
    );
    expect(transcriptTurn.querySelector('[data-sticky-user-message="true"]')).not.toBeNull();
    expect(within(transcriptTurn).getByText("run the tests")).toBeInTheDocument();
    expect(within(body).getByText("done")).toBeInTheDocument();
  });

  it("re-anchors the owning turn when the sticky user bubble is focused", () => {
    const scrollIntoView = vi.fn();
    const originalScrollIntoView = HTMLElement.prototype.scrollIntoView;
    Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
      configurable: true,
      value: scrollIntoView,
    });

    try {
      render(<TranscriptTurn turn={turn({})} />);

      fireEvent.focus(screen.getByTestId("sticky-user-message-bubble"));

      expect(scrollIntoView).toHaveBeenCalledWith({
        behavior: "smooth",
        block: "start",
      });
    } finally {
      Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
        configurable: true,
        value: originalScrollIntoView,
      });
    }
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

  it("renders pending Bash and Task widgets when supplied", () => {
    const pendingBash: BashDetail = {
      toolName: "Bash",
      command: "cargo test --lib",
      timeoutMs: null,
    };
    const pendingTask: TaskDetail = {
      toolName: "Task",
      prompt: "Summarize the run",
      subagentConversationId: "sub-1",
      state: "running",
    };

    const bashWidget: PendingTurnWidget = { kind: "bash", detail: pendingBash };
    const taskWidget: PendingTurnWidget = { kind: "task", detail: pendingTask };

    const { rerender } = render(
      <TranscriptTurn turn={turn({})} pendingWidget={bashWidget} isLastTurn />,
    );

    expect(screen.getByTestId("bash-widget")).toBeInTheDocument();

    rerender(<TranscriptTurn turn={turn({})} pendingWidget={taskWidget} isLastTurn />);

    expect(screen.getByTestId("task-widget")).toBeInTheDocument();
  });

  it("renders error content inside the turn", () => {
    render(<TranscriptTurn turn={turn({})} error="send failed" />);

    expect(screen.getByText("send failed")).toBeInTheDocument();
  });
});
