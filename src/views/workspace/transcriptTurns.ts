import type { Message } from "@/lib/ipc";

export interface TranscriptTurn {
  id: string;
  user: Message | null;
  rows: Message[];
}

export function groupTranscriptTurns(messages: Message[]): TranscriptTurn[] {
  const turns: TranscriptTurn[] = [];
  let current: TranscriptTurn | null = null;

  for (const message of messages) {
    if (message.role === "user") {
      current = {
        id: message.id,
        user: message,
        rows: [],
      };
      turns.push(current);
      continue;
    }

    if (!current) {
      current = {
        id: message.id,
        user: null,
        rows: [],
      };
      turns.push(current);
    }

    current.rows.push(message);
  }

  return turns;
}
