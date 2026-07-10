import { createRef, useState } from "react";
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
    archiveConversation: vi.fn(),
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
        onOpenSearch={vi.fn()}
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
        onOpenSearch={vi.fn()}
        onOpenSettings={vi.fn()}
      />,
    );

    await userEvent.click(await screen.findByText("First one"));
    expect(onSelect).toHaveBeenCalledWith(conversation);
  });

  it("renders an inactive unseen conversation title with normal foreground color", async () => {
    vi.mocked(commands.listConversations).mockResolvedValue([
      {
        id: "unseen",
        workspaceId: null,
        title: "New output arrived",
        createdAt: 1,
        updatedAt: 10,
        lastSeenAt: 5,
        status: "done",
      },
    ]);

    render(
      <ConversationList
        activeId={null}
        onSelect={vi.fn()}
        onNewConversation={vi.fn()}
        onOpenSearch={vi.fn()}
        onOpenSettings={vi.fn()}
      />,
    );

    expect(await screen.findByText("New output arrived")).toHaveClass(
      "font-medium",
      "text-sidebar-foreground",
    );
  });

  it("renders the active conversation title with accent foreground color even when it has unseen updates", async () => {
    vi.mocked(commands.listConversations).mockResolvedValue([
      {
        id: "active",
        workspaceId: null,
        title: "Currently open",
        createdAt: 1,
        updatedAt: 10,
        lastSeenAt: 5,
        status: "done",
      },
    ]);

    render(
      <ConversationList
        activeId="active"
        onSelect={vi.fn()}
        onNewConversation={vi.fn()}
        onOpenSearch={vi.fn()}
        onOpenSettings={vi.fn()}
      />,
    );

    expect(await screen.findByText("Currently open")).toHaveClass(
      "font-medium",
      "text-sidebar-accent-foreground",
    );
  });

  it("mutes a viewed conversation title after switching to another conversation", async () => {
    const conversations = [
      {
        id: "first",
        workspaceId: null,
        title: "First unseen",
        createdAt: 1,
        updatedAt: 10,
        lastSeenAt: 5,
        status: "done" as const,
      },
      {
        id: "second",
        workspaceId: null,
        title: "Second chat",
        createdAt: 2,
        updatedAt: 6,
        lastSeenAt: 6,
        status: "done" as const,
      },
    ];
    vi.mocked(commands.listConversations).mockResolvedValue(conversations);

    function ControlledConversationList() {
      const [activeId, setActiveId] = useState<string | null>(null);
      return (
        <ConversationList
          activeId={activeId}
          onSelect={(conversation) => setActiveId(conversation.id)}
          onNewConversation={vi.fn()}
          onOpenSearch={vi.fn()}
          onOpenSettings={vi.fn()}
        />
      );
    }

    render(<ControlledConversationList />);

    const first = await screen.findByText("First unseen");
    expect(first).toHaveClass("font-medium", "text-sidebar-foreground");

    await userEvent.click(first);
    expect(first).toHaveClass("font-medium", "text-sidebar-accent-foreground");

    await userEvent.click(screen.getByText("Second chat"));
    expect(first).toHaveClass("font-medium", "text-sidebar-foreground/55");
  });

  it("keeps the selected conversation highlighted with the sidebar accent styles", async () => {
    vi.mocked(commands.listConversations).mockResolvedValue([
      {
        id: "selected",
        workspaceId: null,
        title: "Selected thread",
        createdAt: 1,
        updatedAt: 1,
        lastSeenAt: 1,
        status: "done",
      },
    ]);

    render(
      <ConversationList
        activeId="selected"
        onSelect={vi.fn()}
        onNewConversation={vi.fn()}
        onOpenSearch={vi.fn()}
        onOpenSettings={vi.fn()}
      />,
    );

    const row = await screen.findByTestId("conversation-item");
    expect(row).toHaveClass("bg-sidebar-accent", "text-sidebar-accent-foreground");
    expect(row).not.toHaveClass("bg-transparent");
  });

  it("uses accent text styling for active-row title, workspace, timestamp, and work-state", async () => {
    const now = new Date("2026-01-01T12:00:00Z").getTime();
    const dateNow = vi.spyOn(Date, "now").mockReturnValue(now);

    try {
      const updatedAt = now - 2 * 60_000;
      vi.mocked(commands.listConversations).mockResolvedValue([
        {
          id: "selected",
          workspaceId: "ws-code",
          title: "Selected thread",
          createdAt: updatedAt - 60_000,
          updatedAt,
          lastSeenAt: updatedAt,
          status: "in_progress",
        },
      ]);
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
          activeId="selected"
          onSelect={vi.fn()}
          onNewConversation={vi.fn()}
          onOpenSearch={vi.fn()}
          onOpenSettings={vi.fn()}
        />,
      );

      expect(await screen.findByText("Selected thread")).toHaveClass(
        "text-sidebar-accent-foreground",
      );
      expect(screen.getByText("~/code/doce")).toHaveClass("text-sidebar-accent-foreground/70");
      expect(screen.getByText("2m")).toHaveClass("text-sidebar-accent-foreground/80");
      expect(screen.getByText("Working")).toHaveClass("text-sidebar-accent-foreground/70");
    } finally {
      dateNow.mockRestore();
    }
  });

  it("archives a conversation from the hover trash button without selecting it", async () => {
    const conversation = {
      id: "archive-me",
      workspaceId: null,
      title: "Archive me",
      createdAt: 1,
      updatedAt: 3,
      lastSeenAt: 3,
      status: "done" as const,
    };
    vi.mocked(commands.listConversations).mockResolvedValue([conversation]);
    vi.mocked(commands.archiveConversation).mockResolvedValue();
    const onSelect = vi.fn();

    render(
      <ConversationList
        activeId={null}
        onSelect={onSelect}
        onNewConversation={vi.fn()}
        onOpenSearch={vi.fn()}
        onOpenSettings={vi.fn()}
      />,
    );

    const row = await screen.findByTestId("conversation-item");
    await userEvent.hover(row);
    const archiveButton = screen.getByLabelText("Archive Archive me");
    expect(archiveButton).toHaveClass("bg-transparent", "size-6");
    expect(archiveButton.querySelector("svg")).toBeInTheDocument();

    await userEvent.click(archiveButton);

    expect(commands.archiveConversation).toHaveBeenCalledWith("archive-me");
    expect(onSelect).not.toHaveBeenCalled();
    expect(screen.queryByText("Archive me")).not.toBeInTheDocument();
  });

  it("archives a conversation through the imperative archiveById handle", async () => {
    const conversation = {
      id: "archive-from-handle",
      workspaceId: null,
      title: "Archive from handle",
      createdAt: 1,
      updatedAt: 3,
      lastSeenAt: 3,
      status: "done" as const,
    };
    vi.mocked(commands.listConversations).mockResolvedValue([conversation]);
    vi.mocked(commands.archiveConversation).mockResolvedValue();
    const ref = createRef<ConversationListHandle>();

    render(
      <ConversationList
        ref={ref}
        activeId={null}
        onSelect={vi.fn()}
        onNewConversation={vi.fn()}
        onOpenSearch={vi.fn()}
        onOpenSettings={vi.fn()}
      />,
    );

    await screen.findByText("Archive from handle");
    ref.current?.archiveById("archive-from-handle");

    expect(commands.archiveConversation).toHaveBeenCalledWith("archive-from-handle");
    await waitFor(() =>
      expect(screen.queryByText("Archive from handle")).not.toBeInTheDocument(),
    );
  });

  it("reveals the archive trash button only on row hover, not from selected row focus", async () => {
    vi.mocked(commands.listConversations).mockResolvedValue([
      {
        id: "selected",
        workspaceId: null,
        title: "Selected thread",
        createdAt: 1,
        updatedAt: 1,
        lastSeenAt: 1,
        status: "done",
      },
    ]);

    render(
      <ConversationList
        activeId="selected"
        onSelect={vi.fn()}
        onNewConversation={vi.fn()}
        onOpenSearch={vi.fn()}
        onOpenSettings={vi.fn()}
      />,
    );

    const archiveButton = await screen.findByLabelText("Archive Selected thread");
    expect(archiveButton).toHaveClass("opacity-0", "group-hover:opacity-100");
    expect(archiveButton).not.toHaveClass("group-focus-within:opacity-100");
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
          onOpenSearch={vi.fn()}
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
          onOpenSearch={vi.fn()}
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
        onOpenSearch={vi.fn()}
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
        onOpenSearch={vi.fn()}
        onOpenSettings={vi.fn()}
      />,
    );

    await waitFor(() => expect(ref.current).not.toBeNull());
    ref.current!.createNew();

    expect(onNewConversation).toHaveBeenCalledTimes(1);
  });

  it("calls the parent search handler from the sidebar Search action", async () => {
    const onOpenSearch = vi.fn();
    render(
      <ConversationList
        activeId={null}
        onSelect={vi.fn()}
        onNewConversation={vi.fn()}
        onOpenSearch={onOpenSearch}
        onOpenSettings={vi.fn()}
      />,
    );

    await userEvent.click(await screen.findByTestId("open-search"));

    expect(onOpenSearch).toHaveBeenCalledTimes(1);
  });

  it("renders sidebar actions at the top of the sidebar body and routes Search through onOpenSearch", async () => {
    vi.mocked(commands.listConversations).mockResolvedValue([]);
    const onOpenSearch = vi.fn();

    render(
      <ConversationList
        activeId={null}
        onSelect={vi.fn()}
        onNewConversation={vi.fn()}
        onOpenSearch={onOpenSearch}
        onOpenSettings={vi.fn()}
      />,
    );

    const sidebar = await screen.findByTestId("conversation-list");
    const actions = screen.getByTestId("sidebar-actions");
    expect(sidebar.firstElementChild).toBe(actions);
    expect(actions).not.toHaveClass("mt-8");

    await userEvent.click(screen.getByTestId("open-search"));

    expect(onOpenSearch).toHaveBeenCalledTimes(1);
    expect(sidebar.firstElementChild).toBe(actions);
    expect(actions).toBeInTheDocument();
  });

  it("exposes conversation accessors via the imperative handle for app-owned search", async () => {
    const conversation = {
      id: "c-search",
      workspaceId: null,
      title: "Search target",
      createdAt: 1,
      updatedAt: 2,
      lastSeenAt: 1,
      status: "done" as const,
    };
    vi.mocked(commands.listConversations).mockResolvedValue([conversation]);
    const ref = createRef<ConversationListHandle>();
    const onSelect = vi.fn();

    render(
      <ConversationList
        ref={ref}
        activeId={null}
        onSelect={onSelect}
        onNewConversation={vi.fn()}
        onOpenSearch={vi.fn()}
        onOpenSettings={vi.fn()}
      />,
    );

    await waitFor(() => expect(ref.current).not.toBeNull());
    expect(ref.current).toEqual({
      archiveById: expect.any(Function),
      createNew: expect.any(Function),
      getConversations: expect.any(Function),
      selectById: expect.any(Function),
    });
    expect(ref.current!.getConversations()).toEqual([conversation]);

    expect(ref.current!.selectById("c-search")).toBe(true);
    expect(onSelect).toHaveBeenCalledWith(conversation);
    expect(ref.current!.selectById("missing")).toBe(false);
  });
});
