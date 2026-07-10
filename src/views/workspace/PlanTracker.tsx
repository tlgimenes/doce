import { useEffect, useState } from "react";
import { Check, Circle } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Item, ItemContent, ItemGroup, ItemMedia, ItemTitle } from "@/components/ui/item";
import { Progress } from "@/components/ui/progress";
import { Spinner } from "@/components/ui/spinner";
import { commands, events, type PlanSnapshot } from "@/lib/ipc";
import { cn } from "@/lib/cn";

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
 * get_active_plan, and unmounts immediately when the turn ends (plan:
 * null). The card/rail split is pure CSS container queries (the chat
 * surface is the container): the full card when the gutter fits it, the
 * numbered dot rail when it doesn't. Both render in the DOM — jsdom can't
 * evaluate container queries, and tests assert both forms directly.
 */
export default function PlanTracker({ conversationId }: PlanTrackerProps) {
  const [plan, setPlan] = useState<PlanSnapshot | null>(null);
  // The rail's tap-to-expand: force-shows the card at narrow widths.
  const [expanded, setExpanded] = useState(false);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;

    const applyUpdate = (next: PlanSnapshot | null) => {
      if (next) {
        setPlan(next);
        return;
      }
      // Turn ended: unmount immediately.
      setExpanded(false);
      setPlan(null);
    };

    // Set once a plan-update event for THIS conversation has been seen, so
    // the mount-time recovery invoke below can tell it's become stale --
    // covers both a stale snapshot clobbering a fresher live event, and a
    // `plan: null` event unmounting the tracker before recovery even
    // resolves (a late, stale resolve must not resurrect it).
    let sawEvent = false;

    setPlan(null);
    setExpanded(false);
    void commands
      .getActivePlan(conversationId)
      .then((recovered) => {
        if (!cancelled && recovered && !sawEvent) setPlan(recovered);
      })
      .catch(() => {});
    void events
      .onPlanUpdate((payload) => {
        if (cancelled || payload.conversationId !== conversationId) return;
        sawEvent = true;
        applyUpdate(payload.plan);
      })
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [conversationId]);

  if (!plan || plan.steps.length === 0) return null;

  const doneCount = plan.steps.filter((s) => s.done).length;

  return (
    <div className="absolute top-3 right-3 z-10" data-testid="plan-tracker">
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
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="mt-1 w-full @5xl:hidden"
          onClick={() => setExpanded(false)}
          aria-label="Hide plan"
          data-testid="plan-collapse"
        >
          collapse
        </Button>
      )}
    </div>
  );
}

function stepState(
  step: PlanSnapshot["steps"][number],
  index: number,
  currentStepIndex: number | null,
) {
  if (step.done) return "done";
  return index === currentStepIndex ? "current" : "todo";
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
    <Card className="w-64 gap-2 py-3">
      <CardHeader className="gap-1 px-3">
        <CardTitle className="truncate" title={plan.goal}>
          {plan.goal}
        </CardTitle>
        <Badge variant="secondary">
          {doneCount}/{plan.steps.length}
        </Badge>
        <Progress value={plan.steps.length > 0 ? (doneCount / plan.steps.length) * 100 : 0} />
      </CardHeader>
      <CardContent className="px-3">
        {collapseDone && doneCount > 0 && (
          <div data-testid="plan-done-collapsed">✓ {doneCount} done</div>
        )}
        <ItemGroup>
          {pendingVisible.map(({ step, index }) => (
            <Item
              key={index}
              size="xs"
              data-state={stepState(step, index, plan.currentStepIndex)}
              data-current={index === plan.currentStepIndex ? "true" : undefined}
              data-testid="plan-step"
            >
              <ItemMedia variant="icon">
                {step.done ? (
                  <Check />
                ) : index === plan.currentStepIndex ? (
                  <Spinner role="presentation" aria-label={undefined} />
                ) : (
                  <Circle />
                )}
              </ItemMedia>
              <ItemContent>
                <ItemTitle className="truncate" title={step.description}>
                  {step.description}
                </ItemTitle>
              </ItemContent>
            </Item>
          ))}
        </ItemGroup>
        {hiddenCount > 0 && <div data-testid="plan-more">+{hiddenCount} more</div>}
      </CardContent>
    </Card>
  );
}

function PlanRail({ plan, doneCount }: { plan: PlanSnapshot; doneCount: number }) {
  if (plan.steps.length > RAIL_MAX_DOTS) {
    return (
      <Badge variant="secondary" data-testid="plan-chip">
        {doneCount}/{plan.steps.length}
      </Badge>
    );
  }
  return (
    <span className="flex flex-col items-center gap-1">
      {plan.steps.map((step, index) => (
        <Badge
          key={index}
          variant={
            step.done ? "default" : index === plan.currentStepIndex ? "secondary" : "outline"
          }
          className="size-5 justify-center p-0"
          data-state={stepState(step, index, plan.currentStepIndex)}
          data-current={index === plan.currentStepIndex ? "true" : undefined}
          data-testid="plan-dot"
        >
          {step.done ? <Check /> : index + 1}
        </Badge>
      ))}
    </span>
  );
}
