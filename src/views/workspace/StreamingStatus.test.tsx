import { act, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import StreamingStatus from "./StreamingStatus";

describe("StreamingStatus", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("renders a quiet accessible working status with decorative animation", () => {
    vi.useFakeTimers();
    vi.setSystemTime(10_000);

    render(<StreamingStatus startedAt={8_750} />);

    const status = screen.getByRole("status", { name: "Working" });
    const timer = screen.getByTestId("agent-thinking-timer");

    expect(status).toBeInTheDocument();
    expect(status).toHaveTextContent("Working");
    expect(status).not.toContainElement(timer);
    expect(screen.getByTestId("agent-thinking")).toHaveTextContent("Working");
    // The working signal is shimmer-only — no spinner icon at all.
    expect(screen.queryByTestId("agent-thinking-spinner")).not.toBeInTheDocument();
    expect(screen.getByTestId("agent-thinking").querySelector('[data-slot="spinner"]')).toBeNull();
    expect(timer).toHaveTextContent("1.3s");
    expect(timer).toHaveAttribute("aria-live", "off");
    expect(timer).toHaveClass("tabular-nums");
    expect(screen.getByText("Working")).toHaveClass("shimmer");
  });

  it("uses a fresh fallback startedAt on each mount when no timestamp is provided", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(5_000);

    const mountWithFallback = render(<StreamingStatus startedAt={null} />);

    expect(screen.getByTestId("agent-thinking-timer")).toHaveTextContent("0.0s");

    mountWithFallback.unmount();

    vi.setSystemTime(6_400);
    render(<StreamingStatus startedAt={null} />);

    expect(screen.getByTestId("agent-thinking-timer")).toHaveTextContent("0.0s");
  });

  it("ticks across the 0.9s -> 1.0s boundary", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(9_900);

    render(<StreamingStatus startedAt={9_000} />);

    expect(screen.getByTestId("agent-thinking-timer")).toHaveTextContent("0.9s");
    expect(screen.getByTestId("agent-thinking-timer")).toHaveClass("tabular-nums");

    vi.setSystemTime(9_900);
    await act(async () => {
      vi.advanceTimersByTime(100);
    });

    expect(screen.getByTestId("agent-thinking-timer")).toHaveTextContent("1.0s");
    expect(screen.getByTestId("agent-thinking-timer")).toHaveClass("tabular-nums");
  });
});

describe("StreamingStatus turn token accumulator", () => {
  it("renders live in/out totals next to the chron when provided", () => {
    render(<StreamingStatus startedAt={Date.now()} tokens={{ input: 986, output: 78 }} />);
    expect(screen.getByTestId("agent-thinking-tokens")).toHaveTextContent("↑ 986 ↓ 78");
  });

  it("renders no token counter when the caller has no turn yet", () => {
    render(<StreamingStatus startedAt={Date.now()} />);
    expect(screen.queryByTestId("agent-thinking-tokens")).not.toBeInTheDocument();
  });
});

describe("StreamingStatus zero-value hiding", () => {
  it("hides the zero-valued direction", () => {
    render(<StreamingStatus startedAt={Date.now()} tokens={{ input: 42, output: 0 }} />);
    const tokens = screen.getByTestId("agent-thinking-tokens");
    expect(tokens).toHaveTextContent("↑ 42");
    expect(tokens).not.toHaveTextContent("↓");
  });

  it("renders nothing when both directions are zero", () => {
    render(<StreamingStatus startedAt={Date.now()} tokens={{ input: 0, output: 0 }} />);
    expect(screen.queryByTestId("agent-thinking-tokens")).not.toBeInTheDocument();
  });
});

describe("StreamingStatus live reasoning line", () => {
  it("shows the latest think line beside Working, advancing line by line", () => {
    const { rerender } = render(
      <StreamingStatus startedAt={Date.now()} stream={"<think>\nfirst I read the file"} />,
    );
    // Working keeps the shimmer; the reasoning rides the same row.
    expect(screen.getByTestId("agent-thinking-status")).toHaveTextContent("Working");
    expect(screen.getByTestId("agent-thinking-stream")).toHaveTextContent("first I read the file");

    rerender(
      <StreamingStatus
        startedAt={Date.now()}
        stream={"<think>\nfirst I read the file\nnow comparing the schemas"}
      />,
    );
    expect(screen.getByTestId("agent-thinking-stream")).toHaveTextContent(
      "now comparing the schemas",
    );
  });

  it("shows no reasoning segment before thinking starts or after it closes", () => {
    const { rerender } = render(<StreamingStatus startedAt={Date.now()} stream="" />);
    expect(screen.queryByTestId("agent-thinking-stream")).not.toBeInTheDocument();

    // After </think> the stream is the tool-call tail — grammar syntax,
    // not reasoning — so the segment disappears.
    rerender(
      <StreamingStatus
        startedAt={Date.now()}
        stream={'<think>\ndone reasoning\n</think><tool_call>{"name"'}
      />,
    );
    expect(screen.queryByTestId("agent-thinking-stream")).not.toBeInTheDocument();
  });
});

describe("StreamingStatus reasoning filter vs tool-call syntax", () => {
  it("never renders grammar-forced call syntax as thinking", () => {
    // A no-think generation goes straight into the call.
    render(
      <StreamingStatus
        startedAt={Date.now()}
        stream={'<function name="FinishTask"><param name="answer">Meu nome'}
      />,
    );
    expect(screen.queryByTestId("agent-thinking-stream")).not.toBeInTheDocument();
  });

  it("suppresses a partially-sampled marker instead of flickering it", () => {
    render(<StreamingStatus startedAt={Date.now()} stream={"<fun"} />);
    expect(screen.queryByTestId("agent-thinking-stream")).not.toBeInTheDocument();
  });
});

describe("StreamingStatus shows the model verbatim", () => {
  it("renders even degenerate reasoning lines — the ticker is a window, not a censor", () => {
    render(<StreamingStatus startedAt={Date.now()} stream={"******"} />);
    expect(screen.getByTestId("agent-thinking-stream")).toHaveTextContent("******");
  });
});
