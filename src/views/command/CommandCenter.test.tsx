import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import CommandCenter, { type CommandCenterAction } from "./CommandCenter";

const actions: CommandCenterAction[] = [
  { id: "new-agent", label: "New Agent", shortcut: "Cmd+N", run: vi.fn() },
  { id: "search", label: "Search Conversations", shortcut: "Cmd+F", run: vi.fn() },
  { id: "archive", label: "Archive Current Conversation", run: vi.fn(), disabled: true },
];

describe("CommandCenter", () => {
  it("renders enabled and disabled actions", () => {
    render(<CommandCenter open={true} onOpenChange={vi.fn()} actions={actions} />);

    expect(screen.getByTestId("command-center")).toBeInTheDocument();
    expect(screen.getByRole("dialog", { name: "Command center" })).toBeInTheDocument();
    expect(screen.getByTestId("command-center").querySelector('[data-slot="command"]')).toBeTruthy();
    expect(screen.getByTestId("command-center").querySelector('[data-slot="command-input"]')).toBeTruthy();
    expect(screen.getByTestId("command-center").querySelector('[data-slot="command-list"]')).toBeTruthy();
    expect(screen.getByTestId("command-center").querySelectorAll('[data-slot="command-item"]')).toHaveLength(3);
    expect(screen.getByRole("button", { name: /New Agent/ })).toBeEnabled();
    expect(screen.getByRole("button", { name: /Archive Current Conversation/ })).toBeDisabled();
  });

  it("runs an enabled action and closes", async () => {
    const onOpenChange = vi.fn();
    const run = vi.fn();

    render(
      <CommandCenter
        open={true}
        onOpenChange={onOpenChange}
        actions={[{ id: "settings", label: "Open Settings", run }]}
      />,
    );

    await userEvent.click(screen.getByRole("button", { name: /Open Settings/ }));

    expect(run).toHaveBeenCalledTimes(1);
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("filters actions from the command input", async () => {
    render(<CommandCenter open={true} onOpenChange={vi.fn()} actions={actions} />);

    await userEvent.type(screen.getByPlaceholderText("Type a command or search"), "archive");

    expect(screen.getByRole("button", { name: /Archive Current Conversation/ })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /New Agent/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Search Conversations/ })).not.toBeInTheDocument();
  });

  it("runs the focused command item with Enter and closes", async () => {
    const onOpenChange = vi.fn();
    const run = vi.fn();

    render(
      <CommandCenter
        open={true}
        onOpenChange={onOpenChange}
        actions={[{ id: "settings", label: "Open Settings", run }]}
      />,
    );

    const action = screen.getByRole("button", { name: /Open Settings/ });
    action.focus();
    expect(action).toHaveFocus();

    await userEvent.keyboard("{Enter}");

    expect(run).toHaveBeenCalledTimes(1);
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("runs the first enabled visible action when Enter is pressed from the command input", async () => {
    const onOpenChange = vi.fn();
    const settingsRun = vi.fn();
    const searchRun = vi.fn();

    render(
      <CommandCenter
        open={true}
        onOpenChange={onOpenChange}
        actions={[
          { id: "search", label: "Search Conversations", run: searchRun },
          { id: "settings", label: "Open Settings", run: settingsRun },
        ]}
      />,
    );

    const input = screen.getByPlaceholderText("Type a command or search");
    await userEvent.type(input, "settings");
    await userEvent.keyboard("{Enter}");

    expect(settingsRun).toHaveBeenCalledTimes(1);
    expect(searchRun).not.toHaveBeenCalled();
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("does not run or close when Enter is pressed from the command input and only disabled actions match", async () => {
    const onOpenChange = vi.fn();
    const archiveRun = vi.fn();

    render(
      <CommandCenter
        open={true}
        onOpenChange={onOpenChange}
        actions={[
          {
            id: "archive",
            label: "Archive Current Conversation",
            run: archiveRun,
            disabled: true,
          },
        ]}
      />,
    );

    const input = screen.getByPlaceholderText("Type a command or search");
    await userEvent.type(input, "archive");
    await userEvent.keyboard("{Enter}");

    expect(archiveRun).not.toHaveBeenCalled();
    expect(onOpenChange).not.toHaveBeenCalledWith(false);
  });
});
