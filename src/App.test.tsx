import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, fireEvent, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import App from "./App";
import { commands, events } from "@/lib/ipc";

// Covers 005-keyboard-shortcuts: the app's first global (not input-scoped)
// keyboard shortcuts, exercised against the real App component with every
// child view's IPC surface mocked (matching Chat.test.tsx/ConversationList.
// test.tsx/Workspace.test.tsx/Settings.test.tsx's existing mock shapes).
//
// 006-chat-empty-state changed what landing on "no conversation selected"
// means: it's always the EmptyState composer now (never a bare,
// input-less placeholder), and "+ New conversation"/Cmd+N no longer
// instant-creates — so several of these cases now go through the composer's
// real open_workspace -> create_conversation -> send_agent_message sequence
// instead of a single mocked createConversation() call.
vi.mock("@/lib/ipc", () => ({
  commands: {
    listModels: vi.fn(),
    setFocusedConversation: vi.fn(),
    listConversations: vi.fn(),
    createConversation: vi.fn(),
    openWorkspace: vi.fn(),
    sendAgentMessage: vi.fn(),
    listMessages: vi.fn(),
    sendMessage: vi.fn(),
    cancelGeneration: vi.fn(),
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

vi.mock("@tauri-apps/api/path", () => ({
  homeDir: vi.fn(() => Promise.resolve("/Users/tester")),
}));

function pressCmd(key: string) {
  fireEvent.keyDown(window, { key, metaKey: true });
}

async function waitForReady() {
  await waitFor(() => expect(screen.getByTestId("conversation-list")).toBeInTheDocument());
  // EmptyState resolves Home asynchronously — wait for it so a stray
  // post-test homeDir() resolution doesn't bleed into the next test.
  await screen.findByTestId("folder-target-selector");
}

async function createWorkspaceConversationViaComposer(text: string) {
  await userEvent.type(await screen.findByTestId("empty-state-input"), text);
  await userEvent.click(screen.getByTestId("empty-state-submit"));
  return screen.findByTestId("agent-input");
}

describe("App keyboard shortcuts (005-keyboard-shortcuts, updated for 006-chat-empty-state)", () => {
  beforeEach(() => {
    vi.clearAllMocks();

    vi.mocked(commands.listModels).mockResolvedValue([
      { id: "m", hardwareTier: "tier1", isActive: true, installed: true },
    ]);
    vi.mocked(commands.listConversations).mockResolvedValue([]);
    vi.mocked(commands.openWorkspace).mockResolvedValue({
      id: "ws-home",
      path: "/Users/tester",
      displayName: "tester",
      createdAt: 1,
      lastOpenedAt: 1,
    });
    vi.mocked(commands.createConversation).mockResolvedValue({
      id: "new-conv",
      workspaceId: "ws-home",
      title: "New conversation",
      createdAt: 1,
      updatedAt: 1,
      status: "done",
    });
    vi.mocked(commands.sendAgentMessage).mockResolvedValue("On it.");
    vi.mocked(commands.listMessages).mockResolvedValue([]);
    vi.mocked(commands.listMcpServers).mockResolvedValue([]);
    vi.mocked(commands.listSkills).mockResolvedValue([]);
    vi.mocked(events.onAssistantToken).mockResolvedValue(() => {});
    vi.mocked(events.onAssistantMessageComplete).mockResolvedValue(() => {});
    vi.mocked(events.onAssistantMessageError).mockResolvedValue(() => {});
    vi.mocked(events.onGenerationQueueUpdate).mockResolvedValue(() => {});
  });

  it("Cmd+L focuses the composer input when no conversation is selected (US1, updated for 006)", async () => {
    render(<App />);
    await waitForReady();
    const emptyStateInput = screen.getByTestId("empty-state-input");

    document.body.focus();
    expect(document.activeElement).not.toBe(emptyStateInput);
    pressCmd("l");
    expect(document.activeElement).toBe(emptyStateInput);

    // Already focused: pressing again must not disturb it.
    pressCmd("l");
    expect(document.activeElement).toBe(emptyStateInput);
  });

  it("Cmd+L focuses the plain chat input for a pre-existing, non-workspace conversation (US1, FR-012 regression guard)", async () => {
    vi.mocked(commands.listConversations).mockResolvedValue([
      { id: "legacy-1", workspaceId: null, title: "Before 006", createdAt: 1, updatedAt: 1, status: "done" },
    ]);

    render(<App />);
    await waitForReady();

    await userEvent.click(await screen.findByText("Before 006"));
    const chatInput = await screen.findByTestId("chat-input");

    document.body.focus();
    expect(document.activeElement).not.toBe(chatInput);
    pressCmd("l");
    expect(document.activeElement).toBe(chatInput);
  });

  it("Cmd+L focuses the agent task input for a workspace-scoped conversation, not the chat input (US1)", async () => {
    render(<App />);
    await waitForReady();

    const agentInput = await createWorkspaceConversationViaComposer("fix the bug");

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
    expect(screen.queryByTestId("empty-state-input")).not.toBeInTheDocument();
  });

  it("typing a plain 'l' (no Cmd) does not trigger the shortcut", async () => {
    render(<App />);
    await waitForReady();
    const emptyStateInput = screen.getByTestId("empty-state-input");
    document.body.focus();

    fireEvent.keyDown(window, { key: "l", metaKey: false });
    expect(document.activeElement).not.toBe(emptyStateInput);
  });

  it("Cmd+N shows the empty-state composer instead of instantly creating a conversation (US2, 006 FR-002)", async () => {
    render(<App />);
    await waitForReady();
    await createWorkspaceConversationViaComposer("first task");
    vi.mocked(commands.createConversation).mockClear();

    pressCmd("n");

    await screen.findByTestId("empty-state-input");
    expect(commands.createConversation).not.toHaveBeenCalled();
  });

  it("Cmd+N returns to the composer from Settings and from a workspace conversation (US2)", async () => {
    render(<App />);
    await waitForReady();

    await userEvent.click(await screen.findByTestId("open-settings"));
    await screen.findByTestId("settings-view");

    pressCmd("n");

    await screen.findByTestId("empty-state-input");
    expect(screen.queryByTestId("settings-view")).not.toBeInTheDocument();
  });

  it("Cmd+K opens the shortcuts dialog listing all shortcuts, and pressing it again closes it (US3, FR-006)", async () => {
    render(<App />);
    await waitForReady();

    pressCmd("k");
    const dialog = await screen.findByTestId("shortcuts-dialog");
    expect(screen.getAllByTestId("shortcut-item")).toHaveLength(4);

    // Scoped to the dialog: the sidebar's own search button independently
    // shows a "⌘F" hover hint, so a page-wide query would match both.
    expect(within(dialog).getByText("Open conversation search")).toBeInTheDocument();
    expect(within(dialog).getByText("⌘F")).toBeInTheDocument();

    pressCmd("k");
    await waitFor(() => expect(screen.queryByTestId("shortcuts-dialog")).not.toBeInTheDocument());
  });

  it("Cmd+F opens the sidebar conversation search", async () => {
    render(<App />);
    await waitForReady();

    pressCmd("f");

    await screen.findByTestId("search-panel");
    expect(screen.getByTestId("search-input")).toBeInTheDocument();
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

    const agentInput = await createWorkspaceConversationViaComposer("first task");

    pressCmd("k");
    await screen.findByTestId("shortcuts-dialog");

    document.body.focus();
    pressCmd("l");
    expect(document.activeElement).not.toBe(agentInput);

    pressCmd("n");
    expect(screen.queryByTestId("empty-state-input")).not.toBeInTheDocument();

    // Once dismissed, the shortcuts work again.
    pressCmd("k");
    await waitFor(() => expect(screen.queryByTestId("shortcuts-dialog")).not.toBeInTheDocument());
    pressCmd("n");
    await screen.findByTestId("empty-state-input");
  });
});
