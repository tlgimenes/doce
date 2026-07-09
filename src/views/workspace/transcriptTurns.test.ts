import { describe, expect, it } from "vitest";
import type { Message } from "@/lib/ipc";
import { groupTranscriptTurns } from "./transcriptTurns";

function message(overrides: Partial<Message> & { id: string }): Message {
  return {
    id: overrides.id,
    conversationId: "conv-1",
    role: "assistant",
    contentType: "text",
    content: overrides.id,
    toolName: null,
    createdAt: 1,
    durationMs: null,
    tokenCount: null,
    ...overrides,
  };
}

describe("groupTranscriptTurns", () => {
  it("groups each user message with following rows until the next user message", () => {
    const u1 = message({ id: "u1", role: "user", content: "first request" });
    const a1 = message({ id: "a1", role: "assistant", content: "first answer" });
    const tool = message({
      id: "tr1",
      role: "assistant",
      contentType: "tool_result",
      toolName: "Read",
      content: JSON.stringify({
        toolName: "Read",
        filePath: "notes.txt",
        offset: null,
        limit: null,
        outcome: { ok: true, content: "hello", truncated: false },
      }),
    });
    const u2 = message({ id: "u2", role: "user", content: "second request" });
    const a2 = message({ id: "a2", role: "assistant", content: "second answer" });

    const turns = groupTranscriptTurns([u1, a1, tool, u2, a2]);

    expect(turns).toHaveLength(2);
    expect(turns[0]).toEqual({ id: "u1", user: u1, rows: [a1, tool] });
    expect(turns[1]).toEqual({ id: "u2", user: u2, rows: [a2] });
  });

  it("keeps assistant-only rows before the first user message in a standalone turn", () => {
    const intro = message({ id: "a0", role: "assistant", content: "welcome" });
    const u1 = message({ id: "u1", role: "user", content: "request" });
    const a1 = message({ id: "a1", role: "assistant", content: "answer" });

    const turns = groupTranscriptTurns([intro, u1, a1]);

    expect(turns).toHaveLength(2);
    expect(turns[0]).toEqual({ id: "a0", user: null, rows: [intro] });
    expect(turns[1]).toEqual({ id: "u1", user: u1, rows: [a1] });
  });

  it("keeps plan-machine rows in their owning turn so MessageContent can filter them", () => {
    const u1 = message({ id: "u1", role: "user", content: "make a plan" });
    const planTool = message({
      id: "tc1",
      role: "assistant",
      contentType: "tool_call",
      toolName: "CreatePlan",
      content: JSON.stringify({ plan: true }),
    });

    const turns = groupTranscriptTurns([u1, planTool]);

    expect(turns).toEqual([{ id: "u1", user: u1, rows: [planTool] }]);
  });
});
