import { useEffect, useState } from "react";

interface TimerProps {
  createdAt: number;
  /** Once known (generation finished), freezes the display instead of ticking. */
  durationMs: number | null;
}

/**
 * Elapsed-time badge anchored to a stored timestamp rather than component
 * mount time, so a reload mid-generation resumes the correct count instead
 * of restarting from zero.
 */
export default function Timer({ createdAt, durationMs }: TimerProps) {
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    if (durationMs != null) return;
    const id = setInterval(() => setNow(Date.now()), 100);
    return () => clearInterval(id);
  }, [durationMs]);

  const elapsedMs = durationMs ?? Math.max(0, now - createdAt);
  return <span className="font-mono tabular-nums">{(elapsedMs / 1000).toFixed(1)}s</span>;
}
