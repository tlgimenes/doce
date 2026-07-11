import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import TranscriptRow from "./TranscriptRow";
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
    tokenCount: null,
    ...overrides,
  };
}

describe("TranscriptRow (004-tool-call-widgets, Foundational)", () => {
  it("renders a user message as a plain markdown bubble", () => {
    render(<TranscriptRow message={baseMessage({ role: "user", content: "hi there" })} />);

    const row = screen.getByTestId("chat-message");
    const bubble = screen.getByTestId("user-message-bubble");
    expect(row).toHaveAttribute("role", "group");
    expect(row).toHaveAttribute("aria-label", "You said");
    expect(row).toHaveAttribute("data-slot", "message");
    expect(row.querySelector('[data-slot="message-content"]')).not.toBeNull();
    expect(bubble).toHaveAttribute("data-slot", "bubble-content");
    expect(bubble.closest('[data-slot="bubble"]')).not.toBeNull();
    expect(bubble).toHaveTextContent("hi there");
  });

  it("renders a live assistant timer only when showTimer is true and no persisted duration exists", () => {
    const { rerender } = render(
      <TranscriptRow
        message={baseMessage({
          contentType: "text",
          content: "the answer",
          durationMs: null,
        })}
      />,
    );
    expect(screen.getByText("the answer")).toBeInTheDocument();
    expect(screen.queryByText(/0\.5s/)).not.toBeInTheDocument();

    rerender(
      <TranscriptRow
        message={baseMessage({
          contentType: "text",
          content: "the answer",
          durationMs: null,
        })}
        showTimer
      />,
    );
    expect(screen.getByTestId("token-meter")).toHaveTextContent(/^\d+\.\d+s$/);
  });

  it("shows assistant duration and tokens together for completed text replies", () => {
    render(
      <TranscriptRow
        message={baseMessage({
          contentType: "text",
          content: "the answer",
          tokenCount: 15600,
        })}
      />,
    );

    expect(screen.getByTestId("token-meter")).toHaveTextContent("0.5s · ↓ 15.6k tokens");
  });

  it("shows only assistant duration when tokens are unavailable for a completed text reply", () => {
    render(<TranscriptRow message={baseMessage({ contentType: "text", content: "the answer" })} />);

    expect(screen.getByTestId("token-meter")).toHaveTextContent("0.5s");
  });

  it("shows only assistant tokens when duration is unavailable", () => {
    render(
      <TranscriptRow
        message={baseMessage({
          contentType: "text",
          content: "the answer",
          durationMs: null,
          tokenCount: 100,
        })}
      />,
    );

    const meter = screen.getByTestId("token-meter");
    expect(meter).toHaveTextContent("↓ 100 tokens");
    expect(meter).not.toHaveTextContent("0.5s");
  });

  it("shows no assistant metadata footer when neither duration nor tokens are available", () => {
    render(
      <TranscriptRow
        message={baseMessage({
          contentType: "text",
          content: "the answer",
          durationMs: null,
          tokenCount: null,
        })}
      />,
    );

    expect(screen.queryByTestId("token-meter")).not.toBeInTheDocument();
  });

  it("continues to render markdown after the markdown renderer is shared", () => {
    render(
      <TranscriptRow
        message={baseMessage({
          contentType: "text",
          content: "## Heading\n\n- one\n- two",
        })}
      />,
    );

    expect(screen.getByRole("heading", { level: 2, name: "Heading" })).toBeInTheDocument();
    expect(screen.getByText("one")).toBeInTheDocument();
    expect(screen.getByText("two")).toBeInTheDocument();
    const row = screen.getByTestId("chat-message");
    expect(row).toHaveAttribute("data-slot", "message");
    expect(row.querySelector('[data-slot="bubble"]')).not.toBeNull();
    expect(row.querySelector('[data-slot="bubble-content"]')).not.toBeNull();
  });

  // --- 010-context-window-management (UI refactor): token meter ---

  it("shows an input-token meter (↑) on a user message when tokenCount is known", () => {
    render(
      <TranscriptRow
        message={baseMessage({
          role: "user",
          content: "hi there",
          tokenCount: 42,
        })}
      />,
    );
    expect(screen.getByTestId("token-meter")).toHaveTextContent("↑ 42 tokens");
  });

  it("keeps the user token meter wired through the top-level TranscriptRow row", () => {
    render(
      <TranscriptRow
        message={baseMessage({
          role: "user",
          content: "hi there",
          tokenCount: 42,
        })}
      />,
    );

    const row = screen.getByTestId("chat-message");
    expect(row).toHaveAttribute("role", "group");
    expect(row).toHaveAttribute("aria-label", "You said");
    expect(screen.getByTestId("token-meter")).toHaveTextContent("↑ 42 tokens");
  });

  it("shows no token meter on a user message when tokenCount is unknown yet", () => {
    render(
      <TranscriptRow
        message={baseMessage({
          role: "user",
          content: "hi there",
          tokenCount: null,
        })}
      />,
    );
    expect(screen.queryByTestId("token-meter")).not.toBeInTheDocument();
  });

  it("renders rich_text user content through UserMessageContent", () => {
    render(
      <TranscriptRow
        message={baseMessage({
          id: "u9",
          role: "user",
          contentType: "rich_text",
          content: JSON.stringify({
            segments: [{ type: "text", text: "rich hello" }],
          }),
        })}
      />,
    );
    expect(screen.getByTestId("user-message-bubble")).toHaveTextContent("rich hello");
  });

  it("combines the live elapsed-time chron and an output-token meter (↓) on an assistant message when showTimer is enabled", () => {
    render(
      <TranscriptRow
        message={baseMessage({
          contentType: "text",
          content: "the answer",
          durationMs: null,
          tokenCount: 15600,
        })}
        showTimer
      />,
    );
    const meter = screen.getByTestId("token-meter");
    expect(meter).toHaveTextContent(/↓ 15\.6k tokens/);
  });

  it("renders an error message distinctly", () => {
    render(<TranscriptRow message={baseMessage({ contentType: "error", content: "boom" })} />);
    const row = screen.getByTestId("chat-message");
    expect(row).toHaveAttribute("data-slot", "message");
    expect(screen.getByTestId("error-message")).toHaveAttribute("data-slot", "alert");
    expect(screen.getByTestId("error-message")).toHaveTextContent(/boom/);
  });

  it("renders a persisted error row politely (role=status, not an assertive alert) on conversation load", () => {
    render(<TranscriptRow message={baseMessage({ contentType: "error", content: "boom" })} />);
    const errorMessage = screen.getByTestId("error-message");
    expect(errorMessage).toHaveAttribute("role", "status");
    expect(errorMessage).not.toHaveAttribute("role", "alert");
  });

  it("renders nothing for a tool_call row (paired tool_result carries the widget)", () => {
    const { container } = render(
      <TranscriptRow
        message={baseMessage({
          contentType: "tool_call",
          toolName: "Bash",
          content: "{}",
        })}
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
      <TranscriptRow
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
      <TranscriptRow
        message={baseMessage({
          contentType: "tool_result",
          toolName: "SomeMcpTool",
          content: "not valid json",
        })}
      />,
    );
    expect(screen.getByTestId("unknown-tool-widget")).toBeInTheDocument();
  });

  it("does not add assistant duration metadata to non-text rows", () => {
    render(
      <TranscriptRow
        message={baseMessage({
          contentType: "tool_result",
          toolName: "SomeMcpTool",
          content: JSON.stringify({
            toolName: "SomeMcpTool",
            arguments: { input: "x" },
            outcome: { ok: true, text: "did the thing" },
          }),
          tokenCount: 100,
        })}
      />,
    );

    expect(screen.queryByTestId("token-meter")).not.toBeInTheDocument();
  });

  // --- 010-context-window-management/US2: context_notice dispatch ---

  it("renders a 'cleared' notice as a small, muted inline line", () => {
    render(
      <TranscriptRow
        message={baseMessage({
          contentType: "context_notice",
          content: JSON.stringify({
            kind: "cleared",
            clearedCount: 3,
            notice: "3 old tool results cleared to save space",
          }),
        })}
      />,
    );
    const notice = screen.getByTestId("context-notice");
    expect(notice).toHaveAttribute("data-notice-kind", "cleared");
    expect(notice).toHaveTextContent("3 old tool results cleared to save space");
  });

  it("renders context notices as marker-style status rows", () => {
    render(
      <TranscriptRow
        message={{
          id: "n1",
          conversationId: "c1",
          role: "assistant",
          contentType: "context_notice",
          content: JSON.stringify({ kind: "cleared", notice: "Old tool result cleared" }),
          toolName: null,
          createdAt: 1,
          durationMs: null,
          tokenCount: null,
        }}
      />,
    );

    const notice = screen.getByTestId("context-notice");
    expect(notice.closest('[data-slot="message"]')).not.toBeNull();
    expect(notice).toHaveAttribute("data-slot", "marker");
    expect(notice).toHaveAttribute("role", "status");
    expect(notice.querySelector('[data-slot="marker-content"]')).not.toBeNull();
    expect(notice).toHaveTextContent("Old tool result cleared");
  });

  it("renders a 'summarized' notice as a more visible bubble", () => {
    render(
      <TranscriptRow
        message={baseMessage({
          contentType: "context_notice",
          content: JSON.stringify({
            kind: "summarized",
            summary: "the user asked about X, agreed on Y",
            notice: "Conversation condensed to save space",
          }),
        })}
      />,
    );
    const notice = screen.getByTestId("context-notice");
    expect(notice).toHaveAttribute("data-notice-kind", "summarized");
    expect(notice).toHaveTextContent("Conversation condensed to save space");
  });

  it("degrades to a plain-text notice on malformed context_notice content rather than crashing", () => {
    render(
      <TranscriptRow
        message={baseMessage({
          contentType: "context_notice",
          content: "not valid json",
        })}
      />,
    );
    expect(screen.getByTestId("context-notice")).toHaveTextContent("not valid json");
  });

  it("renders nothing for plan-machine tool rows (plan activity is tracker-only)", () => {
    const planCall = {
      id: "pc1",
      conversationId: "c1",
      role: "assistant",
      contentType: "tool_call",
      content: JSON.stringify({ arguments: { goal: "g", steps: ["a"] } }),
      toolName: "CreatePlan",
      createdAt: 1,
      durationMs: null,
      tokenCount: null,
    } as const;
    const planResult = {
      ...planCall,
      id: "pr1",
      role: "tool",
      contentType: "tool_result",
      content: JSON.stringify({
        toolName: "CreatePlan",
        arguments: { goal: "g", steps: ["a"] },
        plan: true,
        outcome: { ok: true, text: "Plan created with 1 steps." },
      }),
    } as const;
    // A state-gated rejection carries a REGULAR tool name but the plan
    // marker — it must be skipped by the marker, not the name.
    const gatedRejection = {
      ...planResult,
      id: "pr2",
      toolName: "Write",
      content: JSON.stringify({
        toolName: "Write",
        arguments: {},
        plan: true,
        outcome: {
          ok: false,
          text: "Error: Write is not available in the current phase",
        },
      }),
    } as const;

    const { container: c1 } = render(<TranscriptRow message={planCall} />);
    const { container: c2 } = render(<TranscriptRow message={planResult} />);
    const { container: c3 } = render(<TranscriptRow message={gatedRejection} />);
    expect(c1).toBeEmptyDOMElement();
    expect(c2).toBeEmptyDOMElement();
    expect(c3).toBeEmptyDOMElement();
  });
});
