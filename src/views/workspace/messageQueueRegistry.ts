import type { RichMessageContent } from "@/lib/ipc";

/**
 * A message the user composed while a turn was in flight and chose to QUEUE
 * (the default) rather than steer immediately. Held entirely client-side until
 * either the running turn completes (drained FIFO as its own new turn) or the
 * user clicks "Send now" on its row (steered into the running turn).
 * `richContent`/`setGoal` ride along so a drained/steered message keeps its
 * chips and goal intent.
 */
export interface QueuedMessage {
  id: string;
  content: string;
  richContent?: RichMessageContent;
  setGoal?: boolean;
}

/**
 * Per-conversation queue registry, modeled 1:1 on Workspace.tsx's module-global
 * `conversationsWithSendInFlight` set. Module-scoped (not React state) for the
 * same two reasons that registry is:
 *
 * 1. **Per-conversation isolation for free** — Workspace is NOT remounted on a
 *    conversation switch (it re-runs a `conversationId` effect), so a plain
 *    `useState` queue would leak across switches unless manually reset; a keyed
 *    Map never does.
 * 2. **Remount persistence** — the queue is only ever populated *while a turn is
 *    in flight*, which is exactly the mid-turn remount window the send-in-flight
 *    registry was made module-global to survive. A `useState` queue would drop
 *    unsent user intent on such a remount.
 */
const queuesByConversation = new Map<string, QueuedMessage[]>();
const listeners = new Set<() => void>();

// Reference-stable empty snapshot: `useSyncExternalStore` requires the snapshot
// getter to return the SAME reference when nothing changed, or it re-renders in
// a loop. A conversation with no queue always yields this one shared array.
const EMPTY: readonly QueuedMessage[] = Object.freeze([]);

function notify() {
  listeners.forEach((listener) => listener());
}

export function subscribeToQueue(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function getQueueSnapshot(conversationId: string): readonly QueuedMessage[] {
  return queuesByConversation.get(conversationId) ?? EMPTY;
}

export function enqueueMessage(conversationId: string, item: QueuedMessage): void {
  const current = queuesByConversation.get(conversationId) ?? [];
  // Replace-on-mutate (new array ref) so the snapshot reference changes and
  // subscribers re-render.
  queuesByConversation.set(conversationId, [...current, item]);
  notify();
}

export function removeQueuedMessage(conversationId: string, itemId: string): void {
  const current = queuesByConversation.get(conversationId);
  if (!current) return;
  const next = current.filter((item) => item.id !== itemId);
  if (next.length === current.length) return;
  if (next.length === 0) {
    queuesByConversation.delete(conversationId);
  } else {
    queuesByConversation.set(conversationId, next);
  }
  notify();
}

/**
 * Test-only: clears the module-global queue registry. Like
 * `__resetSendInFlightRegistryForTests`, the registry deliberately survives
 * unmount, so a test that leaves messages queued would otherwise leak them into
 * the next test.
 */
export function __resetQueueRegistryForTests(): void {
  queuesByConversation.clear();
  notify();
}
