import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import Workspace from "./Workspace";
import { commands } from "@/lib/ipc";

vi.mock("@/lib/ipc", () => ({
  commands: {
    listMessages: vi.fn(),
    sendAgentMessage: vi.fn(),
  },
}));

describe("Workspace (006-chat-empty-state: conversationId-driven agent view)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(commands.listMessages).mockResolvedValue([]);
  });

  it("loads and renders a workspace-scoped conversation's existing messages", async () => {
    vi.mocked(commands.listMessages).mockResolvedValue([
      {
        id: "m1",
        conversationId: "conv-1",
        role: "user",
        contentType: "text",
        content: "hi",
        toolName: null,
        createdAt: 1,
        durationMs: null,
      },
      {
        id: "m2",
        conversationId: "conv-1",
        role: "assistant",
        contentType: "text",
        content: "hello",
        toolName: null,
        createdAt: 2,
        durationMs: 5,
      },
    ]);

    render(<Workspace conversationId="conv-1" />);

    await waitFor(() => {
      expect(commands.listMessages).toHaveBeenCalledWith("conv-1");
      expect(screen.getAllByTestId("chat-message")).toHaveLength(2);
    });
  });

  it("sends a task and shows a thinking state until the real (non-streamed) reply returns", async () => {
    let resolveAgent!: (value: string) => void;
    vi.mocked(commands.sendAgentMessage).mockReturnValue(
      new Promise((resolve) => {
        resolveAgent = resolve;
      }),
    );

    render(<Workspace conversationId="conv-1" />);
    await screen.findByTestId("agent-input");

    await userEvent.type(screen.getByTestId("agent-input"), "list the files here");
    await userEvent.click(screen.getByTestId("agent-send"));

    await waitFor(() => expect(screen.getByTestId("agent-thinking")).toBeInTheDocument());

    resolveAgent("Found 3 files: a.rs, b.rs, c.rs");
    await waitFor(() => {
      expect(screen.queryByTestId("agent-thinking")).not.toBeInTheDocument();
      expect(screen.getByText(/Found 3 files/)).toBeInTheDocument();
    });

    // Guards against the user's turn being dropped or reordered.
    const renderedMessages = screen.getAllByTestId("chat-message");
    expect(renderedMessages).toHaveLength(2);
    expect(renderedMessages[0].textContent).toContain("list the files here");
    expect(renderedMessages[1].textContent).toContain("Found 3 files");
  });

  it("009-rich-chat-input regression: a message containing a chip forwards richContent to sendAgentMessage, not just the flat text", async () => {
    vi.mocked(commands.sendAgentMessage).mockResolvedValue("ok");

    render(<Workspace conversationId="conv-1" />);
    const input = await screen.findByTestId("agent-input");

    // Crosses the paste-collapse threshold — produces a real pastedText
    // chip, matching RichInput's own US2 test's paste-simulation pattern.
    const pastedBlock = Array.from({ length: 15 }, (_, i) => `line-${i}`).join("\n");
    fireEvent.paste(input, { clipboardData: { items: [], getData: () => pastedBlock } });
    await screen.findByTestId("pasted-text-chip");

    await userEvent.click(screen.getByTestId("agent-send"));

    await waitFor(() => expect(commands.sendAgentMessage).toHaveBeenCalled());
    const [, , richContentArg] = vi.mocked(commands.sendAgentMessage).mock.calls[0];
    expect(richContentArg).toBeDefined();
    const parsed = JSON.parse(richContentArg as string);
    expect(
      parsed.segments.some(
        (s: { type: string; text?: string }) => s.type === "pastedText" && s.text === pastedBlock,
      ),
    ).toBe(true);
  });

  it("switching to a different conversationId reloads its own messages", async () => {
    vi.mocked(commands.listMessages)
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([
        {
          id: "m3",
          conversationId: "conv-2",
          role: "user",
          contentType: "text",
          content: "second workspace",
          toolName: null,
          createdAt: 1,
          durationMs: null,
        },
      ]);

    const { rerender } = render(<Workspace conversationId="conv-1" />);
    await waitFor(() => expect(commands.listMessages).toHaveBeenCalledWith("conv-1"));

    rerender(<Workspace conversationId="conv-2" />);
    await waitFor(() => {
      expect(commands.listMessages).toHaveBeenCalledWith("conv-2");
      expect(screen.getByText("second workspace")).toBeInTheDocument();
    });
  });

  it("shows an error instead of hanging if sending fails", async () => {
    vi.mocked(commands.sendAgentMessage).mockRejectedValue(new Error("inference crashed"));

    render(<Workspace conversationId="conv-1" />);
    await screen.findByTestId("agent-input");
    await userEvent.type(screen.getByTestId("agent-input"), "do something");
    await userEvent.click(screen.getByTestId("agent-send"));

    await waitFor(() => {
      expect(screen.getByTestId("workspace-error")).toHaveTextContent("inference crashed");
    });
  });
});
