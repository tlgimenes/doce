import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import FolderPicker from "./FolderPicker";
import { commands } from "@/lib/ipc";
import { open } from "@tauri-apps/plugin-dialog";

vi.mock("@/lib/ipc", () => ({
  commands: {
    listWorkspaces: vi.fn(),
  },
}));

vi.mock("@tauri-apps/api/path", () => ({
  homeDir: vi.fn(() => Promise.resolve("/Users/tester")),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
}));

const WORKSPACES = [
  { id: "ws-1", path: "/Users/tester/code/doce", displayName: "doce", createdAt: 1, lastOpenedAt: 20 },
  { id: "ws-2", path: "/Users/tester/code/other", displayName: "other", createdAt: 1, lastOpenedAt: 10 },
];

describe("FolderPicker (006-chat-empty-state, US2/US3)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders a pinned Home entry plus one row per listWorkspaces() result, with the current selection indicated", async () => {
    vi.mocked(commands.listWorkspaces).mockResolvedValue(WORKSPACES);

    render(<FolderPicker currentPath="/Users/tester/code/doce" onSelect={vi.fn()} onDismiss={vi.fn()} />);

    await waitFor(() => {
      expect(screen.getByTestId("folder-picker-home")).toBeInTheDocument();
      expect(screen.getAllByTestId("folder-picker-item")).toHaveLength(2);
    });
    expect(screen.getByText("doce")).toBeInTheDocument();
    expect(screen.getByText("other")).toBeInTheDocument();

    expect(screen.getByText("doce").closest("button")).toHaveAttribute("aria-current", "true");
    expect(screen.getByText("other").closest("button")).toHaveAttribute("aria-current", "false");
    expect(screen.getByTestId("folder-picker-home")).toHaveAttribute("aria-current", "false");
  });

  it("renders only the Home entry with no error state when there are no previously used folders (fresh install)", async () => {
    vi.mocked(commands.listWorkspaces).mockResolvedValue([]);

    render(<FolderPicker currentPath="/Users/tester" onSelect={vi.fn()} onDismiss={vi.fn()} />);

    await waitFor(() => expect(screen.getByTestId("folder-picker-home")).toBeInTheDocument());
    expect(screen.queryAllByTestId("folder-picker-item")).toHaveLength(0);
    expect(screen.queryByText(/error/i)).not.toBeInTheDocument();
  });

  it("typing into the filter field hides non-matching rows (FR-007)", async () => {
    vi.mocked(commands.listWorkspaces).mockResolvedValue(WORKSPACES);

    render(<FolderPicker currentPath="/Users/tester" onSelect={vi.fn()} onDismiss={vi.fn()} />);
    await waitFor(() => expect(screen.getAllByTestId("folder-picker-item")).toHaveLength(2));

    await userEvent.type(screen.getByTestId("folder-picker-filter"), "doc");

    expect(screen.getAllByTestId("folder-picker-item")).toHaveLength(1);
    expect(screen.getByText("doce")).toBeInTheDocument();
    expect(screen.queryByText("other")).not.toBeInTheDocument();
  });

  it("clicking a recent-folder row calls onSelect with a recent-kind target", async () => {
    vi.mocked(commands.listWorkspaces).mockResolvedValue(WORKSPACES);
    const onSelect = vi.fn();

    render(<FolderPicker currentPath="/Users/tester" onSelect={onSelect} onDismiss={vi.fn()} />);
    await waitFor(() => expect(screen.getAllByTestId("folder-picker-item")).toHaveLength(2));

    await userEvent.click(screen.getByText("doce"));

    expect(onSelect).toHaveBeenCalledWith({
      kind: "recent",
      path: "/Users/tester/code/doce",
      displayLabel: "doce",
    });
  });

  it("clicking the Home row calls onSelect with a home-kind target resolved to the real home directory", async () => {
    vi.mocked(commands.listWorkspaces).mockResolvedValue([]);
    const onSelect = vi.fn();

    render(<FolderPicker currentPath="/Users/tester" onSelect={onSelect} onDismiss={vi.fn()} />);
    await waitFor(() => expect(screen.getByTestId("folder-picker-home")).toBeInTheDocument());

    await userEvent.click(screen.getByTestId("folder-picker-home"));

    expect(onSelect).toHaveBeenCalledWith({ kind: "home", path: "/Users/tester", displayLabel: "Home" });
  });

  it("pressing Escape calls onDismiss without calling onSelect, leaving the selection unchanged (FR-011)", async () => {
    vi.mocked(commands.listWorkspaces).mockResolvedValue(WORKSPACES);
    const onSelect = vi.fn();
    const onDismiss = vi.fn();

    render(<FolderPicker currentPath="/Users/tester" onSelect={onSelect} onDismiss={onDismiss} />);
    await waitFor(() => expect(screen.getAllByTestId("folder-picker-item")).toHaveLength(2));

    await userEvent.keyboard("{Escape}");

    expect(onDismiss).toHaveBeenCalledTimes(1);
    expect(onSelect).not.toHaveBeenCalled();
  });

  it("clicking outside the picker calls onDismiss without calling onSelect (FR-011)", async () => {
    vi.mocked(commands.listWorkspaces).mockResolvedValue(WORKSPACES);
    const onSelect = vi.fn();
    const onDismiss = vi.fn();

    render(
      <div>
        <button data-testid="outside">outside</button>
        <FolderPicker currentPath="/Users/tester" onSelect={onSelect} onDismiss={onDismiss} />
      </div>,
    );
    await waitFor(() => expect(screen.getAllByTestId("folder-picker-item")).toHaveLength(2));

    await userEvent.click(screen.getByTestId("outside"));

    expect(onDismiss).toHaveBeenCalledTimes(1);
    expect(onSelect).not.toHaveBeenCalled();
  });

  it("US3: the Browse… entry opens the native folder dialog and selects the returned path (FR-008)", async () => {
    vi.mocked(commands.listWorkspaces).mockResolvedValue([]);
    vi.mocked(open).mockResolvedValue("/Volumes/External/never-opened-before");
    const onSelect = vi.fn();

    render(<FolderPicker currentPath="/Users/tester" onSelect={onSelect} onDismiss={vi.fn()} />);
    await waitFor(() => expect(screen.getByTestId("folder-picker-home")).toBeInTheDocument());

    await userEvent.click(screen.getByTestId("folder-picker-browse"));

    expect(open).toHaveBeenCalledWith({ directory: true });
    await waitFor(() =>
      expect(onSelect).toHaveBeenCalledWith({
        kind: "browsed",
        path: "/Volumes/External/never-opened-before",
        displayLabel: "never-opened-before",
      }),
    );
  });

  it("US3: cancelling the native dialog leaves the current target unchanged", async () => {
    vi.mocked(commands.listWorkspaces).mockResolvedValue([]);
    vi.mocked(open).mockResolvedValue(null);
    const onSelect = vi.fn();
    const onDismiss = vi.fn();

    render(<FolderPicker currentPath="/Users/tester" onSelect={onSelect} onDismiss={onDismiss} />);
    await waitFor(() => expect(screen.getByTestId("folder-picker-home")).toBeInTheDocument());

    await userEvent.click(screen.getByTestId("folder-picker-browse"));

    await waitFor(() => expect(open).toHaveBeenCalled());
    expect(onSelect).not.toHaveBeenCalled();
    expect(onDismiss).not.toHaveBeenCalled();
  });
});
