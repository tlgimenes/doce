import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import ContextUsageGauge from "./ContextUsageGauge";
import { commands } from "@/lib/ipc";
import { useContextUsageStore } from "@/state/contextUsageStore";

vi.mock("@/lib/ipc", () => ({
  commands: {
    getContextUsage: vi.fn(),
  },
}));

describe("ContextUsageGauge (010-context-window-management, UI refactor)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useContextUsageStore.setState({ usage: {} });
  });

  it("renders nothing until usage has loaded", async () => {
    vi.mocked(commands.getContextUsage).mockReturnValue(new Promise(() => {}));
    render(<ContextUsageGauge conversationId="c1" />);
    expect(screen.queryByTestId("context-usage-gauge")).not.toBeInTheDocument();
  });

  it("fetches usage on mount and shows the percentage in its tooltip/aria-label", async () => {
    vi.mocked(commands.getContextUsage).mockResolvedValue({
      conversationId: "c1",
      tokensUsed: 200,
      tokenBudget: 2048,
      state: "normal",
    });

    render(<ContextUsageGauge conversationId="c1" />);

    await waitFor(() => expect(commands.getContextUsage).toHaveBeenCalledWith("c1"));
    const gauge = await screen.findByTestId("context-usage-gauge");
    expect(gauge).toHaveAttribute("aria-label", expect.stringContaining("10%"));
    expect(screen.getByTestId("context-usage-tooltip")).toHaveTextContent("10% of context used");
  });

  it("is not a clickable button — compaction is triggered via the /compact command instead", async () => {
    vi.mocked(commands.getContextUsage).mockResolvedValue({
      conversationId: "c1",
      tokensUsed: 200,
      tokenBudget: 2048,
      state: "normal",
    });
    render(<ContextUsageGauge conversationId="c1" />);
    await screen.findByTestId("context-usage-gauge");
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });

  it("mentions 'just compacted' in the tooltip when in that state", async () => {
    vi.mocked(commands.getContextUsage).mockResolvedValue({
      conversationId: "c1",
      tokensUsed: 300,
      tokenBudget: 2048,
      state: "justCompacted",
    });
    render(<ContextUsageGauge conversationId="c1" />);
    expect(await screen.findByTestId("context-usage-tooltip")).toHaveTextContent(
      "just compacted",
    );
  });

  it("silently swallows a getContextUsage failure (e.g. no model loaded yet)", async () => {
    vi.mocked(commands.getContextUsage).mockRejectedValue(new Error("No model loaded"));
    render(<ContextUsageGauge conversationId="c1" />);
    await waitFor(() => expect(commands.getContextUsage).toHaveBeenCalled());
    expect(screen.queryByTestId("context-usage-gauge")).not.toBeInTheDocument();
  });
});
