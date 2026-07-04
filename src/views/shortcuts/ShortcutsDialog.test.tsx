import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import ShortcutsDialog from "./ShortcutsDialog";
import type { Shortcut } from "@/lib/shortcuts";

const SHORTCUTS: Shortcut[] = [
  {
    id: "focus-input",
    combo: "⌘L",
    metaKey: true,
    key: "l",
    description: "Focus the message input",
    action: vi.fn(),
  },
  {
    id: "new-conversation",
    combo: "⌘N",
    metaKey: true,
    key: "n",
    description: "Start a new conversation",
    action: vi.fn(),
  },
  {
    id: "show-shortcuts",
    combo: "⌘K",
    metaKey: true,
    key: "k",
    description: "Show keyboard shortcuts",
    action: vi.fn(),
  },
];

describe("ShortcutsDialog", () => {
  it("renders one row per entry in the shared shortcuts registry (FR-010)", () => {
    render(<ShortcutsDialog open={true} onClose={vi.fn()} shortcuts={SHORTCUTS} />);

    const rows = screen.getAllByTestId("shortcut-item");
    expect(rows).toHaveLength(3);
    expect(screen.getByText("Focus the message input")).toBeInTheDocument();
    expect(screen.getByText("⌘L")).toBeInTheDocument();
    expect(screen.getByText("Start a new conversation")).toBeInTheDocument();
    expect(screen.getByText("⌘N")).toBeInTheDocument();
    expect(screen.getByText("Show keyboard shortcuts")).toBeInTheDocument();
    expect(screen.getByText("⌘K")).toBeInTheDocument();
  });

  it("calling the close button invokes onClose", async () => {
    const onClose = vi.fn();
    render(<ShortcutsDialog open={true} onClose={onClose} shortcuts={SHORTCUTS} />);

    await userEvent.click(screen.getByTestId("close-shortcuts-dialog"));
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
