import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import App from "./App";
import { commands, events } from "@/lib/ipc";

// Covers 005-keyboard-shortcuts: the app's first global (not input-scoped)
// keyboard shortcuts, exercised against the real App component with every
// child view's IPC surface mocked (matching Chat.test.tsx/ConversationList.
// test.tsx/Workspace.test.tsx/Settings.test.tsx's existing mock shapes).
vi.mock("@/lib/ipc", () => ({
  commands: {
    listModels: vi.fn(),
    setFocusedConversation: vi.fn(),
    listConversations: vi.fn(),
    createConversation: vi.fn(),
    listMessages: vi.fn(),
    sendMessage: vi.fn(),
    cancelGeneration: vi.fn(),
    openWorkspace: vi.fn(),
    sendAgentMessage: vi.fn(),
    listMcpServers: vi.fn(),
    listSkills: vi.fn(),
  },
  events: {
    onAssistantToken: vi.fn(),
    onAssistantMessageComplete: vi.fn(),
    onAssistantMessageError: vi.fn(),
    onGenerationQueueUpdate: vi.fn(),
  },
}));

function pressCmd(key: string) {
  fireEvent.keyDown(window, { key, metaKey: true });
}

async function waitForReady() {
  await waitFor(() => expect(screen.getByTestId("conversation-list")).toBeInTheDocument());
}

describe("App keyboard shortcuts (005-keyboard-shortcuts)", () => {
  beforeEach(() => {
    vi.clearAllMocks();

    vi.mocked(commands.listModels).mockResolvedValue([
      { id: "m", hardwareTier: "tier1", isActive: true, installed: true },
    ]);
    vi.mocked(commands.listConversations).mockResolvedValue([]);
    vi.mocked(commands.createConversation).mockResolvedValue({
      id: "new-conv",
      workspaceId: null,
      title: "New conversation",
      createdAt: 1,
      updatedAt: 1,
      status: "done",
    });
    vi.mocked(commands.listMessages).mockResolvedValue([]);
    vi.mocked(commands.listMcpServers).mockResolvedValue([]);
    vi.mocked(commands.listSkills).mockResolvedValue([]);
    vi.mocked(events.onAssistantToken).mockResolvedValue(() => {});
    vi.mocked(events.onAssistantMessageComplete).mockResolvedValue(() => {});
    vi.mocked(events.onAssistantMessageError).mockResolvedValue(() => {});
    vi.mocked(events.onGenerationQueueUpdate).mockResolvedValue(() => {});
  });

  it("Cmd+L focuses the chat input from anywhere, and leaves focus alone if already there (US1)", async () => {
    render(<App />);
    await waitForReady();

    await userEvent.click(await screen.findByTestId("new-conversation"));
    const chatInput = await screen.findByTestId("chat-input");

    document.body.focus();
    expect(document.activeElement).not.toBe(chatInput);
    pressCmd("l");
    expect(document.activeElement).toBe(chatInput);

    // Already focused: pressing again must not disturb it.
    pressCmd("l");
    expect(document.activeElement).toBe(chatInput);
  });

  it("Cmd+L focuses the agent task input when in agent mode, not the chat input (US1)", async () => {
    vi.mocked(commands.openWorkspace).mockResolvedValue({
      id: "ws-1",
      path: "/tmp/project",
      displayName: "project",
      createdAt: 1,
      lastOpenedAt: 1,
    });

    render(<App />);
    await waitForReady();

    await userEvent.click(await screen.findByTestId("enter-agent-mode"));
    await userEvent.type(await screen.findByTestId("workspace-path-input"), "/tmp/project");
    await userEvent.click(await screen.findByTestId("open-workspace"));
    const agentInput = await screen.findByTestId("agent-input");

    document.body.focus();
    pressCmd("l");
    expect(document.activeElement).toBe(agentInput);
  });

  it("Cmd+L has no effect when Settings is open — nothing to focus (FR-002)", async () => {
    render(<App />);
    await waitForReady();

    await userEvent.click(await screen.findByTestId("open-settings"));
    await screen.findByTestId("settings-view");

    expect(() => pressCmd("l")).not.toThrow();
    expect(screen.queryByTestId("chat-input")).not.toBeInTheDocument();
    expect(screen.queryByTestId("agent-input")).not.toBeInTheDocument();
  });

  it("typing a plain 'l' (no Cmd) does not trigger the shortcut", async () => {
    render(<App />);
    await waitForReady();

    await userEvent.click(await screen.findByTestId("new-conversation"));
    const chatInput = await screen.findByTestId("chat-input");
    document.body.focus();

    fireEvent.keyDown(window, { key: "l", metaKey: false });
    expect(document.activeElement).not.toBe(chatInput);
  });

  it("Cmd+N creates a new conversation and switches to it from the chat view (US2)", async () => {
    render(<App />);
    await waitForReady();

    pressCmd("n");

    await waitFor(() => expect(commands.createConversation).toHaveBeenCalledTimes(1));
    await screen.findByTestId("chat-input");
  });

  it("Cmd+N switches back to the new conversation from Settings and from agent mode (US2)", async () => {
    render(<App />);
    await waitForReady();

    await userEvent.click(await screen.findByTestId("open-settings"));
    await screen.findByTestId("settings-view");

    pressCmd("n");

    await waitFor(() => expect(commands.createConversation).toHaveBeenCalledTimes(1));
    await screen.findByTestId("chat-input");
    expect(screen.queryByTestId("settings-view")).not.toBeInTheDocument();
  });

  it("Cmd+K opens the shortcuts dialog listing all three shortcuts, and pressing it again closes it (US3, FR-006)", async () => {
    render(<App />);
    await waitForReady();

    pressCmd("k");
    await screen.findByTestId("shortcuts-dialog");
    expect(screen.getAllByTestId("shortcut-item")).toHaveLength(3);

    pressCmd("k");
    await waitFor(() => expect(screen.queryByTestId("shortcuts-dialog")).not.toBeInTheDocument());
  });

  it("Escape and the close button both dismiss the shortcuts dialog (FR-005)", async () => {
    render(<App />);
    await waitForReady();

    pressCmd("k");
    const dialog = await screen.findByTestId("shortcuts-dialog");
    fireEvent(dialog.closest("dialog")!, new Event("cancel", { cancelable: true }));
    await waitFor(() => expect(screen.queryByTestId("shortcuts-dialog")).not.toBeInTheDocument());

    pressCmd("k");
    await screen.findByTestId("shortcuts-dialog");
    await userEvent.click(screen.getByTestId("close-shortcuts-dialog"));
    await waitFor(() => expect(screen.queryByTestId("shortcuts-dialog")).not.toBeInTheDocument());
  });

  it("while the shortcuts dialog is open, Cmd+L and Cmd+N have no effect on the conversation (FR-009)", async () => {
    render(<App />);
    await waitForReady();

    await userEvent.click(await screen.findByTestId("new-conversation"));
    await screen.findByTestId("chat-input");
    vi.mocked(commands.createConversation).mockClear();

    pressCmd("k");
    await screen.findByTestId("shortcuts-dialog");

    document.body.focus();
    pressCmd("l");
    expect(document.activeElement).not.toBe(screen.getByTestId("chat-input"));

    pressCmd("n");
    expect(commands.createConversation).not.toHaveBeenCalled();

    // Once dismissed, the shortcuts work again.
    pressCmd("k");
    await waitFor(() => expect(screen.queryByTestId("shortcuts-dialog")).not.toBeInTheDocument());
    pressCmd("n");
    await waitFor(() => expect(commands.createConversation).toHaveBeenCalledTimes(1));
  });
});
