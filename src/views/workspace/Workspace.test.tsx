import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import Workspace from "./Workspace";
import { commands, events } from "@/lib/ipc";
import type { RichMessageContent } from "@/lib/ipc";

vi.mock("@/lib/ipc", async (importOriginal) => {
  // Partial mock: `commands`/`events` are fully replaced, but
  // `parseContextNoticeDetail`/`parseToolResultDetail` etc. (real, pure,
  // side-effect-free parsing helpers `MessageContent` calls) stay real
  // rather than needing their own mock entries here.
  const actual = await importOriginal<typeof import("@/lib/ipc")>();
  return {
    ...actual,
    commands: {
      listMessages: vi.fn(),
      sendAgentMessage: vi.fn(),
      getContextUsage: vi.fn(),
      compactConversation: vi.fn(),
      listSkills: vi.fn(),
      answerUserQuestion: vi.fn(),
    },
    events: {
      onAgentMessagePersisted: vi.fn(),
    },
  };
});

describe("Workspace (006-chat-empty-state: conversationId-driven agent view)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(commands.listMessages).mockResolvedValue([]);
    // No model loaded in these unit tests — ContextUsageGauge's
    // getContextUsage call is expected to fail and swallow the error,
    // leaving the gauge simply unrendered.
    vi.mocked(commands.getContextUsage).mockRejectedValue(new Error("No model loaded"));
    vi.mocked(commands.listSkills).mockResolvedValue([]);
    // Streaming (UI refactor): no live events fire by default in these unit
    // tests -- each test that specifically exercises streaming updates
    // messages by driving `listMessages`'s mock resolution/timing directly
    // instead, since the real signal is "listMessages was called again",
    // not the event itself.
    vi.mocked(events.onAgentMessagePersisted).mockResolvedValue(() => {});
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
        tokenCount: null,
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
        tokenCount: null,
      },
    ]);

    render(<Workspace conversationId="conv-1" />);

    await waitFor(() => {
      expect(commands.listMessages).toHaveBeenCalledWith("conv-1");
      expect(screen.getAllByTestId("chat-message")).toHaveLength(2);
    });
  });

  it("sends a task and shows a thinking state until the real (non-streamed) reply returns", async () => {
    // Streaming (UI refactor): `send()` no longer builds the final message
    // from `sendAgentMessage`'s own return value -- it relies on the
    // `finally` block's safety-net `listMessages` refetch (the same one
    // `agent-message-persisted` events would normally trigger live; this
    // test drives it directly via the mock's second resolution instead of
    // firing a real event, since the *effect* -- a fresh transcript once
    // the turn is done -- is what matters here, not the event plumbing
    // itself).
    vi.mocked(commands.listMessages).mockResolvedValueOnce([]).mockResolvedValueOnce([
      {
        id: "u1",
        conversationId: "conv-1",
        role: "user",
        contentType: "text",
        content: "list the files here",
        toolName: null,
        createdAt: 1,
        durationMs: null,
        tokenCount: null,
      },
      {
        id: "a1",
        conversationId: "conv-1",
        role: "assistant",
        contentType: "text",
        content: "Found 3 files: a.rs, b.rs, c.rs",
        toolName: null,
        createdAt: 2,
        durationMs: null,
        tokenCount: null,
      },
    ]);
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

  // --- Streaming (UI refactor): agent-message-persisted mid-turn ---

  it("re-renders with a new tool_call/tool_result pair the moment an agent-message-persisted event fires, before the turn's own promise resolves", async () => {
    let firePersisted!: (p: { conversationId: string }) => void;
    vi.mocked(events.onAgentMessagePersisted).mockImplementation(async (cb) => {
      firePersisted = cb;
      return () => {};
    });

    vi.mocked(commands.listMessages)
      .mockResolvedValueOnce([]) // initial mount
      .mockResolvedValueOnce([
        // what the DB looks like right after the first tool call lands,
        // fetched in response to the live event below
        {
          id: "u1",
          conversationId: "conv-1",
          role: "user",
          contentType: "text",
          content: "list the files here",
          toolName: null,
          createdAt: 1,
          durationMs: null,
          tokenCount: null,
        },
        {
          id: "t1",
          conversationId: "conv-1",
          role: "tool",
          contentType: "tool_result",
          content: JSON.stringify({
            toolName: "Bash",
            command: "ls",
            outcome: { ok: true, exitCode: 0, stdout: "a.rs\nb.rs", stderr: "" },
          }),
          toolName: "Bash",
          createdAt: 2,
          durationMs: null,
          tokenCount: null,
        },
      ]);

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

    // The live event fires mid-turn -- the promise itself is still pending.
    firePersisted({ conversationId: "conv-1" });

    await waitFor(() => {
      expect(screen.getByTestId("bash-widget")).toBeInTheDocument();
    });
    // Still "thinking": the turn's own promise hasn't resolved yet, only a
    // live event landed -- this is the whole point of streaming loop
    // progress separately from the turn's final completion.
    expect(screen.getByTestId("agent-thinking")).toBeInTheDocument();

    resolveAgent("Found 2 files.");
  });

  it("ignores an agent-message-persisted event for a different conversation", async () => {
    let firePersisted!: (p: { conversationId: string }) => void;
    vi.mocked(events.onAgentMessagePersisted).mockImplementation(async (cb) => {
      firePersisted = cb;
      return () => {};
    });
    vi.mocked(commands.listMessages).mockResolvedValue([]);

    render(<Workspace conversationId="conv-1" />);
    await screen.findByTestId("agent-input");
    const callsBefore = vi.mocked(commands.listMessages).mock.calls.length;

    firePersisted({ conversationId: "some-other-conversation" });

    // Give any (incorrect) refetch a chance to happen, then confirm it didn't.
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(vi.mocked(commands.listMessages).mock.calls.length).toBe(callsBefore);
  });

  // --- Regression: a pending AskUserQuestion must be answerable, not a
  // silent hang. Found live: send_agent_message blocks forever on
  // `rx.await` while the model waits for an answer no UI ever showed,
  // holding the one global inference-engine lock the whole time. ---

  it("shows the pending question widget (not \"Working…\") when the latest message is an unanswered AskUserQuestion tool_call, and answering it calls answerUserQuestion", async () => {
    vi.mocked(commands.listMessages).mockResolvedValue([
      {
        id: "u1",
        conversationId: "conv-1",
        role: "user",
        contentType: "text",
        content: "ask me something",
        toolName: null,
        createdAt: 1,
        durationMs: null,
        tokenCount: null,
      },
      {
        id: "tc1",
        conversationId: "conv-1",
        role: "assistant",
        contentType: "tool_call",
        content: JSON.stringify({
          arguments: {
            header: "Quick check",
            question: "What would you like to do?",
            options: [{ label: "A" }, { label: "B" }],
            multiSelect: false,
            questionId: "q1",
          },
        }),
        toolName: "AskUserQuestion",
        createdAt: 2,
        durationMs: null,
        tokenCount: null,
      },
    ]);
    // send_agent_message's own promise never resolves in this test -- it's
    // genuinely still blocked server-side, exactly like the real bug.
    vi.mocked(commands.sendAgentMessage).mockReturnValue(new Promise(() => {}));

    render(<Workspace conversationId="conv-1" />);

    const widget = await screen.findByTestId("question-widget");
    expect(widget).toHaveTextContent("What would you like to do?");
    expect(screen.queryByTestId("agent-thinking")).not.toBeInTheDocument();
    // The composer must not accept a new message while this is pending --
    // that's exactly how a second message ("e?") got queued up behind the
    // same stuck lock in the real incident.
    expect(screen.getByTestId("agent-input")).toHaveAttribute("contenteditable", "false");

    await userEvent.click(screen.getByText("A"));
    expect(commands.answerUserQuestion).toHaveBeenCalledWith("q1", ["A"]);
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

  it("consumes a pending initial turn once without wiping the optimistic first message", async () => {
    let resolveInitialMessages!: (messages: []) => void;
    vi.mocked(commands.listMessages).mockReturnValueOnce(
      new Promise((resolve) => {
        resolveInitialMessages = resolve;
      }),
    );
    vi.mocked(commands.sendAgentMessage).mockReturnValue(new Promise(() => {}));
    const onConsumed = vi.fn();
    const pendingInitialTurn = { conversationId: "conv-1", content: "first task" };

    const { rerender } = render(
      <Workspace
        conversationId="conv-1"
        pendingInitialTurn={pendingInitialTurn}
        onPendingInitialTurnConsumed={onConsumed}
      />,
    );

    await waitFor(() => {
      expect(screen.getByText("first task")).toBeInTheDocument();
      expect(screen.getByTestId("agent-thinking")).toBeInTheDocument();
      expect(commands.sendAgentMessage).toHaveBeenCalledWith("conv-1", "first task", undefined);
      expect(onConsumed).toHaveBeenCalledWith("conv-1");
    });

    resolveInitialMessages([]);
    await waitFor(() => {
      expect(screen.getByText("first task")).toBeInTheDocument();
    });

    rerender(
      <Workspace
        conversationId="conv-1"
        pendingInitialTurn={pendingInitialTurn}
        onPendingInitialTurnConsumed={onConsumed}
      />,
    );

    expect(commands.sendAgentMessage).toHaveBeenCalledTimes(1);
    expect(onConsumed).toHaveBeenCalledTimes(1);
  });

  it("preserves the optimistic pending initial turn after the parent clears the consumed prop", async () => {
    let resolveInitialMessages!: (messages: []) => void;
    vi.mocked(commands.listMessages).mockReturnValueOnce(
      new Promise((resolve) => {
        resolveInitialMessages = resolve;
      }),
    );
    vi.mocked(commands.sendAgentMessage).mockReturnValue(new Promise(() => {}));
    const onConsumed = vi.fn();
    const pendingInitialTurn = { conversationId: "conv-1", content: "first task" };

    const { rerender } = render(
      <Workspace
        conversationId="conv-1"
        pendingInitialTurn={pendingInitialTurn}
        onPendingInitialTurnConsumed={onConsumed}
      />,
    );

    await waitFor(() => {
      expect(screen.getByText("first task")).toBeInTheDocument();
      expect(onConsumed).toHaveBeenCalledWith("conv-1");
    });

    rerender(
      <Workspace
        conversationId="conv-1"
        pendingInitialTurn={null}
        onPendingInitialTurnConsumed={onConsumed}
      />,
    );
    resolveInitialMessages([]);
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(screen.getByText("first task")).toBeInTheDocument();
    expect(screen.getByTestId("agent-thinking")).toBeInTheDocument();
  });

  it("forwards rich content from a pending initial turn as a JSON string", async () => {
    const richContent: RichMessageContent = {
      segments: [
        { type: "text", text: "review this" },
        { type: "pastedText", text: "line 1\nline 2" },
      ],
    };
    vi.mocked(commands.sendAgentMessage).mockReturnValue(new Promise(() => {}));

    render(
      <Workspace
        conversationId="conv-1"
        pendingInitialTurn={{ conversationId: "conv-1", content: "review this", richContent }}
      />,
    );

    await waitFor(() => {
      expect(commands.sendAgentMessage).toHaveBeenCalledWith(
        "conv-1",
        "review this",
        JSON.stringify(richContent),
      );
    });
  });

  it("surfaces an error when a pending initial turn send fails", async () => {
    vi.mocked(commands.sendAgentMessage).mockRejectedValue(new Error("pending send failed"));

    render(
      <Workspace
        conversationId="conv-1"
        pendingInitialTurn={{ conversationId: "conv-1", content: "first task" }}
      />,
    );

    await waitFor(() => {
      expect(screen.getByTestId("workspace-error")).toHaveTextContent("pending send failed");
    });
  });

  it("marks the workspace composer shell for chat composer view transitions", async () => {
    render(<Workspace conversationId="conv-1" />);

    const shell = await screen.findByTestId("workspace-composer-shell");
    expect(shell).toHaveClass("border-t", "border-border", "p-4");
    expect(shell.className).toContain("[view-transition-name:chat-composer]");
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
          tokenCount: null,
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

  it("ignores stale listMessages results from a previous conversation", async () => {
    let resolveConv1Messages!: (messages: Awaited<ReturnType<typeof commands.listMessages>>) => void;
    let resolveConv2Messages!: (messages: Awaited<ReturnType<typeof commands.listMessages>>) => void;
    vi.mocked(commands.listMessages)
      .mockReturnValueOnce(
        new Promise((resolve) => {
          resolveConv1Messages = resolve;
        }),
      )
      .mockReturnValueOnce(
        new Promise((resolve) => {
          resolveConv2Messages = resolve;
        }),
      );

    const { rerender } = render(<Workspace conversationId="conv-1" />);
    await waitFor(() => expect(commands.listMessages).toHaveBeenCalledWith("conv-1"));

    rerender(<Workspace conversationId="conv-2" />);
    await waitFor(() => expect(commands.listMessages).toHaveBeenCalledWith("conv-2"));

    resolveConv2Messages([
      {
        id: "m2",
        conversationId: "conv-2",
        role: "user",
        contentType: "text",
        content: "second workspace",
        toolName: null,
        createdAt: 2,
        durationMs: null,
        tokenCount: null,
      },
    ]);
    await screen.findByText("second workspace");

    resolveConv1Messages([
      {
        id: "m1",
        conversationId: "conv-1",
        role: "user",
        contentType: "text",
        content: "stale first workspace",
        toolName: null,
        createdAt: 1,
        durationMs: null,
        tokenCount: null,
      },
    ]);
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(screen.getByText("second workspace")).toBeInTheDocument();
    expect(screen.queryByText("stale first workspace")).not.toBeInTheDocument();
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

  // --- 010-context-window-management (UI refactor): /compact command ---

  it("typing /compact triggers compaction directly instead of sending a normal agent turn", async () => {
    vi.mocked(commands.compactConversation).mockResolvedValue({
      conversationId: "conv-1",
      tokensUsed: 100,
      tokenBudget: 2048,
      state: "justCompacted",
    });
    vi.mocked(commands.listMessages).mockResolvedValueOnce([]).mockResolvedValueOnce([
      {
        id: "notice-1",
        conversationId: "conv-1",
        role: "assistant",
        contentType: "context_notice",
        content: JSON.stringify({
          kind: "summarized",
          summary: "the gist of it",
          notice: "Conversation condensed to save space",
        }),
        toolName: null,
        createdAt: Date.now(),
        durationMs: null,
        tokenCount: null,
      },
    ]);

    render(<Workspace conversationId="conv-1" />);
    await screen.findByTestId("agent-input");
    await userEvent.type(screen.getByTestId("agent-input"), "/compact");
    await userEvent.click(screen.getByTestId("agent-send"));

    await waitFor(() => expect(commands.compactConversation).toHaveBeenCalledWith("conv-1"));
    expect(commands.sendAgentMessage).not.toHaveBeenCalled();
    expect(await screen.findByTestId("context-notice")).toHaveTextContent(
      "Conversation condensed to save space",
    );
  });
});
