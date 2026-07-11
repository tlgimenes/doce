import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import SearchPanel from "./SearchPanel";
import { commands, type Conversation, type SearchResult } from "@/lib/ipc";

vi.mock("@/lib/ipc", () => ({
  commands: {
    searchConversations: vi.fn(),
  },
}));

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe("SearchPanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders ranked results with highlighted excerpts, selecting one calls onSelect", async () => {
    vi.mocked(commands.searchConversations).mockResolvedValue([
      {
        conversationId: "c1",
        title: "About foxes",
        excerpt: "the quick brown <mark>fox</mark> jumps",
        rank: -5,
      },
    ]);

    const onSelect = vi.fn();
    render(<SearchPanel onSelect={onSelect} />);

    expect(screen.getByRole("combobox", { name: "Search conversations" })).toBeInTheDocument();

    await userEvent.type(screen.getByTestId("search-input"), "fox");

    await waitFor(() => expect(screen.getByTestId("search-result")).toBeInTheDocument());
    expect(screen.getByText("fox").tagName).toBe("MARK");

    await userEvent.click(screen.getByTestId("search-result"));
    expect(onSelect).toHaveBeenCalledWith("c1");
  });

  it("gives result rows a non-clipping height class for multi-line content", async () => {
    vi.mocked(commands.searchConversations).mockResolvedValue([
      {
        conversationId: "c1",
        title: "A very long search result title that wraps onto another line",
        excerpt: "an excerpt that also wraps onto another line for the row layout",
        rank: -1,
      },
    ]);

    render(<SearchPanel onSelect={vi.fn()} />);
    await userEvent.type(screen.getByTestId("search-input"), "wrap");

    const row = await screen.findByTestId("search-result");
    expect(row.className).toContain("h-auto");
  });

  it("shows recent conversations before typing, newest first and capped to ten", async () => {
    const recentConversations: Conversation[] = Array.from({ length: 12 }, (_, i) => ({
      id: `c${i}`,
      workspaceId: null,
      title: `Conversation ${i}`,
      createdAt: i,
      updatedAt: i,
      lastSeenAt: i,
      status: "done",
    }));
    const onSelect = vi.fn();

    render(<SearchPanel onSelect={onSelect} recentConversations={recentConversations} />);

    const rows = screen.getAllByTestId("search-result");
    expect(rows).toHaveLength(10);
    expect(rows[0]).toHaveTextContent("Conversation 11");
    expect(rows[9]).toHaveTextContent("Conversation 2");
    expect(commands.searchConversations).not.toHaveBeenCalled();

    await userEvent.click(rows[0]);
    expect(onSelect).toHaveBeenCalledWith("c11");
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

    render(<SearchPanel onSelect={vi.fn()} />);
    await userEvent.type(screen.getByTestId("search-input"), "match");

    await waitFor(() => expect(screen.getByTestId("search-result")).toBeInTheDocument());
    expect(document.querySelector("script")).not.toBeInTheDocument();
    expect(screen.getByText(/<script>alert\(1\)<\/script>/)).toBeInTheDocument();
  });

  it("shows no results message when the query matches nothing", async () => {
    vi.mocked(commands.searchConversations).mockResolvedValue([]);

    render(<SearchPanel onSelect={vi.fn()} />);
    await userEvent.type(screen.getByTestId("search-input"), "nothingmatchesthis");

    await waitFor(() => expect(screen.getByText("No results.")).toBeInTheDocument());
  });

  it("shows a loading state while search is in flight", async () => {
    vi.mocked(commands.searchConversations).mockReturnValue(new Promise(() => {}));

    render(<SearchPanel onSelect={vi.fn()} />);
    await userEvent.type(screen.getByTestId("search-input"), "slow");

    expect(await screen.findByTestId("search-loading")).toHaveTextContent("Searching");
  });

  it("shows a backend error without closing the panel", async () => {
    vi.mocked(commands.searchConversations).mockRejectedValue(new Error("fts unavailable"));

    render(<SearchPanel onSelect={vi.fn()} />);
    await userEvent.type(screen.getByTestId("search-input"), "broken");

    expect(await screen.findByTestId("search-error")).toHaveTextContent("fts unavailable");
    expect(screen.getByTestId("search-panel")).toBeInTheDocument();
  });

  it("keeps only the latest in-flight search request in control of results and loading state", async () => {
    const first =
      deferred<Array<{ conversationId: string; title: string; excerpt: string; rank: number }>>();
    const second =
      deferred<Array<{ conversationId: string; title: string; excerpt: string; rank: number }>>();
    vi.mocked(commands.searchConversations).mockImplementation((query) => {
      if (query === "a") return first.promise;
      if (query === "ab") return second.promise;
      return Promise.resolve([]);
    });

    render(<SearchPanel onSelect={vi.fn()} />);

    await userEvent.type(screen.getByTestId("search-input"), "ab");
    expect(screen.getByTestId("search-loading")).toBeInTheDocument();

    first.resolve([
      {
        conversationId: "stale-result",
        title: "Stale result",
        excerpt: "old match",
        rank: -1,
      },
    ]);

    await waitFor(() => expect(screen.queryByText("Stale result")).not.toBeInTheDocument());
    expect(screen.getByTestId("search-loading")).toBeInTheDocument();

    second.resolve([
      {
        conversationId: "latest-result",
        title: "Latest result",
        excerpt: "new match",
        rank: -2,
      },
    ]);

    expect(await screen.findByText("Latest result")).toBeInTheDocument();
    await waitFor(() => expect(screen.queryByTestId("search-loading")).not.toBeInTheDocument());
    expect(screen.queryByText("Stale result")).not.toBeInTheDocument();
  });

  it("supports arrow-key navigation and Enter selection across visible results", async () => {
    vi.mocked(commands.searchConversations).mockResolvedValue([
      {
        conversationId: "c1",
        title: "First result",
        excerpt: "first match",
        rank: -5,
      },
      {
        conversationId: "c2",
        title: "Second result",
        excerpt: "second match",
        rank: -4,
      },
    ]);

    const onSelect = vi.fn();
    render(<SearchPanel onSelect={onSelect} />);

    const input = screen.getByTestId("search-input");
    await userEvent.type(input, "fi");

    const rows = await screen.findAllByTestId("search-result");
    expect(rows).toHaveLength(2);

    await userEvent.keyboard("{ArrowDown}");
    expect(input).toHaveAttribute("aria-activedescendant", "search-result-option-0");
    expect(rows[0]).toHaveAttribute("aria-selected", "true");
    expect(rows[0]).toHaveClass("bg-accent", "ring-1", "ring-ring");
    expect(rows[1]).toHaveAttribute("aria-selected", "false");

    await userEvent.keyboard("{ArrowDown}");
    expect(input).toHaveAttribute("aria-activedescendant", "search-result-option-1");
    expect(rows[0]).toHaveAttribute("aria-selected", "false");
    expect(rows[1]).toHaveAttribute("aria-selected", "true");
    expect(rows[1]).toHaveClass("bg-accent", "ring-1", "ring-ring");

    await userEvent.keyboard("{ArrowUp}");
    expect(input).toHaveAttribute("aria-activedescendant", "search-result-option-0");
    expect(rows[0]).toHaveAttribute("aria-selected", "true");

    await userEvent.keyboard("{Enter}");
    expect(onSelect).toHaveBeenCalledWith("c1");
  });

  it("does not allow Enter to select stale results while a newer query is still loading", async () => {
    const first = deferred<SearchResult[]>();
    const second = deferred<SearchResult[]>();
    vi.mocked(commands.searchConversations).mockImplementation((query) => {
      if (query === "a") return first.promise;
      if (query === "ab") return second.promise;
      return Promise.resolve([]);
    });

    const onSelect = vi.fn();
    render(<SearchPanel onSelect={onSelect} />);

    const input = screen.getByTestId("search-input");
    await userEvent.type(input, "a");
    first.resolve([
      {
        conversationId: "stale-result",
        title: "Stale result",
        excerpt: "old match",
        rank: -1,
      },
    ]);

    await screen.findByText("Stale result");
    await userEvent.keyboard("{ArrowDown}");
    expect(input).toHaveAttribute("aria-activedescendant", "search-result-option-0");

    await userEvent.type(input, "b");

    expect(screen.getByTestId("search-loading")).toBeInTheDocument();
    await userEvent.keyboard("{Enter}");
    expect(onSelect).not.toHaveBeenCalled();

    second.resolve([
      {
        conversationId: "fresh-result",
        title: "Fresh result",
        excerpt: "new match",
        rank: -2,
      },
    ]);

    expect(await screen.findByText("Fresh result")).toBeInTheDocument();
  });

  it("ignores stale rejected requests so a newer in-flight search keeps loading and error ownership", async () => {
    const first = deferred<SearchResult[]>();
    const second = deferred<SearchResult[]>();
    vi.mocked(commands.searchConversations).mockImplementation((query) => {
      if (query === "a") return first.promise;
      if (query === "ab") return second.promise;
      return Promise.resolve([]);
    });

    render(<SearchPanel onSelect={vi.fn()} />);

    await userEvent.type(screen.getByTestId("search-input"), "ab");
    expect(screen.getByTestId("search-loading")).toBeInTheDocument();

    first.reject(new Error("stale failure"));

    await waitFor(() => expect(screen.queryByTestId("search-error")).not.toBeInTheDocument());
    expect(screen.getByTestId("search-loading")).toBeInTheDocument();

    second.resolve([
      {
        conversationId: "latest-result",
        title: "Latest result",
        excerpt: "new match",
        rank: -2,
      },
    ]);

    expect(await screen.findByText("Latest result")).toBeInTheDocument();
    await waitFor(() => expect(screen.queryByTestId("search-loading")).not.toBeInTheDocument());
    expect(screen.queryByTestId("search-error")).not.toBeInTheDocument();
  });

  it("omits an inline close button and renders a taller dialog body", () => {
    render(<SearchPanel onSelect={vi.fn()} />);

    expect(screen.queryByRole("button", { name: "Close" })).not.toBeInTheDocument();
    expect(screen.queryByTestId("close-search-dialog")).not.toBeInTheDocument();

    const panel = screen.getByTestId("search-panel");
    expect(panel.className).toContain("h-[28rem]");
    expect(panel.className).toContain("max-h-[70vh]");
  });
});
