import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { commands, events, type FeedCard } from "@/lib/ipc";
import ActivityView from "./ActivityView";

vi.mock("@/lib/ipc", () => ({
  commands: {
    listFeedCards: vi.fn(),
    dismissFeedCard: vi.fn(),
  },
  events: {
    onFeedCardCreated: vi.fn(),
  },
}));

const mockCommands = vi.mocked(commands);
const mockEvents = vi.mocked(events);

function card(overrides: Partial<FeedCard> = {}): FeedCard {
  return {
    id: "card-1",
    conversationId: "c1",
    kind: "draft",
    title: "Gmail: create_draft",
    preview: "Subject: hi",
    sourceTool: "create_draft",
    status: "pending",
    createdAt: Date.now(),
    ...overrides,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  mockCommands.listFeedCards.mockResolvedValue([]);
  mockCommands.dismissFeedCard.mockResolvedValue(undefined);
  // Default: no live event wiring; individual tests override.
  mockEvents.onFeedCardCreated.mockResolvedValue(vi.fn());
});

describe("ActivityView", () => {
  it("renders the empty state when there are no cards", async () => {
    render(<ActivityView />);
    await waitFor(() => {
      expect(screen.getByTestId("activity-empty")).toBeInTheDocument();
    });
    expect(mockCommands.listFeedCards).toHaveBeenCalledWith(undefined);
  });

  it("renders a loaded card with its preview", async () => {
    mockCommands.listFeedCards.mockResolvedValue([card({ preview: "Draft body here" })]);
    render(<ActivityView />);
    await waitFor(() => {
      expect(screen.getByText("Gmail: create_draft")).toBeInTheDocument();
    });
    expect(screen.getByText("Draft body here")).toBeInTheDocument();
  });

  it("dismisses a card via the command", async () => {
    // A shell card exposes a single Dismiss button.
    mockCommands.listFeedCards.mockResolvedValue([
      card({ kind: "shell", title: "Slack: send_message" }),
    ]);
    render(<ActivityView />);
    await waitFor(() => {
      expect(screen.getByText("Slack: send_message")).toBeInTheDocument();
    });

    await userEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(mockCommands.dismissFeedCard).toHaveBeenCalledWith("card-1");
  });

  it("appends a card when the feed-card-created event fires", async () => {
    let emit: ((c: FeedCard) => void) | undefined;
    mockEvents.onFeedCardCreated.mockImplementation((cb) => {
      emit = cb;
      return Promise.resolve(vi.fn());
    });
    render(<ActivityView />);
    await waitFor(() => expect(emit).toBeDefined());

    emit?.(card({ id: "card-2", kind: "file", title: "Google Drive: create_file" }));
    await waitFor(() => {
      expect(screen.getByText("Google Drive: create_file")).toBeInTheDocument();
    });
  });
});
