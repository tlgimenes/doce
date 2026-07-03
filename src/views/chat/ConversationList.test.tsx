import { createRef } from "react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import ConversationList, { type ConversationListHandle } from "./ConversationList";
import { commands } from "@/lib/ipc";

vi.mock("@/lib/ipc", () => ({
  commands: {
    listConversations: vi.fn(),
  },
}));

describe("ConversationList", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders titles and a status dot per conversation (FR-011/FR-012)", async () => {
    vi.mocked(commands.listConversations).mockResolvedValue([
      { id: "a", workspaceId: null, title: "First one", createdAt: 1, updatedAt: 3, status: "done" },
      { id: "b", workspaceId: null, title: "Needs input", createdAt: 2, updatedAt: 2, status: "requires_action" },
      { id: "c", workspaceId: null, title: "Broke", createdAt: 3, updatedAt: 1, status: "failed" },
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
});
