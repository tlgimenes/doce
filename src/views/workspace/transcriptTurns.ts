import type { Message } from "@/lib/ipc";

export interface TranscriptTurn {
  id: string;
  user: Message | null;
  rows: Message[];
}

export interface TurnTokenTotals {
  /** Everything the turn ADDED to the model's context: the user prompt's
   * tokens plus every tool result's annotated count. */
  input: number;
  /** Everything the model GENERATED as text this turn. (Tool-call rows
   * carry no token counts, so generation spent on tool calls is not
   * included.) */
  output: number;
}

/**
 * Accumulates a turn's in/out token flow for the streaming indicator and
 * the final reply's footer — replaces the per-widget and per-message
 * counters. Works mid-turn: totals grow as tool results land.
 */
export function accumulateTurnTokens(turn: TranscriptTurn): TurnTokenTotals {
  let input = turn.user?.tokenCount ?? 0;
  let output = 0;

  for (const row of turn.rows) {
    if (row.role === "assistant" && row.contentType === "text") {
      output += row.tokenCount ?? 0;
      continue;
    }
    if (row.contentType === "tool_result") {
      try {
        const parsed = JSON.parse(row.content) as { tokenCount?: unknown };
        if (typeof parsed.tokenCount === "number") input += parsed.tokenCount;
      } catch {
        // Fallback-shaped results carry no count.
      }
    }
  }

  return { input, output };
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

    if (!current || current.user === null) {
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
