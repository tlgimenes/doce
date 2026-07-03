import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import Workspace from "./Workspace";
import { commands } from "@/lib/ipc";

vi.mock("@/lib/ipc", () => ({
  commands: {
    openWorkspace: vi.fn(),
    createConversation: vi.fn(),
    sendAgentMessage: vi.fn(),
  },
}));

describe("Workspace (User Story 3: open a folder to enter agent mode)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("opening a folder creates a workspace-scoped conversation and shows the agent chat", async () => {
    vi.mocked(commands.openWorkspace).mockResolvedValue({
      id: "ws-1",
      path: "/tmp/project",
      displayName: "project",
      createdAt: 1,
      lastOpenedAt: 1,
    });
    vi.mocked(commands.createConversation).mockResolvedValue({
      id: "conv-1",
      workspaceId: "ws-1",
      title: "New conversation",
      createdAt: 1,
      updatedAt: 1,
      status: "done",
    });

    render(<Workspace />);
    await userEvent.type(screen.getByTestId("workspace-path-input"), "/tmp/project");
    await userEvent.click(screen.getByTestId("open-workspace"));

    await waitFor(() => {
      expect(commands.createConversation).toHaveBeenCalledWith("ws-1");
      expect(screen.getByTestId("agent-input")).toBeInTheDocument();
    });
  });

  it("sends a task and shows a thinking state until the real (non-streamed) reply returns", async () => {
    vi.mocked(commands.openWorkspace).mockResolvedValue({
      id: "ws-1",
      path: "/tmp/project",
      displayName: "project",
      createdAt: 1,
      lastOpenedAt: 1,
    });
    vi.mocked(commands.createConversation).mockResolvedValue({
      id: "conv-1",
      workspaceId: "ws-1",
      title: "New conversation",
      createdAt: 1,
      updatedAt: 1,
      status: "done",
    });

    let resolveAgent!: (value: string) => void;
    vi.mocked(commands.sendAgentMessage).mockReturnValue(
      new Promise((resolve) => {
        resolveAgent = resolve;
      }),
    );

    render(<Workspace />);
    await userEvent.type(screen.getByTestId("workspace-path-input"), "/tmp/project");
    await userEvent.click(screen.getByTestId("open-workspace"));
    await screen.findByTestId("agent-input");

    await userEvent.type(screen.getByTestId("agent-input"), "list the files here");
    await userEvent.click(screen.getByTestId("agent-send"));

    await waitFor(() => expect(screen.getByTestId("agent-thinking")).toBeInTheDocument());

    resolveAgent("Found 3 files: a.rs, b.rs, c.rs");
    await waitFor(() => {
      expect(screen.queryByTestId("agent-thinking")).not.toBeInTheDocument();
      expect(screen.getByText(/Found 3 files/)).toBeInTheDocument();
    });

    // Guards against the user's turn being dropped or reordered (e.g. an
    // accidental setMessages([reply]) instead of appending) — mirrors the
    // equivalent regression guard in Chat.test.tsx.
    const renderedMessages = screen.getAllByTestId("chat-message");
    expect(renderedMessages).toHaveLength(2);
    expect(renderedMessages[0].textContent).toContain("list the files here");
    expect(renderedMessages[1].textContent).toContain("Found 3 files");
  });

  it("shows an error instead of hanging if opening the workspace fails", async () => {
    vi.mocked(commands.openWorkspace).mockRejectedValue(new Error("not a directory"));

    render(<Workspace />);
    await userEvent.type(screen.getByTestId("workspace-path-input"), "/not/a/real/path");
    await userEvent.click(screen.getByTestId("open-workspace"));

    await waitFor(() => {
      expect(screen.getByText(/not a directory/)).toBeInTheDocument();
    });
  });
});
