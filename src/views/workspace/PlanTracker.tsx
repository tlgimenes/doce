import { useEffect, useRef, useState } from "react";
import { cn } from "@/lib/cn";
import { commands, events, type PlanSnapshot } from "@/lib/ipc";

const FADE_OUT_MS = 300;
/** Card caps (spec): completed steps collapse into one "✓ n done" line
 * once the plan exceeds 6 steps; visible pending capped at 4. */
const CARD_COLLAPSE_THRESHOLD = 6;
const CARD_MAX_PENDING = 4;
/** Rail cap (spec): per-step dots up to 12 steps, then a single n/m chip. */
const RAIL_MAX_DOTS = 12;

interface PlanTrackerProps {
  conversationId: string;
}

/**
 * The live plan/todo tracker (spec:
 * docs/superpowers/specs/2026-07-09-plan-tracker-design.md): floats over
 * the transcript's top-right gutter inside Workspace's StickToBottom
 * wrapper. Live-turn chrome only — appears when the agent creates a plan,
 * follows plan-update events, recovers across reloads via
 * get_active_plan, and fades out when the turn ends (plan: null). The
 * card/rail split is pure CSS container queries (the chat surface is the
 * container): the full card when the gutter fits it, the numbered dot
 * rail when it doesn't. Both render in the DOM — jsdom can't evaluate
 * container queries, and tests assert both forms directly.
 */
export default function PlanTracker({ conversationId }: PlanTrackerProps) {
  const [plan, setPlan] = useState<PlanSnapshot | null>(null);
  const [leaving, setLeaving] = useState(false);
  // The rail's tap-to-expand: force-shows the card at narrow widths.
  const [expanded, setExpanded] = useState(false);
  const leaveTimerRef = useRef<number | null>(null);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;

    const applyUpdate = (next: PlanSnapshot | null) => {
      if (leaveTimerRef.current !== null) {
        window.clearTimeout(leaveTimerRef.current);
        leaveTimerRef.current = null;
      }
      if (next) {
        setLeaving(false);
        setPlan(next);
        return;
      }
      // Turn ended: fade, then unmount.
      setExpanded(false);
      setLeaving(true);
      leaveTimerRef.current = window.setTimeout(() => {
        setPlan(null);
        setLeaving(false);
        leaveTimerRef.current = null;
      }, FADE_OUT_MS);
    };

    setPlan(null);
    setLeaving(false);
    setExpanded(false);
    void commands
      .getActivePlan(conversationId)
      .then((recovered) => {
        if (!cancelled && recovered) setPlan(recovered);
      })
      .catch(() => {});
    void events
      .onPlanUpdate((payload) => {
        if (cancelled || payload.conversationId !== conversationId) return;
        applyUpdate(payload.plan);
      })
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      });

    return () => {
      cancelled = true;
      unlisten?.();
      if (leaveTimerRef.current !== null) {
        window.clearTimeout(leaveTimerRef.current);
        leaveTimerRef.current = null;
      }
    };
  }, [conversationId]);

  if (!plan || plan.steps.length === 0) return null;

  const doneCount = plan.steps.filter((s) => s.done).length;

  return (
    <div
      className={cn(
        "absolute top-3 right-3 z-10 transition-opacity duration-300",
        leaving && "opacity-0",
      )}
      data-testid="plan-tracker"
    >
      {/* Full card: shown when the container is wide enough for the
          gutter (>= 64rem), or when the rail was tapped open. */}
      <div className={cn("hidden @5xl:block", expanded && "block")} data-testid="plan-card">
        <PlanCard plan={plan} doneCount={doneCount} />
      </div>
      {/* Collapsed rail: numbered dots (the selected mockup), only below
          the breakpoint and only while not tapped open. */}
      <button
        type="button"
        className={cn("block @5xl:hidden", expanded && "hidden")}
        onClick={() => setExpanded(true)}
        aria-label="Show plan"
        data-testid="plan-rail"
      >
        <PlanRail plan={plan} doneCount={doneCount} />
      </button>
      {expanded && (
        <button
          type="button"
          className="mt-1 block w-full text-center text-xs text-muted-foreground @5xl:hidden"
          onClick={() => setExpanded(false)}
          aria-label="Hide plan"
          data-testid="plan-collapse"
        >
          collapse
        </button>
      )}
    </div>
  );
}

function PlanCard({ plan, doneCount }: { plan: PlanSnapshot; doneCount: number }) {
  const collapseDone = plan.steps.length > CARD_COLLAPSE_THRESHOLD;
  const rows = plan.steps
    .map((step, index) => ({ step, index }))
    .filter(({ step, index }) => {
      if (!collapseDone) return true;
      // Keep the current step and pending ones; completed fold into the
      // "✓ n done" header line.
      return !step.done || index === plan.currentStepIndex;
    });
  const pendingVisible = collapseDone ? rows.slice(0, CARD_MAX_PENDING + 1) : rows;
  const hiddenCount = rows.length - pendingVisible.length;

  return (
    <div className="w-60 rounded-lg border border-border bg-card/95 p-3 text-sm shadow-sm backdrop-blur supports-[backdrop-filter]:bg-card/80">
      <div className="mb-1.5 flex items-baseline justify-between gap-2">
        <span className="truncate text-xs font-semibold" title={plan.goal}>
          {plan.goal}
        </span>
        <span className="shrink-0 text-xs text-muted-foreground">
          {doneCount}/{plan.steps.length}
        </span>
      </div>
      {collapseDone && doneCount > 0 && (
        <p className="text-xs text-muted-foreground" data-testid="plan-done-collapsed">
          ✓ {doneCount} done
        </p>
      )}
      <ul className="space-y-0.5">
        {pendingVisible.map(({ step, index }) => (
          <li
            key={index}
            className={cn(
              "flex items-baseline gap-1.5 text-xs",
              step.done && "text-muted-foreground line-through",
              index === plan.currentStepIndex && "font-semibold",
            )}
            data-current={index === plan.currentStepIndex ? "true" : undefined}
            data-testid="plan-step"
          >
            <span className="w-3 shrink-0 no-underline">
              {step.done ? "✓" : index === plan.currentStepIndex ? "●" : "○"}
            </span>
            <span className="truncate" title={step.description}>
              {step.description}
            </span>
          </li>
        ))}
      </ul>
      {hiddenCount > 0 && (
        <p className="text-xs text-muted-foreground" data-testid="plan-more">
          +{hiddenCount} more
        </p>
      )}
    </div>
  );
}

function PlanRail({ plan, doneCount }: { plan: PlanSnapshot; doneCount: number }) {
  const pill =
    "rounded-full border border-border bg-card/95 shadow-sm backdrop-blur supports-[backdrop-filter]:bg-card/80";
  if (plan.steps.length > RAIL_MAX_DOTS) {
    return (
      <span className={cn(pill, "px-2.5 py-1 text-xs font-semibold")} data-testid="plan-chip">
        {doneCount}/{plan.steps.length}
      </span>
    );
  }
  return (
    <span className={cn(pill, "flex flex-col items-center gap-1 px-1.5 py-2")}>
      {plan.steps.map((step, index) => (
        <span
          key={index}
          className={cn(
            "flex h-4.5 w-4.5 items-center justify-center rounded-full text-[10px] font-semibold",
            step.done
              ? "bg-emerald-600 text-white"
              : index === plan.currentStepIndex
                ? "border-2 border-amber-500 text-amber-600"
                : "border border-border text-muted-foreground",
          )}
          data-current={index === plan.currentStepIndex ? "true" : undefined}
          data-testid="plan-dot"
        >
          {step.done ? "✓" : index + 1}
        </span>
      ))}
    </span>
  );
}
