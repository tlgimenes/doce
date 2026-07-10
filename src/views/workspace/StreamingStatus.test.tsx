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
    const spinner = screen.getByTestId("agent-thinking-spinner");
    expect(spinner).toHaveAttribute("aria-hidden", "true");
    const spinnerIcon = spinner.querySelector('[data-slot="spinner"]');
    expect(spinnerIcon).not.toBeNull();
    expect(spinnerIcon).toHaveAttribute("role", "presentation");
    expect(spinnerIcon).not.toHaveAttribute("aria-label");
    expect(timer).toHaveTextContent("1.3s");
    expect(timer).toHaveAttribute("aria-live", "off");
    expect(timer).toHaveClass("tabular-nums");
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
