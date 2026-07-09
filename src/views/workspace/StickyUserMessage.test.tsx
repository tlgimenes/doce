import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { Message } from "@/lib/ipc";
import StickyUserMessage from "./StickyUserMessage";

function userMessage(content = "line one\n\nline two"): Message {
  return {
    id: "u1",
    conversationId: "conv-1",
    role: "user",
    contentType: "text",
    content,
    toolName: null,
    createdAt: 1,
    durationMs: null,
    tokenCount: 12,
  };
}

describe("StickyUserMessage", () => {
  it("renders as a sticky chat message with a clipped user bubble by default", () => {
    render(<StickyUserMessage message={userMessage()} />);

    const shell = screen.getByTestId("chat-message");
    const focusTarget = screen.getByTestId("sticky-user-message-bubble");
    const bubble = screen.getByTestId("user-message-bubble");

    expect(shell).toHaveAttribute("data-sticky-user-message", "true");
    expect(shell).toHaveClass("sticky", "top-4", "z-40");
    expect(shell).toHaveAttribute("aria-label", "You said");
    expect(focusTarget).toHaveAttribute("tabindex", "0");
    expect(bubble).toHaveClass("max-h-[84px]", "overflow-hidden");
    expect(bubble.className).toContain("[mask-image:linear-gradient");
    expect(screen.getByTestId("token-meter")).toHaveTextContent("↑ 12 tokens");
  });

  it("expands on focus and calls onScrollToTurn", () => {
    const onScrollToTurn = vi.fn();
    render(<StickyUserMessage message={userMessage()} onScrollToTurn={onScrollToTurn} />);

    const focusTarget = screen.getByTestId("sticky-user-message-bubble");

    fireEvent.focus(focusTarget);

    expect(onScrollToTurn).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId("user-message-bubble")).toHaveClass("max-h-[50vh]", "overflow-auto");
  });

  it("calls onScrollToTurn once per pointer click, including an already-focused target", async () => {
    const onScrollToTurn = vi.fn();
    render(<StickyUserMessage message={userMessage()} onScrollToTurn={onScrollToTurn} />);

    const user = userEvent.setup();
    const focusTarget = screen.getByTestId("sticky-user-message-bubble");

    await user.click(focusTarget);
    expect(onScrollToTurn).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId("user-message-bubble")).toHaveClass("max-h-[50vh]", "overflow-auto");

    onScrollToTurn.mockClear();
    await user.click(focusTarget);

    expect(onScrollToTurn).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId("user-message-bubble")).toHaveClass("max-h-[50vh]", "overflow-auto");
  });

  it("treats touch pointer activation as one scroll request", () => {
    const onScrollToTurn = vi.fn();
    render(<StickyUserMessage message={userMessage()} onScrollToTurn={onScrollToTurn} />);

    const focusTarget = screen.getByTestId("sticky-user-message-bubble");

    fireEvent.pointerDown(focusTarget, { pointerType: "touch" });
    fireEvent.focus(focusTarget);
    fireEvent.click(focusTarget);

    expect(onScrollToTurn).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId("user-message-bubble")).toHaveClass("max-h-[50vh]", "overflow-auto");
  });

  it("collapses when focus leaves the sticky component", () => {
    render(<StickyUserMessage message={userMessage()} />);

    const focusTarget = screen.getByTestId("sticky-user-message-bubble");

    fireEvent.focus(focusTarget);
    fireEvent.blur(focusTarget, { relatedTarget: document.createElement("button") });

    expect(screen.getByTestId("user-message-bubble")).toHaveClass(
      "max-h-[84px]",
      "overflow-hidden",
    );
  });
});
