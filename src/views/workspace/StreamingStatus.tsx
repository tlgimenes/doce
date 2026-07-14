import { useEffect, useState } from "react";

import { Marker, MarkerContent } from "@/components/ui/marker";
import { formatTokenCount } from "@/lib/formatTokenCount";
import type { TurnTokenTotals } from "./transcriptTurns";

interface StreamingStatusProps {
  startedAt: number | null;
  /** Live in/out token totals for the in-flight turn — grows as tool
   * results land. Omitted (null) when the caller has no turn yet. */
  tokens?: TurnTokenTotals | null;
  /** The in-flight generation's raw sampled text (mostly the model's
   * <think> reasoning) — its latest line BECOMES the working line,
   * advancing line by line as the model reasons. */
  stream?: string;
}

/**
 * The reasoning line currently shown in place of "Working": the latest
 * non-empty line of the think block. Content after `</think>` is the
 * tool-call tail — grammar syntax, not reasoning — so the display reverts
 * to the fallback once thinking closes. `null` means "nothing to show
 * yet".
 */
function currentThinkingLine(stream: string): string | null {
  // Reasoning ends at whichever comes first: the think close OR a tool
  // call opening — a generation that skips thinking goes straight into
  // grammar-forced call syntax (`<function name=…` / `<tool_call>`),
  // which must never render as "thinking".
  for (const marker of ["</think>", "<tool_call>", "<function"]) {
    if (stream.includes(marker)) return null;
  }
  const lines = stream
    .replace("<think>", "")
    .split("\n")
    .map((line) => line.trim())
    // A line still starting with "<" is a partially-sampled marker (the
    // model emits tags token by token) — suppress rather than flicker
    // "<fun" for a frame. Everything else shows verbatim: the ticker is a
    // window into the model, not a censor (degenerate output is a signal
    // the user should see).
    .filter((line) => line !== "" && !line.startsWith("<"));
  return lines.length > 0 ? lines[lines.length - 1] : null;
}

function formatElapsedMs(elapsedMs: number) {
  return `${(Math.max(0, elapsedMs) / 1000).toFixed(1)}s`;
}

export default function StreamingStatus({
  startedAt,
  tokens = null,
  stream = "",
}: StreamingStatusProps) {
  const [fallbackStartedAt] = useState(() => Date.now());
  const effectiveStartedAt = startedAt ?? fallbackStartedAt;
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    const intervalId = window.setInterval(() => setNow(Date.now()), 100);
    return () => window.clearInterval(intervalId);
  }, []);

  const thinkingLine = currentThinkingLine(stream);

  return (
    <div className="px-4" data-testid="agent-thinking">
      <div className="mx-auto max-w-xl py-2">
        <Marker>
          <MarkerContent className="shrink-0">
            <span
              role="status"
              aria-atomic="true"
              aria-label="Working"
              className="shimmer"
              data-testid="agent-thinking-status"
            >
              Working
            </span>
          </MarkerContent>
          <span
            aria-live="off"
            className="shrink-0 self-center font-mono text-xs tabular-nums"
            data-testid="agent-thinking-timer"
          >
            {formatElapsedMs(now - effectiveStartedAt)}
          </span>
          {/* Zero-valued directions stay hidden — "↓ 0" is noise while the
              first generation is still running. */}
          {tokens && (tokens.input > 0 || tokens.output > 0) && (
            <span
              aria-live="off"
              className="shrink-0 self-center font-mono text-xs tabular-nums text-muted-foreground"
              data-testid="agent-thinking-tokens"
            >
              {tokens.input > 0 && <>↑ {formatTokenCount(tokens.input)}</>}
              {tokens.input > 0 && tokens.output > 0 && " "}
              {tokens.output > 0 && <>↓ {formatTokenCount(tokens.output)}</>}
            </span>
          )}
          {/* The model's current reasoning line rides the SAME row, after
              the counters — line-by-line as the think block streams, gone
              once </think> closes (the tool-call tail is grammar syntax,
              not reasoning). */}
          {thinkingLine != null && (
            <span
              aria-hidden="true"
              className="min-w-0 flex-1 truncate text-xs leading-5 text-muted-foreground"
              data-testid="agent-thinking-stream"
            >
              {thinkingLine}
            </span>
          )}
        </Marker>
      </div>
    </div>
  );
}
