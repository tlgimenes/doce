import { useEffect, useState } from "react";

import { Marker, MarkerContent } from "@/components/ui/marker";

interface StreamingStatusProps {
  startedAt: number | null;
}

function formatElapsedMs(elapsedMs: number) {
  return `${(Math.max(0, elapsedMs) / 1000).toFixed(1)}s`;
}

export default function StreamingStatus({ startedAt }: StreamingStatusProps) {
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
        </Marker>
      </div>
    </div>
  );
}
