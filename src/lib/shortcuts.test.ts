import { describe, expect, it, vi } from "vitest";
import { buildShortcuts } from "./shortcuts";

function handlers() {
  return {
    focusInput: vi.fn(),
    newConversation: vi.fn(),
    openSearch: vi.fn(),
    openCommandCenter: vi.fn(),
    toggleWidgetGallery: vi.fn(),
    archiveCurrent: vi.fn(),
    toggleGoal: vi.fn(),
  };
}

describe("buildShortcuts", () => {
  it("binds Cmd+K to the command center", () => {
    const h = handlers();
    const shortcut = buildShortcuts(h).find((s) => s.id === "open-command-center");

    expect(shortcut).toMatchObject({
      combo: "Cmd+K",
      metaKey: true,
      key: "k",
      description: "Open command center",
    });

    shortcut?.action();
    expect(h.openCommandCenter).toHaveBeenCalledTimes(1);
  });

  it("binds Cmd+G to toggling goal mode", () => {
    const h = handlers();
    const shortcut = buildShortcuts(h).find((s) => s.id === "toggle-goal");

    expect(shortcut).toMatchObject({
      combo: "Cmd+G",
      metaKey: true,
      key: "g",
      description: "Toggle goal mode",
    });

    shortcut?.action();
    expect(h.toggleGoal).toHaveBeenCalledTimes(1);
  });

  it("keeps Cmd+F dedicated to conversation search", () => {
    const h = handlers();
    const shortcut = buildShortcuts(h).find((s) => s.id === "search-conversations");

    expect(shortcut).toMatchObject({
      combo: "Cmd+F",
      metaKey: true,
      key: "f",
      description: "Search conversations",
    });
  });
});
