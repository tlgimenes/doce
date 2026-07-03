import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import SearchPanel from "./SearchPanel";
import { commands } from "@/lib/ipc";

vi.mock("@/lib/ipc", () => ({
  commands: {
    searchConversations: vi.fn(),
  },
}));

describe("SearchPanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders ranked results with highlighted excerpts, selecting one calls onSelect", async () => {
    vi.mocked(commands.searchConversations).mockResolvedValue([
      { conversationId: "c1", title: "About foxes", excerpt: "the quick brown <mark>fox</mark> jumps", rank: -5 },
    ]);

    const onSelect = vi.fn();
    render(<SearchPanel onSelect={onSelect} onClose={vi.fn()} />);

    await userEvent.type(screen.getByTestId("search-input"), "fox");

    await waitFor(() => expect(screen.getByTestId("search-result")).toBeInTheDocument());
    expect(screen.getByText("fox").tagName).toBe("MARK");

    await userEvent.click(screen.getByTestId("search-result"));
    expect(onSelect).toHaveBeenCalledWith("c1");
  });

  it("does not interpret excerpt content as HTML beyond the mark markers (no injection from a user's own message)", async () => {
    vi.mocked(commands.searchConversations).mockResolvedValue([
      {
        conversationId: "c1",
        title: "weird",
        excerpt: "a message with <script>alert(1)</script> and <mark>match</mark> in it",
        rank: -1,
      },
    ]);

    render(<SearchPanel onSelect={vi.fn()} onClose={vi.fn()} />);
    await userEvent.type(screen.getByTestId("search-input"), "match");

    await waitFor(() => expect(screen.getByTestId("search-result")).toBeInTheDocument());
    expect(document.querySelector("script")).not.toBeInTheDocument();
    expect(screen.getByText(/<script>alert\(1\)<\/script>/)).toBeInTheDocument();
  });

  it("shows no results message when the query matches nothing", async () => {
    vi.mocked(commands.searchConversations).mockResolvedValue([]);

    render(<SearchPanel onSelect={vi.fn()} onClose={vi.fn()} />);
    await userEvent.type(screen.getByTestId("search-input"), "nothingmatchesthis");

    await waitFor(() => expect(screen.getByText("No results.")).toBeInTheDocument());
  });
});
