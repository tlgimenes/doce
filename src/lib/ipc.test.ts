import { describe, it, expect } from "vitest";
import { parsePendingBashCallDetail, parsePendingTaskCallDetail } from "./ipc";

describe("parsePendingBashCallDetail", () => {
  it("parses a pending Bash tool_call row's arguments into an outcome-less BashDetail", () => {
    const content = JSON.stringify({ arguments: { command: "cargo test", timeoutMs: 5000 } });
    const detail = parsePendingBashCallDetail(content);
    expect(detail).toEqual({
      toolName: "Bash",
      command: "cargo test",
      timeoutMs: 5000,
    });
  });

  it("defaults timeoutMs to null when absent", () => {
    const content = JSON.stringify({ arguments: { command: "ls" } });
    const detail = parsePendingBashCallDetail(content);
    expect(detail?.timeoutMs).toBeNull();
  });

  it("returns null when command is missing", () => {
    const content = JSON.stringify({ arguments: {} });
    expect(parsePendingBashCallDetail(content)).toBeNull();
  });

  it("returns null on malformed JSON", () => {
    expect(parsePendingBashCallDetail("not json")).toBeNull();
  });
});

describe("parsePendingTaskCallDetail", () => {
  it("parses a pending Task tool_call row's arguments into a running TaskDetail", () => {
    const content = JSON.stringify({ arguments: { prompt: "go investigate the bug" } });
    const detail = parsePendingTaskCallDetail(content);
    expect(detail).toEqual({
      toolName: "Task",
      prompt: "go investigate the bug",
      subagentConversationId: "",
      state: "running",
    });
  });

  it("returns null when prompt is missing", () => {
    const content = JSON.stringify({ arguments: {} });
    expect(parsePendingTaskCallDetail(content)).toBeNull();
  });

  it("returns null on malformed JSON", () => {
    expect(parsePendingTaskCallDetail("not json")).toBeNull();
  });
});
