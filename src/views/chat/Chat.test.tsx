import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import Chat from "./Chat";
import { commands, events } from "@/lib/ipc";
import { useConversationStreamStore } from "@/state/conversationStreamStore";

type ErrorCb = (p: { conversationId: string; messageId: string; error: string }) => void;
type QueueCb = (p: {
  requestId: string;
  conversationId: string;
  state: "queued" | "generating";
  position: number | null;
}) => void;

vi.mock("@/lib/ipc", () => ({
  commands: {
    listMessages: vi.fn(),
    sendMessage: vi.fn(),
    cancelGeneration: vi.fn(),
  },
  events: {
    onAssistantToken: vi.fn(),
    onAssistantMessageComplete: vi.fn(),
    onAssistantMessageError: vi.fn(),
    onGenerationQueueUpdate: vi.fn(),
  },
}));

// Fast, deterministic coverage for the queued -> generating -> done loading
// states — the real e2e chat spec deliberately does NOT assert on these,
// since on this model/hardware a reply can complete in well under a
// second, making the transient placeholder flash in and out between two
// e2e poll ticks. Unit-testing it here with manually-driven fake events
// sidesteps that race entirely.
describe("Chat loading states", () => {
  let tokenCallback: (p: { conversationId: string; messageId: string; token: string }) => void;
  let completeCallback: (p: { conversationId: string; messageId: string; durationMs: number }) => void;
  let errorCallback: ErrorCb;
  let queueCallback: QueueCb;

  beforeEach(() => {
    vi.clearAllMocks();
    useConversationStreamStore.setState({ streams: {} });

    vi.mocked(commands.listMessages).mockResolvedValue([]);
    vi.mocked(events.onAssistantToken).mockImplementation(async (cb) => {
      tokenCallback = cb;
      return () => {};
    });
    vi.mocked(events.onAssistantMessageComplete).mockImplementation(async (cb) => {
      completeCallback = cb;
      return () => {};
    });
    vi.mocked(events.onAssistantMessageError).mockImplementation(async (cb) => {
      errorCallback = cb;
      return () => {};
    });
    vi.mocked(events.onGenerationQueueUpdate).mockImplementation(async (cb) => {
      queueCallback = cb;
      return () => {};
    });
  });

  it("shows Queued, then Generating, then the final message — never all user bubbles before any reply", async () => {
    vi.mocked(commands.sendMessage).mockResolvedValue({
      messageId: "user-msg-1",
      requestId: "req-1",
      assistantMessageId: "assistant-msg-1",
      assistantCreatedAt: Date.now(),
    });

    render(<Chat conversationId="conv-1" />);
    await waitFor(() => expect(commands.listMessages).toHaveBeenCalledWith("conv-1"));

    const input = await screen.findByTestId("chat-input");
    await userEvent.type(input, "Say hello in exactly three words.");
    await userEvent.click(screen.getByTestId("chat-send"));

    // Queued: sent, no tokens yet.
    await waitFor(() => {
      expect(screen.getByTestId("generation-status")).toHaveTextContent("Queued…");
    });

    // Generating: first token arrives. In the real app,
    // `wireConversationStreamEvents` (called once from App.tsx, not from
    // Chat.tsx) is what appends tokens into the store; Chat.tsx's own
    // `onAssistantToken` subscription only flips the queued->generating
    // status. Rendering `<Chat />` in isolation means that store wiring
    // isn't mounted, so both effects have to be driven explicitly here.
    useConversationStreamStore.getState().appendToken("conv-1", "Hi");
    tokenCallback({ conversationId: "conv-1", messageId: "assistant-msg-1", token: "Hi" });
    await waitFor(() => {
      expect(screen.getByTestId("assistant-stream")).toHaveTextContent("Hi");
    });

    // Done: the placeholder is replaced by the real, final message bubble —
    // this is what the reported bug broke (bubbles stayed grouped by role
    // instead of alternating).
    completeCallback({ conversationId: "conv-1", messageId: "assistant-msg-1", durationMs: 420 });
    await waitFor(() => {
      expect(screen.queryByTestId("generation-status")).not.toBeInTheDocument();
      expect(screen.queryByTestId("assistant-stream")).not.toBeInTheDocument();
    });

    const renderedMessages = screen.getAllByTestId("chat-message");
    expect(renderedMessages).toHaveLength(2);
    expect(renderedMessages[0].textContent).toContain("Say hello in exactly three words");
    expect(renderedMessages[1].textContent).toContain("Hi");
  });

  it("009-rich-chat-input regression: a message containing a chip forwards richContent to sendMessage, not just the flat text", async () => {
    vi.mocked(commands.sendMessage).mockResolvedValue({
      messageId: "user-msg-1",
      requestId: "req-1",
      assistantMessageId: "assistant-msg-1",
      assistantCreatedAt: Date.now(),
    });

    render(<Chat conversationId="conv-1" />);
    const input = await screen.findByTestId("chat-input");

    const pastedBlock = Array.from({ length: 15 }, (_, i) => `line-${i}`).join("\n");
    fireEvent.paste(input, { clipboardData: { items: [], getData: () => pastedBlock } });
    await screen.findByTestId("pasted-text-chip");

    await userEvent.click(screen.getByTestId("chat-send"));

    await waitFor(() => expect(commands.sendMessage).toHaveBeenCalled());
    const [, , richContentArg] = vi.mocked(commands.sendMessage).mock.calls[0];
    expect(richContentArg).toBeDefined();
    const parsed = JSON.parse(richContentArg as string);
    expect(
      parsed.segments.some(
        (s: { type: string; text?: string }) => s.type === "pastedText" && s.text === pastedBlock,
      ),
    ).toBe(true);
  });

  it("shows queue position while queued behind other work (US5/FR-025)", async () => {
    vi.mocked(commands.sendMessage).mockResolvedValue({
      messageId: "user-msg-1",
      requestId: "req-1",
      assistantMessageId: "assistant-msg-1",
      assistantCreatedAt: Date.now(),
    });

    render(<Chat conversationId="conv-1" />);
    await screen.findByTestId("chat-input");
    await userEvent.type(screen.getByTestId("chat-input"), "hello");
    await userEvent.click(screen.getByTestId("chat-send"));

    queueCallback({ requestId: "req-1", conversationId: "conv-1", state: "queued", position: 2 });
    await waitFor(() => {
      expect(screen.getByTestId("generation-status")).toHaveTextContent("Queued (2 ahead)…");
    });
  });

  it("cancel button calls cancelGeneration with the request id", async () => {
    vi.mocked(commands.sendMessage).mockResolvedValue({
      messageId: "user-msg-1",
      requestId: "req-1",
      assistantMessageId: "assistant-msg-1",
      assistantCreatedAt: Date.now(),
    });
    vi.mocked(commands.cancelGeneration).mockResolvedValue(true);

    render(<Chat conversationId="conv-1" />);
    await screen.findByTestId("chat-input");
    await userEvent.type(screen.getByTestId("chat-input"), "hello");
    await userEvent.click(screen.getByTestId("chat-send"));

    await screen.findByTestId("cancel-generation");
    await userEvent.click(screen.getByTestId("cancel-generation"));

    expect(commands.cancelGeneration).toHaveBeenCalledWith("req-1");
  });

  it("shows an error and clears the pending placeholder instead of hanging forever on failure", async () => {
    vi.mocked(commands.sendMessage).mockResolvedValue({
      messageId: "user-msg-1",
      requestId: "req-1",
      assistantMessageId: "assistant-msg-1",
      assistantCreatedAt: Date.now(),
    });

    render(<Chat conversationId="conv-1" />);
    await screen.findByTestId("chat-input");
    await userEvent.type(screen.getByTestId("chat-input"), "hello");
    await userEvent.click(screen.getByTestId("chat-send"));

    await screen.findByTestId("generation-status");
    errorCallback({ conversationId: "conv-1", messageId: "assistant-msg-1", error: "inference crashed" });

    await waitFor(() => {
      expect(screen.queryByTestId("generation-status")).not.toBeInTheDocument();
      expect(screen.getByTestId("chat-error")).toHaveTextContent("inference crashed");
    });
  });
});
