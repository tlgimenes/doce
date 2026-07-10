import { useEffect, useState } from "react";

import { Marker, MarkerContent, MarkerIcon } from "@/components/ui/marker";
import { Spinner } from "@/components/ui/spinner";

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
      <div className="mx-auto max-w-3xl py-2">
        <Marker>
          <MarkerIcon data-testid="agent-thinking-spinner">
            <Spinner role="presentation" aria-label={undefined} />
          </MarkerIcon>
          <MarkerContent>
            <span role="status" aria-atomic="true" aria-label="Working">
              Working
            </span>
          </MarkerContent>
          <span
            aria-live="off"
            className="ml-auto shrink-0 tabular-nums"
            data-testid="agent-thinking-timer"
          >
            {formatElapsedMs(now - effectiveStartedAt)}
          </span>
        </Marker>
      </div>
    </div>
  );
}
