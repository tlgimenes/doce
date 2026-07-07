import { describe, expect, it } from "vitest";
import {
  formatConversationRelativeTime,
  formatWorkspacePathLabel,
  getConversationWorkspaceLabel,
  getConversationWorkStateLabel,
} from "./sidebarConversationRow";

describe("sidebarConversationRow", () => {
  it("formats compact relative update times", () => {
    const now = 1_800_000_000_000;

    expect(formatConversationRelativeTime(now - 30_000, now)).toBe("now");
    expect(formatConversationRelativeTime(now - 2 * 60_000, now)).toBe("2m");
    expect(formatConversationRelativeTime(now - 3 * 60 * 60_000, now)).toBe("3h");
    expect(formatConversationRelativeTime(now - 4 * 24 * 60 * 60_000, now)).toBe("4d");
    expect(formatConversationRelativeTime(now - 40 * 24 * 60 * 60_000, now)).toBe("1mo");
    expect(formatConversationRelativeTime(now - 2 * 365 * 24 * 60 * 60_000, now)).toBe("2y");
  });

  it("formats workspace paths with Home and tilde labels", () => {
    expect(formatWorkspacePathLabel(null, "/Users/tester")).toBe("Home");
    expect(formatWorkspacePathLabel("/Users/tester", "/Users/tester")).toBe("Home");
    expect(formatWorkspacePathLabel("/Users/tester/", "/Users/tester")).toBe("Home");
    expect(formatWorkspacePathLabel("/Users/tester/code/doce", "/Users/tester")).toBe(
      "~/code/doce",
    );
    expect(formatWorkspacePathLabel("/Volumes/projects/doce", "/Users/tester")).toBe(
      "/Volumes/projects/doce",
    );
  });

  it("uses Home while a workspace cannot be resolved by id", () => {
    const workspaces = new Map([
      [
        "ws-code",
        {
          path: "/Users/tester/code/doce",
        },
      ],
    ]);

    expect(getConversationWorkspaceLabel(null, workspaces, "/Users/tester")).toBe("Home");
    expect(getConversationWorkspaceLabel("missing", workspaces, "/Users/tester")).toBe("Home");
    expect(getConversationWorkspaceLabel("ws-code", workspaces, null)).toBe("Home");
    expect(getConversationWorkspaceLabel("ws-code", workspaces, "/Users/tester")).toBe(
      "~/code/doce",
    );
  });

  it("maps technical statuses to product-facing work states", () => {
    expect(getConversationWorkStateLabel("in_progress")).toBe("Working");
    expect(getConversationWorkStateLabel("requires_action")).toBe("Review");
    expect(getConversationWorkStateLabel("failed")).toBe("Blocked");
    expect(getConversationWorkStateLabel("done")).toBe("Ready");
  });
});
