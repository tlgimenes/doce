import { useEffect, useState } from "react";

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
    <div className="border-b border-border px-4" data-testid="agent-thinking">
      <div className="mx-auto flex h-8 max-w-3xl items-center gap-2 text-xs text-muted-foreground">
        <span className="inline-flex gap-1" data-testid="agent-thinking-dots" aria-hidden="true">
          <span
            className="h-1 w-1 animate-pulse rounded-full bg-current [animation-delay:-300ms]"
            data-testid="agent-thinking-dot"
          />
          <span
            className="h-1 w-1 animate-pulse rounded-full bg-current [animation-delay:-150ms]"
            data-testid="agent-thinking-dot"
          />
          <span
            className="h-1 w-1 animate-pulse rounded-full bg-current"
            data-testid="agent-thinking-dot"
          />
        </span>
        <span role="status" aria-atomic="true" aria-label="Thinking">
          Thinking
        </span>
        <span
          aria-live="off"
          className="w-[7ch] shrink-0 text-right font-mono tabular-nums"
          data-testid="agent-thinking-timer"
        >
          {formatElapsedMs(now - effectiveStartedAt)}
        </span>
      </div>
    </div>
  );
}
