import { useEffect, useState } from "react";

import { Marker, MarkerContent } from "@/components/ui/marker";
import { formatTokenCount } from "@/lib/formatTokenCount";
import type { TurnTokenTotals } from "./transcriptTurns";

interface StreamingStatusProps {
  startedAt: number | null;
  /** Live in/out token totals for the in-flight turn — grows as tool
   * results land. Omitted (null) when the caller has no turn yet. */
  tokens?: TurnTokenTotals | null;
}

function formatElapsedMs(elapsedMs: number) {
  return `${(Math.max(0, elapsedMs) / 1000).toFixed(1)}s`;
}

export default function StreamingStatus({ startedAt, tokens = null }: StreamingStatusProps) {
  const [fallbackStartedAt] = useState(() => Date.now());
  const effectiveStartedAt = startedAt ?? fallbackStartedAt;
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    const intervalId = window.setInterval(() => setNow(Date.now()), 100);
    return () => window.clearInterval(intervalId);
  }, []);

  return (
    <div className="px-4" data-testid="agent-thinking">
      <div className="mx-auto max-w-xl py-2">
        <Marker>
          <MarkerContent>
            <span role="status" aria-atomic="true" aria-label="Working" className="shimmer">
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
        </Marker>
      </div>
    </div>
  );
}
