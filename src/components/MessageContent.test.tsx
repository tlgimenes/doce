import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import MessageContent from "./MessageContent";
import type { Message } from "@/lib/ipc";

function baseMessage(overrides: Partial<Message>): Message {
  return {
    id: "m1",
    conversationId: "c1",
    role: "assistant",
    contentType: "text",
    content: "hello",
    toolName: null,
    createdAt: 1000,
    durationMs: 500,
    ...overrides,
  };
}

describe("MessageContent (004-tool-call-widgets, Foundational)", () => {
  it("renders a user message as a plain markdown bubble", () => {
    render(<MessageContent message={baseMessage({ role: "user", content: "hi there" })} />);
    expect(screen.getByText("hi there")).toBeInTheDocument();
  });

  it("renders a text assistant message as markdown, with Timer only when showTimer is true", () => {
    const { rerender } = render(
      <MessageContent message={baseMessage({ contentType: "text", content: "the answer" })} />,
    );
    expect(screen.getByText("the answer")).toBeInTheDocument();
    expect(screen.queryByText(/0\.5s/)).not.toBeInTheDocument();

    rerender(
      <MessageContent
        message={baseMessage({ contentType: "text", content: "the answer" })}
        showTimer
      />,
    );
    expect(screen.getByText("0.5s")).toBeInTheDocument();
  });

  it("renders an error message distinctly", () => {
    render(<MessageContent message={baseMessage({ contentType: "error", content: "boom" })} />);
    expect(screen.getByText("boom")).toBeInTheDocument();
  });

  it("renders nothing for a tool_call row (paired tool_result carries the widget)", () => {
    const { container } = render(
      <MessageContent
        message={baseMessage({ contentType: "tool_call", toolName: "Bash", content: "{}" })}
      />,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("renders the fallback widget for a tool_result whose toolName has no dedicated widget (e.g. an MCP-provided tool)", () => {
    const detail = {
      toolName: "SomeMcpTool",
      arguments: { input: "x" },
      outcome: { ok: true, text: "did the thing" },
    };
    render(
      <MessageContent
        message={baseMessage({
          contentType: "tool_result",
          toolName: "SomeMcpTool",
          content: JSON.stringify(detail),
        })}
      />,
    );
    expect(screen.getByTestId("unknown-tool-widget")).toBeInTheDocument();
    expect(screen.getByText("SomeMcpTool")).toBeInTheDocument();
  });

  it("degrades to the fallback widget on unparseable tool_result content rather than crashing", () => {
    render(
      <MessageContent
        message={baseMessage({
          contentType: "tool_result",
          toolName: "SomeMcpTool",
          content: "not valid json",
        })}
      />,
    );
    expect(screen.getByTestId("unknown-tool-widget")).toBeInTheDocument();
  });
});
