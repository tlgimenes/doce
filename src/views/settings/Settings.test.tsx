import { type ReactElement } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { render as rtlRender, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ThemeProvider } from "next-themes";
import { commands, events } from "@/lib/ipc";
import Settings from "./Settings";

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

vi.mock("@/lib/ipc", () => ({
  commands: {
    listMcpServers: vi.fn(),
    listSkills: vi.fn(),
    addMcpServer: vi.fn(),
    listMcpServerTools: vi.fn(),
    getModelState: vi.fn(),
    selectCuratedModel: vi.fn(),
    selectLocalModel: vi.fn(),
    dismissModelNotice: vi.fn(),
    listOauthAccounts: vi.fn(),
    listGoogleWorkspaceServices: vi.fn(),
    connectOauthAccount: vi.fn(),
    removeOauthAccount: vi.fn(),
    addGoogleWorkspaceServers: vi.fn(),
    listFeedCards: vi.fn(),
    dismissFeedCard: vi.fn(),
  },
  events: {
    onModelInstallProgress: vi.fn(),
    onFeedCardCreated: vi.fn(),
  },
}));

const DEFAULT_MODEL_STATE = {
  hardware: { tier: "32gb", ramGb: 32, chip: "Apple M3", diskFreeGb: 120 },
  options: [
    {
      id: "balanced",
      displayName: "Balanced",
      description: "Fast and efficient for everyday work.",
      technicalName: "Qwen 3.5 4B",
      parameterCount: "4B",
      quantization: "Q4_K_M",
      sizeBytes: 2_700_000_000,
      recommended: true,
      installed: true,
      active: true,
      selected: true,
      sourceKind: "curated",
      localPath: null,
      state: "active",
      bytesDownloaded: 2_700_000_000,
      bytesTotal: 2_700_000_000,
    },
  ],
  activeId: "balanced",
  selectedId: "balanced",
  fallbackNotice: null,
  downloads: [],
} as unknown as Awaited<ReturnType<typeof commands.getModelState>>;

function render(ui: ReactElement) {
  return rtlRender(
    <ThemeProvider attribute="class" defaultTheme="system" enableSystem disableTransitionOnChange>
      {ui}
    </ThemeProvider>,
  );
}

describe("Settings", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(commands.listMcpServers).mockResolvedValue([]);
    vi.mocked(commands.listSkills).mockResolvedValue([]);
    vi.mocked(commands.listOauthAccounts).mockResolvedValue([]);
    vi.mocked(commands.listGoogleWorkspaceServices).mockResolvedValue([]);
    vi.mocked(commands.getModelState).mockResolvedValue(DEFAULT_MODEL_STATE);
    vi.mocked(commands.selectCuratedModel).mockResolvedValue(DEFAULT_MODEL_STATE);
    vi.mocked(commands.selectLocalModel).mockResolvedValue(DEFAULT_MODEL_STATE);
    vi.mocked(commands.dismissModelNotice).mockResolvedValue(undefined);
    vi.mocked(commands.listFeedCards).mockResolvedValue([]);
    vi.mocked(commands.dismissFeedCard).mockResolvedValue(undefined);
    vi.mocked(events.onModelInstallProgress).mockResolvedValue(vi.fn());
    vi.mocked(events.onFeedCardCreated).mockResolvedValue(vi.fn());
  });

  afterEach(() => {
    document.documentElement.classList.remove("dark");
  });

  it("fills the shell content area instead of forcing viewport height", () => {
    render(<Settings onClose={vi.fn()} />);

    expect(screen.getByTestId("settings-view")).toHaveClass("h-full");
    expect(screen.getByTestId("settings-view")).not.toHaveClass("h-dvh");
  });

  it("shows the running build's version at the bottom", () => {
    render(<Settings onClose={vi.fn()} />);

    // Version + build commit are injected at build time (vite.config.ts's
    // `define`); assert the shape rather than a pinned commit.
    const version = screen.getByTestId("settings-version");
    expect(version).toHaveTextContent(/^doce v\d+\.\d+\.\d+/);
  });

  it("renders one consolidated, ordered screen without tabs, search, or help", async () => {
    render(<Settings onClose={vi.fn()} />);

    const general = screen.getByTestId("settings-general-section");
    const model = screen.getByTestId("settings-model-section");
    const extensions = screen.getByTestId("settings-extensions-section");
    await screen.findByTestId("model-selector-trigger");

    expect(general).toHaveTextContent("General");
    expect(general).toHaveTextContent("Appearance");
    expect(model).toHaveTextContent("Model");
    expect(extensions).toHaveTextContent("MCP servers");
    expect(extensions).toHaveTextContent("Skills");
    expect(general.compareDocumentPosition(model) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(
      model.compareDocumentPosition(extensions) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(screen.queryByRole("tablist")).not.toBeInTheDocument();
    expect(screen.queryByRole("tab")).not.toBeInTheDocument();
    expect(screen.queryByRole("searchbox")).not.toBeInTheDocument();
    expect(screen.queryByText("Help")).not.toBeInTheDocument();
  });

  it("lists discovered skills without switching views", async () => {
    vi.mocked(commands.listSkills).mockResolvedValue([
      { name: "pdf-tools", description: "Work with PDF files" },
    ]);

    render(<Settings onClose={vi.fn()} />);

    const skill = await screen.findByTestId("skill-item");
    expect(skill).toHaveTextContent("pdf-tools");
    expect(skill).toHaveTextContent("Work with PDF files");
    expect(screen.getByTestId("settings-mcp-panel")).toBeVisible();
  });

  it("adds an MCP server with parsed arguments and refreshes the list", async () => {
    vi.mocked(commands.addMcpServer).mockResolvedValue({
      id: "srv-1",
      name: "my-server",
      transport: "stdio",
      config: "{}",
      enabled: true,
      createdAt: 1,
    });

    render(<Settings onClose={vi.fn()} />);
    await userEvent.type(screen.getByLabelText("Server name"), "my-server");
    await userEvent.type(screen.getByLabelText("Command"), "npx");
    await userEvent.type(screen.getByLabelText("Arguments"), "-y some-package");
    await userEvent.click(screen.getByTestId("add-mcp-server"));

    await waitFor(() => {
      expect(commands.addMcpServer).toHaveBeenCalledWith("my-server", "npx", [
        "-y",
        "some-package",
      ]);
      // One mount read (Settings' MCP panel — Connections moved to the home)
      // plus the post-add refresh.
      expect(commands.listMcpServers).toHaveBeenCalledTimes(2);
    });
  });

  it("tests a server connection and shows its tools", async () => {
    vi.mocked(commands.listMcpServers).mockResolvedValue([
      {
        id: "srv-1",
        name: "my-server",
        transport: "stdio",
        config: "{}",
        enabled: true,
        createdAt: 1,
      },
    ]);
    vi.mocked(commands.listMcpServerTools).mockResolvedValue([
      { name: "echo", description: "Echoes input" },
    ]);

    render(<Settings onClose={vi.fn()} />);
    await screen.findByTestId("mcp-server-item");
    await userEvent.click(screen.getByTestId("test-mcp-server"));

    expect(await screen.findByTestId("mcp-server-tools")).toHaveTextContent("echo");
  });

  it("composes MCP server and skill rows with item primitives", async () => {
    vi.mocked(commands.listMcpServers).mockResolvedValue([
      {
        id: "srv-1",
        name: "my-server",
        transport: "stdio",
        config: "{}",
        enabled: true,
        createdAt: 1,
      },
    ]);
    vi.mocked(commands.listSkills).mockResolvedValue([
      { name: "pdf-tools", description: "Work with PDF files" },
    ]);

    render(<Settings onClose={vi.fn()} />);

    const serverRow = await screen.findByTestId("mcp-server-item");
    expect(serverRow).toHaveAttribute("data-slot", "item");
    expect(serverRow.querySelector('[data-slot="item-content"]')).toBeTruthy();
    expect(serverRow.querySelector('[data-slot="item-actions"]')).toBeTruthy();

    const skillRow = await screen.findByTestId("skill-item");
    expect(skillRow).toHaveAttribute("data-slot", "item");
    expect(skillRow.querySelector('[data-slot="item-content"]')).toBeTruthy();
    expect(skillRow.querySelector('[data-slot="item-title"]')).toBeTruthy();
    expect(skillRow.querySelector('[data-slot="item-description"]')).toBeTruthy();
  });

  it("shows connection and add-server errors without hiding existing rows", async () => {
    vi.mocked(commands.listMcpServers).mockResolvedValue([
      {
        id: "srv-1",
        name: "existing",
        transport: "stdio",
        config: "{}",
        enabled: true,
        createdAt: 1,
      },
    ]);
    vi.mocked(commands.listMcpServerTools).mockRejectedValue(new Error("connection refused"));
    vi.mocked(commands.addMcpServer).mockRejectedValue(new Error("bad command"));

    render(<Settings onClose={vi.fn()} />);
    await screen.findByTestId("mcp-server-item");
    await userEvent.click(screen.getByTestId("test-mcp-server"));
    expect(await screen.findByText("Failed to connect")).toBeInTheDocument();

    await userEvent.type(screen.getByTestId("mcp-name-input"), "broken");
    await userEvent.type(screen.getByTestId("mcp-command-input"), "missing-bin");
    await userEvent.click(screen.getByTestId("add-mcp-server"));

    expect(await screen.findByTestId("mcp-add-error")).toHaveTextContent("bad command");
    expect(screen.getByText("existing")).toBeInTheDocument();
  });

  it("keeps independent settings sections visible when one discovery call fails", async () => {
    vi.mocked(commands.listMcpServers).mockRejectedValue(new Error("offline"));
    vi.mocked(commands.listSkills).mockResolvedValue([
      { name: "pdf-tools", description: "Work with PDF files" },
    ]);

    render(<Settings onClose={vi.fn()} />);

    expect(await screen.findByText("pdf-tools")).toBeInTheDocument();
    expect(screen.getByTestId("settings-model-panel")).toBeVisible();
    expect(screen.getByTestId("settings-mcp-panel")).toBeVisible();
  });

  it("closes settings from the header", async () => {
    const onClose = vi.fn();
    render(<Settings onClose={onClose} />);

    await userEvent.click(screen.getByTestId("close-settings"));
    expect(onClose).toHaveBeenCalledOnce();
  });
});

describe("Settings appearance", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(commands.listMcpServers).mockResolvedValue([]);
    vi.mocked(commands.listSkills).mockResolvedValue([]);
    vi.mocked(commands.listOauthAccounts).mockResolvedValue([]);
    vi.mocked(commands.listGoogleWorkspaceServices).mockResolvedValue([]);
    vi.mocked(commands.getModelState).mockResolvedValue(DEFAULT_MODEL_STATE);
    vi.mocked(events.onModelInstallProgress).mockResolvedValue(vi.fn());
  });

  afterEach(() => {
    document.documentElement.classList.remove("dark");
  });

  it("defaults Appearance to System", async () => {
    render(<Settings onClose={vi.fn()} />);

    expect(await screen.findByTestId("theme-select")).toHaveTextContent("System");
    expect(document.documentElement.classList.contains("dark")).toBe(false);
  });

  it("switches between Dark and Light", async () => {
    render(<Settings onClose={vi.fn()} />);

    const trigger = await screen.findByTestId("theme-select");
    await userEvent.click(trigger);
    await userEvent.click(await screen.findByRole("option", { name: "Dark" }));
    await waitFor(() => expect(document.documentElement.classList.contains("dark")).toBe(true));
    expect(trigger).toHaveTextContent("Dark");

    await userEvent.click(trigger);
    await userEvent.click(await screen.findByRole("option", { name: "Light" }));
    await waitFor(() => expect(document.documentElement.classList.contains("dark")).toBe(false));
  });
});
