import { describe, it, expect, beforeEach } from "vitest";
import { useContextUsageStore } from "./contextUsageStore";

describe("contextUsageStore", () => {
  beforeEach(() => {
    useContextUsageStore.setState({ usage: {} });
  });

  it("setUsage keys by conversationId, keeping other conversations' usage intact", () => {
    const { setUsage } = useContextUsageStore.getState();

    setUsage({ conversationId: "c1", tokensUsed: 100, tokenBudget: 2048, state: "normal" });
    setUsage({ conversationId: "c2", tokensUsed: 1200, tokenBudget: 2048, state: "warning" });

    const { usage } = useContextUsageStore.getState();
    expect(usage.c1).toEqual({
      conversationId: "c1",
      tokensUsed: 100,
      tokenBudget: 2048,
      state: "normal",
    });
    expect(usage.c2.state).toBe("warning");
  });

  it("a later setUsage for the same conversation overwrites the earlier one", () => {
    const { setUsage } = useContextUsageStore.getState();

    setUsage({ conversationId: "c1", tokensUsed: 100, tokenBudget: 2048, state: "normal" });
    setUsage({ conversationId: "c1", tokensUsed: 1900, tokenBudget: 2048, state: "justCompacted" });

    expect(useContextUsageStore.getState().usage.c1.state).toBe("justCompacted");
    expect(useContextUsageStore.getState().usage.c1.tokensUsed).toBe(1900);
  });
});
