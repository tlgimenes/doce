import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import TaskWidget from "./TaskWidget";
import { commands, type Message, type TaskDetail } from "@/lib/ipc";

vi.mock("@/lib/ipc", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc")>();
  return {
    ...actual,
    commands: {
      listMessages: vi.fn(),
    },
  };
});

const readResultMessage = (id: string, filePath: string): Message => ({
  id,
  conversationId: "sub-1",
  role: "tool",
  contentType: "tool_result",
  content: JSON.stringify({
    toolName: "Read",
    filePath,
    offset: null,
    limit: null,
    outcome: { ok: true, content: "x", truncated: false },
  }),
  toolName: "Read",
  createdAt: 1,
  durationMs: null,
  tokenCount: null,
});

describe("TaskWidget (004-tool-call-widgets, US4)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("summarizes a complete delegation as explored files, never the subagent's own content (FR-010)", async () => {
    vi.mocked(commands.listMessages).mockResolvedValue([
      readResultMessage("m1", "/tmp/a.rs"),
      readResultMessage("m2", "/tmp/b.rs"),
      // A repeat read of the same file must not inflate the count.
      readResultMessage("m3", "/tmp/a.rs"),
    ]);
    const detail: TaskDetail = {
      toolName: "Task",
      prompt: "go research the codebase structure",
      subagentConversationId: "sub-1",
      state: "complete",
    };
    render(<TaskWidget detail={detail} />);
    const status = screen.getByTestId("task-status");
    await waitFor(() => expect(status).toHaveTextContent("Explored 2 files"));
    expect(status.querySelector('[data-slot="spinner"]')).toBeNull();
    // No badge — the outcome sentence carries the state on its own.
    expect(screen.getByTestId("task-widget").querySelector('[data-slot="badge"]')).toBeNull();
    // The delegated prompt is fine to show (it's what the parent asked
    // for) — but nothing about what the subagent actually did internally.
    expect(screen.queryByText(/sub-1/)).not.toBeInTheDocument();
    expect(screen.getByText("go research the codebase structure")).toBeInTheDocument();
  });

  it("falls back to a verb-only sentence when the subagent read no files", async () => {
    vi.mocked(commands.listMessages).mockResolvedValue([]);
    const detail: TaskDetail = {
      toolName: "Task",
      prompt: "go research the codebase structure",
      subagentConversationId: "sub-1",
      state: "complete",
    };
    render(<TaskWidget detail={detail} />);
    await waitFor(() => expect(commands.listMessages).toHaveBeenCalledWith("sub-1"));
    expect(screen.getByTestId("task-status")).toHaveTextContent("Finished exploring");
  });

  it("renders a shimmering exploring indicator while running", () => {
    vi.mocked(commands.listMessages).mockResolvedValue([]);
    const detail: TaskDetail = {
      toolName: "Task",
      prompt: "go research the codebase structure",
      subagentConversationId: "sub-1",
      state: "running",
    };
    render(<TaskWidget detail={detail} />);
    const status = screen.getByTestId("task-status");
    expect(status).toHaveTextContent(/exploring/i);
    expect(status).toHaveClass("shimmer");
    // Shimmer only — no spinner icon at all while running.
    expect(status.querySelector('[data-slot="spinner"]')).toBeNull();
    // No badge while running — the shimmer alone carries the state.
    expect(screen.getByTestId("task-widget").querySelector('[data-slot="badge"]')).toBeNull();
    // Running tasks never fetch the subagent transcript.
    expect(commands.listMessages).not.toHaveBeenCalled();
    // This frame stays header-only (no collapsible body) — the prompt is
    // visible without any click.
    expect(screen.getByText("go research the codebase structure")).toBeInTheDocument();
  });

  it("renders an interrupted status — never a false Complete — for a healed crash-orphaned delegation", () => {
    vi.mocked(commands.listMessages).mockResolvedValue([]);
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
    expect(status).not.toHaveTextContent(/explored/i);
    const badge = screen.getByTestId("task-widget").querySelector('[data-slot="badge"]');
    expect(badge).toHaveTextContent("Interrupted");
    expect(commands.listMessages).not.toHaveBeenCalled();
  });
});
