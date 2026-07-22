import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import { commands, events, type OAuthAccount } from "@/lib/ipc";
import HomeFeed from "./HomeFeed";

vi.mock("@/lib/ipc", () => ({
  commands: {
    // Connections (home surface) reads these on mount.
    listOauthAccounts: vi.fn(),
    listMcpServers: vi.fn(),
    listGoogleWorkspaceServices: vi.fn(),
    // ActivityView reads these on mount.
    listFeedCards: vi.fn(),
    dismissFeedCard: vi.fn(),
  },
  events: {
    onFeedCardCreated: vi.fn(),
  },
}));

const ACCOUNT: OAuthAccount = {
  id: "acct-1",
  provider: "google",
  clientId: "123.apps.googleusercontent.com",
  scopes: ["gmail.readonly"],
  expiresAt: 0,
  createdAt: 1_700_000_000_000,
};

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(commands.listOauthAccounts).mockResolvedValue([]);
  vi.mocked(commands.listMcpServers).mockResolvedValue([]);
  vi.mocked(commands.listGoogleWorkspaceServices).mockResolvedValue([]);
  vi.mocked(commands.listFeedCards).mockResolvedValue([]);
  vi.mocked(events.onFeedCardCreated).mockResolvedValue(() => {});
});

describe("HomeFeed (empty-state Stream)", () => {
  it("leads with an emphasized connect card and dashed (not-ready) preview cards when disconnected", async () => {
    render(<HomeFeed />);

    // Connect Google is the first, emphasized card.
    const connect = await screen.findByTestId("connect-service-card");
    expect(connect).toHaveTextContent("Google Workspace");
    expect(connect.className).toMatch(/emerald/);

    // The preview cards are the empty-feed body, dashed and dimmed.
    const previews = screen.getAllByTestId("preview-card");
    expect(previews).toHaveLength(2);
    for (const card of previews) {
      expect(card).not.toHaveAttribute("data-ready");
      expect(card).toHaveTextContent("soon");
    }
    expect(screen.getByTestId("home-preview")).toHaveTextContent(/Once you connect/);
  });

  it("swaps the connect card for a slim chip and brightens the preview cards once connected", async () => {
    vi.mocked(commands.listOauthAccounts).mockResolvedValue([ACCOUNT]);

    render(<HomeFeed />);

    // Connected: a slim chip replaces the connect card entirely.
    await screen.findByTestId("home-connected-chip");
    expect(screen.queryByTestId("connect-service-card")).not.toBeInTheDocument();

    // Preview cards brighten to the "waiting" (ready) state.
    await waitFor(() => {
      const previews = screen.getAllByTestId("preview-card");
      expect(previews[0]).toHaveAttribute("data-ready", "true");
    });
    for (const card of screen.getAllByTestId("preview-card")) {
      expect(card).toHaveTextContent("waiting");
    }
    expect(screen.getByTestId("home-preview")).toHaveTextContent(/^Connected\./);
  });

  it("shows the real activity card instead of previews once the agent has acted", async () => {
    vi.mocked(commands.listOauthAccounts).mockResolvedValue([ACCOUNT]);
    vi.mocked(commands.listFeedCards).mockResolvedValue([
      {
        id: "card-1",
        conversationId: "c1",
        kind: "draft",
        title: "Gmail: create_draft",
        preview: "Subject: hi",
        sourceTool: "create_draft",
        status: "pending",
        createdAt: Date.now(),
      },
    ]);

    render(<HomeFeed />);

    await waitFor(() => {
      expect(screen.getByText("Gmail: create_draft")).toBeInTheDocument();
    });
    // Real activity → the preview placeholders step aside.
    expect(screen.queryByTestId("home-preview")).not.toBeInTheDocument();
    expect(screen.queryByTestId("preview-card")).not.toBeInTheDocument();
  });
});
