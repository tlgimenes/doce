import { StrictMode } from "react";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import Workspace, { __resetSendInFlightRegistryForTests } from "./Workspace";
import { __resetQueueRegistryForTests } from "./messageQueueRegistry";
import { commands, events } from "@/lib/ipc";
import type { RichMessageContent } from "@/lib/ipc";

type TestDocument = Document & {
  startViewTransition?: (callback: () => void) => unknown;
};

const originalStartViewTransition = (document as TestDocument).startViewTransition;

const scrollToEndSpy = vi.hoisted(() => vi.fn());
vi.mock("@shadcn/react/message-scroller", async (importOriginal) => {
  const original = await importOriginal<typeof import("@shadcn/react/message-scroller")>();
  return {
    ...original,
    useMessageScroller: () => ({
      scrollToEnd: scrollToEndSpy,
      scrollToMessage: vi.fn(),
      scrollToStart: vi.fn(),
    }),
  };
});

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
      isGenerationActive: vi.fn(),
      stopGeneration: vi.fn(),
      steerGeneration: vi.fn(),
      getActivePlan: vi.fn(),
    },
    events: {
      onAgentMessagePersisted: vi.fn(),
      onAgentGenerationPiece: vi.fn(),
      onPlanUpdate: vi.fn(),
    },
  };
});

function messageFixture(id: string, content: string, createdAt = 1) {
  return {
    id,
    conversationId: "conv-1",
    role: "user" as const,
    contentType: "text" as const,
    content,
    toolName: null,
    createdAt,
    durationMs: null,
    tokenCount: null,
  };
}

function expectElementBefore(first: HTMLElement, second: HTMLElement) {
  expect(Boolean(first.compareDocumentPosition(second) & Node.DOCUMENT_POSITION_FOLLOWING)).toBe(
    true,
  );
}

describe("Workspace (006-chat-empty-state: conversationId-driven agent view)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    scrollToEndSpy.mockClear();
    // The in-flight-send registry is module-global and survives unmount, so
    // a prior test that left a turn pending (never-resolving `sendAgentMessage`
    // mock) would otherwise leak `turnInFlight` into this test.
    __resetSendInFlightRegistryForTests();
    // Queue & steer: the queue registry is module-global and survives unmount
    // too, so reset it between tests just like the send-in-flight registry.
    __resetQueueRegistryForTests();
    vi.mocked(commands.listMessages).mockResolvedValue([]);
    vi.mocked(commands.isGenerationActive).mockResolvedValue(false);
    vi.mocked(commands.stopGeneration).mockResolvedValue(undefined);
    vi.mocked(commands.steerGeneration).mockResolvedValue("injected");
    // No model loaded in these unit tests — ContextUsageIndicator's
    // getContextUsage call is expected to fail and swallow the error,
    // leaving the indicator simply unrendered.
    vi.mocked(commands.getContextUsage).mockRejectedValue(new Error("No model loaded"));
    vi.mocked(commands.listSkills).mockResolvedValue([]);
    // Streaming (UI refactor): no live events fire by default in these unit
    // tests -- each test that specifically exercises streaming updates
    // messages by driving `listMessages`'s mock resolution/timing directly
    // instead, since the real signal is "listMessages was called again",
    // not the event itself.
    vi.mocked(events.onAgentMessagePersisted).mockResolvedValue(() => {});
    vi.mocked(events.onAgentGenerationPiece).mockResolvedValue(() => {});
    vi.mocked(commands.getActivePlan).mockResolvedValue(null);
    vi.mocked(events.onPlanUpdate).mockResolvedValue(() => {});
  });

  afterEach(() => {
    if (originalStartViewTransition) {
      Object.defineProperty(document, "startViewTransition", {
        configurable: true,
        writable: true,
        value: originalStartViewTransition,
      });
    } else {
      Object.defineProperty(document, "startViewTransition", {
        configurable: true,
        writable: true,
        value: undefined,
      });
    }
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

  it("renders assistant duration metadata beside tokens for persisted transcript replies", async () => {
    vi.mocked(commands.listMessages).mockResolvedValue([
      {
        id: "m1",
        conversationId: "conv-1",
        role: "assistant",
        contentType: "text",
        content: "hello",
        toolName: null,
        createdAt: 2,
        durationMs: 500,
        tokenCount: 15600,
      },
    ]);

    render(<Workspace conversationId="conv-1" />);

    await waitFor(() => expect(screen.getByTestId("token-meter")).toHaveTextContent("0.5s"));
  });

  it("fills the shell content area instead of forcing viewport height", async () => {
    render(<Workspace conversationId="conv-1" />);

    const root = screen
      .getByTestId("workspace-scroll-container")
      .closest('[data-slot="message-scroller"]');
    expect(root).not.toBeNull();
    expect(root!).toHaveClass("h-auto");
    expect(root!).not.toHaveClass("h-dvh");
    await waitFor(() => expect(commands.listMessages).toHaveBeenCalledWith("conv-1"));
  });

  it("renders user messages as turn anchors that own following assistant rows", async () => {
    vi.mocked(commands.listMessages).mockResolvedValue([
      messageFixture("u1", "first request", 1),
      {
        id: "a1",
        conversationId: "conv-1",
        role: "assistant",
        contentType: "text",
        content: "first answer",
        toolName: null,
        createdAt: 2,
        durationMs: null,
        tokenCount: null,
      },
      messageFixture("u2", "second request", 3),
      {
        id: "a2",
        conversationId: "conv-1",
        role: "assistant",
        contentType: "text",
        content: "second answer",
        toolName: null,
        createdAt: 4,
        durationMs: null,
        tokenCount: null,
      },
    ]);

    render(<Workspace conversationId="conv-1" />);

    await screen.findByText("second answer");

    const turns = screen.getAllByTestId("transcript-turn");
    expect(turns).toHaveLength(2);
    expect(turns[0]).toHaveTextContent("first request");
    expect(turns[0]).toHaveTextContent("first answer");
    expect(turns[0]).not.toHaveTextContent("second request");
    expect(turns[1]).toHaveTextContent("second request");
    expect(turns[1]).toHaveTextContent("second answer");
    expect(screen.getAllByRole("group", { name: "You said" })).toHaveLength(2);
  });

  it("renders the transcript inside the shadcn message scroller", async () => {
    vi.mocked(commands.listMessages).mockResolvedValue([]);
    render(<Workspace conversationId="conv-1" />);
    const viewport = await screen.findByTestId("workspace-scroll-container");
    const content = screen.getByTestId("workspace-transcript-content");
    expect(viewport).toHaveAttribute("data-slot", "message-scroller-viewport");
    expect(content).toHaveAttribute("data-slot", "message-scroller-content");
    expect(viewport.closest('[data-slot="message-scroller"]')).not.toBeNull();
  });

  it("remounts the scroller when switching conversations", async () => {
    vi.mocked(commands.listMessages).mockResolvedValue([]);
    const { rerender } = render(<Workspace conversationId="conv-1" />);
    const before = await screen.findByTestId("workspace-scroll-container");

    rerender(<Workspace conversationId="conv-2" />);
    await waitFor(() => {
      const after = screen.getByTestId("workspace-scroll-container");
      expect(after).not.toBe(before);
    });
  });

  it("notifies when active messages refresh so the app can mark the conversation seen", async () => {
    const onConversationSeen = vi.fn();
    vi.mocked(commands.listMessages).mockResolvedValue([
      { ...messageFixture("m1", "hello"), conversationId: "c1" },
    ]);

    render(<Workspace conversationId="c1" onConversationSeen={onConversationSeen} />);

    await waitFor(() => expect(onConversationSeen).toHaveBeenCalledWith("c1"));
  });

  it("notifies when an agent-message-persisted event refreshes active messages", async () => {
    let firePersisted!: (p: { conversationId: string }) => void;
    vi.mocked(events.onAgentMessagePersisted).mockImplementation(async (cb) => {
      firePersisted = cb;
      return () => {};
    });
    vi.mocked(commands.listMessages)
      .mockResolvedValueOnce([messageFixture("m1", "first message")])
      .mockResolvedValueOnce([
        messageFixture("m1", "first message"),
        messageFixture("m2", "second message", 2),
      ]);
    const onConversationSeen = vi.fn();

    render(<Workspace conversationId="conv-1" onConversationSeen={onConversationSeen} />);
    await screen.findByText("first message");
    await waitFor(() => expect(onConversationSeen).toHaveBeenCalledWith("conv-1"));
    onConversationSeen.mockClear();

    firePersisted({ conversationId: "conv-1" });

    await screen.findByText("second message");
    expect(onConversationSeen).toHaveBeenCalledWith("conv-1");
  });

  it("wraps the agent-message-persisted refresh in a view transition when a pending question arrives", async () => {
    const startViewTransition = vi.fn((callback: () => void) => {
      callback();
      return {};
    });
    Object.defineProperty(document, "startViewTransition", {
      configurable: true,
      writable: true,
      value: startViewTransition,
    });

    let firePersisted!: (p: { conversationId: string }) => void;
    vi.mocked(events.onAgentMessagePersisted).mockImplementation(async (cb) => {
      firePersisted = cb;
      return () => {};
    });
    vi.mocked(commands.listMessages)
      .mockResolvedValueOnce([messageFixture("m1", "first message")])
      .mockResolvedValueOnce([
        messageFixture("m1", "first message"),
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

    render(<Workspace conversationId="conv-1" />);
    await screen.findByText("first message");
    startViewTransition.mockClear();

    firePersisted({ conversationId: "conv-1" });
    await screen.findByTestId("user-ask-widget");

    expect(startViewTransition).toHaveBeenCalledTimes(1);
  });

  it("does not start a view transition when a refresh doesn't change whether a question is pending", async () => {
    const startViewTransition = vi.fn((callback: () => void) => {
      callback();
      return {};
    });
    Object.defineProperty(document, "startViewTransition", {
      configurable: true,
      writable: true,
      value: startViewTransition,
    });

    let firePersisted!: (p: { conversationId: string }) => void;
    vi.mocked(events.onAgentMessagePersisted).mockImplementation(async (cb) => {
      firePersisted = cb;
      return () => {};
    });
    vi.mocked(commands.listMessages)
      .mockResolvedValueOnce([messageFixture("m1", "first message")])
      .mockResolvedValueOnce([
        messageFixture("m1", "first message"),
        messageFixture("m2", "second message", 2),
      ]);

    render(<Workspace conversationId="conv-1" />);
    await screen.findByText("first message");
    startViewTransition.mockClear();

    firePersisted({ conversationId: "conv-1" });
    await screen.findByText("second message");

    expect(startViewTransition).not.toHaveBeenCalled();
  });

  it("does not reload or resubscribe when only the seen callback identity changes", async () => {
    let firePersisted!: (p: { conversationId: string }) => void;
    const unlisten = vi.fn();
    vi.mocked(events.onAgentMessagePersisted).mockImplementation(async (cb) => {
      firePersisted = cb;
      return unlisten;
    });
    vi.mocked(commands.listMessages)
      .mockResolvedValueOnce([messageFixture("m1", "first message")])
      .mockResolvedValueOnce([
        messageFixture("m1", "first message"),
        messageFixture("m2", "second message", 2),
      ]);
    const firstSeenCallback = vi.fn();
    const secondSeenCallback = vi.fn();

    const { rerender } = render(
      <Workspace conversationId="conv-1" onConversationSeen={firstSeenCallback} />,
    );
    await screen.findByText("first message");
    await waitFor(() => expect(events.onAgentMessagePersisted).toHaveBeenCalledTimes(1));

    firstSeenCallback.mockClear();
    rerender(<Workspace conversationId="conv-1" onConversationSeen={secondSeenCallback} />);
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(commands.listMessages).toHaveBeenCalledTimes(1);
    expect(events.onAgentMessagePersisted).toHaveBeenCalledTimes(1);
    expect(unlisten).not.toHaveBeenCalled();

    firePersisted({ conversationId: "conv-1" });

    await screen.findByText("second message");
    expect(firstSeenCallback).not.toHaveBeenCalled();
    expect(secondSeenCallback).toHaveBeenCalledWith("conv-1");
  });

  it("sends a task and shows a working state until the real (non-streamed) reply returns", async () => {
    const nowSpy = vi.spyOn(Date, "now").mockReturnValue(10_000);
    // Streaming (UI refactor): `send()` no longer builds the final message
    // from `sendAgentMessage`'s own return value -- it relies on the
    // `finally` block's safety-net `listMessages` refetch (the same one
    // `agent-message-persisted` events would normally trigger live; this
    // test drives it directly via the mock's second resolution instead of
    // firing a real event, since the *effect* -- a fresh transcript once
    // the turn is done -- is what matters here, not the event plumbing
    // itself).
    vi.mocked(commands.listMessages)
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([
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

    const onConversationSeen = vi.fn();

    render(<Workspace conversationId="conv-1" onConversationSeen={onConversationSeen} />);
    await screen.findByTestId("agent-input");
    await waitFor(() => expect(onConversationSeen).toHaveBeenCalledWith("conv-1"));
    onConversationSeen.mockClear();

    await userEvent.type(screen.getByTestId("agent-input"), "list the files here");
    await userEvent.click(screen.getByTestId("agent-send"));

    const status = await screen.findByTestId("agent-thinking");
    const composerShell = screen.getByTestId("workspace-composer-shell");
    expect(status).toHaveTextContent("Working");
    expect(screen.getByTestId("agent-thinking-timer")).toHaveTextContent("0.0s");
    // The prompt's ↑ cost shows immediately on submit (chars/4 estimate for
    // the optimistic row — "list the files here" is 19 chars → 5); the
    // zero-valued ↓ stays hidden until real output lands.
    const thinkingTokens = screen.getByTestId("agent-thinking-tokens");
    expect(thinkingTokens).toHaveTextContent("↑ 5");
    expect(thinkingTokens).not.toHaveTextContent("↓");
    expect(status.closest('[data-testid="chat-message"]')).toBeNull();
    expect(status.closest('[data-testid="transcript-turn"]')).toBeNull();
    expectElementBefore(status, composerShell);
    expect(composerShell).not.toHaveClass("border-t");

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
    expect(onConversationSeen).toHaveBeenCalledWith("conv-1");
    nowSpy.mockRestore();
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
            outcome: {
              ok: true,
              exitCode: 0,
              stdout: "a.rs\nb.rs",
              stderr: "",
            },
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
    // Still working: the turn's own promise hasn't resolved yet, only a
    // live event landed -- this is the whole point of streaming loop
    // progress separately from the turn's final completion.
    expect(screen.getByTestId("agent-thinking")).toBeInTheDocument();

    resolveAgent("Found 2 files.");
    await waitFor(() => {
      expect(screen.getByTestId("agent-input")).toHaveAttribute("contenteditable", "true");
    });
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

  it('shows the pending question widget in the composer slot (not "Working…", not the normal chat input) when the latest message is an unanswered AskUserQuestion tool_call, and answering it calls answerUserQuestion', async () => {
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

    const widget = await screen.findByTestId("user-ask-widget");
    expect(widget).toHaveTextContent("What would you like to do?");
    expect(screen.queryByTestId("agent-thinking")).not.toBeInTheDocument();
    // The normal composer is replaced entirely, not merely disabled -- the
    // question widget sits in its place instead.
    expect(screen.queryByTestId("agent-input")).not.toBeInTheDocument();
    expect(widget.closest('[data-testid="transcript-turn"]')).toBeNull();

    await userEvent.click(screen.getByRole("radio", { name: "A" }));
    expect(commands.answerUserQuestion).not.toHaveBeenCalled();

    await userEvent.click(screen.getByTestId("question-submit"));
    expect(commands.answerUserQuestion).toHaveBeenCalledWith("q1", ["A"]);
  });

  it("closing the pending question widget reveals a free-text composer whose submission answers the question", async () => {
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
    vi.mocked(commands.sendAgentMessage).mockReturnValue(new Promise(() => {}));
    vi.mocked(commands.answerUserQuestion).mockResolvedValue(undefined);

    render(<Workspace conversationId="conv-1" />);

    await screen.findByTestId("user-ask-widget");
    await userEvent.click(screen.getByTestId("question-close"));

    const editable = screen.getByTestId("question-answer-input");
    await userEvent.click(editable);
    await userEvent.type(editable, "actually, do both{Enter}");

    expect(commands.answerUserQuestion).toHaveBeenCalledWith("q1", ["actually, do both"]);
  });

  it('shows a pending Bash widget (not "Working…") when the latest message is an unfinished Bash tool_call', async () => {
    vi.mocked(commands.listMessages).mockResolvedValue([
      {
        id: "u1",
        conversationId: "conv-1",
        role: "user",
        contentType: "text",
        content: "run the tests",
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
        content: JSON.stringify({ arguments: { command: "cargo test --lib" } }),
        toolName: "Bash",
        createdAt: 2,
        durationMs: null,
        tokenCount: null,
      },
    ]);
    vi.mocked(commands.sendAgentMessage).mockReturnValue(new Promise(() => {}));

    render(<Workspace conversationId="conv-1" />);

    const status = await screen.findByTestId("bash-status");
    expect(status).toHaveTextContent("$ cargo test --lib");
    expect(status).toHaveClass("shimmer");
    expect(screen.getByTestId("bash-command")).toHaveTextContent("cargo test --lib");
    expect(status.closest('[data-testid="transcript-turn"]')).toHaveTextContent("run the tests");
    expect(screen.queryByTestId("agent-thinking")).not.toBeInTheDocument();
  });

  it("shows a pending Task widget when the latest message is an unfinished Task tool_call", async () => {
    vi.mocked(commands.listMessages).mockResolvedValue([
      {
        id: "u1",
        conversationId: "conv-1",
        role: "user",
        contentType: "text",
        content: "investigate the bug",
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
          arguments: { prompt: "find the root cause" },
        }),
        toolName: "Task",
        createdAt: 2,
        durationMs: null,
        tokenCount: null,
      },
    ]);
    vi.mocked(commands.sendAgentMessage).mockReturnValue(new Promise(() => {}));

    render(<Workspace conversationId="conv-1" />);

    const status = await screen.findByTestId("task-status");
    expect(status).toHaveTextContent(/exploring/i);
    expect(status.closest('[data-testid="transcript-turn"]')).toHaveTextContent(
      "investigate the bug",
    );
    expect(screen.queryByTestId("agent-thinking")).not.toBeInTheDocument();
  });

  it("keeps the composer editable but QUEUES (never duplicate-sends) when the latest message is an unfinished tool_call with no dedicated pending widget (e.g. Grep)", async () => {
    // Regression + queue & steer: a turn stuck inside a non-widget tool (a slow
    // Grep) must not let a submit fire a DUPLICATE turn. The composer is now
    // editable (so a message can be composed to queue), but submitting mid-turn
    // enqueues rather than calling sendAgentMessage again.
    vi.mocked(commands.listMessages).mockResolvedValue([
      {
        id: "u1",
        conversationId: "conv-1",
        role: "user",
        contentType: "text",
        content: "find the needle",
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
          arguments: { pattern: "needle", path: "/tmp" },
        }),
        toolName: "Grep",
        createdAt: 2,
        durationMs: null,
        tokenCount: null,
      },
    ]);

    render(<Workspace conversationId="conv-1" />);

    expect(await screen.findByTestId("agent-thinking")).toHaveTextContent("Working");
    const input = screen.getByTestId("agent-input");
    expect(input).toHaveAttribute("contenteditable", "true");

    // Submitting mid-tool enqueues instead of sending a duplicate turn.
    await userEvent.type(input, "queue this");
    await userEvent.click(screen.getByTestId("agent-send"));
    expect(screen.getByTestId("queued-message-row")).toHaveTextContent("queue this");
    expect(commands.sendAgentMessage).not.toHaveBeenCalled();
  });

  it("keeps the composer editable but QUEUES after a reload while the backend reports the turn still active, even with no trailing tool_call (generation phase)", async () => {
    // The trailing-tool_call signal only covers the tool-execution window.
    // While the model is *generating* (latest row = user text or a paired
    // tool_result — the longest phases with local inference), only the
    // backend's ActiveGenerations knows a turn is live; a reload wipes every
    // in-memory frontend flag. The composer is editable, but a submit enqueues
    // rather than firing a duplicate turn.
    vi.mocked(commands.listMessages).mockResolvedValue([messageFixture("u1", "find the needle")]);
    vi.mocked(commands.isGenerationActive).mockResolvedValue(true);

    render(<Workspace conversationId="conv-1" />);

    await screen.findByTestId("agent-thinking");
    const input = screen.getByTestId("agent-input");
    expect(input).toHaveAttribute("contenteditable", "true");

    await userEvent.type(input, "queue this");
    await userEvent.click(screen.getByTestId("agent-send"));
    expect(screen.getByTestId("queued-message-row")).toHaveTextContent("queue this");
    expect(commands.sendAgentMessage).not.toHaveBeenCalled();
  });

  it("starts the streaming chron from the latest persisted user message during a backend-active reload", async () => {
    const nowSpy = vi.spyOn(Date, "now").mockReturnValue(6_000);
    vi.mocked(commands.listMessages).mockResolvedValue([
      messageFixture("u1", "find the needle", 4_000),
    ]);
    vi.mocked(commands.isGenerationActive).mockResolvedValue(true);

    render(<Workspace conversationId="conv-1" />);

    expect(await screen.findByTestId("agent-thinking-timer")).toHaveTextContent("2.0s");
    nowSpy.mockRestore();
  });

  it("does not reset the streaming chron when an unpaired non-dedicated tool call appears", async () => {
    const nowSpy = vi.spyOn(Date, "now").mockReturnValue(4_000);
    let firePersisted!: (p: { conversationId: string }) => void;
    vi.mocked(events.onAgentMessagePersisted).mockImplementation(async (cb) => {
      firePersisted = cb;
      return () => {};
    });
    vi.mocked(commands.listMessages)
      .mockResolvedValueOnce([
        {
          id: "u1",
          conversationId: "conv-1",
          role: "user",
          contentType: "text",
          content: "find the needle",
          toolName: null,
          createdAt: 1_000,
          durationMs: null,
          tokenCount: null,
        },
      ])
      .mockResolvedValueOnce([
        {
          id: "u1",
          conversationId: "conv-1",
          role: "user",
          contentType: "text",
          content: "find the needle",
          toolName: null,
          createdAt: 1_000,
          durationMs: null,
          tokenCount: null,
        },
        {
          id: "tc1",
          conversationId: "conv-1",
          role: "assistant",
          contentType: "tool_call",
          content: JSON.stringify({
            arguments: { pattern: "needle", path: "/tmp" },
          }),
          toolName: "Grep",
          createdAt: 3_000,
          durationMs: null,
          tokenCount: null,
        },
      ]);
    vi.mocked(commands.isGenerationActive).mockResolvedValue(true);

    render(<Workspace conversationId="conv-1" />);

    expect(await screen.findByTestId("agent-thinking-timer")).toHaveTextContent("3.0s");
    firePersisted({ conversationId: "conv-1" });

    await waitFor(() =>
      expect(screen.getByTestId("agent-thinking-timer")).toHaveTextContent("3.0s"),
    );
    nowSpy.mockRestore();
  });

  it("renders the composer without a divider when the streaming status is hidden", async () => {
    vi.mocked(commands.listMessages).mockResolvedValue([]);

    render(<Workspace conversationId="conv-1" />);

    expect(await screen.findByTestId("workspace-composer-shell")).not.toHaveClass("border-t");
    expect(screen.queryByTestId("agent-thinking")).not.toBeInTheDocument();
  });

  it("does not show a pending Bash widget once the tool_result has landed (latest message is the result, not the call)", async () => {
    vi.mocked(commands.listMessages).mockResolvedValue([
      {
        id: "tc1",
        conversationId: "conv-1",
        role: "assistant",
        contentType: "tool_call",
        content: JSON.stringify({ arguments: { command: "cargo test --lib" } }),
        toolName: "Bash",
        createdAt: 1,
        durationMs: null,
        tokenCount: null,
      },
      {
        id: "tr1",
        conversationId: "conv-1",
        role: "tool",
        contentType: "tool_result",
        content: JSON.stringify({
          toolName: "Bash",
          command: "cargo test --lib",
          outcome: { ok: true, exitCode: 0, stdout: "ok", stderr: "" },
        }),
        toolName: "Bash",
        createdAt: 2,
        durationMs: null,
        tokenCount: null,
      },
    ]);

    render(<Workspace conversationId="conv-1" />);

    await screen.findByTestId("bash-widget");
    // Exactly one widget (the result), and no pending shimmer status — a
    // successful completed command renders quietly with meta only.
    expect(screen.getAllByTestId("bash-widget")).toHaveLength(1);
    expect(screen.queryByTestId("bash-status")).not.toBeInTheDocument();
    expect(screen.getByTestId("bash-meta")).toHaveTextContent("exit 0");
  });

  it("009-rich-chat-input regression: a message containing a chip forwards richContent to sendAgentMessage, not just the flat text", async () => {
    vi.mocked(commands.sendAgentMessage).mockResolvedValue("ok");

    render(<Workspace conversationId="conv-1" />);
    const input = await screen.findByTestId("agent-input");

    // Crosses the paste-collapse threshold — produces a real pastedText
    // chip, matching RichInput's own US2 test's paste-simulation pattern.
    const pastedBlock = Array.from({ length: 15 }, (_, i) => `line-${i}`).join("\n");
    fireEvent.paste(input, {
      clipboardData: { items: [], getData: () => pastedBlock },
    });
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
    let resolveSend!: (value: string) => void;
    vi.mocked(commands.sendAgentMessage).mockReturnValue(
      new Promise((resolve) => {
        resolveSend = resolve;
      }),
    );
    const onConsumed = vi.fn();
    const pendingInitialTurn = {
      conversationId: "conv-1",
      content: "first task",
    };

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

    resolveSend("done");
    await waitFor(() => {
      expect(screen.getByTestId("agent-input")).toHaveAttribute("contenteditable", "true");
    });
  });

  it("preserves the optimistic pending initial turn after the parent clears the consumed prop", async () => {
    let resolveInitialMessages!: (messages: []) => void;
    vi.mocked(commands.listMessages).mockReturnValueOnce(
      new Promise((resolve) => {
        resolveInitialMessages = resolve;
      }),
    );
    let resolveSend!: (value: string) => void;
    vi.mocked(commands.sendAgentMessage).mockReturnValue(
      new Promise((resolve) => {
        resolveSend = resolve;
      }),
    );
    const onConsumed = vi.fn();
    const pendingInitialTurn = {
      conversationId: "conv-1",
      content: "first task",
    };

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

    resolveSend("done");
    await waitFor(() => {
      expect(screen.getByTestId("agent-input")).toHaveAttribute("contenteditable", "true");
    });
  });

  it("leaves a pending initial turn unconsumed until an existing in-flight send for the same conversation clears", async () => {
    let resolveFirstSend!: (value: string) => void;
    let resolvePendingSend!: (value: string) => void;
    vi.mocked(commands.sendAgentMessage)
      .mockReturnValueOnce(
        new Promise((resolve) => {
          resolveFirstSend = resolve;
        }),
      )
      .mockReturnValueOnce(
        new Promise((resolve) => {
          resolvePendingSend = resolve;
        }),
      );
    const onConsumed = vi.fn();
    const pendingInitialTurn = {
      conversationId: "conv-1",
      content: "pending followup",
    };

    const { rerender } = render(<Workspace conversationId="conv-1" />);
    await screen.findByTestId("agent-input");

    await userEvent.type(screen.getByTestId("agent-input"), "first task");
    await userEvent.click(screen.getByTestId("agent-send"));
    await waitFor(() => {
      expect(commands.sendAgentMessage).toHaveBeenCalledWith("conv-1", "first task", undefined);
      expect(screen.getByTestId("agent-thinking")).toBeInTheDocument();
    });

    rerender(
      <Workspace
        key="conv-1-remount"
        conversationId="conv-1"
        pendingInitialTurn={pendingInitialTurn}
        onPendingInitialTurnConsumed={onConsumed}
      />,
    );

    await waitFor(() => {
      expect(screen.getByTestId("agent-thinking")).toBeInTheDocument();
    });
    expect(commands.sendAgentMessage).toHaveBeenCalledTimes(1);
    expect(onConsumed).not.toHaveBeenCalled();

    resolveFirstSend("first done");
    rerender(
      <Workspace
        key="conv-1-remount"
        conversationId="conv-1"
        pendingInitialTurn={pendingInitialTurn}
        onPendingInitialTurnConsumed={onConsumed}
      />,
    );

    await waitFor(() => {
      expect(commands.sendAgentMessage).toHaveBeenCalledTimes(2);
      expect(commands.sendAgentMessage).toHaveBeenLastCalledWith(
        "conv-1",
        "pending followup",
        undefined,
      );
      expect(onConsumed).toHaveBeenCalledWith("conv-1");
    });

    resolvePendingSend("pending done");
    await waitFor(() => {
      expect(screen.getByTestId("agent-input")).toHaveAttribute("contenteditable", "true");
    });
  });

  it("forwards rich content from a pending initial turn as a JSON string", async () => {
    const richContent: RichMessageContent = {
      segments: [
        { type: "text", text: "review this" },
        {
          type: "pastedText",
          id: "paste-1",
          text: "line 1\nline 2",
          lineCount: 2,
        },
      ],
    };
    let resolveSend!: (value: string) => void;
    vi.mocked(commands.sendAgentMessage).mockReturnValue(
      new Promise((resolve) => {
        resolveSend = resolve;
      }),
    );

    render(
      <Workspace
        conversationId="conv-1"
        pendingInitialTurn={{
          conversationId: "conv-1",
          content: "review this",
          richContent,
        }}
      />,
    );

    await waitFor(() => {
      expect(commands.sendAgentMessage).toHaveBeenCalledWith(
        "conv-1",
        "review this",
        JSON.stringify(richContent),
      );
    });

    resolveSend("done");
    await waitFor(() => {
      expect(screen.getByTestId("agent-input")).toHaveAttribute("contenteditable", "true");
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

  it("marks the composer COLUMN (not the full-width shell) for view transitions", async () => {
    render(<Workspace conversationId="conv-1" />);

    const shell = await screen.findByTestId("workspace-composer-shell");
    expect(shell).toHaveClass("p-4");
    // The view-transition name must live on an element with the SAME width
    // as EmptyState's named element (max-w-xl) — naming the full-width
    // shell makes the empty-state -> chat transition morph on the x axis.
    expect(shell.className).not.toContain("[view-transition-name:chat-composer]");
    const named = shell.querySelector('[class*="view-transition-name:chat-composer"]');
    expect(named).not.toBeNull();
    expect(named).toHaveClass("max-w-xl", "mx-auto");
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
    let resolveConv1Messages!: (
      messages: Awaited<ReturnType<typeof commands.listMessages>>,
    ) => void;
    let resolveConv2Messages!: (
      messages: Awaited<ReturnType<typeof commands.listMessages>>,
    ) => void;
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

  it("ignores stale /compact refresh results after switching conversations", async () => {
    let resolveCompact!: (usage: Awaited<ReturnType<typeof commands.compactConversation>>) => void;
    let resolveStaleCompactMessages!: (
      messages: Awaited<ReturnType<typeof commands.listMessages>>,
    ) => void;
    let conv1ListCalls = 0;
    vi.mocked(commands.compactConversation).mockReturnValue(
      new Promise((resolve) => {
        resolveCompact = resolve;
      }),
    );
    vi.mocked(commands.listMessages).mockImplementation((requestedConversationId) => {
      if (requestedConversationId === "conv-1") {
        conv1ListCalls += 1;
        if (conv1ListCalls === 1) return Promise.resolve([]);
        return new Promise((resolve) => {
          resolveStaleCompactMessages = resolve;
        });
      }

      return Promise.resolve([
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
    });

    const { rerender } = render(<Workspace conversationId="conv-1" />);
    await screen.findByTestId("agent-input");

    await userEvent.type(screen.getByTestId("agent-input"), "/compact");
    await userEvent.click(screen.getByTestId("agent-send"));
    await waitFor(() => expect(commands.compactConversation).toHaveBeenCalledWith("conv-1"));

    resolveCompact({
      conversationId: "conv-1",
      tokensUsed: 100,
      tokenBudget: 2048,
      state: "justCompacted",
    });
    await waitFor(() => expect(commands.listMessages).toHaveBeenCalledWith("conv-1"));

    rerender(<Workspace conversationId="conv-2" />);
    await screen.findByText("second workspace");

    resolveStaleCompactMessages([
      {
        id: "stale-compact",
        conversationId: "conv-1",
        role: "assistant",
        contentType: "context_notice",
        content: JSON.stringify({
          kind: "summarized",
          summary: "old summary",
          notice: "stale compact notice",
        }),
        toolName: null,
        createdAt: 1,
        durationMs: null,
        tokenCount: null,
      },
    ]);
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(screen.getByText("second workspace")).toBeInTheDocument();
    expect(screen.queryByText("stale compact notice")).not.toBeInTheDocument();
  });

  it("does not notify when a /compact refresh completes after keyed unmount", async () => {
    let resolveCompact!: (usage: Awaited<ReturnType<typeof commands.compactConversation>>) => void;
    let resolveStaleCompactMessages!: (
      messages: Awaited<ReturnType<typeof commands.listMessages>>,
    ) => void;
    let conv1ListCalls = 0;
    const onConversationSeen = vi.fn();
    vi.mocked(commands.compactConversation).mockReturnValue(
      new Promise((resolve) => {
        resolveCompact = resolve;
      }),
    );
    vi.mocked(commands.listMessages).mockImplementation((requestedConversationId) => {
      if (requestedConversationId === "conv-1") {
        conv1ListCalls += 1;
        if (conv1ListCalls === 1) return Promise.resolve([]);
        return new Promise((resolve) => {
          resolveStaleCompactMessages = resolve;
        });
      }

      return Promise.resolve([
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
    });

    const { rerender } = render(
      <Workspace key="conv-1" conversationId="conv-1" onConversationSeen={onConversationSeen} />,
    );
    await screen.findByTestId("agent-input");
    await waitFor(() => expect(onConversationSeen).toHaveBeenCalledWith("conv-1"));
    onConversationSeen.mockClear();

    await userEvent.type(screen.getByTestId("agent-input"), "/compact");
    await userEvent.click(screen.getByTestId("agent-send"));
    await waitFor(() => expect(commands.compactConversation).toHaveBeenCalledWith("conv-1"));

    resolveCompact({
      conversationId: "conv-1",
      tokensUsed: 100,
      tokenBudget: 2048,
      state: "justCompacted",
    });
    await waitFor(() => expect(conv1ListCalls).toBe(2));

    rerender(
      <Workspace key="conv-2" conversationId="conv-2" onConversationSeen={onConversationSeen} />,
    );
    await screen.findByText("second workspace");
    onConversationSeen.mockClear();

    resolveStaleCompactMessages([
      {
        id: "stale-compact",
        conversationId: "conv-1",
        role: "assistant",
        contentType: "context_notice",
        content: JSON.stringify({
          kind: "summarized",
          summary: "old summary",
          notice: "stale compact notice",
        }),
        toolName: null,
        createdAt: 1,
        durationMs: null,
        tokenCount: null,
      },
    ]);
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(onConversationSeen).not.toHaveBeenCalled();
  });

  it("keeps /compact refreshes active after StrictMode effect replay", async () => {
    vi.mocked(commands.compactConversation).mockResolvedValue({
      conversationId: "conv-1",
      tokensUsed: 100,
      tokenBudget: 2048,
      state: "justCompacted",
    });

    let listMessagesCalls = 0;
    vi.mocked(commands.listMessages).mockImplementation(() => {
      listMessagesCalls += 1;
      if (listMessagesCalls <= 2) return Promise.resolve([]);
      return Promise.resolve([
        {
          id: "notice-1",
          conversationId: "conv-1",
          role: "assistant",
          contentType: "context_notice",
          content: JSON.stringify({
            kind: "summarized",
            summary: "the gist of it",
            notice: "StrictMode compact refresh",
          }),
          toolName: null,
          createdAt: 2,
          durationMs: null,
          tokenCount: null,
        },
      ]);
    });

    render(
      <StrictMode>
        <Workspace conversationId="conv-1" />
      </StrictMode>,
    );
    await screen.findByTestId("agent-input");

    await userEvent.type(screen.getByTestId("agent-input"), "/compact");
    await userEvent.click(screen.getByTestId("agent-send"));

    await waitFor(() => expect(commands.compactConversation).toHaveBeenCalledWith("conv-1"));
    expect(await screen.findByTestId("context-notice")).toHaveTextContent(
      "StrictMode compact refresh",
    );
  });

  it("refreshes a /compact result after returning to the same conversation before completion", async () => {
    let resolveCompact!: (usage: Awaited<ReturnType<typeof commands.compactConversation>>) => void;
    let compactFinished = false;
    const onConversationSeen = vi.fn();

    vi.mocked(commands.compactConversation).mockReturnValue(
      new Promise((resolve) => {
        resolveCompact = resolve;
      }),
    );
    vi.mocked(commands.listMessages).mockImplementation((requestedConversationId) => {
      if (requestedConversationId === "conv-2") {
        return Promise.resolve([
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
      }

      if (!compactFinished) return Promise.resolve([]);
      return Promise.resolve([
        {
          id: "late-compact",
          conversationId: "conv-1",
          role: "assistant",
          contentType: "context_notice",
          content: JSON.stringify({
            kind: "summarized",
            summary: "old summary",
            notice: "late compact notice",
          }),
          toolName: null,
          createdAt: 3,
          durationMs: null,
          tokenCount: null,
        },
      ]);
    });

    const { rerender } = render(
      <Workspace key="conv-1" conversationId="conv-1" onConversationSeen={onConversationSeen} />,
    );
    await screen.findByTestId("agent-input");

    await userEvent.type(screen.getByTestId("agent-input"), "/compact");
    await userEvent.click(screen.getByTestId("agent-send"));
    await waitFor(() => expect(commands.compactConversation).toHaveBeenCalledWith("conv-1"));

    rerender(
      <Workspace key="conv-2" conversationId="conv-2" onConversationSeen={onConversationSeen} />,
    );
    await screen.findByText("second workspace");

    rerender(
      <Workspace
        key="conv-1-return"
        conversationId="conv-1"
        onConversationSeen={onConversationSeen}
      />,
    );
    await screen.findByTestId("agent-input");
    onConversationSeen.mockClear();

    compactFinished = true;
    resolveCompact({
      conversationId: "conv-1",
      tokensUsed: 100,
      tokenBudget: 2048,
      state: "justCompacted",
    });

    expect(await screen.findByTestId("context-notice")).toHaveTextContent("late compact notice");
    expect(onConversationSeen).toHaveBeenCalledWith("conv-1");
  });

  it("ignores stale /compact errors after switching conversations", async () => {
    let rejectCompact!: (error: Error) => void;
    vi.mocked(commands.compactConversation).mockReturnValue(
      new Promise((_, reject) => {
        rejectCompact = reject;
      }),
    );
    vi.mocked(commands.listMessages).mockImplementation((requestedConversationId) => {
      if (requestedConversationId === "conv-2") {
        return Promise.resolve([
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
      }

      return Promise.resolve([]);
    });

    const { rerender } = render(<Workspace conversationId="conv-1" />);
    await screen.findByTestId("agent-input");

    await userEvent.type(screen.getByTestId("agent-input"), "/compact");
    await userEvent.click(screen.getByTestId("agent-send"));
    await waitFor(() => expect(commands.compactConversation).toHaveBeenCalledWith("conv-1"));

    rerender(<Workspace conversationId="conv-2" />);
    await screen.findByText("second workspace");

    rejectCompact(new Error("stale compact failed"));
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(screen.getByText("second workspace")).toBeInTheDocument();
    expect(screen.queryByTestId("workspace-error")).not.toBeInTheDocument();
  });

  it("resets pending send state when switching conversations before the old send settles", async () => {
    vi.mocked(commands.listMessages)
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([
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
    let resolveSend!: (value: string) => void;
    vi.mocked(commands.sendAgentMessage).mockReturnValue(
      new Promise((resolve) => {
        resolveSend = resolve;
      }),
    );

    const { rerender } = render(<Workspace conversationId="conv-1" />);
    await screen.findByTestId("agent-input");

    await userEvent.type(screen.getByTestId("agent-input"), "first workspace task");
    await userEvent.click(screen.getByTestId("agent-send"));

    await waitFor(() => {
      expect(screen.getByTestId("agent-thinking")).toBeInTheDocument();
      // The composer is editable throughout now; the per-conversation "a turn
      // is in flight here" signal is the stop button (shown when turnInFlight).
      expect(screen.getByTestId("stop-generation")).toBeInTheDocument();
    });

    rerender(<Workspace conversationId="conv-2" />);
    await screen.findByText("second workspace");

    // conv-2 has no in-flight turn of its own — no stop button leaks across.
    expect(screen.queryByTestId("stop-generation")).not.toBeInTheDocument();
    expect(screen.queryByText("first workspace task")).not.toBeInTheDocument();

    resolveSend("old conversation done");
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(screen.queryByTestId("stop-generation")).not.toBeInTheDocument();
    expect(screen.getByText("second workspace")).toBeInTheDocument();
    expect(screen.queryByText("first workspace task")).not.toBeInTheDocument();
  });

  it("does not notify when a send safety-net refresh completes after keyed unmount", async () => {
    let resolveSend!: (value: string) => void;
    let resolveFinalMessages!: (
      messages: Awaited<ReturnType<typeof commands.listMessages>>,
    ) => void;
    let conv1ListCalls = 0;
    const onConversationSeen = vi.fn();
    vi.mocked(commands.sendAgentMessage).mockReturnValue(
      new Promise((resolve) => {
        resolveSend = resolve;
      }),
    );
    vi.mocked(commands.listMessages).mockImplementation((requestedConversationId) => {
      if (requestedConversationId === "conv-1") {
        conv1ListCalls += 1;
        if (conv1ListCalls === 1) return Promise.resolve([]);
        return new Promise((resolve) => {
          resolveFinalMessages = resolve;
        });
      }

      return Promise.resolve([
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
    });

    const { rerender } = render(
      <Workspace key="conv-1" conversationId="conv-1" onConversationSeen={onConversationSeen} />,
    );
    await screen.findByTestId("agent-input");
    await waitFor(() => expect(onConversationSeen).toHaveBeenCalledWith("conv-1"));
    onConversationSeen.mockClear();

    await userEvent.type(screen.getByTestId("agent-input"), "first workspace task");
    await userEvent.click(screen.getByTestId("agent-send"));
    await waitFor(() => {
      expect(commands.sendAgentMessage).toHaveBeenCalledWith(
        "conv-1",
        "first workspace task",
        undefined,
      );
    });

    resolveSend("old conversation done");
    await waitFor(() => expect(conv1ListCalls).toBe(2));

    rerender(
      <Workspace key="conv-2" conversationId="conv-2" onConversationSeen={onConversationSeen} />,
    );
    await screen.findByText("second workspace");
    onConversationSeen.mockClear();

    resolveFinalMessages([messageFixture("m1", "old final message")]);
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(onConversationSeen).not.toHaveBeenCalled();
  });

  it("runs the send safety-net refresh after StrictMode effect replay", async () => {
    let listMessagesCalls = 0;
    vi.mocked(commands.listMessages).mockImplementation(() => {
      listMessagesCalls += 1;
      if (listMessagesCalls <= 2) return Promise.resolve([]);
      return Promise.resolve([
        messageFixture("u1", "strict task"),
        {
          id: "a1",
          conversationId: "conv-1",
          role: "assistant",
          contentType: "text",
          content: "strict reply",
          toolName: null,
          createdAt: 2,
          durationMs: null,
          tokenCount: null,
        },
      ]);
    });
    vi.mocked(commands.sendAgentMessage).mockResolvedValue("ok");

    render(
      <StrictMode>
        <Workspace conversationId="conv-1" />
      </StrictMode>,
    );
    await screen.findByTestId("agent-input");

    await userEvent.type(screen.getByTestId("agent-input"), "strict task");
    await userEvent.click(screen.getByTestId("agent-send"));

    await waitFor(() =>
      expect(commands.sendAgentMessage).toHaveBeenCalledWith("conv-1", "strict task", undefined),
    );
    expect(await screen.findByText("strict reply")).toBeInTheDocument();
    expect(screen.queryByTestId("agent-thinking")).not.toBeInTheDocument();
  });

  it("refreshes a send safety-net result after returning to the same conversation before completion", async () => {
    let resolveSend!: (value: string) => void;
    let sendFinished = false;
    const onConversationSeen = vi.fn();

    vi.mocked(commands.sendAgentMessage).mockReturnValue(
      new Promise((resolve) => {
        resolveSend = resolve;
      }),
    );
    vi.mocked(commands.listMessages).mockImplementation((requestedConversationId) => {
      if (requestedConversationId === "conv-2") {
        return Promise.resolve([
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
      }

      if (!sendFinished) return Promise.resolve([]);
      return Promise.resolve([
        messageFixture("u1", "late task"),
        {
          id: "a1",
          conversationId: "conv-1",
          role: "assistant",
          contentType: "text",
          content: "late reply",
          toolName: null,
          createdAt: 3,
          durationMs: null,
          tokenCount: null,
        },
      ]);
    });

    const { rerender } = render(
      <Workspace key="conv-1" conversationId="conv-1" onConversationSeen={onConversationSeen} />,
    );
    await screen.findByTestId("agent-input");

    await userEvent.type(screen.getByTestId("agent-input"), "late task");
    await userEvent.click(screen.getByTestId("agent-send"));
    await waitFor(() =>
      expect(commands.sendAgentMessage).toHaveBeenCalledWith("conv-1", "late task", undefined),
    );

    rerender(
      <Workspace key="conv-2" conversationId="conv-2" onConversationSeen={onConversationSeen} />,
    );
    await screen.findByText("second workspace");

    rerender(
      <Workspace
        key="conv-1-return"
        conversationId="conv-1"
        onConversationSeen={onConversationSeen}
      />,
    );
    await screen.findByTestId("agent-input");
    onConversationSeen.mockClear();

    sendFinished = true;
    resolveSend("ok");

    expect(await screen.findByText("late reply")).toBeInTheDocument();
    expect(onConversationSeen).toHaveBeenCalledWith("conv-1");
  });

  it("keeps the original conversation's turn in flight across remounts and QUEUES (never duplicate-sends) while its send is still pending", async () => {
    let resolveSend!: (value: string) => void;
    vi.mocked(commands.listMessages).mockImplementation((requestedConversationId) => {
      if (requestedConversationId === "conv-2") {
        return Promise.resolve([
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
      }

      return Promise.resolve([]);
    });
    vi.mocked(commands.sendAgentMessage).mockReturnValue(
      new Promise((resolve) => {
        resolveSend = resolve;
      }),
    );

    const { rerender } = render(<Workspace key="conv-1" conversationId="conv-1" />);
    await screen.findByTestId("agent-input");

    await userEvent.type(screen.getByTestId("agent-input"), "first workspace task");
    await userEvent.click(screen.getByTestId("agent-send"));

    await waitFor(() => {
      expect(commands.sendAgentMessage).toHaveBeenCalledWith(
        "conv-1",
        "first workspace task",
        undefined,
      );
      // Composer stays editable throughout (queue & steer); the stop button is
      // the "turn in flight" signal.
      expect(screen.getByTestId("stop-generation")).toBeInTheDocument();
    });

    rerender(<Workspace key="conv-2" conversationId="conv-2" />);
    await screen.findByText("second workspace");
    expect(screen.queryByTestId("stop-generation")).not.toBeInTheDocument();

    rerender(<Workspace key="conv-1-return" conversationId="conv-1" />);
    await waitFor(() => {
      expect(screen.getByTestId("stop-generation")).toBeInTheDocument();
      expect(screen.getByTestId("agent-thinking")).toBeInTheDocument();
    });

    // The composer is editable and the send button IS present — but because the
    // turn is still in flight, a second submit ENQUEUES rather than firing a
    // duplicate send_agent_message (the anti-duplicate invariant, now enforced
    // by queueing instead of by disabling the composer).
    expect(screen.getByTestId("agent-input")).toHaveAttribute("contenteditable", "true");
    await userEvent.type(screen.getByTestId("agent-input"), "a second message");
    await userEvent.click(screen.getByTestId("agent-send"));
    expect(screen.getByTestId("queued-message-row")).toHaveTextContent("a second message");
    // The one send is still the only send — the second submit was queued, not
    // fired. (On completion it would drain as its own turn; covered separately.)
    expect(commands.sendAgentMessage).toHaveBeenCalledTimes(1);

    // Leave the send pending — the next test's registry reset clears it.
    void resolveSend;
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

  // Autoscroll and the scroll-to-end button's show/hide behavior are the
  // shadcn message-scroller primitive's own contract (IntersectionObserver-
  // driven) — inert in jsdom (see the stub in src/test/setup.ts), so it
  // isn't re-tested here; it's covered by browser-level verification. What
  // IS ours to test is the wiring: the button renders inside the scroller,
  // and sending a message re-arms autoscroll via scrollToEnd.

  it("renders the self-managing scroll-to-end button inside the scroller", async () => {
    vi.mocked(commands.listMessages).mockResolvedValue([]);
    render(<Workspace conversationId="conv-1" />);
    const button = await screen.findByTestId("scroll-to-bottom");
    expect(button).toHaveAttribute("data-slot", "message-scroller-button");
  });

  it("re-arms autoscroll by scrolling to the end when a message is sent", async () => {
    vi.mocked(commands.listMessages).mockResolvedValue([]);
    vi.mocked(commands.sendAgentMessage).mockResolvedValue("ok");
    render(<Workspace conversationId="conv-1" />);
    await screen.findByTestId("agent-input");

    await userEvent.type(screen.getByTestId("agent-input"), "hello");
    await userEvent.click(screen.getByTestId("agent-send"));

    expect(scrollToEndSpy).toHaveBeenCalled();
  });

  // --- 010-context-window-management (UI refactor): /compact command ---

  it("typing /compact triggers compaction directly instead of sending a normal agent turn", async () => {
    vi.mocked(commands.compactConversation).mockResolvedValue({
      conversationId: "conv-1",
      tokensUsed: 100,
      tokenBudget: 2048,
      state: "justCompacted",
    });
    vi.mocked(commands.listMessages)
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([
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

    const onConversationSeen = vi.fn();

    render(<Workspace conversationId="conv-1" onConversationSeen={onConversationSeen} />);
    await screen.findByTestId("agent-input");
    await waitFor(() => expect(onConversationSeen).toHaveBeenCalledWith("conv-1"));
    onConversationSeen.mockClear();

    await userEvent.type(screen.getByTestId("agent-input"), "/compact");
    await userEvent.click(screen.getByTestId("agent-send"));

    await waitFor(() => expect(commands.compactConversation).toHaveBeenCalledWith("conv-1"));
    expect(commands.sendAgentMessage).not.toHaveBeenCalled();
    expect(await screen.findByTestId("context-notice")).toHaveTextContent(
      "Conversation condensed to save space",
    );
    expect(onConversationSeen).toHaveBeenCalledWith("conv-1");
  });

  // --- Plan tracker integration ---

  it("docks the plan tracker between the transcript and the composer", async () => {
    vi.mocked(commands.getActivePlan).mockResolvedValue({
      goal: "Ship it",
      currentStepIndex: 0,
      steps: [{ description: "Step one", done: false }],
    });
    vi.mocked(commands.listMessages).mockResolvedValue([]);
    render(<Workspace conversationId="conv-1" />);

    const tracker = await screen.findByTestId("plan-tracker");
    const scroller = screen.getByTestId("workspace-scroll-container");
    const composer = screen.getByTestId("workspace-composer-shell");
    // Not inside the scroller any more…
    expect(scroller.contains(tracker)).toBe(false);
    // Load-bearing guard: the old location was inside the MessageScroller
    // ROOT (a sibling of the viewport), which the viewport check can't see.
    expect(document.querySelector('[data-slot="message-scroller"]')?.contains(tracker)).toBe(false);
    // …and between it and the composer in document order.
    expect(
      scroller.compareDocumentPosition(tracker) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(
      tracker.compareDocumentPosition(composer) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });

  it("keeps the prompt's ↑ estimate when the persisted user row lands without its token count yet", async () => {
    let firePersisted!: (p: { conversationId: string }) => void;
    vi.mocked(events.onAgentMessagePersisted).mockImplementation(async (cb) => {
      firePersisted = cb;
      return () => {};
    });
    // First load: empty. After the persist event: the REAL user row, whose
    // token_count is still NULL (the backend UPDATEs it only after the
    // engine loads and re-announces the row then).
    vi.mocked(commands.listMessages)
      .mockResolvedValueOnce([])
      .mockResolvedValue([
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
      ]);
    vi.mocked(commands.sendAgentMessage).mockReturnValue(new Promise(() => {}));

    render(<Workspace conversationId="conv-1" />);
    await screen.findByTestId("agent-input");

    await userEvent.type(screen.getByTestId("agent-input"), "list the files here");
    await userEvent.click(screen.getByTestId("agent-send"));
    expect(screen.getByTestId("agent-thinking-tokens")).toHaveTextContent("↑ 5");

    // The refetch replaces the optimistic row with the count-less persisted
    // row — the estimate must survive it, not blank the counter.
    firePersisted({ conversationId: "conv-1" });
    await waitFor(() => expect(commands.listMessages).toHaveBeenCalledTimes(2));
    expect(screen.getByTestId("agent-thinking-tokens")).toHaveTextContent("↑ 5");
  });

  it("streams live generation pieces into the working shimmer and clears at persisted boundaries", async () => {
    let firePiece!: (p: { conversationId: string; piece: string }) => void;
    let firePersisted!: (p: { conversationId: string }) => void;
    vi.mocked(events.onAgentGenerationPiece).mockImplementation(async (cb) => {
      firePiece = cb;
      return () => {};
    });
    vi.mocked(events.onAgentMessagePersisted).mockImplementation(async (cb) => {
      firePersisted = cb;
      return () => {};
    });
    vi.mocked(commands.sendAgentMessage).mockReturnValue(new Promise(() => {}));

    render(<Workspace conversationId="conv-1" />);
    await screen.findByTestId("agent-input");

    await userEvent.type(screen.getByTestId("agent-input"), "go");
    await userEvent.click(screen.getByTestId("agent-send"));
    await screen.findByTestId("agent-thinking");
    expect(screen.getByTestId("agent-thinking-status")).toHaveTextContent("Working");

    firePiece({ conversationId: "conv-1", piece: "let me think" });
    firePiece({ conversationId: "conv-1", piece: " about this" });
    await waitFor(() =>
      expect(screen.getByTestId("agent-thinking-stream")).toHaveTextContent(
        "let me think about this",
      ),
    );

    // Pieces for OTHER conversations are ignored.
    firePiece({ conversationId: "conv-9", piece: " NOT MINE" });
    expect(screen.getByTestId("agent-thinking-stream")).not.toHaveTextContent("NOT MINE");

    // A persisted row is a generation boundary: the reasoning segment resets.
    firePersisted({ conversationId: "conv-1" });
    await waitFor(() =>
      expect(screen.queryByTestId("agent-thinking-stream")).not.toBeInTheDocument(),
    );
  });

  // --- Queue & steer (2026-07-20) ---

  // Starts a turn whose `sendAgentMessage` never resolves on its own, so the
  // conversation stays "in flight"; returns the resolver so a test can complete
  // it. Leaves the composer editable with the stop button showing.
  async function startPendingTurn(text = "start the turn"): Promise<(v: string) => void> {
    let resolve!: (v: string) => void;
    vi.mocked(commands.sendAgentMessage).mockReturnValueOnce(
      new Promise<string>((r) => {
        resolve = r;
      }),
    );
    await userEvent.type(screen.getByTestId("agent-input"), text);
    await userEvent.click(screen.getByTestId("agent-send"));
    await screen.findByTestId("stop-generation");
    return resolve;
  }

  it("queues a composer submission while a turn is in flight instead of sending it", async () => {
    render(<Workspace conversationId="conv-1" />);
    await screen.findByTestId("agent-input");
    const resolveFirst = await startPendingTurn("first turn");
    expect(commands.sendAgentMessage).toHaveBeenCalledTimes(1);

    await userEvent.type(screen.getByTestId("agent-input"), "queued follow-up");
    await userEvent.click(screen.getByTestId("agent-send"));

    expect(screen.getByTestId("queued-message-row")).toHaveTextContent("queued follow-up");
    // No second send — it was queued, not dispatched.
    expect(commands.sendAgentMessage).toHaveBeenCalledTimes(1);
    void resolveFirst;
  });

  it("drains the queue FIFO as sequential turns when the running turn completes", async () => {
    render(<Workspace conversationId="conv-1" />);
    await screen.findByTestId("agent-input");
    const resolveFirst = await startPendingTurn("first turn");

    await userEvent.type(screen.getByTestId("agent-input"), "message A");
    await userEvent.click(screen.getByTestId("agent-send"));
    await userEvent.type(screen.getByTestId("agent-input"), "message B");
    await userEvent.click(screen.getByTestId("agent-send"));
    expect(screen.getAllByTestId("queued-message-row")).toHaveLength(2);

    // Drained turns resolve immediately so the queue fully empties.
    vi.mocked(commands.sendAgentMessage).mockResolvedValue("done");
    resolveFirst("first done");

    await waitFor(() => expect(screen.queryByTestId("queued-message-row")).not.toBeInTheDocument());
    const sentTexts = vi.mocked(commands.sendAgentMessage).mock.calls.map((c) => c[1]);
    expect(sentTexts).toContain("message A");
    expect(sentTexts).toContain("message B");
    // FIFO: A dispatched before B.
    expect(sentTexts.indexOf("message A")).toBeLessThan(sentTexts.indexOf("message B"));
  });

  it("Send now steers via steerGeneration and removes the row on injected", async () => {
    render(<Workspace conversationId="conv-1" />);
    await screen.findByTestId("agent-input");
    const resolveFirst = await startPendingTurn("first turn");

    await userEvent.type(screen.getByTestId("agent-input"), "steer me now");
    await userEvent.click(screen.getByTestId("agent-send"));
    expect(screen.getByTestId("queued-message-row")).toBeInTheDocument();

    vi.mocked(commands.steerGeneration).mockResolvedValue("injected");
    await userEvent.click(screen.getByTestId("queued-message-send-now"));

    await waitFor(() => {
      expect(commands.steerGeneration).toHaveBeenCalledWith("conv-1", "steer me now", undefined);
      expect(screen.queryByTestId("queued-message-row")).not.toBeInTheDocument();
    });
    // Steering injects into the running turn — no new sendAgentMessage.
    expect(commands.sendAgentMessage).toHaveBeenCalledTimes(1);
    void resolveFirst;
  });

  it("Send now falls back to sendAgentMessage when steer returns noActiveTurn", async () => {
    render(<Workspace conversationId="conv-1" />);
    await screen.findByTestId("agent-input");
    const resolveFirst = await startPendingTurn("first turn");

    await userEvent.type(screen.getByTestId("agent-input"), "late steer");
    await userEvent.click(screen.getByTestId("agent-send"));

    // Manual stop leaves the row queued; completing the (now-stopped) turn takes
    // the conversation idle without draining, so a subsequent steer finds no
    // active turn.
    await userEvent.click(screen.getByTestId("stop-generation"));
    vi.mocked(commands.sendAgentMessage).mockResolvedValue("ok");
    resolveFirst("stopped");
    await waitFor(() => expect(screen.queryByTestId("stop-generation")).not.toBeInTheDocument());
    expect(screen.getByTestId("queued-message-row")).toBeInTheDocument();

    vi.mocked(commands.steerGeneration).mockResolvedValue("noActiveTurn");
    const callsBefore = vi.mocked(commands.sendAgentMessage).mock.calls.length;
    await userEvent.click(screen.getByTestId("queued-message-send-now"));

    await waitFor(() => {
      const calls = vi.mocked(commands.sendAgentMessage).mock.calls;
      expect(calls.length).toBe(callsBefore + 1);
      expect(calls[calls.length - 1]).toEqual(["conv-1", "late steer", undefined]);
      expect(screen.queryByTestId("queued-message-row")).not.toBeInTheDocument();
    });
  });

  it("Send now keeps the row and surfaces an error when steer is rejected", async () => {
    render(<Workspace conversationId="conv-1" />);
    await screen.findByTestId("agent-input");
    const resolveFirst = await startPendingTurn("first turn");

    await userEvent.type(screen.getByTestId("agent-input"), "reject me");
    await userEvent.click(screen.getByTestId("agent-send"));

    vi.mocked(commands.steerGeneration).mockResolvedValue("rejected");
    await userEvent.click(screen.getByTestId("queued-message-send-now"));

    await waitFor(() => expect(screen.getByTestId("queue-steer-error")).toBeInTheDocument());
    // Row stays; no fallback send.
    expect(screen.getByTestId("queued-message-row")).toHaveTextContent("reject me");
    expect(commands.sendAgentMessage).toHaveBeenCalledTimes(1);
    void resolveFirst;
  });

  it("edit recalls a queued message into the composer and removes the row", async () => {
    render(<Workspace conversationId="conv-1" />);
    await screen.findByTestId("agent-input");
    const resolveFirst = await startPendingTurn("first turn");

    await userEvent.type(screen.getByTestId("agent-input"), "edit me");
    await userEvent.click(screen.getByTestId("agent-send"));
    expect(screen.getByTestId("queued-message-row")).toBeInTheDocument();

    await userEvent.click(screen.getByTestId("queued-message-edit"));

    await waitFor(() => {
      expect(screen.getByTestId("agent-input")).toHaveTextContent("edit me");
      expect(screen.queryByTestId("queued-message-row")).not.toBeInTheDocument();
    });
    void resolveFirst;
  });

  it("delete removes a queued message without sending or steering", async () => {
    render(<Workspace conversationId="conv-1" />);
    await screen.findByTestId("agent-input");
    const resolveFirst = await startPendingTurn("first turn");

    await userEvent.type(screen.getByTestId("agent-input"), "delete me");
    await userEvent.click(screen.getByTestId("agent-send"));
    expect(screen.getByTestId("queued-message-row")).toBeInTheDocument();

    await userEvent.click(screen.getByTestId("queued-message-delete"));

    expect(screen.queryByTestId("queued-message-row")).not.toBeInTheDocument();
    expect(commands.steerGeneration).not.toHaveBeenCalled();
    expect(commands.sendAgentMessage).toHaveBeenCalledTimes(1);
    void resolveFirst;
  });

  it("an idle submit still sends immediately without queuing", async () => {
    vi.mocked(commands.sendAgentMessage).mockResolvedValue("ok");
    render(<Workspace conversationId="conv-1" />);
    await screen.findByTestId("agent-input");

    await userEvent.type(screen.getByTestId("agent-input"), "just send it");
    await userEvent.click(screen.getByTestId("agent-send"));

    await waitFor(() =>
      expect(commands.sendAgentMessage).toHaveBeenCalledWith("conv-1", "just send it", undefined),
    );
    expect(screen.queryByTestId("queued-messages")).not.toBeInTheDocument();
  });

  it("a manual Stop leaves the queued messages intact and does not auto-drain", async () => {
    render(<Workspace conversationId="conv-1" />);
    await screen.findByTestId("agent-input");
    const resolveFirst = await startPendingTurn("first turn");

    await userEvent.type(screen.getByTestId("agent-input"), "survive the stop");
    await userEvent.click(screen.getByTestId("agent-send"));
    expect(screen.getByTestId("queued-message-row")).toBeInTheDocument();

    await userEvent.click(screen.getByTestId("stop-generation"));
    expect(commands.stopGeneration).toHaveBeenCalledWith("conv-1");
    // Completing the stopped turn goes idle WITHOUT draining the queue.
    resolveFirst("stopped");

    await waitFor(() => expect(screen.queryByTestId("stop-generation")).not.toBeInTheDocument());
    expect(screen.getByTestId("queued-message-row")).toHaveTextContent("survive the stop");
    // The queued message was never dispatched (only the initial send happened).
    expect(commands.sendAgentMessage).toHaveBeenCalledTimes(1);
  });

  it("a Stop with an empty queue does not suppress a later turn's drain", async () => {
    // Regression (drainSuppressedRef stranding): stopping a turn with NOTHING
    // queued must not leave the suppress flag set to poison the next turn's
    // drain.
    render(<Workspace conversationId="conv-1" />);
    await screen.findByTestId("agent-input");

    // First turn, empty queue, then Stop.
    const resolveFirst = await startPendingTurn("first turn");
    await userEvent.click(screen.getByTestId("stop-generation"));
    resolveFirst("stopped");
    await waitFor(() => expect(screen.queryByTestId("stop-generation")).not.toBeInTheDocument());

    // Second turn, queue a message, complete it NATURALLY — it must drain.
    const resolveSecond = await startPendingTurn("second turn");
    await userEvent.type(screen.getByTestId("agent-input"), "should drain");
    await userEvent.click(screen.getByTestId("agent-send"));
    expect(screen.getByTestId("queued-message-row")).toBeInTheDocument();

    vi.mocked(commands.sendAgentMessage).mockResolvedValue("done");
    resolveSecond("second done");

    await waitFor(() => {
      const sent = vi.mocked(commands.sendAgentMessage).mock.calls.map((c) => c[1]);
      expect(sent).toContain("should drain");
    });
    await waitFor(() => expect(screen.queryByTestId("queued-message-row")).not.toBeInTheDocument());
  });

  it("queued messages are isolated per conversation", async () => {
    const { rerender } = render(<Workspace key="conv-1" conversationId="conv-1" />);
    await screen.findByTestId("agent-input");
    const resolveFirst = await startPendingTurn("first turn");

    await userEvent.type(screen.getByTestId("agent-input"), "conv-1 only");
    await userEvent.click(screen.getByTestId("agent-send"));
    expect(screen.getByTestId("queued-message-row")).toHaveTextContent("conv-1 only");

    rerender(<Workspace key="conv-2" conversationId="conv-2" />);
    await screen.findByTestId("agent-input");
    expect(screen.queryByTestId("queued-message-row")).not.toBeInTheDocument();

    rerender(<Workspace key="conv-1-again" conversationId="conv-1" />);
    await waitFor(() =>
      expect(screen.getByTestId("queued-message-row")).toHaveTextContent("conv-1 only"),
    );
    void resolveFirst;
  });

  it("a goal-mode submit while busy queues and the row hides Send now", async () => {
    render(<Workspace conversationId="conv-1" />);
    await screen.findByTestId("agent-input");
    const resolveFirst = await startPendingTurn("first turn");

    // Turn on goal mode, then submit — routes to onSendAsGoal, which queues with
    // goal intent while busy.
    await userEvent.click(screen.getByTestId("rich-input-goal-toggle"));
    await userEvent.type(screen.getByTestId("agent-input"), "pursue this goal");
    await userEvent.click(screen.getByTestId("agent-send"));

    const row = await screen.findByTestId("queued-message-row");
    expect(row).toHaveTextContent("pursue this goal");
    // Goal rows can't be steered — no "Send now" — but stay editable/deletable.
    expect(screen.queryByTestId("queued-message-send-now")).not.toBeInTheDocument();
    expect(screen.getByTestId("queued-message-edit")).toBeInTheDocument();
    expect(screen.getByTestId("queued-message-delete")).toBeInTheDocument();
    void resolveFirst;
  });
});
