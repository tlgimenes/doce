import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor, fireEvent, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import App, { checkReadyWithRetries } from "./App";
import { commands, events } from "@/lib/ipc";

type TestDocument = Document & {
  startViewTransition?: (callback: () => void) => unknown;
};

const originalStartViewTransition = (document as TestDocument).startViewTransition;

// Covers 005-keyboard-shortcuts: the app's first global (not input-scoped)
// keyboard shortcuts, exercised against the real App component with every
// child view's IPC surface mocked (matching ConversationList.test.tsx,
// Workspace.test.tsx, and Settings.test.tsx's existing mock shapes).
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
    listWorkspaces: vi.fn(),
    createConversation: vi.fn(),
    openWorkspace: vi.fn(),
    sendAgentMessage: vi.fn(),
    listMessages: vi.fn(),
    listMcpServers: vi.fn(),
    listSkills: vi.fn(),
    getContextUsage: vi.fn(),
    compactConversation: vi.fn(),
  },
  events: {
    onContextUsageUpdate: vi.fn(),
    onAgentMessagePersisted: vi.fn(),
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
    vi.mocked(commands.listWorkspaces).mockResolvedValue([]);
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
    // No model loaded in these unit tests — ContextUsageGauge's
    // getContextUsage call is expected to fail and swallow the error,
    // leaving the indicator simply unrendered.
    vi.mocked(commands.getContextUsage).mockRejectedValue(new Error("No model loaded"));
    vi.mocked(events.onContextUsageUpdate).mockResolvedValue(() => {});
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

  it("routes into Workspace immediately after conversation creation while the agent send is still pending", async () => {
    vi.mocked(commands.sendAgentMessage).mockReturnValue(new Promise(() => {}));
    vi.mocked(commands.listMessages).mockReturnValue(new Promise(() => {}));

    render(<App />);
    await waitForReady();

    await userEvent.type(screen.getByTestId("empty-state-input"), "first task");
    await userEvent.click(screen.getByTestId("empty-state-submit"));

    expect(await screen.findByTestId("agent-input")).toBeInTheDocument();
    expect(await screen.findByText("first task")).toBeInTheDocument();
    expect(await screen.findByTestId("agent-thinking")).toBeInTheDocument();
    expect(commands.sendAgentMessage).toHaveBeenCalledWith("new-conv", "first task", undefined);
  });

  it("starts a view transition when conversation creation is supported by the document", async () => {
    const startViewTransition = vi.fn((callback: () => void) => {
      callback();
      return {};
    });
    Object.defineProperty(document, "startViewTransition", {
      configurable: true,
      writable: true,
      value: startViewTransition,
    });

    render(<App />);
    await waitForReady();

    await userEvent.type(screen.getByTestId("empty-state-input"), "first task");
    await userEvent.click(screen.getByTestId("empty-state-submit"));

    await screen.findByTestId("agent-input");
    expect(startViewTransition).toHaveBeenCalledTimes(1);
  });

  it("marks the main content pane as the chat-surface view transition target", async () => {
    render(<App />);
    await waitForReady();

    expect(screen.getByTestId("app-content-pane")).toHaveClass(
      "[view-transition-name:chat-surface]",
    );
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

  it("renders Workspace and focuses the agent input for any selected conversation", async () => {
    vi.mocked(commands.listConversations).mockResolvedValue([
      {
        id: "legacy-1",
        workspaceId: null,
        title: "Before 006",
        createdAt: 1,
        updatedAt: 1,
        status: "done",
      },
    ]);

    render(<App />);
    await waitForReady();

    await userEvent.click(await screen.findByText("Before 006"));
    const agentInput = await screen.findByTestId("agent-input");

    document.body.focus();
    expect(document.activeElement).not.toBe(agentInput);
    pressCmd("l");
    expect(document.activeElement).toBe(agentInput);
  });

  it("Cmd+L focuses the agent task input after creating a conversation", async () => {
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
    // shows a "⌘ + F" hover hint, so a page-wide query would match both.
    expect(within(dialog).getByText("Open conversation search")).toBeInTheDocument();
    expect(within(dialog).getByTestId("shortcut-combo-search-conversations")).toHaveTextContent(
      "⌘+F",
    );

    pressCmd("k");
    await waitFor(() => expect(screen.queryByTestId("shortcuts-dialog")).not.toBeInTheDocument());
  });

  it("Cmd+F opens conversation search in a dialog", async () => {
    render(<App />);
    await waitForReady();

    pressCmd("f");

    const searchPanel = await screen.findByTestId("search-panel");
    const dialog = searchPanel.closest("dialog");
    expect(dialog).toBeInTheDocument();
    expect(dialog).toHaveAttribute("open");
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

// Regression coverage for the App.tsx robustness fix: `ready` used to stay
// `null` forever (rendering nothing) if listModels() never settled — the
// exact shape of a still-unresolved CI-only failure (see tasks.md's T095
// note). checkReadyWithRetries() now bounds that wait and always resolves
// to a real boolean.
describe("App's initial readiness check survives a stuck listModels() call", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("resolves true immediately when listModels() succeeds with an installed model", async () => {
    vi.mocked(commands.listModels).mockResolvedValue([
      { id: "m", hardwareTier: "tier1", isActive: true, installed: true },
    ]);
    const resultPromise = checkReadyWithRetries();
    await vi.advanceTimersByTimeAsync(0);
    await expect(resultPromise).resolves.toBe(true);
    expect(commands.listModels).toHaveBeenCalledTimes(1);
  });

  it("resolves false, without hanging forever, when listModels() never settles across every retry", async () => {
    vi.mocked(commands.listModels).mockReturnValue(new Promise(() => {}));
    const resultPromise = checkReadyWithRetries();
    // 3 attempts * 8s timeout each = 24s worst case.
    await vi.advanceTimersByTimeAsync(24_000);
    await expect(resultPromise).resolves.toBe(false);
    expect(commands.listModels).toHaveBeenCalledTimes(3);
  });

  it("resolves true if a later retry succeeds after earlier ones hang", async () => {
    vi.mocked(commands.listModels)
      .mockReturnValueOnce(new Promise(() => {}))
      .mockResolvedValueOnce([{ id: "m", hardwareTier: "tier1", isActive: true, installed: true }]);
    const resultPromise = checkReadyWithRetries();
    await vi.advanceTimersByTimeAsync(8_000);
    await expect(resultPromise).resolves.toBe(true);
    expect(commands.listModels).toHaveBeenCalledTimes(2);
  });
});
