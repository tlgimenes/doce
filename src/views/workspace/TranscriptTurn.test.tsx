import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
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
  it("renders the user message as a chat row above assistant rows", () => {
    render(<TranscriptTurn turn={turn({})} />);

    const transcriptTurn = screen.getByTestId("transcript-turn");
    const body = screen.getByTestId("transcript-turn-body");

    expect(transcriptTurn).toHaveAttribute("data-slot", "message-group");
    expect(screen.queryByTestId("sticky-user-background")).not.toBeInTheDocument();
    expect(document.querySelector('[data-sticky-user-message="true"]')).toBeNull();
    expect(within(transcriptTurn).getByText("run the tests")).toBeInTheDocument();
    expect(within(body).getByText("done")).toBeInTheDocument();
    // The user row precedes the body in DOM order.
    const userRow = within(transcriptTurn).getByRole("group", { name: "You said" });
    expect(userRow.compareDocumentPosition(body) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it("renders assistant-only turns without a user row", () => {
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
    expect(screen.queryByRole("group", { name: "You said" })).not.toBeInTheDocument();
  });

  it("marks transcript turns with chat primitive data attributes", () => {
    render(<TranscriptTurn turn={turn({})} />);

    expect(screen.getByTestId("transcript-turn")).toHaveAttribute("data-chat-turn", "true");
    expect(screen.getByTestId("transcript-turn-body")).toHaveClass("min-w-0");
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

  it("renders error content as a destructive alert inside the turn", () => {
    render(<TranscriptTurn turn={turn({})} error="send failed" />);

    const alert = screen.getByTestId("workspace-error");
    expect(alert).toHaveAttribute("data-slot", "alert");
    expect(alert).toHaveTextContent("send failed");
  });
});
