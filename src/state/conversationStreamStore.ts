import { create } from "zustand";
import { events } from "@/lib/ipc";

interface ConversationStreamState {
  streams: Record<string, string>;
  appendToken: (conversationId: string, token: string) => void;
  clearStream: (conversationId: string) => void;
}

// research.md §14: a small Zustand store scoped to streaming state, kept
// separate from TanStack Query (which owns request/response data only).
// Selector-based subscriptions avoid re-rendering unrelated components on
// every streamed token.
export const useConversationStreamStore = create<ConversationStreamState>((set) => ({
  streams: {},
  appendToken: (conversationId, token) =>
    set((s) => ({
      streams: { ...s.streams, [conversationId]: (s.streams[conversationId] ?? "") + token },
    })),
  clearStream: (conversationId) =>
    set((s) => {
      const next = { ...s.streams };
      delete next[conversationId];
      return { streams: next };
    }),
}));

let wired = false;
export async function wireConversationStreamEvents() {
  if (wired) return;
  wired = true;
  await events.onAssistantToken((p) => {
    useConversationStreamStore.getState().appendToken(p.conversationId, p.token);
  });
  // Completion handling (promoting the finished stream into a persisted
  // message, clearing it here) lives in Chat.tsx: it owns the message list
  // that the finished text needs to be appended to.
}
