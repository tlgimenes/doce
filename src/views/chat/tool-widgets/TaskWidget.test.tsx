import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import TaskWidget from "./TaskWidget";
import type { TaskDetail } from "@/lib/ipc";

describe("TaskWidget (004-tool-call-widgets, US4)", () => {
  it("renders a complete status indicator, never the subagent's own content (FR-010)", () => {
    const detail: TaskDetail = {
      toolName: "Task",
      prompt: "go research the codebase structure",
      subagentConversationId: "sub-1",
      state: "complete",
    };
    render(<TaskWidget detail={detail} />);
    expect(screen.getByTestId("task-status")).toHaveTextContent(/complete/i);
    // The delegated prompt is fine to show (it's what the parent asked
    // for) — but nothing about what the subagent actually did internally.
    expect(screen.queryByText(/sub-1/)).not.toBeInTheDocument();
  });

  it("renders a running status indicator when state is running", () => {
    const detail: TaskDetail = {
      toolName: "Task",
      prompt: "go research the codebase structure",
      subagentConversationId: "sub-1",
      state: "running",
    };
    render(<TaskWidget detail={detail} />);
    expect(screen.getByTestId("task-status")).toHaveTextContent(/running/i);
  });
});
