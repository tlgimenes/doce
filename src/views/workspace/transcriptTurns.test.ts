import { describe, expect, it } from "vitest";
import type { Message } from "@/lib/ipc";
import { accumulateTurnTokens, groupTranscriptTurns } from "./transcriptTurns";

function message({ id, ...overrides }: Partial<Message> & { id: string }): Message {
  return {
    id,
    conversationId: "conv-1",
    role: "assistant",
    contentType: "text",
    content: id,
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

  it("keeps each assistant-only row before the first user message in its own standalone turn", () => {
    const a0 = message({ id: "a0", role: "assistant", content: "welcome" });
    const a1 = message({ id: "a1", role: "assistant", content: "still waiting" });
    const u1 = message({ id: "u1", role: "user", content: "request" });

    const turns = groupTranscriptTurns([a0, a1, u1]);

    expect(turns).toEqual([
      { id: "a0", user: null, rows: [a0] },
      { id: "a1", user: null, rows: [a1] },
      { id: "u1", user: u1, rows: [] },
    ]);
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

describe("accumulateTurnTokens", () => {
  it("sums the user prompt and tool-result counts as input, assistant text as output", () => {
    const turn = groupTranscriptTurns([
      message({ id: "u1", role: "user", content: "which folders?", tokenCount: 42 }),
      message({
        id: "tc1",
        contentType: "tool_call",
        toolName: "Glob",
        content: JSON.stringify({ arguments: { pattern: "*" } }),
      }),
      message({
        id: "tr1",
        role: "tool",
        contentType: "tool_result",
        toolName: "Glob",
        content: JSON.stringify({ toolName: "Glob", pattern: "*", matches: [], tokenCount: 986 }),
      }),
      message({ id: "a1", content: "here they are", tokenCount: 78 }),
    ])[0];

    expect(accumulateTurnTokens(turn)).toEqual({ input: 42 + 986, output: 78 });
  });

  it("treats missing counts and unparseable results as zero", () => {
    const turn = groupTranscriptTurns([
      message({ id: "u1", role: "user", content: "hi", tokenCount: null }),
      message({
        id: "tr1",
        role: "tool",
        contentType: "tool_result",
        toolName: "Weird",
        content: "not json",
      }),
      message({ id: "a1", content: "answer", tokenCount: null }),
    ])[0];

    expect(accumulateTurnTokens(turn)).toEqual({ input: 0, output: 0 });
  });

  it("accumulates across multiple tool results and replies mid-turn", () => {
    const turn = groupTranscriptTurns([
      message({ id: "u1", role: "user", content: "go", tokenCount: 10 }),
      message({
        id: "tr1",
        role: "tool",
        contentType: "tool_result",
        toolName: "Read",
        content: JSON.stringify({ toolName: "Read", tokenCount: 100 }),
      }),
      message({
        id: "tr2",
        role: "tool",
        contentType: "tool_result",
        toolName: "Grep",
        content: JSON.stringify({ toolName: "Grep", tokenCount: 200 }),
      }),
      message({ id: "a1", content: "partial", tokenCount: 5 }),
      message({ id: "a2", content: "final", tokenCount: 20 }),
    ])[0];

    expect(accumulateTurnTokens(turn)).toEqual({ input: 310, output: 25 });
  });
});
