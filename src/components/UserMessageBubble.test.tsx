import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { Message } from "@/lib/ipc";
import UserMessageBubble from "./UserMessageBubble";

function userMessage(overrides: Partial<Message> = {}): Message {
  return {
    id: "u1",
    conversationId: "conv-1",
    role: "user",
    contentType: "text",
    content: "hello **there**",
    toolName: null,
    createdAt: 1,
    durationMs: null,
    tokenCount: null,
    ...overrides,
  };
}

describe("UserMessageBubble", () => {
  it("renders text user content through the markdown bubble", () => {
    render(<UserMessageBubble message={userMessage()} />);

    const bubble = screen.getByTestId("user-message-bubble");
    expect(bubble).toHaveTextContent("hello");
    expect(bubble).toHaveTextContent("there");
    expect(bubble).toHaveClass(
      "ml-auto",
      "max-w-[85%]",
      "rounded-md",
      "border",
      "border-border",
      "bg-[var(--color-doce-cream)]",
      "p-3",
      "text-sm",
      "text-foreground",
      "shadow-sm",
    );
  });

  it("applies caller classes to the visual bubble without moving the token meter", () => {
    render(
      <UserMessageBubble
        message={userMessage({ tokenCount: 4200 })}
        bubbleClassName="max-h-[84px] overflow-hidden"
      />,
    );

    expect(screen.getByTestId("user-message-bubble")).toHaveClass(
      "max-h-[84px]",
      "overflow-hidden",
    );
    expect(screen.getByTestId("token-meter")).toHaveTextContent("↑ 4.2k tokens");
    expect(screen.getByTestId("token-meter")).not.toHaveClass("max-h-[84px]");
  });

  it("renders rich user content with the same bubble test id", () => {
    render(
      <UserMessageBubble
        message={userMessage({
          contentType: "rich_text",
          content: JSON.stringify({
            segments: [{ type: "text", text: "rich hello" }],
          }),
        })}
      />,
    );

    expect(screen.getByTestId("user-message-bubble")).toHaveTextContent("rich hello");
  });
});
