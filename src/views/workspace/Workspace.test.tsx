import { StrictMode } from "react";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import Workspace from "./Workspace";
import { commands, events } from "@/lib/ipc";
import type { RichMessageContent } from "@/lib/ipc";

type TestDocument = Document & {
  startViewTransition?: (callback: () => void) => unknown;
};

const originalStartViewTransition = (document as TestDocument)
  .startViewTransition;

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
    },
    events: {
      onAgentMessagePersisted: vi.fn(),
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

function setScrollMetrics(
  element: HTMLElement,
  metrics: { scrollHeight: number; clientHeight: number; scrollTop: number },
) {
  let currentScrollTop = metrics.scrollTop;
  Object.defineProperty(element, "scrollHeight", {
    configurable: true,
    value: metrics.scrollHeight,
  });
  Object.defineProperty(element, "clientHeight", {
    configurable: true,
    value: metrics.clientHeight,
  });
  Object.defineProperty(element, "scrollTop", {
    configurable: true,
    get: () => currentScrollTop,
    set: (value: number) => {
      currentScrollTop = value;
    },
  });
}

function expectElementBefore(first: HTMLElement, second: HTMLElement) {
  expect(Boolean(first.compareDocumentPosition(second) & Node.DOCUMENT_POSITION_FOLLOWING)).toBe(
    true,
  );
}

describe("Workspace (006-chat-empty-state: conversationId-driven agent view)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(commands.listMessages).mockResolvedValue([]);
    vi.mocked(commands.isGenerationActive).mockResolvedValue(false);
    // No model loaded in these unit tests — ContextUsageGauge's
    // getContextUsage call is expected to fail and swallow the error,
    // leaving the gauge simply unrendered.
    vi.mocked(commands.getContextUsage).mockRejectedValue(
      new Error("No model loaded"),
    );
    vi.mocked(commands.listSkills).mockResolvedValue([]);
    // Streaming (UI refactor): no live events fire by default in these unit
    // tests -- each test that specifically exercises streaming updates
    // messages by driving `listMessages`'s mock resolution/timing directly
    // instead, since the real signal is "listMessages was called again",
    // not the event itself.
    vi.mocked(events.onAgentMessagePersisted).mockResolvedValue(() => {});
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

  it("fills the shell content area instead of forcing viewport height", async () => {
    render(<Workspace conversationId="conv-1" />);

    const root = screen.getByTestId("workspace-scroll-container").parentElement
      ?.parentElement;
    expect(root).not.toBeNull();
    expect(root!).toHaveClass("h-full");
    expect(root!).not.toHaveClass("h-dvh");
    await waitFor(() =>
      expect(commands.listMessages).toHaveBeenCalledWith("conv-1"),
    );
  });

  it("notifies when active messages refresh so the app can mark the conversation seen", async () => {
    const onConversationSeen = vi.fn();
    vi.mocked(commands.listMessages).mockResolvedValue([
      { ...messageFixture("m1", "hello"), conversationId: "c1" },
    ]);

    render(
      <Workspace conversationId="c1" onConversationSeen={onConversationSeen} />,
    );

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

    render(
      <Workspace
        conversationId="conv-1"
        onConversationSeen={onConversationSeen}
      />,
    );
    await screen.findByText("first message");
    await waitFor(() =>
      expect(onConversationSeen).toHaveBeenCalledWith("conv-1"),
    );
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
      <Workspace
        conversationId="conv-1"
        onConversationSeen={firstSeenCallback}
      />,
    );
    await screen.findByText("first message");
    await waitFor(() =>
      expect(events.onAgentMessagePersisted).toHaveBeenCalledTimes(1),
    );

    firstSeenCallback.mockClear();
    rerender(
      <Workspace
        conversationId="conv-1"
        onConversationSeen={secondSeenCallback}
      />,
    );
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(commands.listMessages).toHaveBeenCalledTimes(1);
    expect(events.onAgentMessagePersisted).toHaveBeenCalledTimes(1);
    expect(unlisten).not.toHaveBeenCalled();

    firePersisted({ conversationId: "conv-1" });

    await screen.findByText("second message");
    expect(firstSeenCallback).not.toHaveBeenCalled();
    expect(secondSeenCallback).toHaveBeenCalledWith("conv-1");
  });

  it("sends a task and shows a thinking state until the real (non-streamed) reply returns", async () => {
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

    render(
      <Workspace
        conversationId="conv-1"
        onConversationSeen={onConversationSeen}
      />,
    );
    await screen.findByTestId("agent-input");
    await waitFor(() =>
      expect(onConversationSeen).toHaveBeenCalledWith("conv-1"),
    );
    onConversationSeen.mockClear();

    await userEvent.type(
      screen.getByTestId("agent-input"),
      "list the files here",
    );
    await userEvent.click(screen.getByTestId("agent-send"));

    const status = await screen.findByTestId("agent-thinking");
    const composerShell = screen.getByTestId("workspace-composer-shell");
    expect(status).toHaveTextContent("Thinking");
    expect(screen.getByTestId("agent-thinking-timer")).toHaveTextContent("0.0s");
    expect(status).not.toHaveTextContent("Working");
    expect(status.closest('[data-testid="chat-message"]')).toBeNull();
    expectElementBefore(status, composerShell);
    expect(status).toHaveClass("border-b");
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
    await userEvent.type(
      screen.getByTestId("agent-input"),
      "list the files here",
    );
    await userEvent.click(screen.getByTestId("agent-send"));

    await waitFor(() =>
      expect(screen.getByTestId("agent-thinking")).toBeInTheDocument(),
    );

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
    await waitFor(() => {
      expect(screen.getByTestId("agent-input")).toHaveAttribute(
        "contenteditable",
        "true",
      );
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
    expect(vi.mocked(commands.listMessages).mock.calls.length).toBe(
      callsBefore,
    );
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

    expect(commands.answerUserQuestion).toHaveBeenCalledWith("q1", [
      "actually, do both",
    ]);
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
    expect(status).toHaveTextContent(/running/i);
    expect(screen.getByTestId("bash-command")).toHaveTextContent(
      "cargo test --lib",
    );
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
    expect(status).toHaveTextContent(/running/i);
    expect(screen.queryByTestId("agent-thinking")).not.toBeInTheDocument();
  });

  it("blocks the composer and shows Thinking when the latest message is an unfinished tool_call with no dedicated pending widget (e.g. Grep)", async () => {
    // Regression: only AskUserQuestion/Bash/Task counted as "in flight",
    // so a turn stuck inside any other tool (a slow Grep, in production)
    // left the composer enabled after a reload wiped the in-memory
    // send-in-flight flag — letting the user queue a duplicate user
    // message behind the still-running turn.
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

    expect(await screen.findByTestId("agent-thinking")).toHaveTextContent(
      "Thinking",
    );
    expect(screen.getByTestId("agent-input")).toHaveAttribute(
      "contenteditable",
      "false",
    );
  });

  it("keeps the composer blocked after a reload while the backend reports the turn still active, even with no trailing tool_call (generation phase)", async () => {
    // The trailing-tool_call signal only covers the tool-execution window.
    // While the model is *generating* (latest row = user text or a paired
    // tool_result — the longest phases with local inference), only the
    // backend's ActiveGenerations knows a turn is live; a reload wipes
    // every in-memory frontend flag.
    vi.mocked(commands.listMessages).mockResolvedValue([
      messageFixture("u1", "find the needle"),
    ]);
    vi.mocked(commands.isGenerationActive).mockResolvedValue(true);

    render(<Workspace conversationId="conv-1" />);

    await screen.findByTestId("agent-thinking");
    expect(screen.getByTestId("agent-input")).toHaveAttribute(
      "contenteditable",
      "false",
    );
  });

  it("starts the streaming chron from the latest persisted user message during a backend-active reload", async () => {
    const nowSpy = vi.spyOn(Date, "now").mockReturnValue(6_000);
    vi.mocked(commands.listMessages).mockResolvedValue([
      messageFixture("u1", "find the needle", 4_000),
    ]);
    vi.mocked(commands.isGenerationActive).mockResolvedValue(true);

    render(<Workspace conversationId="conv-1" />);

    expect(await screen.findByTestId("agent-thinking-timer")).toHaveTextContent(
      "2.0s",
    );
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

    expect(await screen.findByTestId("agent-thinking-timer")).toHaveTextContent(
      "3.0s",
    );
    firePersisted({ conversationId: "conv-1" });

    await waitFor(() =>
      expect(screen.getByTestId("agent-thinking-timer")).toHaveTextContent(
        "3.0s",
      ),
    );
    nowSpy.mockRestore();
  });

  it("keeps the composer divider when the streaming status is hidden", async () => {
    vi.mocked(commands.listMessages).mockResolvedValue([]);

    render(<Workspace conversationId="conv-1" />);

    expect(await screen.findByTestId("workspace-composer-shell")).toHaveClass(
      "border-t",
    );
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
    const statuses = screen.getAllByTestId("bash-status");
    expect(statuses).toHaveLength(1);
    expect(statuses[0]).not.toHaveTextContent(/running/i);
  });

  it("009-rich-chat-input regression: a message containing a chip forwards richContent to sendAgentMessage, not just the flat text", async () => {
    vi.mocked(commands.sendAgentMessage).mockResolvedValue("ok");

    render(<Workspace conversationId="conv-1" />);
    const input = await screen.findByTestId("agent-input");

    // Crosses the paste-collapse threshold — produces a real pastedText
    // chip, matching RichInput's own US2 test's paste-simulation pattern.
    const pastedBlock = Array.from({ length: 15 }, (_, i) => `line-${i}`).join(
      "\n",
    );
    fireEvent.paste(input, {
      clipboardData: { items: [], getData: () => pastedBlock },
    });
    await screen.findByTestId("pasted-text-chip");

    await userEvent.click(screen.getByTestId("agent-send"));

    await waitFor(() => expect(commands.sendAgentMessage).toHaveBeenCalled());
    const [, , richContentArg] = vi.mocked(commands.sendAgentMessage).mock
      .calls[0];
    expect(richContentArg).toBeDefined();
    const parsed = JSON.parse(richContentArg as string);
    expect(
      parsed.segments.some(
        (s: { type: string; text?: string }) =>
          s.type === "pastedText" && s.text === pastedBlock,
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
      expect(commands.sendAgentMessage).toHaveBeenCalledWith(
        "conv-1",
        "first task",
        undefined,
      );
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
      expect(screen.getByTestId("agent-input")).toHaveAttribute(
        "contenteditable",
        "true",
      );
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
      expect(screen.getByTestId("agent-input")).toHaveAttribute(
        "contenteditable",
        "true",
      );
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
      expect(commands.sendAgentMessage).toHaveBeenCalledWith(
        "conv-1",
        "first task",
        undefined,
      );
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
      expect(screen.getByTestId("agent-input")).toHaveAttribute(
        "contenteditable",
        "true",
      );
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
      expect(screen.getByTestId("agent-input")).toHaveAttribute(
        "contenteditable",
        "true",
      );
    });
  });

  it("surfaces an error when a pending initial turn send fails", async () => {
    vi.mocked(commands.sendAgentMessage).mockRejectedValue(
      new Error("pending send failed"),
    );

    render(
      <Workspace
        conversationId="conv-1"
        pendingInitialTurn={{ conversationId: "conv-1", content: "first task" }}
      />,
    );

    await waitFor(() => {
      expect(screen.getByTestId("workspace-error")).toHaveTextContent(
        "pending send failed",
      );
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
    await waitFor(() =>
      expect(commands.listMessages).toHaveBeenCalledWith("conv-1"),
    );

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
    await waitFor(() =>
      expect(commands.listMessages).toHaveBeenCalledWith("conv-1"),
    );

    rerender(<Workspace conversationId="conv-2" />);
    await waitFor(() =>
      expect(commands.listMessages).toHaveBeenCalledWith("conv-2"),
    );

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
    let resolveCompact!: (
      usage: Awaited<ReturnType<typeof commands.compactConversation>>,
    ) => void;
    let resolveStaleCompactMessages!: (
      messages: Awaited<ReturnType<typeof commands.listMessages>>,
    ) => void;
    let conv1ListCalls = 0;
    vi.mocked(commands.compactConversation).mockReturnValue(
      new Promise((resolve) => {
        resolveCompact = resolve;
      }),
    );
    vi.mocked(commands.listMessages).mockImplementation(
      (requestedConversationId) => {
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
      },
    );

    const { rerender } = render(<Workspace conversationId="conv-1" />);
    await screen.findByTestId("agent-input");

    await userEvent.type(screen.getByTestId("agent-input"), "/compact");
    await userEvent.click(screen.getByTestId("agent-send"));
    await waitFor(() =>
      expect(commands.compactConversation).toHaveBeenCalledWith("conv-1"),
    );

    resolveCompact({
      conversationId: "conv-1",
      tokensUsed: 100,
      tokenBudget: 2048,
      state: "justCompacted",
    });
    await waitFor(() =>
      expect(commands.listMessages).toHaveBeenCalledWith("conv-1"),
    );

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
    let resolveCompact!: (
      usage: Awaited<ReturnType<typeof commands.compactConversation>>,
    ) => void;
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
    vi.mocked(commands.listMessages).mockImplementation(
      (requestedConversationId) => {
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
      },
    );

    const { rerender } = render(
      <Workspace
        key="conv-1"
        conversationId="conv-1"
        onConversationSeen={onConversationSeen}
      />,
    );
    await screen.findByTestId("agent-input");
    await waitFor(() =>
      expect(onConversationSeen).toHaveBeenCalledWith("conv-1"),
    );
    onConversationSeen.mockClear();

    await userEvent.type(screen.getByTestId("agent-input"), "/compact");
    await userEvent.click(screen.getByTestId("agent-send"));
    await waitFor(() =>
      expect(commands.compactConversation).toHaveBeenCalledWith("conv-1"),
    );

    resolveCompact({
      conversationId: "conv-1",
      tokensUsed: 100,
      tokenBudget: 2048,
      state: "justCompacted",
    });
    await waitFor(() => expect(conv1ListCalls).toBe(2));

    rerender(
      <Workspace
        key="conv-2"
        conversationId="conv-2"
        onConversationSeen={onConversationSeen}
      />,
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

    await waitFor(() =>
      expect(commands.compactConversation).toHaveBeenCalledWith("conv-1"),
    );
    expect(await screen.findByTestId("context-notice")).toHaveTextContent(
      "StrictMode compact refresh",
    );
  });

  it("refreshes a /compact result after returning to the same conversation before completion", async () => {
    let resolveCompact!: (
      usage: Awaited<ReturnType<typeof commands.compactConversation>>,
    ) => void;
    let compactFinished = false;
    const onConversationSeen = vi.fn();

    vi.mocked(commands.compactConversation).mockReturnValue(
      new Promise((resolve) => {
        resolveCompact = resolve;
      }),
    );
    vi.mocked(commands.listMessages).mockImplementation(
      (requestedConversationId) => {
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
      },
    );

    const { rerender } = render(
      <Workspace
        key="conv-1"
        conversationId="conv-1"
        onConversationSeen={onConversationSeen}
      />,
    );
    await screen.findByTestId("agent-input");

    await userEvent.type(screen.getByTestId("agent-input"), "/compact");
    await userEvent.click(screen.getByTestId("agent-send"));
    await waitFor(() =>
      expect(commands.compactConversation).toHaveBeenCalledWith("conv-1"),
    );

    rerender(
      <Workspace
        key="conv-2"
        conversationId="conv-2"
        onConversationSeen={onConversationSeen}
      />,
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

    expect(await screen.findByTestId("context-notice")).toHaveTextContent(
      "late compact notice",
    );
    expect(onConversationSeen).toHaveBeenCalledWith("conv-1");
  });

  it("ignores stale /compact errors after switching conversations", async () => {
    let rejectCompact!: (error: Error) => void;
    vi.mocked(commands.compactConversation).mockReturnValue(
      new Promise((_, reject) => {
        rejectCompact = reject;
      }),
    );
    vi.mocked(commands.listMessages).mockImplementation(
      (requestedConversationId) => {
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
      },
    );

    const { rerender } = render(<Workspace conversationId="conv-1" />);
    await screen.findByTestId("agent-input");

    await userEvent.type(screen.getByTestId("agent-input"), "/compact");
    await userEvent.click(screen.getByTestId("agent-send"));
    await waitFor(() =>
      expect(commands.compactConversation).toHaveBeenCalledWith("conv-1"),
    );

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

    await userEvent.type(
      screen.getByTestId("agent-input"),
      "first workspace task",
    );
    await userEvent.click(screen.getByTestId("agent-send"));

    await waitFor(() => {
      expect(screen.getByTestId("agent-thinking")).toBeInTheDocument();
      expect(screen.getByTestId("agent-input")).toHaveAttribute(
        "contenteditable",
        "false",
      );
    });

    rerender(<Workspace conversationId="conv-2" />);
    await screen.findByText("second workspace");

    expect(screen.getByTestId("agent-input")).toHaveAttribute(
      "contenteditable",
      "true",
    );
    expect(screen.queryByText("first workspace task")).not.toBeInTheDocument();

    resolveSend("old conversation done");
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(screen.getByTestId("agent-input")).toHaveAttribute(
      "contenteditable",
      "true",
    );
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
    vi.mocked(commands.listMessages).mockImplementation(
      (requestedConversationId) => {
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
      },
    );

    const { rerender } = render(
      <Workspace
        key="conv-1"
        conversationId="conv-1"
        onConversationSeen={onConversationSeen}
      />,
    );
    await screen.findByTestId("agent-input");
    await waitFor(() =>
      expect(onConversationSeen).toHaveBeenCalledWith("conv-1"),
    );
    onConversationSeen.mockClear();

    await userEvent.type(
      screen.getByTestId("agent-input"),
      "first workspace task",
    );
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
      <Workspace
        key="conv-2"
        conversationId="conv-2"
        onConversationSeen={onConversationSeen}
      />,
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
      expect(commands.sendAgentMessage).toHaveBeenCalledWith(
        "conv-1",
        "strict task",
        undefined,
      ),
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
    vi.mocked(commands.listMessages).mockImplementation(
      (requestedConversationId) => {
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
      },
    );

    const { rerender } = render(
      <Workspace
        key="conv-1"
        conversationId="conv-1"
        onConversationSeen={onConversationSeen}
      />,
    );
    await screen.findByTestId("agent-input");

    await userEvent.type(screen.getByTestId("agent-input"), "late task");
    await userEvent.click(screen.getByTestId("agent-send"));
    await waitFor(() =>
      expect(commands.sendAgentMessage).toHaveBeenCalledWith(
        "conv-1",
        "late task",
        undefined,
      ),
    );

    rerender(
      <Workspace
        key="conv-2"
        conversationId="conv-2"
        onConversationSeen={onConversationSeen}
      />,
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

  it("keeps the original conversation disabled across remounts while its send is still pending", async () => {
    let resolveSend!: (value: string) => void;
    vi.mocked(commands.listMessages).mockImplementation(
      (requestedConversationId) => {
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
      },
    );
    vi.mocked(commands.sendAgentMessage).mockReturnValue(
      new Promise((resolve) => {
        resolveSend = resolve;
      }),
    );

    const { rerender } = render(
      <Workspace key="conv-1" conversationId="conv-1" />,
    );
    await screen.findByTestId("agent-input");

    await userEvent.type(
      screen.getByTestId("agent-input"),
      "first workspace task",
    );
    await userEvent.click(screen.getByTestId("agent-send"));

    await waitFor(() => {
      expect(commands.sendAgentMessage).toHaveBeenCalledWith(
        "conv-1",
        "first workspace task",
        undefined,
      );
      expect(screen.getByTestId("agent-input")).toHaveAttribute(
        "contenteditable",
        "false",
      );
    });

    rerender(<Workspace key="conv-2" conversationId="conv-2" />);
    await screen.findByText("second workspace");
    expect(screen.getByTestId("agent-input")).toHaveAttribute(
      "contenteditable",
      "true",
    );

    rerender(<Workspace key="conv-1-return" conversationId="conv-1" />);
    await waitFor(() => {
      expect(screen.getByTestId("agent-input")).toHaveAttribute(
        "contenteditable",
        "false",
      );
      expect(screen.getByTestId("agent-thinking")).toBeInTheDocument();
    });

    await userEvent.click(screen.getByTestId("agent-send"));
    expect(commands.sendAgentMessage).toHaveBeenCalledTimes(1);

    resolveSend("first send done");
    await waitFor(() => {
      expect(screen.getByTestId("agent-input")).toHaveAttribute(
        "contenteditable",
        "true",
      );
    });
  });

  it("shows an error instead of hanging if sending fails", async () => {
    vi.mocked(commands.sendAgentMessage).mockRejectedValue(
      new Error("inference crashed"),
    );

    render(<Workspace conversationId="conv-1" />);
    await screen.findByTestId("agent-input");
    await userEvent.type(screen.getByTestId("agent-input"), "do something");
    await userEvent.click(screen.getByTestId("agent-send"));

    await waitFor(() => {
      expect(screen.getByTestId("workspace-error")).toHaveTextContent(
        "inference crashed",
      );
    });
  });

  // Autoscroll itself (following content growth while pinned, initial
  // scroll to the bottom) is use-stick-to-bottom's own tested contract,
  // driven by ResizeObserver — inert in jsdom, so it isn't re-tested here.
  // What IS ours to test is the wiring: the scroll-to-bottom button follows
  // the library's isAtBottom state (fed by real scroll events), and
  // clicking it hands off to the library's scrollToBottom.

  it("shows the scroll-to-bottom button after scrolling up and hides it again near the bottom", async () => {
    vi.mocked(commands.listMessages).mockResolvedValueOnce([
      messageFixture("m1", "first message"),
    ]);

    render(<Workspace conversationId="conv-1" />);
    const scrollContainer = await screen.findByTestId(
      "workspace-scroll-container",
    );
    await screen.findByText("first message");

    expect(screen.queryByTestId("scroll-to-bottom")).not.toBeInTheDocument();

    // use-stick-to-bottom escapes its lock on *upward* scroll movement, so
    // establish a bottom baseline first, then scroll up.
    setScrollMetrics(scrollContainer, {
      scrollHeight: 1000,
      clientHeight: 300,
      scrollTop: 700,
    });
    fireEvent.scroll(scrollContainer);
    scrollContainer.scrollTop = 200;
    fireEvent.scroll(scrollContainer);

    await waitFor(() =>
      expect(screen.getByTestId("scroll-to-bottom")).toHaveClass(
        "left-1/2",
        "-translate-x-1/2",
      ),
    );

    // Scrolling back to within the library's near-bottom threshold re-pins
    // and hides the button.
    scrollContainer.scrollTop = 680;
    fireEvent.scroll(scrollContainer);

    await waitFor(() =>
      expect(screen.queryByTestId("scroll-to-bottom")).not.toBeInTheDocument(),
    );
  });

  it("scrolls to bottom and hides the scroll-to-bottom button when clicked", async () => {
    vi.mocked(commands.listMessages).mockResolvedValueOnce([
      messageFixture("m1", "first message"),
    ]);

    render(<Workspace conversationId="conv-1" />);
    const scrollContainer = await screen.findByTestId(
      "workspace-scroll-container",
    );
    await screen.findByText("first message");

    setScrollMetrics(scrollContainer, {
      scrollHeight: 1000,
      clientHeight: 300,
      scrollTop: 700,
    });
    fireEvent.scroll(scrollContainer);
    scrollContainer.scrollTop = 200;
    fireEvent.scroll(scrollContainer);
    await waitFor(() => screen.getByTestId("scroll-to-bottom"));

    await userEvent.click(
      screen.getByRole("button", { name: "Scroll to bottom" }),
    );

    // The library's spring animation converges asymptotically (a real
    // browser clamps scrollTop to integers; the jsdom metrics mock
    // doesn't), so assert "back at the bottom" rather than an exact px.
    await waitFor(
      () => expect(scrollContainer.scrollTop).toBeGreaterThanOrEqual(695),
      {
        timeout: 4000,
      },
    );
    await waitFor(() =>
      expect(screen.queryByTestId("scroll-to-bottom")).not.toBeInTheDocument(),
    );
  });

  it("always scrolls back to the bottom when the user sends a message, even after the sticky lock escaped", async () => {
    // use-stick-to-bottom only follows content growth while its lock is
    // engaged; ANY upward scroll/wheel silently escapes it — and within
    // the library's 70px near-bottom threshold the scroll-to-bottom
    // button stays hidden, so autoscroll *looks* active while sends
    // don't follow. Sending your own message must always re-engage
    // (the library README's own ChatBox pattern).
    //
    // A dedicated conversation id: the never-resolving send below leaves
    // this conversation in the module-level send-in-flight Set for the
    // rest of the file, which must not block other tests' "conv-1" sends.
    vi.mocked(commands.sendAgentMessage).mockReturnValue(new Promise(() => {}));
    vi.mocked(commands.listMessages).mockResolvedValueOnce([
      messageFixture("m1", "first message"),
    ]);

    render(<Workspace conversationId="conv-scroll-send" />);
    const scrollContainer = await screen.findByTestId(
      "workspace-scroll-container",
    );
    await screen.findByText("first message");

    // Escape the lock: bottom baseline, then an upward scroll (a stray
    // trackpad flick while reading).
    setScrollMetrics(scrollContainer, {
      scrollHeight: 1000,
      clientHeight: 300,
      scrollTop: 700,
    });
    fireEvent.scroll(scrollContainer);
    scrollContainer.scrollTop = 200;
    fireEvent.scroll(scrollContainer);
    await waitFor(() => screen.getByTestId("scroll-to-bottom"));

    const input = screen.getByTestId("agent-input");
    await userEvent.click(input);
    await userEvent.type(input, "follow up{Enter}");

    await waitFor(
      () => expect(scrollContainer.scrollTop).toBeGreaterThanOrEqual(695),
      {
        timeout: 4000,
      },
    );
  });

  it("hides the scroll-to-bottom button when switching conversations", async () => {
    // StickToBottom is keyed by conversationId: switching remounts the
    // scroll state, so the new conversation starts pinned at the bottom
    // with no stale detached-button state carried over.
    vi.mocked(commands.listMessages)
      .mockResolvedValueOnce([messageFixture("m1", "first workspace")])
      .mockResolvedValueOnce([
        {
          ...messageFixture("m2", "second workspace"),
          conversationId: "conv-2",
        },
      ]);

    const { rerender } = render(<Workspace conversationId="conv-1" />);
    const scrollContainer = await screen.findByTestId(
      "workspace-scroll-container",
    );
    await screen.findByText("first workspace");

    setScrollMetrics(scrollContainer, {
      scrollHeight: 1000,
      clientHeight: 300,
      scrollTop: 700,
    });
    fireEvent.scroll(scrollContainer);
    scrollContainer.scrollTop = 200;
    fireEvent.scroll(scrollContainer);
    await waitFor(() =>
      expect(screen.getByTestId("scroll-to-bottom")).toBeInTheDocument(),
    );

    rerender(<Workspace conversationId="conv-2" />);

    expect(screen.queryByTestId("scroll-to-bottom")).not.toBeInTheDocument();
    await screen.findByText("second workspace");
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

    render(
      <Workspace
        conversationId="conv-1"
        onConversationSeen={onConversationSeen}
      />,
    );
    await screen.findByTestId("agent-input");
    await waitFor(() =>
      expect(onConversationSeen).toHaveBeenCalledWith("conv-1"),
    );
    onConversationSeen.mockClear();

    await userEvent.type(screen.getByTestId("agent-input"), "/compact");
    await userEvent.click(screen.getByTestId("agent-send"));

    await waitFor(() =>
      expect(commands.compactConversation).toHaveBeenCalledWith("conv-1"),
    );
    expect(commands.sendAgentMessage).not.toHaveBeenCalled();
    expect(await screen.findByTestId("context-notice")).toHaveTextContent(
      "Conversation condensed to save space",
    );
    expect(onConversationSeen).toHaveBeenCalledWith("conv-1");
  });
});
