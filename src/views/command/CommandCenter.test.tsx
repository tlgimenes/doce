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
});
