import { create } from "zustand";
import { events, type ContextUsage } from "@/lib/ipc";

interface ContextUsageState {
  usage: Record<string, ContextUsage>;
  setUsage: (u: ContextUsage) => void;
}

// 010-context-window-management/US1: live per-conversation context-usage
// state, mirroring conversationStreamStore.ts's shape/conventions. Keyed by
// conversationId since more than one conversation's usage may be known at
// once (e.g. after switching away and back).
export const useContextUsageStore = create<ContextUsageState>((set) => ({
  usage: {},
  setUsage: (u) =>
    set((s) => ({
      usage: { ...s.usage, [u.conversationId]: u },
    })),
}));

let wired = false;
export async function wireContextUsageEvents() {
  if (wired) return;
  wired = true;
  await events.onContextUsageUpdate((p) => {
    useContextUsageStore.getState().setUsage(p);
  });
}
