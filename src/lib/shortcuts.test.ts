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
