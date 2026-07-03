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

  it("lists discovered skills", async () => {
    vi.mocked(commands.listSkills).mockResolvedValue([
      { name: "pdf-tools", description: "Work with PDF files" },
    ]);

    render(<Settings onClose={vi.fn()} />);
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
    await userEvent.type(screen.getByTestId("mcp-name-input"), "my-server");
    await userEvent.type(screen.getByTestId("mcp-command-input"), "npx");
    await userEvent.type(screen.getByTestId("mcp-args-input"), "-y some-package");
    await userEvent.click(screen.getByTestId("add-mcp-server"));

    await waitFor(() => {
      expect(commands.addMcpServer).toHaveBeenCalledWith("my-server", "npx", ["-y", "some-package"]);
      expect(commands.listMcpServers).toHaveBeenCalledTimes(2); // initial + post-add refresh
    });
  });

  it("testing a server connection shows its tools", async () => {
    vi.mocked(commands.listMcpServers).mockResolvedValue([
      { id: "srv-1", name: "my-server", transport: "stdio", config: "{}", enabled: true, createdAt: 1 },
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

  it("shows an error if testing a server connection fails", async () => {
    vi.mocked(commands.listMcpServers).mockResolvedValue([
      { id: "srv-1", name: "my-server", transport: "stdio", config: "{}", enabled: true, createdAt: 1 },
    ]);
    vi.mocked(commands.listMcpServerTools).mockRejectedValue(new Error("connection refused"));

    render(<Settings onClose={vi.fn()} />);
    await screen.findByTestId("mcp-server-item");
    await userEvent.click(screen.getByTestId("test-mcp-server"));

    await waitFor(() => {
      expect(screen.getByText("Failed to connect")).toBeInTheDocument();
    });
  });
});
