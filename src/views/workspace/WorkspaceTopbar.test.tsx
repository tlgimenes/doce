import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import { homeDir } from "@tauri-apps/api/path";
import { TopbarHost, TopbarProvider } from "@/components/Topbar";
import { commands, type Conversation } from "@/lib/ipc";
import { useContextUsageStore } from "@/state/contextUsageStore";
import WorkspaceTopbar from "./WorkspaceTopbar";

vi.mock("@/lib/ipc", () => ({
  commands: {
    listWorkspaces: vi.fn(),
    getContextUsage: vi.fn(),
  },
}));

vi.mock("@tauri-apps/api/path", () => ({
  homeDir: vi.fn(() => Promise.resolve("/Users/tester")),
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    startDragging: vi.fn(),
  }),
}));

function conversationFixture(overrides: Partial<Conversation> = {}): Conversation {
  return {
    id: "conv-1",
    workspaceId: "ws-code",
    title: "Plan the workspace topbar",
    createdAt: 1,
    updatedAt: 1,
    lastSeenAt: 1,
    status: "done",
    ...overrides,
  };
}

function renderTopbar(conversation: Conversation) {
  render(
    <TopbarProvider>
      <TopbarHost target="main" />
      <WorkspaceTopbar conversation={conversation} />
    </TopbarProvider>,
  );
}

describe("WorkspaceTopbar", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useContextUsageStore.setState({ usage: {} });
    vi.mocked(homeDir).mockResolvedValue("/Users/tester");
    vi.mocked(commands.listWorkspaces).mockResolvedValue([
      {
        id: "ws-code",
        path: "/Users/tester/code/doce",
        displayName: "doce",
        createdAt: 1,
        lastOpenedAt: 1,
      },
    ]);
    vi.mocked(commands.getContextUsage).mockResolvedValue({
      conversationId: "conv-1",
      tokensUsed: 512,
      tokenBudget: 2048,
      state: "normal",
    });
  });

  it("portals the conversation title and compact workspace path into the main topbar", async () => {
    renderTopbar(conversationFixture());

    const host = screen.getByTestId("topbar-main");
    const topbar = await screen.findByTestId("workspace-topbar");

    expect(host).toContainElement(topbar);
    expect(screen.getByTestId("workspace-topbar-title")).toHaveTextContent(
      "Plan the workspace topbar",
    );
    await waitFor(() =>
      expect(screen.getByTestId("workspace-topbar-path")).toHaveTextContent("~/code/doce"),
    );
  });

  it("renders Home when the conversation has no workspace id", async () => {
    renderTopbar(conversationFixture({ workspaceId: null }));

    await waitFor(() =>
      expect(screen.getByTestId("workspace-topbar-path")).toHaveTextContent("Home"),
    );
  });

  it("falls back to Home when the conversation workspace is missing", async () => {
    vi.mocked(commands.listWorkspaces).mockResolvedValue([]);

    renderTopbar(conversationFixture({ workspaceId: "missing" }));

    await waitFor(() =>
      expect(screen.getByTestId("workspace-topbar-path")).toHaveTextContent("Home"),
    );
  });

  it("renders an absolute workspace path when the home directory cannot be resolved", async () => {
    vi.mocked(homeDir).mockRejectedValue(new Error("home unavailable"));

    renderTopbar(conversationFixture());

    await waitFor(() =>
      expect(screen.getByTestId("workspace-topbar-path")).toHaveTextContent(
        "/Users/tester/code/doce",
      ),
    );
  });

  it("renders the context usage gauge for the conversation in a non-drag control wrapper", async () => {
    renderTopbar(conversationFixture());

    const gauge = await screen.findByTestId("context-usage-gauge");
    expect(gauge).toHaveAttribute("aria-label", expect.stringContaining("25%"));
    expect(gauge.closest("[data-topbar-no-drag]")).not.toBeNull();
    expect(commands.getContextUsage).toHaveBeenCalledWith("conv-1");
  });
});
