import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import FolderPicker from "./FolderPicker";
import { commands } from "@/lib/ipc";
import { open } from "@tauri-apps/plugin-dialog";

vi.mock("@/lib/ipc", () => ({
  commands: {
    listWorkspaces: vi.fn(),
    searchFolders: vi.fn(),
  },
}));

vi.mock("@tauri-apps/api/path", () => ({
  homeDir: vi.fn(() => Promise.resolve("/Users/tester")),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
}));

const WORKSPACES = [
  {
    id: "ws-1",
    path: "/Users/tester/code/doce",
    displayName: "doce",
    createdAt: 1,
    lastOpenedAt: 20,
  },
  {
    id: "ws-2",
    path: "/Users/tester/code/other",
    displayName: "other",
    createdAt: 1,
    lastOpenedAt: 10,
  },
];
const WORKSPACES_WITH_HOME = [
  { id: "ws-0", path: "/Users/tester", displayName: "gimenes", createdAt: 1, lastOpenedAt: 30 },
  ...WORKSPACES,
];
const LIVE_SEARCH_RESULTS = {
  folders: [{ path: "/Users/tester/code/doce", displayName: "doce" }],
  truncated: false,
};
const HOME_PATH_SEARCH_RESULT = {
  folders: [{ path: "/Users/tester/code/folder 1", displayName: "folder 1" }],
  truncated: false,
};

describe("FolderPicker (006-chat-empty-state, US2/US3)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders one row per listWorkspaces() result, with the current selection indicated", async () => {
    vi.mocked(commands.listWorkspaces).mockResolvedValue(WORKSPACES);

    render(
      <FolderPicker currentPath="/Users/tester/code/doce" onSelect={vi.fn()} onDismiss={vi.fn()} />,
    );

    await waitFor(() => {
      expect(screen.getAllByTestId("folder-picker-item")).toHaveLength(2);
    });
    expect(screen.getByText("doce")).toBeInTheDocument();
    expect(screen.getByText("other")).toBeInTheDocument();

    expect(screen.getByText("doce").closest('[data-slot="command-item"]')).toHaveAttribute(
      "aria-current",
      "true",
    );
    expect(screen.getByText("other").closest('[data-slot="command-item"]')).toHaveAttribute(
      "aria-current",
      "false",
    );
  });

  it("does not render a Home entry and only shows recents when there are no previously used folders", async () => {
    vi.mocked(commands.listWorkspaces).mockResolvedValue([]);

    render(<FolderPicker currentPath="/Users/tester" onSelect={vi.fn()} onDismiss={vi.fn()} />);

    await waitFor(() => expect(screen.queryByTestId("folder-picker-home")).not.toBeInTheDocument());
    expect(screen.queryAllByTestId("folder-picker-item")).toHaveLength(0);
    expect(screen.queryByText(/error/i)).not.toBeInTheDocument();
  });

  it("does not render the user's home folder as a duplicate folder row", async () => {
    vi.mocked(commands.listWorkspaces).mockResolvedValue(WORKSPACES_WITH_HOME);

    render(
      <FolderPicker currentPath="/Users/tester/code/doce" onSelect={vi.fn()} onDismiss={vi.fn()} />,
    );

    await waitFor(() => {
      expect(screen.getAllByTestId("folder-picker-item")).toHaveLength(2);
    });

    expect(screen.queryByText("gimenes")).not.toBeInTheDocument();
    expect(screen.getByText("doce")).toBeInTheDocument();
    expect(screen.getByText("other")).toBeInTheDocument();
  });

  it("autofocuses the filter field when opened", async () => {
    vi.mocked(commands.listWorkspaces).mockResolvedValue([]);

    render(<FolderPicker currentPath="/Users/tester" onSelect={vi.fn()} onDismiss={vi.fn()} />);
    const filter = await screen.findByTestId("folder-picker-filter");

    expect(filter).toHaveFocus();
  });

  it("typing into the filter field uses live filesystem search", async () => {
    vi.mocked(commands.listWorkspaces).mockResolvedValue(WORKSPACES);
    vi.mocked(commands.searchFolders).mockResolvedValue(LIVE_SEARCH_RESULTS);

    render(<FolderPicker currentPath="/Users/tester" onSelect={vi.fn()} onDismiss={vi.fn()} />);
    await waitFor(() => expect(screen.getAllByTestId("folder-picker-item")).toHaveLength(2));

    await userEvent.type(screen.getByTestId("folder-picker-filter"), "doc");

    await waitFor(() => expect(screen.getAllByTestId("folder-picker-item")).toHaveLength(1));
    expect(screen.getByText("doce")).toBeInTheDocument();
    expect(screen.queryByText("other")).not.toBeInTheDocument();
    expect(vi.mocked(commands.searchFolders)).toHaveBeenCalledWith("doc", 10);
  });

  it("runs path-aware search when query starts with a path prefix", async () => {
    vi.mocked(commands.listWorkspaces).mockResolvedValue([]);
    vi.mocked(commands.searchFolders).mockResolvedValue({
      folders: [{ path: "/Users/tester/code/doce", displayName: "doce" }],
      truncated: false,
    });

    render(<FolderPicker currentPath="/Users/tester" onSelect={vi.fn()} onDismiss={vi.fn()} />);
    await waitFor(() => expect(screen.getByTestId("folder-picker-filter")).toBeInTheDocument());

    await userEvent.type(screen.getByTestId("folder-picker-filter"), "/");

    await waitFor(() => expect(commands.searchFolders).toHaveBeenCalledWith("/", 10));
  });

  it("renders path-style suggestions with the typed path prefix in bold", async () => {
    vi.mocked(commands.listWorkspaces).mockResolvedValue([]);
    vi.mocked(commands.searchFolders).mockResolvedValue(HOME_PATH_SEARCH_RESULT);

    render(<FolderPicker currentPath="/Users/tester" onSelect={vi.fn()} onDismiss={vi.fn()} />);
    await userEvent.type(screen.getByTestId("folder-picker-filter"), "~/code/");

    await waitFor(() => expect(screen.getByText("folder 1")).toBeInTheDocument());
    const item = screen.getByRole("option", { name: /folder 1/i });
    expect(within(item).getByText("~/code/")).toHaveClass("font-semibold");
    expect(item).toHaveTextContent("~/code/folder 1");
  });

  it("uses an exact existing full-path match as the first row", async () => {
    vi.mocked(commands.listWorkspaces).mockResolvedValue([]);
    const onSelect = vi.fn();
    vi.mocked(commands.searchFolders).mockResolvedValue({
      folders: [
        { path: "/Users/tester/code/mesh", displayName: "mesh" },
        { path: "/Users/tester/code/mesh/child", displayName: "child" },
      ],
      truncated: false,
    });

    render(<FolderPicker currentPath="/Users/tester" onSelect={onSelect} onDismiss={vi.fn()} />);
    await userEvent.type(screen.getByTestId("folder-picker-filter"), "~/code/mesh");

    const items = await screen.findAllByTestId("folder-picker-item");
    expect(items[0]).toHaveTextContent("~/code/mesh");
    expect(items[0]).toHaveAttribute("aria-selected", "true");
    await userEvent.keyboard("{Enter}");
    expect(onSelect).toHaveBeenCalledWith({
      kind: "recent",
      path: "/Users/tester/code/mesh",
      displayLabel: "~/code/mesh",
    });
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
      displayLabel: "~/code/doce",
    });
  });

  it("autoselects the first row on Enter while typing", async () => {
    vi.mocked(commands.listWorkspaces).mockResolvedValue(WORKSPACES);
    const onSelect = vi.fn();

    render(
      <FolderPicker
        currentPath="/Users/tester/code/other"
        onSelect={onSelect}
        onDismiss={vi.fn()}
      />,
    );
    await waitFor(() => expect(screen.getAllByTestId("folder-picker-item")).toHaveLength(2));

    await userEvent.type(screen.getByTestId("folder-picker-filter"), "d");

    await waitFor(() =>
      expect(screen.getByText("doce").closest('[data-slot="command-item"]')).toHaveAttribute(
        "aria-selected",
        "true",
      ),
    );
    await userEvent.keyboard("{Enter}");

    expect(onSelect).toHaveBeenCalledWith({
      kind: "recent",
      path: "/Users/tester/code/doce",
      displayLabel: "~/code/doce",
    });
  });

  it("uses arrow keys to move through rows and enter selects the current row", async () => {
    vi.mocked(commands.listWorkspaces).mockResolvedValue(WORKSPACES);
    const onSelect = vi.fn();

    render(
      <FolderPicker
        currentPath="/Users/tester/code/other"
        onSelect={onSelect}
        onDismiss={vi.fn()}
      />,
    );
    await waitFor(() => expect(screen.getAllByTestId("folder-picker-item")).toHaveLength(2));

    await userEvent.click(screen.getByTestId("folder-picker-filter"));
    expect(screen.getByText("doce").closest('[data-slot="command-item"]')).toHaveAttribute(
      "aria-selected",
      "true",
    );

    await userEvent.keyboard("{ArrowDown}");
    expect(screen.getByText("other").closest('[data-slot="command-item"]')).toHaveAttribute(
      "aria-selected",
      "true",
    );

    await userEvent.keyboard("{Enter}");
    expect(onSelect).toHaveBeenCalledWith({
      kind: "recent",
      path: "/Users/tester/code/other",
      displayLabel: "~/code/other",
    });
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
    await waitFor(() => expect(screen.getByTestId("folder-picker-filter")).toBeInTheDocument());

    await userEvent.click(screen.getByTestId("folder-picker-browse"));

    expect(open).toHaveBeenCalledWith({ directory: true });
    await waitFor(() =>
      expect(onSelect).toHaveBeenCalledWith({
        kind: "browsed",
        path: "/Volumes/External/never-opened-before",
        displayLabel: "/Volumes/External/never-opened-before",
      }),
    );
  });

  it("US3: cancelling the native dialog leaves the current target unchanged", async () => {
    vi.mocked(commands.listWorkspaces).mockResolvedValue([]);
    vi.mocked(open).mockResolvedValue(null);
    const onSelect = vi.fn();
    const onDismiss = vi.fn();

    render(<FolderPicker currentPath="/Users/tester" onSelect={onSelect} onDismiss={onDismiss} />);
    await waitFor(() => expect(screen.getByTestId("folder-picker-filter")).toBeInTheDocument());

    await userEvent.click(screen.getByTestId("folder-picker-browse"));

    await waitFor(() => expect(open).toHaveBeenCalled());
    expect(onSelect).not.toHaveBeenCalled();
    expect(onDismiss).not.toHaveBeenCalled();
  });
});
