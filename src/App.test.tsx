import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor, fireEvent, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { homeDir } from "@tauri-apps/api/path";
import App, { checkReadyWithRetries } from "./App";
import { commands, events } from "@/lib/ipc";
import { useContextUsageStore } from "@/state/contextUsageStore";

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
    searchConversations: vi.fn(),
    markConversationSeen: vi.fn(),
    archiveConversation: vi.fn(),
    listWorkspaces: vi.fn(),
    createConversation: vi.fn(),
    openWorkspace: vi.fn(),
    sendAgentMessage: vi.fn(),
    listMessages: vi.fn(),
    listMcpServers: vi.fn(),
    listSkills: vi.fn(),
    getContextUsage: vi.fn(),
    compactConversation: vi.fn(),
    isGenerationActive: vi.fn(),
    getActivePlan: vi.fn(),
  },
  events: {
    onContextUsageUpdate: vi.fn(),
    onAgentMessagePersisted: vi.fn(),
    onPlanUpdate: vi.fn(),
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
    useContextUsageStore.setState({ usage: {} });

    vi.mocked(commands.listModels).mockResolvedValue([
      { id: "m", hardwareTier: "tier1", isActive: true, installed: true },
    ]);
    vi.mocked(commands.listConversations).mockResolvedValue([]);
    vi.mocked(commands.searchConversations).mockResolvedValue([]);
    vi.mocked(commands.markConversationSeen).mockResolvedValue();
    vi.mocked(commands.archiveConversation).mockResolvedValue();
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
      lastSeenAt: 1,
      status: "done",
    });
    vi.mocked(commands.sendAgentMessage).mockResolvedValue("On it.");
    vi.mocked(commands.listMessages).mockResolvedValue([]);
    vi.mocked(commands.isGenerationActive).mockResolvedValue(false);
    vi.mocked(commands.listMcpServers).mockResolvedValue([]);
    vi.mocked(commands.listSkills).mockResolvedValue([]);
    // No model loaded in these unit tests — ContextUsageGauge's
    // getContextUsage call is expected to fail and swallow the error,
    // leaving the indicator simply unrendered.
    vi.mocked(commands.getContextUsage).mockRejectedValue(new Error("No model loaded"));
    vi.mocked(events.onContextUsageUpdate).mockResolvedValue(() => {});
    vi.mocked(events.onAgentMessagePersisted).mockResolvedValue(() => {});
    vi.mocked(commands.getActivePlan).mockResolvedValue(null);
    vi.mocked(events.onPlanUpdate).mockResolvedValue(() => {});
    vi.mocked(homeDir).mockResolvedValue("/Users/tester");
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

  it("renders shared sidebar and main topbars while the empty state keeps the main topbar blank", async () => {
    render(<App />);
    await waitForReady();

    const sidebarTopbar = screen.getByTestId("topbar-sidebar");
    const mainTopbar = screen.getByTestId("topbar-main");

    expect(sidebarTopbar).toHaveClass("h-10", "shrink-0");
    expect(mainTopbar).toHaveClass("h-10", "shrink-0", "bg-transparent");
    expect(mainTopbar).not.toHaveClass("bg-sidebar");
    expect(mainTopbar).not.toHaveClass("border-b", "shadow-sm");
    expect(sidebarTopbar).toHaveAttribute("data-tauri-drag-region");
    expect(mainTopbar).toHaveAttribute("data-tauri-drag-region");
    expect(mainTopbar).toBeEmptyDOMElement();
    expect(screen.getByTestId("empty-state-input")).toBeInTheDocument();
  });

  it("renders active conversation metadata in the main topbar and keeps context usage out of the composer", async () => {
    const conversation = {
      id: "conv-topbar",
      workspaceId: "ws-doce",
      title: "Shared topbar polish",
      createdAt: 1,
      updatedAt: 1,
      lastSeenAt: 1,
      status: "done" as const,
    };
    vi.mocked(homeDir).mockResolvedValue("/Users/gimenes");
    vi.mocked(commands.listConversations).mockResolvedValue([conversation]);
    vi.mocked(commands.listWorkspaces).mockResolvedValue([
      {
        id: "ws-doce",
        path: "/Users/gimenes/code/doce",
        displayName: "doce",
        createdAt: 1,
        lastOpenedAt: 1,
      },
    ]);
    vi.mocked(commands.getContextUsage).mockResolvedValue({
      conversationId: "conv-topbar",
      tokensUsed: 512,
      tokenBudget: 2048,
      state: "normal",
    });

    render(<App />);
    await waitForReady();
    await userEvent.click(await screen.findByText("Shared topbar polish"));

    const mainTopbar = screen.getByTestId("topbar-main");
    expect(mainTopbar).toHaveClass("bg-transparent", "text-foreground");
    expect(mainTopbar).not.toHaveClass("bg-sidebar", "border-b", "border-sidebar-border", "shadow-sm");
    await within(mainTopbar).findByTestId("workspace-topbar");

    expect(within(mainTopbar).getByTestId("workspace-topbar-title")).toHaveTextContent(
      "Shared topbar polish",
    );
    await waitFor(() =>
      expect(within(mainTopbar).getByTestId("workspace-topbar-path")).toHaveTextContent(
        "~/code/doce",
      ),
    );
    expect(await within(mainTopbar).findByTestId("context-usage-gauge")).toHaveAttribute(
      "aria-label",
      expect.stringContaining("25%"),
    );
    expect(
      within(screen.getByTestId("workspace-composer-shell")).queryByTestId("context-usage-gauge"),
    ).not.toBeInTheDocument();
  });

  it("keeps active conversation metadata in the main topbar synced with refreshed sidebar rows", async () => {
    const initialConversation = {
      id: "conv-refresh-title",
      workspaceId: null,
      title: "New conversation",
      createdAt: 1,
      updatedAt: 1,
      lastSeenAt: 1,
      status: "in_progress" as const,
    };
    const refreshedConversation = {
      ...initialConversation,
      title: "Generated implementation plan",
      updatedAt: 3,
      status: "done" as const,
    };
    vi.mocked(commands.listConversations)
      .mockResolvedValueOnce([initialConversation])
      .mockResolvedValue([refreshedConversation]);

    render(<App />);
    await waitForReady();
    await userEvent.click(await screen.findByText("New conversation"));

    const mainTopbar = screen.getByTestId("topbar-main");
    expect(await within(mainTopbar).findByTestId("workspace-topbar-title")).toHaveTextContent(
      "New conversation",
    );

    await waitFor(() => expect(commands.listConversations).toHaveBeenCalledTimes(2), {
      timeout: 3000,
    });
    await waitFor(() =>
      expect(within(mainTopbar).getByTestId("workspace-topbar-title")).toHaveTextContent(
        "Generated implementation plan",
      ),
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
        lastSeenAt: 1,
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

  it("marks a conversation seen when the user selects it from the sidebar", async () => {
    const conversation = {
      id: "c1",
      workspaceId: null,
      title: "Unread thread",
      createdAt: 1,
      updatedAt: 10,
      lastSeenAt: 5,
      status: "done" as const,
    };
    vi.mocked(commands.listConversations).mockResolvedValue([conversation]);

    render(<App />);

    await userEvent.click(await screen.findByText("Unread thread"));

    expect(commands.markConversationSeen).toHaveBeenCalledWith("c1");
  });

  it("returns to the empty state when the active conversation is archived", async () => {
    const conversation = {
      id: "c1",
      workspaceId: null,
      title: "Thread to archive",
      createdAt: 1,
      updatedAt: 1,
      lastSeenAt: 1,
      status: "done" as const,
    };
    vi.mocked(commands.listConversations).mockResolvedValue([conversation]);

    render(<App />);

    await userEvent.click(await screen.findByText("Thread to archive"));
    expect(await screen.findByTestId("agent-input")).toBeInTheDocument();

    await userEvent.click(screen.getByLabelText("Archive Thread to archive"));

    expect(commands.archiveConversation).toHaveBeenCalledWith("c1");
    expect(await screen.findByTestId("empty-state-input")).toBeInTheDocument();
    expect(screen.queryByTestId("agent-input")).not.toBeInTheDocument();
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

    const emptyStateInput = await screen.findByTestId("empty-state-input");
    await waitFor(() => expect(document.activeElement).toBe(emptyStateInput));
    expect(commands.createConversation).not.toHaveBeenCalled();
  });

  it("Cmd+N returns to the composer from Settings and from a workspace conversation (US2)", async () => {
    render(<App />);
    await waitForReady();

    await userEvent.click(await screen.findByTestId("open-settings"));
    await screen.findByTestId("settings-view");

    pressCmd("n");

    const emptyStateInput = await screen.findByTestId("empty-state-input");
    await waitFor(() => expect(document.activeElement).toBe(emptyStateInput));
    expect(screen.queryByTestId("settings-view")).not.toBeInTheDocument();
  });

  it("clears the hidden command-center latch when Settings takes over so Cmd+F works after close", async () => {
    render(<App />);
    await waitForReady();

    pressCmd("k");

    await userEvent.click(await screen.findByTestId("open-settings"));
    expect(await screen.findByTestId("settings-view")).toBeInTheDocument();

    await userEvent.click(screen.getByTestId("close-settings"));
    await waitFor(() => expect(screen.queryByTestId("settings-view")).not.toBeInTheDocument());

    pressCmd("f");
    expect(await screen.findByTestId("search-panel")).toBeInTheDocument();
  });

  it("lets Cmd+F reopen search after Cmd+K lands behind an already-open Settings view", async () => {
    render(<App />);
    await waitForReady();

    await userEvent.click(await screen.findByTestId("open-settings"));
    expect(await screen.findByTestId("settings-view")).toBeInTheDocument();

    pressCmd("k");

    await userEvent.click(screen.getByTestId("close-settings"));
    await waitFor(() => expect(screen.queryByTestId("settings-view")).not.toBeInTheDocument());

    pressCmd("f");
    expect(await screen.findByTestId("search-panel")).toBeInTheDocument();
  });

  it("lets Cmd+F reopen search after Cmd+K lands behind an already-open search dialog", async () => {
    render(<App />);
    await waitForReady();

    pressCmd("f");
    expect(await screen.findByTestId("search-panel")).toBeInTheDocument();

    pressCmd("k");

    await userEvent.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByTestId("search-panel")).not.toBeInTheDocument());

    pressCmd("f");
    expect(await screen.findByTestId("search-panel")).toBeInTheDocument();
  });

  it("opens command center with Cmd+K and keeps Cmd+F for conversation search", async () => {
    render(<App />);
    await waitForReady();

    pressCmd("k");
    expect(await screen.findByTestId("command-center")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /New Agent/ })).toBeInTheDocument();

    pressCmd("f");
    expect(screen.queryByTestId("search-panel")).not.toBeInTheDocument();

    await userEvent.keyboard("{Escape}");
    pressCmd("f");
    expect(await screen.findByTestId("search-panel")).toBeInTheDocument();
  });

  it("switches from Widget Gallery to Settings when the command center opens Settings", async () => {
    render(<App />);
    await waitForReady();

    pressCmd("d");
    expect(await screen.findByTestId("widget-gallery")).toBeInTheDocument();

    pressCmd("k");
    await userEvent.click(screen.getByRole("button", { name: /Open Settings/i }));

    expect(await screen.findByTestId("settings-view")).toBeInTheDocument();
    expect(screen.queryByTestId("widget-gallery")).not.toBeInTheDocument();
  });

  it("switches from Settings to Widget Gallery when the command center opens Widget Gallery", async () => {
    render(<App />);
    await waitForReady();

    await userEvent.click(await screen.findByTestId("open-settings"));
    expect(await screen.findByTestId("settings-view")).toBeInTheDocument();

    pressCmd("k");
    await userEvent.click(screen.getByRole("button", { name: /Open Widget Gallery/i }));

    expect(await screen.findByTestId("widget-gallery")).toBeInTheDocument();
    expect(screen.queryByTestId("settings-view")).not.toBeInTheDocument();
  });

  it("hands off from the shortcuts dialog to Cmd+K and lets Cmd+F open search after dismiss", async () => {
    render(<App />);
    await waitForReady();

    await userEvent.click(screen.getByTestId("open-shortcuts-dialog"));
    expect(await screen.findByTestId("shortcuts-dialog")).toBeInTheDocument();

    pressCmd("f");
    expect(screen.queryByTestId("search-panel")).not.toBeInTheDocument();

    pressCmd("k");
    await waitFor(() => expect(screen.queryByTestId("shortcuts-dialog")).not.toBeInTheDocument());

    await userEvent.keyboard("{Escape}");
    pressCmd("f");
    expect(await screen.findByTestId("search-panel")).toBeInTheDocument();
  });

  it("keeps Cmd+K routed to an open command-center state until Task 4 adds its close path", async () => {
    render(<App />);
    await waitForReady();

    pressCmd("k");
    pressCmd("k");
    pressCmd("f");

    expect(screen.queryByTestId("search-panel")).not.toBeInTheDocument();

    await userEvent.keyboard("{Escape}");
    pressCmd("f");
    expect(await screen.findByTestId("search-panel")).toBeInTheDocument();
  });

  it("Cmd+F opens conversation search in a dialog", async () => {
    render(<App />);
    await waitForReady();

    pressCmd("f");

    expect(await screen.findByTestId("conversation-search-dialog")).toBeInTheDocument();
    expect(screen.getByTestId("search-panel")).toBeInTheDocument();
  });

  it("opens a searched conversation even when the sidebar cache does not contain it", async () => {
    const searchedConversation = {
      id: "conv-from-backend",
      workspaceId: null,
      title: "Backend-only match",
      createdAt: 10,
      updatedAt: 20,
      lastSeenAt: 10,
      status: "done" as const,
    };
    vi.mocked(commands.listConversations)
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([searchedConversation]);
    vi.mocked(commands.searchConversations).mockResolvedValue([
      {
        conversationId: searchedConversation.id,
        title: searchedConversation.title,
        excerpt: "found via backend",
        rank: -1,
      },
    ]);

    render(<App />);
    await waitForReady();

    pressCmd("f");
    await userEvent.type(screen.getByTestId("search-input"), "backend");
    await userEvent.click(await screen.findByTestId("search-result"));

    expect(await screen.findByTestId("agent-input")).toBeInTheDocument();
    expect(screen.getByTestId("workspace-topbar-title")).toHaveTextContent("Backend-only match");
    await waitFor(() =>
      expect(screen.queryByTestId("conversation-search-dialog")).not.toBeInTheDocument(),
    );
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
