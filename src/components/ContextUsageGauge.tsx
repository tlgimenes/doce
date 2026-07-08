import { useEffect } from "react";
import { commands, type ContextState } from "@/lib/ipc";
import { useContextUsageStore } from "@/state/contextUsageStore";
import { cn } from "@/lib/cn";

interface ContextUsageGaugeProps {
  conversationId: string;
}

const RADIUS = 8;
const STROKE_WIDTH = 2.5;
const CIRCUMFERENCE = 2 * Math.PI * RADIUS;

const stateStrokeClasses: Record<ContextState, string> = {
  normal: "text-muted-foreground/50",
  warning: "text-amber-500",
  justCompacted: "text-emerald-500",
};

/**
 * 010-context-window-management (UI refactor): a small donut gauge that
 * lives right next to the composer's paperclip button, replacing the
 * earlier slim bar + "Compact now" button. Display-only — hovering shows
 * the exact percentage in a tooltip; compaction itself is triggered by
 * typing `/compact` in the workspace composer, mirroring
 * Claude Code's slash-command convention rather than overloading this
 * small glanceable indicator as a click target too.
 */
export default function ContextUsageGauge({ conversationId }: ContextUsageGaugeProps) {
  const usage = useContextUsageStore((s) => s.usage[conversationId]);
  const setUsage = useContextUsageStore((s) => s.setUsage);

  useEffect(() => {
    let cancelled = false;
    commands
      .getContextUsage(conversationId)
      .then((u) => {
        if (!cancelled) setUsage(u);
      })
      .catch(() => {
        // No model loaded yet, or nothing to report — leave the gauge
        // unrendered rather than surfacing an error for a background
        // enrichment call.
      });
    return () => {
      cancelled = true;
    };
  }, [conversationId, setUsage]);

  if (!usage) return null;

  const pct = usage.tokenBudget > 0 ? (usage.tokensUsed / usage.tokenBudget) * 100 : 0;
  const clampedPct = Math.min(100, Math.max(0, pct));
  const dashOffset = CIRCUMFERENCE * (1 - clampedPct / 100);
  const tooltipText =
    usage.state === "justCompacted"
      ? `${Math.round(pct)}% of context used · just compacted`
      : `${Math.round(pct)}% of context used`;

  return (
    <div
      className="group relative flex h-8 w-8 shrink-0 cursor-default items-center justify-center"
      data-testid="context-usage-gauge"
      role="status"
      aria-label={tooltipText}
    >
      <svg width="20" height="20" viewBox="0 0 20 20" className="-rotate-90">
        <circle
          cx="10"
          cy="10"
          r={RADIUS}
          fill="none"
          strokeWidth={STROKE_WIDTH}
          className="stroke-muted"
        />
        <circle
          cx="10"
          cy="10"
          r={RADIUS}
          fill="none"
          strokeWidth={STROKE_WIDTH}
          strokeLinecap="round"
          strokeDasharray={CIRCUMFERENCE}
          strokeDashoffset={dashOffset}
          className={cn(
            "stroke-current transition-[stroke-dashoffset]",
            stateStrokeClasses[usage.state],
          )}
        />
      </svg>
      <div
        className="pointer-events-none absolute bottom-full left-1/2 z-10 mb-2 -translate-x-1/2 whitespace-nowrap rounded-lg border border-border bg-card px-2 py-1 text-xs text-foreground opacity-0 shadow-lg transition-opacity group-hover:opacity-100"
        data-testid="context-usage-tooltip"
      >
        {tooltipText}
      </div>
    </div>
  );
}
