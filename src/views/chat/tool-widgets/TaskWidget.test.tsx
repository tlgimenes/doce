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

  it("renders an interrupted status — never a false Complete — for a healed crash-orphaned delegation", () => {
    // storage::heal_interrupted_tool_calls pairs a crash-orphaned Task
    // call with state:"complete" + interrupted:true; showing a green
    // Complete badge for work that never finished misleads the user.
    const detail: TaskDetail = {
      toolName: "Task",
      prompt: "go research the codebase structure",
      subagentConversationId: "",
      state: "complete",
      interrupted: true,
    };
    render(<TaskWidget detail={detail} />);
    expect(screen.getByTestId("task-status")).toHaveTextContent(/interrupted/i);
    expect(screen.getByTestId("task-status")).not.toHaveTextContent(/complete/i);
  });
});
