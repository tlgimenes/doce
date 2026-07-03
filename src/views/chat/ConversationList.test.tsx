import { createRef } from "react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import ConversationList, { type ConversationListHandle } from "./ConversationList";
import { commands } from "@/lib/ipc";

vi.mock("@/lib/ipc", () => ({
  commands: {
    listConversations: vi.fn(),
    createConversation: vi.fn(),
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

    render(<ConversationList activeId={null} onSelect={vi.fn()} onCreated={vi.fn()} onOpenSettings={vi.fn()} />);

    await waitFor(() => {
      expect(screen.getByText("First one")).toBeInTheDocument();
    });
    expect(screen.getByText("Needs input")).toBeInTheDocument();
    expect(screen.getByText("Broke")).toBeInTheDocument();

    const dots = screen.getAllByTestId("conversation-status-dot");
    expect(dots.map((d) => d.dataset.status)).toEqual(["done", "requires_action", "failed"]);
  });

  it("creating a new conversation calls onCreated with the new id (the reported bug: no way to start a new thread)", async () => {
    vi.mocked(commands.listConversations).mockResolvedValue([]);
    vi.mocked(commands.createConversation).mockResolvedValue({
      id: "new-conv",
      workspaceId: null,
      title: "New conversation",
      createdAt: 1,
      updatedAt: 1,
      status: "done",
    });

    const onCreated = vi.fn();
    render(<ConversationList activeId={null} onSelect={vi.fn()} onCreated={onCreated} onOpenSettings={vi.fn()} />);

    await userEvent.click(await screen.findByTestId("new-conversation"));

    await waitFor(() => expect(onCreated).toHaveBeenCalledWith("new-conv"));
  });

  it("exposes createNew via a ref, calling the same path as clicking the button (005-keyboard-shortcuts, Cmd+N)", async () => {
    vi.mocked(commands.listConversations).mockResolvedValue([]);
    vi.mocked(commands.createConversation).mockResolvedValue({
      id: "new-conv",
      workspaceId: null,
      title: "New conversation",
      createdAt: 1,
      updatedAt: 1,
      status: "done",
    });

    const onCreated = vi.fn();
    const ref = createRef<ConversationListHandle>();
    render(<ConversationList ref={ref} activeId={null} onSelect={vi.fn()} onCreated={onCreated} onOpenSettings={vi.fn()} />);

    await waitFor(() => expect(ref.current).not.toBeNull());
    ref.current!.createNew();

    await waitFor(() => expect(onCreated).toHaveBeenCalledWith("new-conv"));
    expect(commands.createConversation).toHaveBeenCalledTimes(1);
  });
});
