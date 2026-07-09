import { act, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import StreamingStatus from "./StreamingStatus";

describe("StreamingStatus", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("renders a quiet accessible thinking status with decorative animation", () => {
    vi.useFakeTimers();
    vi.setSystemTime(10_000);

    render(<StreamingStatus startedAt={8_750} />);

    expect(screen.getByRole("status", { name: "Thinking" })).toBeInTheDocument();
    expect(screen.getByTestId("agent-thinking")).toHaveTextContent("Thinking");
    expect(screen.getAllByTestId("agent-thinking-dot")).toHaveLength(3);
    expect(screen.getByTestId("agent-thinking-dots")).toHaveAttribute("aria-hidden", "true");
    expect(screen.getByTestId("agent-thinking-timer")).toHaveTextContent("1.3s");
    expect(screen.getByTestId("agent-thinking-timer")).toHaveClass("tabular-nums");
  });

  it("persists fallback startedAt across unmount/remount within the same session", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(5_000);

    const mountWithFallback = render(<StreamingStatus startedAt={null} />);

    expect(screen.getByTestId("agent-thinking-timer")).toHaveTextContent("0.0s");

    mountWithFallback.unmount();

    vi.setSystemTime(6_400);
    render(<StreamingStatus startedAt={null} />);

    expect(screen.getByTestId("agent-thinking-timer")).toHaveTextContent("1.4s");
  });

  it("ticks across the 9.9s -> 10.0s boundary without changing fixed-width classes", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(9_900);

    render(<StreamingStatus startedAt={9_000} />);

    expect(screen.getByTestId("agent-thinking-timer")).toHaveTextContent("0.9s");
    expect(screen.getByTestId("agent-thinking-timer")).toHaveClass("w-[6ch]");
    expect(screen.getByTestId("agent-thinking-timer")).toHaveClass("shrink-0");

    vi.setSystemTime(9_900);
    await act(async () => {
      vi.advanceTimersByTime(100);
    });

    expect(screen.getByTestId("agent-thinking-timer")).toHaveTextContent("1.0s");
    expect(screen.getByTestId("agent-thinking-timer")).toHaveClass("w-[6ch]");
    expect(screen.getByTestId("agent-thinking-timer")).toHaveClass("shrink-0");
  });
});
