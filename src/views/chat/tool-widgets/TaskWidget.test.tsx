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
    const status = screen.getByTestId("task-status");
    expect(status).toHaveTextContent(/complete/i);
    expect(status.querySelector('[data-slot="spinner"]')).toBeNull();
    const badge = screen.getByTestId("task-widget").querySelector('[data-slot="badge"]');
    expect(badge).toHaveTextContent("Complete");
    // The delegated prompt is fine to show (it's what the parent asked
    // for) — but nothing about what the subagent actually did internally.
    expect(screen.queryByText(/sub-1/)).not.toBeInTheDocument();
    expect(screen.getByText("go research the codebase structure")).toBeInTheDocument();
  });

  it("renders a running status indicator when state is running", () => {
    const detail: TaskDetail = {
      toolName: "Task",
      prompt: "go research the codebase structure",
      subagentConversationId: "sub-1",
      state: "running",
    };
    render(<TaskWidget detail={detail} />);
    const status = screen.getByTestId("task-status");
    expect(status).toHaveTextContent(/running/i);
    expect(status.querySelector('[data-slot="spinner"]')).not.toBeNull();
    // No badge while running — the spinner + "Running…" carries the state.
    expect(screen.getByTestId("task-widget").querySelector('[data-slot="badge"]')).toBeNull();
    // This frame stays header-only (no collapsible body) — the prompt is
    // visible without any click.
    expect(screen.getByText("go research the codebase structure")).toBeInTheDocument();
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
    const status = screen.getByTestId("task-status");
    expect(status).toHaveTextContent(/interrupted/i);
    expect(status).not.toHaveTextContent(/complete/i);
    const badge = screen.getByTestId("task-widget").querySelector('[data-slot="badge"]');
    expect(badge).toHaveTextContent("Interrupted");
  });
});
