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

  it("ticks from the provided start timestamp without changing layout width classes", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(10_000);

    render(<StreamingStatus startedAt={9_000} />);

    expect(screen.getByTestId("agent-thinking-timer")).toHaveTextContent("1.0s");

    vi.setSystemTime(12_300);
    await act(async () => {
      vi.advanceTimersByTime(100);
    });

    expect(screen.getByTestId("agent-thinking-timer")).toHaveTextContent("3.4s");
    expect(screen.getByTestId("agent-thinking-timer")).toHaveClass("min-w-[4.5ch]");
  });

  it("falls back to the mount time when no user-message timestamp is available", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(5_000);

    render(<StreamingStatus startedAt={null} />);

    expect(screen.getByTestId("agent-thinking-timer")).toHaveTextContent("0.0s");

    vi.setSystemTime(5_800);
    await act(async () => {
      vi.advanceTimersByTime(100);
    });

    expect(screen.getByTestId("agent-thinking-timer")).toHaveTextContent("0.9s");
  });
});
