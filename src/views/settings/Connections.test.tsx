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
    googleOauthBuiltinAvailable: vi.fn(),
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
    // Default: no built-in client, so the bring-your-own form is required —
    // the built-in-mode tests override this to `true`.
    vi.mocked(commands.googleOauthBuiltinAvailable).mockResolvedValue(false);
  });

  it("shows the empty state: a Google connect card and the privacy note", async () => {
    render(<Connections />);

    expect(await screen.findByTestId("connect-service-card")).toHaveTextContent("Google Workspace");
    expect(screen.getByTestId("connections-privacy-note")).toBeInTheDocument();
    expect(screen.queryByTestId("connected-account-card")).not.toBeInTheDocument();
  });

  it("opens the credential form with all services checked by default", async () => {
    render(<Connections />);

    await userEvent.click(await screen.findByRole("button", { name: /connect/i }));

    expect(screen.getByTestId("connections-form")).toBeInTheDocument();
    expect(screen.getByTestId("oauth-client-id-input")).toBeInTheDocument();
    expect(screen.getByTestId("oauth-client-secret-input")).toBeInTheDocument();
    for (const key of ["gmail", "calendar", "drive"]) {
      const row = screen.getByTestId(`service-picker-${key}`);
      expect(within(row).getByRole("checkbox")).toBeChecked();
    }
  });

  describe("built-in Google client", () => {
    beforeEach(() => {
      vi.mocked(commands.googleOauthBuiltinAvailable).mockResolvedValue(true);
    });

    it("hides the credential fields and connects with an empty client_id", async () => {
      vi.mocked(commands.connectOauthAccount).mockResolvedValue(ACCOUNT);
      vi.mocked(commands.addGoogleWorkspaceServers).mockResolvedValue([]);

      render(<Connections />);

      await userEvent.click(await screen.findByRole("button", { name: /connect/i }));

      // The form opens straight to the service picker — no credential inputs.
      expect(screen.getByTestId("connections-form")).toBeInTheDocument();
      expect(screen.queryByTestId("oauth-client-id-input")).not.toBeInTheDocument();
      expect(screen.queryByTestId("oauth-client-secret-input")).not.toBeInTheDocument();
      expect(screen.getByTestId("service-picker-gmail")).toBeInTheDocument();

      await userEvent.click(screen.getByTestId("connect-continue"));

      // Empty client_id → the backend resolves the built-in client.
      expect(commands.connectOauthAccount).toHaveBeenCalledWith("google", "", undefined, []);
    });

    it("reveals the bring-your-own fields via the advanced toggle", async () => {
      render(<Connections />);

      await userEvent.click(await screen.findByRole("button", { name: /connect/i }));
      expect(screen.queryByTestId("oauth-client-id-input")).not.toBeInTheDocument();

      await userEvent.click(screen.getByTestId("use-own-client-toggle"));

      // The BYO fields appear and the toggle collapses.
      expect(screen.getByTestId("oauth-client-id-input")).toBeInTheDocument();
      expect(screen.getByTestId("oauth-client-secret-input")).toBeInTheDocument();
      expect(screen.queryByTestId("use-own-client-toggle")).not.toBeInTheDocument();
      // Continue is now gated on a client id, as in BYO mode.
      expect(screen.getByTestId("connect-continue")).toBeDisabled();

      await userEvent.type(screen.getByTestId("oauth-client-id-input"), "my-client-id");
      vi.mocked(commands.connectOauthAccount).mockResolvedValue(ACCOUNT);
      vi.mocked(commands.addGoogleWorkspaceServers).mockResolvedValue([]);
      await userEvent.click(screen.getByTestId("connect-continue"));

      expect(commands.connectOauthAccount).toHaveBeenCalledWith(
        "google",
        "my-client-id",
        undefined,
        [],
      );
    });
  });

  it("runs empty → form → waiting → connected, registering the chosen services", async () => {
    let resolveConnect: (account: OAuthAccount) => void = () => {};
    vi.mocked(commands.connectOauthAccount).mockReturnValue(
      new Promise<OAuthAccount>((resolve) => {
        resolveConnect = resolve;
      }),
    );
    vi.mocked(commands.addGoogleWorkspaceServers).mockResolvedValue([]);

    render(<Connections />);

    await userEvent.click(await screen.findByRole("button", { name: /connect/i }));
    await userEvent.type(screen.getByTestId("oauth-client-id-input"), "my-client-id");
    await userEvent.type(screen.getByTestId("oauth-client-secret-input"), "my-secret");

    // After the account connects, the servers register and the list refreshes
    // to the connected view.
    vi.mocked(commands.listOauthAccounts).mockResolvedValue([ACCOUNT]);
    vi.mocked(commands.listMcpServers).mockResolvedValue([
      serverFor("Gmail", ACCOUNT.id),
      serverFor("Google Calendar", ACCOUNT.id),
      serverFor("Google Drive", ACCOUNT.id),
    ]);

    await userEvent.click(screen.getByTestId("connect-continue"));

    // Blocking browser-consent waiting state.
    expect(screen.getByTestId("connect-waiting")).toBeInTheDocument();

    resolveConnect(ACCOUNT);

    const card = await screen.findByTestId("connected-account-card");
    expect(card).toHaveTextContent("Google Workspace");
    expect(within(card).getAllByTestId("granted-service-row")).toHaveLength(3);

    expect(commands.connectOauthAccount).toHaveBeenCalledWith(
      "google",
      "my-client-id",
      "my-secret",
      [],
    );
    expect(commands.addGoogleWorkspaceServers).toHaveBeenCalledWith(ACCOUNT.id, [
      "gmail",
      "calendar",
      "drive",
    ]);
  });

  it("surfaces a connect failure inline and returns to the form to retry", async () => {
    vi.mocked(commands.connectOauthAccount).mockRejectedValue(new Error("access_denied"));

    render(<Connections />);

    await userEvent.click(await screen.findByRole("button", { name: /connect/i }));
    await userEvent.type(screen.getByTestId("oauth-client-id-input"), "cid");
    await userEvent.click(screen.getByTestId("connect-continue"));

    expect(await screen.findByTestId("connect-error")).toHaveTextContent("access_denied");
    expect(screen.getByTestId("connections-form")).toBeInTheDocument();
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
