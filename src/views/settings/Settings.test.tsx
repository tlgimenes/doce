import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import Settings from "./Settings";
import { commands } from "@/lib/ipc";

vi.mock("@/lib/ipc", () => ({
  commands: {
    listMcpServers: vi.fn(),
    listSkills: vi.fn(),
    addMcpServer: vi.fn(),
    listMcpServerTools: vi.fn(),
  },
}));

describe("Settings (User Story 4: MCP servers + skills)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(commands.listMcpServers).mockResolvedValue([]);
    vi.mocked(commands.listSkills).mockResolvedValue([]);
  });

  it("fills the shell content area instead of forcing viewport height", () => {
    render(<Settings onClose={vi.fn()} />);

    expect(screen.getByTestId("settings-view")).toHaveClass("h-full");
    expect(screen.getByTestId("settings-view")).not.toHaveClass("h-dvh");
  });

  it("lists discovered skills in the Skills tab", async () => {
    vi.mocked(commands.listSkills).mockResolvedValue([
      { name: "pdf-tools", description: "Work with PDF files" },
    ]);

    render(<Settings onClose={vi.fn()} />);
    await userEvent.click(await screen.findByTestId("settings-tab-skills"));
    await waitFor(() => {
      expect(screen.getByTestId("skill-item")).toHaveTextContent("pdf-tools");
      expect(screen.getByTestId("skill-item")).toHaveTextContent("Work with PDF files");
    });
  });

  it("adding an MCP server calls add_mcp_server with parsed args and refreshes the list", async () => {
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
      expect(commands.listMcpServers).toHaveBeenCalledTimes(2); // initial + post-add refresh
    });
  });

  it("testing a server connection shows its tools", async () => {
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

    await waitFor(() => {
      expect(screen.getByTestId("mcp-server-tools")).toHaveTextContent("echo");
    });
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

    await userEvent.click(screen.getByTestId("settings-tab-skills"));

    const skillRow = await screen.findByTestId("skill-item");
    expect(skillRow).toHaveAttribute("data-slot", "item");
    expect(skillRow.querySelector('[data-slot="item-content"]')).toBeTruthy();
    expect(skillRow.querySelector('[data-slot="item-title"]')).toBeTruthy();
    expect(skillRow.querySelector('[data-slot="item-description"]')).toBeTruthy();
  });

  it("shows an error if testing a server connection fails", async () => {
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
    vi.mocked(commands.listMcpServerTools).mockRejectedValue(new Error("connection refused"));

    render(<Settings onClose={vi.fn()} />);
    await screen.findByTestId("mcp-server-item");
    await userEvent.click(screen.getByTestId("test-mcp-server"));

    await waitFor(() => {
      expect(screen.getByText("Failed to connect")).toBeInTheDocument();
    });
  });

  it("renders MCP and Skills tabs and switches between them", async () => {
    vi.mocked(commands.listSkills).mockResolvedValue([
      { name: "pdf-tools", description: "Work with PDF files" },
    ]);

    render(<Settings onClose={vi.fn()} />);

    expect(await screen.findByTestId("settings-tab-mcp")).toHaveAttribute("aria-selected", "true");
    await userEvent.click(screen.getByTestId("settings-tab-skills"));

    expect(screen.getByTestId("settings-tab-skills")).toHaveAttribute("aria-selected", "true");
    expect(await screen.findByTestId("skill-item")).toHaveTextContent("pdf-tools");
  });

  it("shows an inline add-server error and keeps existing rows visible", async () => {
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
    vi.mocked(commands.addMcpServer).mockRejectedValue(new Error("bad command"));

    render(<Settings onClose={vi.fn()} />);
    await screen.findByTestId("mcp-server-item");

    await userEvent.type(screen.getByTestId("mcp-name-input"), "broken");
    await userEvent.type(screen.getByTestId("mcp-command-input"), "missing-bin");
    await userEvent.click(screen.getByTestId("add-mcp-server"));

    expect(await screen.findByTestId("mcp-add-error")).toHaveTextContent("bad command");
    expect(screen.getByText("existing")).toBeInTheDocument();
  });
});
