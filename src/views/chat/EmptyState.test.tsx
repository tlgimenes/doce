import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import EmptyState from "./EmptyState";
import { commands, events } from "@/lib/ipc";

vi.mock("@/lib/ipc", () => ({
  commands: {
    openWorkspace: vi.fn(),
    createConversation: vi.fn(),
    sendAgentMessage: vi.fn(),
    listWorkspaces: vi.fn(),
    // The home now hosts Connections + ActivityView, which read these on mount.
    listOauthAccounts: vi.fn(),
    listGoogleWorkspaceServices: vi.fn(),
    listMcpServers: vi.fn(),
    listFeedCards: vi.fn(),
    dismissFeedCard: vi.fn(),
  },
  events: {
    onFeedCardCreated: vi.fn(),
  },
}));

vi.mock("@tauri-apps/api/path", () => ({
  homeDir: vi.fn(() => Promise.resolve("/Users/tester")),
}));

describe("EmptyState (006-chat-empty-state)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(commands.listWorkspaces).mockResolvedValue([]);
    // Keep the home's Connections + Activity mounts inert in these composer tests.
    vi.mocked(commands.listOauthAccounts).mockResolvedValue([]);
    vi.mocked(commands.listGoogleWorkspaceServices).mockResolvedValue([]);
    vi.mocked(commands.listMcpServers).mockResolvedValue([]);
    vi.mocked(commands.listFeedCards).mockResolvedValue([]);
    vi.mocked(events.onFeedCardCreated).mockResolvedValue(() => {});
  });

  it("renders Connections and the Activity feed on the home, around the composer", async () => {
    render(<EmptyState onConversationCreated={vi.fn()} />);
    // The composer stays; Connections + Activity now sit beneath it on the home.
    expect(screen.getByTestId("empty-state-composer")).toBeInTheDocument();
    expect(await screen.findByTestId("home-connections")).toBeInTheDocument();
    expect(screen.getByTestId("connections-section")).toBeInTheDocument();
    expect(screen.getByTestId("home-activity-section")).toBeInTheDocument();
    expect(screen.getByTestId("activity-view")).toBeInTheDocument();
  });

  it("shows the composer, not static text, with Home as the default folder target", async () => {
    render(<EmptyState onConversationCreated={vi.fn()} />);
    expect(screen.getByTestId("empty-state-input")).toBeInTheDocument();
    await waitFor(() => {
      expect(screen.getByTestId("folder-target-selector")).toHaveTextContent("Home");
    });
  });

  it("fills the shell content area instead of forcing viewport height", () => {
    render(<EmptyState onConversationCreated={vi.fn()} />);

    expect(screen.getByTestId("empty-state")).toHaveClass("h-full");
    expect(screen.getByTestId("empty-state")).not.toHaveClass("h-dvh");
  });

  it("US1: submitting with the Home target untouched creates a workspace-scoped conversation and hands off the first turn without waiting for the agent", async () => {
    vi.mocked(commands.openWorkspace).mockResolvedValue({
      id: "ws-home",
      path: "/Users/tester",
      displayName: "tester",
      createdAt: 1,
      lastOpenedAt: 1,
    });
    vi.mocked(commands.createConversation).mockResolvedValue({
      id: "conv-1",
      workspaceId: "ws-home",
      title: "New conversation",
      createdAt: 1,
      updatedAt: 1,
      lastSeenAt: 1,
      status: "done",
    });
    vi.mocked(commands.sendAgentMessage).mockReturnValue(new Promise(() => {}));
    const onConversationCreated = vi.fn();

    render(<EmptyState onConversationCreated={onConversationCreated} />);
    await waitFor(() =>
      expect(screen.getByTestId("folder-target-selector")).toHaveTextContent("Home"),
    );

    await userEvent.type(screen.getByTestId("empty-state-input"), "fix the login bug");
    await userEvent.click(screen.getByTestId("empty-state-submit"));

    await waitFor(() =>
      expect(onConversationCreated).toHaveBeenCalledWith(
        expect.objectContaining({ id: "conv-1", workspaceId: "ws-home" }),
        expect.objectContaining({
          conversationId: "conv-1",
          content: "fix the login bug",
          richContent: undefined,
        }),
      ),
    );

    expect(commands.openWorkspace).toHaveBeenCalledWith("/Users/tester");
    expect(commands.createConversation).toHaveBeenCalledWith("ws-home");
    expect(commands.sendAgentMessage).not.toHaveBeenCalled();

    const openOrder = vi.mocked(commands.openWorkspace).mock.invocationCallOrder[0];
    const createOrder = vi.mocked(commands.createConversation).mock.invocationCallOrder[0];
    const handoffOrder = onConversationCreated.mock.invocationCallOrder[0];
    expect(openOrder).toBeLessThan(createOrder);
    expect(createOrder).toBeLessThan(handoffOrder);
  });

  it("009-rich-chat-input regression: a message containing a chip forwards richContent through the pending initial turn", async () => {
    vi.mocked(commands.openWorkspace).mockResolvedValue({
      id: "ws-home",
      path: "/Users/tester",
      displayName: "tester",
      createdAt: 1,
      lastOpenedAt: 1,
    });
    vi.mocked(commands.createConversation).mockResolvedValue({
      id: "conv-1",
      workspaceId: "ws-home",
      title: "New conversation",
      createdAt: 1,
      updatedAt: 1,
      lastSeenAt: 1,
      status: "done",
    });
    const onConversationCreated = vi.fn();

    render(<EmptyState onConversationCreated={onConversationCreated} />);
    await waitFor(() =>
      expect(screen.getByTestId("folder-target-selector")).toHaveTextContent("Home"),
    );

    const input = screen.getByTestId("empty-state-input");
    const pastedBlock = Array.from({ length: 15 }, (_, i) => `line-${i}`).join("\n");
    fireEvent.paste(input, { clipboardData: { items: [], getData: () => pastedBlock } });
    await screen.findByTestId("pasted-text-chip");

    await userEvent.click(screen.getByTestId("empty-state-submit"));

    await waitFor(() => expect(onConversationCreated).toHaveBeenCalled());
    const [, pendingTurn] = onConversationCreated.mock.calls[0];
    expect(pendingTurn.richContent).toBeDefined();
    expect(
      pendingTurn.richContent.segments.some(
        (s: { type: string; text?: string }) => s.type === "pastedText" && s.text === pastedBlock,
      ),
    ).toBe(true);
    expect(commands.sendAgentMessage).not.toHaveBeenCalled();
  });

  it("marks the empty-state composer as the chat composer view-transition target", async () => {
    render(<EmptyState onConversationCreated={vi.fn()} />);

    expect(await screen.findByTestId("empty-state-composer")).toHaveClass(
      "[view-transition-name:chat-composer]",
    );
  });

  it("submitting empty or whitespace-only text does nothing", async () => {
    render(<EmptyState onConversationCreated={vi.fn()} />);
    await waitFor(() =>
      expect(screen.getByTestId("folder-target-selector")).toHaveTextContent("Home"),
    );

    await userEvent.type(screen.getByTestId("empty-state-input"), "   ");
    await userEvent.click(screen.getByTestId("empty-state-submit"));

    expect(commands.openWorkspace).not.toHaveBeenCalled();
  });

  it("surfaces a failure inline and does not proceed to later steps (contracts/conversation-creation.md Failure handling)", async () => {
    vi.mocked(commands.openWorkspace).mockRejectedValue(new Error("not a directory"));
    const onConversationCreated = vi.fn();

    render(<EmptyState onConversationCreated={onConversationCreated} />);
    await waitFor(() =>
      expect(screen.getByTestId("folder-target-selector")).toHaveTextContent("Home"),
    );

    await userEvent.type(screen.getByTestId("empty-state-input"), "do a thing");
    await userEvent.click(screen.getByTestId("empty-state-submit"));

    await waitFor(() => {
      expect(screen.getByTestId("empty-state-error")).toHaveTextContent("not a directory");
    });
    expect(commands.createConversation).not.toHaveBeenCalled();
    expect(commands.sendAgentMessage).not.toHaveBeenCalled();
    expect(onConversationCreated).not.toHaveBeenCalled();
  });

  it("US2: clicking the folder-target selector opens the picker, and picking a folder updates the target used on submit", async () => {
    vi.mocked(commands.listWorkspaces).mockResolvedValue([
      {
        id: "ws-1",
        path: "/Users/tester/code/doce",
        displayName: "doce",
        createdAt: 1,
        lastOpenedAt: 5,
      },
    ]);
    vi.mocked(commands.openWorkspace).mockResolvedValue({
      id: "ws-1",
      path: "/Users/tester/code/doce",
      displayName: "doce",
      createdAt: 1,
      lastOpenedAt: 5,
    });
    vi.mocked(commands.createConversation).mockResolvedValue({
      id: "conv-2",
      workspaceId: "ws-1",
      title: "New conversation",
      createdAt: 1,
      updatedAt: 1,
      lastSeenAt: 1,
      status: "done",
    });
    vi.mocked(commands.sendAgentMessage).mockResolvedValue("ok");

    render(<EmptyState onConversationCreated={vi.fn()} />);
    await waitFor(() =>
      expect(screen.getByTestId("folder-target-selector")).toHaveTextContent("Home"),
    );

    await userEvent.click(screen.getByTestId("folder-target-selector"));
    await screen.findByTestId("folder-picker");
    await userEvent.click(await screen.findByText("doce"));

    await waitFor(() =>
      expect(screen.getByTestId("folder-target-selector")).toHaveTextContent("~/code/doce"),
    );
    expect(screen.queryByTestId("folder-picker")).not.toBeInTheDocument();

    await userEvent.type(screen.getByTestId("empty-state-input"), "work on doce");
    await userEvent.click(screen.getByTestId("empty-state-submit"));

    await waitFor(() =>
      expect(commands.openWorkspace).toHaveBeenCalledWith("/Users/tester/code/doce"),
    );
  });

  it("US2: dismissing the picker without picking anything leaves the target unchanged (FR-011)", async () => {
    vi.mocked(commands.listWorkspaces).mockResolvedValue([
      {
        id: "ws-1",
        path: "/Users/tester/code/doce",
        displayName: "doce",
        createdAt: 1,
        lastOpenedAt: 5,
      },
    ]);

    render(<EmptyState onConversationCreated={vi.fn()} />);
    await waitFor(() =>
      expect(screen.getByTestId("folder-target-selector")).toHaveTextContent("Home"),
    );

    await userEvent.click(screen.getByTestId("folder-target-selector"));
    await screen.findByTestId("folder-picker");
    await userEvent.keyboard("{Escape}");

    await waitFor(() => expect(screen.queryByTestId("folder-picker")).not.toBeInTheDocument());
    expect(screen.getByTestId("folder-target-selector")).toHaveTextContent("Home");
  });
});
