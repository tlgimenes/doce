import { createRef } from "react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { homeDir } from "@tauri-apps/api/path";
import ConversationList, { type ConversationListHandle } from "./ConversationList";
import { commands } from "@/lib/ipc";

vi.mock("@tauri-apps/api/path", () => ({
  homeDir: vi.fn(() => Promise.resolve("/Users/tester")),
}));

vi.mock("@/lib/ipc", () => ({
  commands: {
    listConversations: vi.fn(),
    listWorkspaces: vi.fn(),
  },
}));

describe("ConversationList", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(homeDir).mockResolvedValue("/Users/tester");
    vi.mocked(commands.listWorkspaces).mockResolvedValue([]);
  });

  it("renders titles and a status dot per conversation (FR-011/FR-012)", async () => {
    vi.mocked(commands.listConversations).mockResolvedValue([
      {
        id: "a",
        workspaceId: null,
        title: "First one",
        createdAt: 1,
        updatedAt: 3,
        lastSeenAt: 3,
        status: "done",
      },
      {
        id: "b",
        workspaceId: null,
        title: "Needs input",
        createdAt: 2,
        updatedAt: 2,
        lastSeenAt: 2,
        status: "requires_action",
      },
      {
        id: "c",
        workspaceId: null,
        title: "Broke",
        createdAt: 3,
        updatedAt: 1,
        lastSeenAt: 1,
        status: "failed",
      },
    ]);

    render(
      <ConversationList
        activeId={null}
        onSelect={vi.fn()}
        onNewConversation={vi.fn()}
        onOpenSettings={vi.fn()}
      />,
    );

    await waitFor(() => {
      expect(screen.getByText("First one")).toBeInTheDocument();
    });
    expect(screen.getByText("Needs input")).toBeInTheDocument();
    expect(screen.getByText("Broke")).toBeInTheDocument();

    const dots = screen.getAllByTestId("conversation-status-dot");
    expect(dots.map((d) => d.dataset.status)).toEqual(["done", "requires_action", "failed"]);
  });

  it("clicking a conversation calls onSelect with the full conversation, not just its id", async () => {
    const conversation = {
      id: "a",
      workspaceId: "ws-1",
      title: "First one",
      createdAt: 1,
      updatedAt: 3,
      lastSeenAt: 3,
      status: "done" as const,
    };
    vi.mocked(commands.listConversations).mockResolvedValue([conversation]);
    const onSelect = vi.fn();

    render(
      <ConversationList
        activeId={null}
        onSelect={onSelect}
        onNewConversation={vi.fn()}
        onOpenSettings={vi.fn()}
      />,
    );

    await userEvent.click(await screen.findByText("First one"));
    expect(onSelect).toHaveBeenCalledWith(conversation);
  });

  it("renders each conversation as title/time plus path/work-state rows", async () => {
    const now = new Date("2026-01-01T12:00:00Z").getTime();
    const dateNow = vi.spyOn(Date, "now").mockReturnValue(now);

    try {
      const updatedAt = now - 2 * 60_000;
      const conversation = {
        id: "active",
        workspaceId: "ws-code",
        title: "Fix fuzzy search ranking",
        createdAt: updatedAt - 60_000,
        updatedAt,
        lastSeenAt: updatedAt,
        status: "in_progress" as const,
      };

      vi.mocked(commands.listConversations).mockResolvedValue([conversation]);
      vi.mocked(commands.listWorkspaces).mockResolvedValue([
        {
          id: "ws-code",
          path: "/Users/tester/code/doce",
          displayName: "doce",
          createdAt: 1,
          lastOpenedAt: 2,
        },
      ]);

      render(
        <ConversationList
          activeId="active"
          onSelect={vi.fn()}
          onNewConversation={vi.fn()}
          onOpenSettings={vi.fn()}
        />,
      );

      const row = await screen.findByTestId("conversation-item");

      await waitFor(() => {
        expect(row).toHaveTextContent("Fix fuzzy search ranking");
        expect(row).toHaveTextContent("2m");
        expect(row).toHaveTextContent("~/code/doce");
        expect(row).toHaveTextContent("Working");
      });
    } finally {
      dateNow.mockRestore();
    }
  });

  it("keeps the sidebar mounted when workspace polling fails", async () => {
    const error = new Error("workspace unavailable");
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => undefined);
    vi.mocked(commands.listConversations).mockResolvedValue([]);
    vi.mocked(commands.listWorkspaces).mockRejectedValue(error);

    try {
      render(
        <ConversationList
          activeId={null}
          onSelect={vi.fn()}
          onNewConversation={vi.fn()}
          onOpenSettings={vi.fn()}
        />,
      );

      await waitFor(() => expect(consoleError).toHaveBeenCalledWith(error));
      expect(screen.getByTestId("conversation-list")).toBeInTheDocument();
    } finally {
      consoleError.mockRestore();
    }
  });

  it("006-chat-empty-state: '+ New conversation' calls onNewConversation instead of creating a conversation immediately (FR-002)", async () => {
    vi.mocked(commands.listConversations).mockResolvedValue([]);
    const onNewConversation = vi.fn();

    render(
      <ConversationList
        activeId={null}
        onSelect={vi.fn()}
        onNewConversation={onNewConversation}
        onOpenSettings={vi.fn()}
      />,
    );

    await userEvent.click(await screen.findByTestId("new-conversation"));

    expect(onNewConversation).toHaveBeenCalledTimes(1);
  });

  it("exposes createNew via a ref, calling the same path as clicking the button (005-keyboard-shortcuts, Cmd+N)", async () => {
    vi.mocked(commands.listConversations).mockResolvedValue([]);
    const onNewConversation = vi.fn();
    const ref = createRef<ConversationListHandle>();

    render(
      <ConversationList
        ref={ref}
        activeId={null}
        onSelect={vi.fn()}
        onNewConversation={onNewConversation}
        onOpenSettings={vi.fn()}
      />,
    );

    await waitFor(() => expect(ref.current).not.toBeNull());
    ref.current!.createNew();

    expect(onNewConversation).toHaveBeenCalledTimes(1);
  });

  it("keeps the window drag affordance and actions in place while search opens in a dialog", async () => {
    vi.mocked(commands.listConversations).mockResolvedValue([]);

    render(
      <ConversationList
        activeId={null}
        onSelect={vi.fn()}
        onNewConversation={vi.fn()}
        onOpenSettings={vi.fn()}
      />,
    );

    const sidebar = await screen.findByTestId("conversation-list");
    const affordance = screen.getByTestId("sidebar-window-affordance");
    expect(sidebar.firstElementChild).toBe(affordance);
    expect(affordance).toHaveClass("h-10", "shrink-0");
    expect(screen.getByTestId("sidebar-actions")).not.toHaveClass("mt-8");

    await userEvent.click(screen.getByTestId("open-search"));

    const searchPanel = screen.getByTestId("search-panel");
    expect(searchPanel.closest("dialog")).toBeInTheDocument();
    expect(sidebar.firstElementChild).toBe(affordance);
    expect(screen.getByTestId("sidebar-actions")).toBeInTheDocument();
  });

  it("shows recent conversations in search and selects them through the full conversation path", async () => {
    const oldConversation = {
      id: "old",
      workspaceId: null,
      title: "Older task",
      createdAt: 1,
      updatedAt: 1,
      lastSeenAt: 1,
      status: "done" as const,
    };
    const recentConversation = {
      id: "recent",
      workspaceId: null,
      title: "Recent task",
      createdAt: 2,
      updatedAt: 3,
      lastSeenAt: 3,
      status: "done" as const,
    };
    vi.mocked(commands.listConversations).mockResolvedValue([oldConversation, recentConversation]);
    const onSelect = vi.fn();

    render(
      <ConversationList
        activeId={null}
        onSelect={onSelect}
        onNewConversation={vi.fn()}
        onOpenSettings={vi.fn()}
      />,
    );

    await userEvent.click(await screen.findByTestId("open-search"));

    const rows = screen.getAllByTestId("search-result");
    expect(rows[0]).toHaveTextContent("Recent task");

    await userEvent.click(rows[0]);
    expect(onSelect).toHaveBeenCalledWith(recentConversation);
  });
});
