import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { commands, type McpServerConnection, type OAuthAccount } from "@/lib/ipc";
import Connections from "./Connections";

vi.mock("@/lib/ipc", () => ({
  commands: {
    listOauthAccounts: vi.fn(),
    listMcpServers: vi.fn(),
    listGoogleWorkspaceServices: vi.fn(),
    connectOauthAccount: vi.fn(),
    addGoogleWorkspaceServers: vi.fn(),
    removeOauthAccount: vi.fn(),
  },
}));

const WORKSPACE_SERVICES = [
  { key: "gmail", displayName: "Gmail", url: "https://gmail/mcp", scopes: ["gmail.readonly"] },
  {
    key: "calendar",
    displayName: "Google Calendar",
    url: "https://cal/mcp",
    scopes: ["calendar.events"],
  },
  { key: "drive", displayName: "Google Drive", url: "https://drive/mcp", scopes: ["drive.file"] },
];

const ACCOUNT: OAuthAccount = {
  id: "acct-1",
  provider: "google",
  clientId: "123.apps.googleusercontent.com",
  scopes: ["gmail.readonly"],
  expiresAt: 0,
  createdAt: 1_700_000_000_000,
};

function serverFor(name: string, accountId: string): McpServerConnection {
  return {
    id: `srv-${name}`,
    name,
    transport: "http",
    config: JSON.stringify({ url: "https://x/mcp", oauth_account_id: accountId }),
    enabled: true,
    createdAt: 0,
  };
}

describe("Connections", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(commands.listOauthAccounts).mockResolvedValue([]);
    vi.mocked(commands.listMcpServers).mockResolvedValue([]);
    vi.mocked(commands.listGoogleWorkspaceServices).mockResolvedValue(WORKSPACE_SERVICES);
  });

  it("shows the empty state: a Google connect card and the privacy note", async () => {
    render(<Connections />);

    expect(await screen.findByTestId("connect-service-card")).toHaveTextContent("Google Workspace");
    expect(screen.getByTestId("connections-privacy-note")).toBeInTheDocument();
    expect(screen.queryByTestId("connected-account-card")).not.toBeInTheDocument();
  });

  it("connects in one click: OAuths with the built-in client and grants every service", async () => {
    let resolveConnect: (account: OAuthAccount) => void = () => {};
    vi.mocked(commands.connectOauthAccount).mockReturnValue(
      new Promise<OAuthAccount>((resolve) => {
        resolveConnect = resolve;
      }),
    );
    vi.mocked(commands.addGoogleWorkspaceServers).mockResolvedValue([]);

    render(<Connections />);

    // No form, no credential fields, no service picker — clicking Connect goes
    // straight to the blocking browser-consent flow.
    await userEvent.click(await screen.findByRole("button", { name: /connect/i }));

    expect(screen.getByTestId("connect-waiting")).toBeInTheDocument();
    expect(screen.queryByTestId("connections-form")).not.toBeInTheDocument();

    // After the account connects, the servers register and the list refreshes.
    vi.mocked(commands.listOauthAccounts).mockResolvedValue([ACCOUNT]);
    vi.mocked(commands.listMcpServers).mockResolvedValue([
      serverFor("Gmail", ACCOUNT.id),
      serverFor("Google Calendar", ACCOUNT.id),
      serverFor("Google Drive", ACCOUNT.id),
    ]);
    resolveConnect(ACCOUNT);

    const card = await screen.findByTestId("connected-account-card");
    expect(card).toHaveTextContent("Google Workspace");
    expect(within(card).getAllByTestId("granted-service-row")).toHaveLength(3);

    // Empty client_id → the backend resolves the baked-in client; every preset
    // service is granted, not a chosen subset.
    expect(commands.connectOauthAccount).toHaveBeenCalledWith("google", "", undefined, []);
    expect(commands.addGoogleWorkspaceServers).toHaveBeenCalledWith(ACCOUNT.id, [
      "gmail",
      "calendar",
      "drive",
    ]);
  });

  it("surfaces a connect failure inline and returns to the connect card", async () => {
    vi.mocked(commands.connectOauthAccount).mockRejectedValue(new Error("access_denied"));

    render(<Connections />);

    await userEvent.click(await screen.findByRole("button", { name: /connect/i }));

    expect(await screen.findByTestId("connect-error")).toHaveTextContent("access_denied");
    // Back to the empty state, ready to retry — no form in between.
    expect(screen.getByTestId("connect-service-card")).toBeInTheDocument();
    expect(commands.addGoogleWorkspaceServers).not.toHaveBeenCalled();
  });

  it("lists a connected account and its granted services", async () => {
    vi.mocked(commands.listOauthAccounts).mockResolvedValue([ACCOUNT]);
    vi.mocked(commands.listMcpServers).mockResolvedValue([serverFor("Gmail", ACCOUNT.id)]);

    render(<Connections />);

    const card = await screen.findByTestId("connected-account-card");
    expect(within(card).getByText("Gmail")).toBeInTheDocument();
    expect(within(card).getAllByTestId("granted-service-row")).toHaveLength(1);
    // The connected card keeps its no-toggle contract.
    expect(within(card).queryAllByRole("switch")).toHaveLength(0);
  });

  it("confirms before disconnecting, then removes the account and refreshes", async () => {
    vi.mocked(commands.listOauthAccounts).mockResolvedValue([ACCOUNT]);
    vi.mocked(commands.listMcpServers).mockResolvedValue([serverFor("Gmail", ACCOUNT.id)]);
    vi.mocked(commands.removeOauthAccount).mockResolvedValue(undefined);

    render(<Connections />);

    await screen.findByTestId("connected-account-card");
    await userEvent.click(screen.getByRole("button", { name: "Disconnect account" }));

    // Account is removed only after the confirmation.
    expect(commands.removeOauthAccount).not.toHaveBeenCalled();

    vi.mocked(commands.listOauthAccounts).mockResolvedValue([]);
    await userEvent.click(await screen.findByTestId("confirm-disconnect"));

    await waitFor(() => expect(commands.removeOauthAccount).toHaveBeenCalledWith(ACCOUNT.id));
    await waitFor(() =>
      expect(screen.queryByTestId("connected-account-card")).not.toBeInTheDocument(),
    );
  });
});
