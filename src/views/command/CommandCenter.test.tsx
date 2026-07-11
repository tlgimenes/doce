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
    // The Command root element has label="Command search", which cmdk wires as
    // the aria-labelledby reference for the input, making the accessible name
    // "Command search".
    const commandInput = screen.getByRole("combobox", { name: "Command search" });
    expect(commandInput).toBeInTheDocument();
    expect(
      screen.getByTestId("command-center").querySelector('[data-slot="command"]'),
    ).toBeTruthy();
    expect(
      screen.getByTestId("command-center").querySelector('[data-slot="command-input"]'),
    ).toBeTruthy();
    expect(
      screen.getByTestId("command-center").querySelector('[data-slot="command-list"]'),
    ).toBeTruthy();
    expect(
      screen.getByTestId("command-center").querySelectorAll('[data-slot="command-item"]'),
    ).toHaveLength(3);
    // cmdk renders items as role="option" (not "button") with aria-disabled
    // reflecting the `disabled` prop — there's no native disabled attribute
    // to assert with toBeDisabled()/toBeEnabled() since the element is a div.
    expect(screen.getByRole("option", { name: /New Agent/ })).toHaveAttribute(
      "aria-disabled",
      "false",
    );
    expect(screen.getByRole("option", { name: /Archive Current Conversation/ })).toHaveAttribute(
      "aria-disabled",
      "true",
    );
  });

  it("sizes the dialog shell to fit the command center without horizontal clipping", async () => {
    render(<CommandCenter open={true} onOpenChange={vi.fn()} actions={actions} />);

    expect(await screen.findByTestId("app-dialog-content")).toHaveClass("w-[34rem]");
    expect(screen.getByTestId("command-center")).toHaveClass("w-full");
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

    await userEvent.click(screen.getByRole("option", { name: /Open Settings/ }));

    expect(run).toHaveBeenCalledTimes(1);
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("filters actions from the command input", async () => {
    render(<CommandCenter open={true} onOpenChange={vi.fn()} actions={actions} />);

    await userEvent.type(screen.getByRole("combobox", { name: "Command search" }), "archive");

    expect(
      screen.getByRole("option", { name: /Archive Current Conversation/ }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("option", { name: /New Agent/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("option", { name: /Search Conversations/ })).not.toBeInTheDocument();
  });

  it("runs the default-highlighted action with Enter from the command input", async () => {
    // cmdk highlights the first enabled item on mount (aria-selected="true")
    // with no explicit focus/selection step needed — items aren't
    // individually focusable the way the old hand-rolled buttons were, so
    // this replaces the previous "focus the item, then Enter" case.
    const onOpenChange = vi.fn();
    const run = vi.fn();

    render(
      <CommandCenter
        open={true}
        onOpenChange={onOpenChange}
        actions={[{ id: "settings", label: "Open Settings", run }]}
      />,
    );

    expect(screen.getByRole("option", { name: /Open Settings/ })).toHaveAttribute(
      "aria-selected",
      "true",
    );

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

    const input = screen.getByRole("combobox", { name: "Command search" });
    await userEvent.type(input, "settings");
    await userEvent.keyboard("{Enter}");

    expect(settingsRun).toHaveBeenCalledTimes(1);
    expect(searchRun).not.toHaveBeenCalled();
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("does not run or close when Enter is pressed from the command input and only disabled actions match", async () => {
    // cmdk excludes aria-disabled items from its selectable set entirely, so
    // a disabled-only filtered list ends up with nothing aria-selected and
    // Enter is a no-op — verified directly against cmdk's source (the `ce`
    // selector used for both auto-select-on-filter and Enter's lookup is
    // `[cmdk-item]:not([aria-disabled="true"])`).
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

    const input = screen.getByRole("combobox", { name: "Command search" });
    await userEvent.type(input, "archive");

    expect(
      screen.getByRole("option", { name: /Archive Current Conversation/ }),
    ).not.toHaveAttribute("aria-selected", "true");

    await userEvent.keyboard("{Enter}");

    expect(archiveRun).not.toHaveBeenCalled();
    expect(onOpenChange).not.toHaveBeenCalledWith(false);
  });

  it("resets the command query after the dialog closes and reopens", async () => {
    const { rerender } = render(
      <CommandCenter open={true} onOpenChange={vi.fn()} actions={actions} />,
    );

    const input = screen.getByRole("combobox", { name: "Command search" });
    await userEvent.type(input, "search");

    expect(screen.getByRole("option", { name: /Search Conversations/ })).toBeInTheDocument();
    expect(screen.queryByRole("option", { name: /New Agent/ })).not.toBeInTheDocument();

    rerender(<CommandCenter open={false} onOpenChange={vi.fn()} actions={actions} />);
    rerender(<CommandCenter open={true} onOpenChange={vi.fn()} actions={actions} />);

    expect(screen.getByRole("combobox", { name: "Command search" })).toHaveValue("");
    expect(screen.getByRole("option", { name: /New Agent/ })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: /Search Conversations/ })).toBeInTheDocument();
  });
});
