import { useEffect, useState } from "react";
import { Check, ChevronDown, Circle } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import {
  Item,
  ItemActions,
  ItemContent,
  ItemDescription,
  ItemGroup,
  ItemMedia,
  ItemTitle,
} from "@/components/ui/item";
import { Progress } from "@/components/ui/progress";
import { commands, events, type PlanSnapshot } from "@/lib/ipc";

/** Card caps (spec): completed steps collapse into one "✓ n done" line
 * once the plan exceeds 6 steps; visible pending capped at 4. */
const CARD_COLLAPSE_THRESHOLD = 6;
const CARD_MAX_PENDING = 4;

interface PlanTrackerProps {
  conversationId: string;
}

/**
 * The live plan/todo tracker (spec:
 * docs/superpowers/specs/2026-07-09-plan-tracker-design.md): docks above
 * the composer as a collapsible strip. Live-turn chrome only — appears
 * when the agent creates a plan, follows plan-update events, recovers
 * across reloads via get_active_plan, and unmounts immediately when the
 * turn ends (plan: null). Collapsed, it's a one-liner showing the current
 * step (or the goal while planning) and n/m progress; expanding it opens
 * the full step list upward, Claude Code style.
 */
export default function PlanTracker({ conversationId }: PlanTrackerProps) {
  const [plan, setPlan] = useState<PlanSnapshot | null>(null);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;

    const applyUpdate = (next: PlanSnapshot | null) => {
      if (next) {
        setPlan(next);
        return;
      }
      // Turn ended: unmount immediately.
      setPlan(null);
    };

    // Set once a plan-update event for THIS conversation has been seen, so
    // the mount-time recovery invoke below can tell it's become stale --
    // covers both a stale snapshot clobbering a fresher live event, and a
    // `plan: null` event unmounting the tracker before recovery even
    // resolves (a late, stale resolve must not resurrect it).
    let sawEvent = false;

    setPlan(null);
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
  const allDone = doneCount === plan.steps.length;
  const currentStep = plan.currentStepIndex != null ? plan.steps[plan.currentStepIndex] : undefined;

  const collapseDone = plan.steps.length > CARD_COLLAPSE_THRESHOLD;
  const rows = plan.steps
    .map((step, index) => ({ step, index }))
    .filter(({ step, index }) => {
      if (!collapseDone) return true;
      // Keep the current step and pending ones; completed fold into the
      // "✓ n done" line.
      return !step.done || index === plan.currentStepIndex;
    });
  const pendingVisible = collapseDone ? rows.slice(0, CARD_MAX_PENDING + 1) : rows;
  const hiddenCount = rows.length - pendingVisible.length;

  return (
    <div className="px-4">
      <Collapsible className="mx-auto max-w-lg" data-testid="plan-tracker">
        {/* Content BEFORE the trigger: the list expands upward, Claude
            Code style — the one-liner stays anchored just above the
            composer. */}
        <CollapsibleContent>
          <Progress
            className="px-2 py-1"
            value={plan.steps.length > 0 ? (doneCount / plan.steps.length) * 100 : 0}
          />
          {collapseDone && doneCount > 0 && (
            <ItemDescription className="px-2" data-testid="plan-done-collapsed">
              ✓ {doneCount} done
            </ItemDescription>
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
                <ItemMedia variant="icon">{step.done ? <Check /> : <Circle />}</ItemMedia>
                <ItemContent>
                  <ItemTitle className="truncate" title={step.description}>
                    {step.description}
                  </ItemTitle>
                </ItemContent>
              </Item>
            ))}
          </ItemGroup>
          {hiddenCount > 0 && (
            <ItemDescription className="px-2" data-testid="plan-more">
              +{hiddenCount} more
            </ItemDescription>
          )}
        </CollapsibleContent>
        <CollapsibleTrigger
          nativeButton={false}
          render={
            <Item
              size="xs"
              variant="muted"
              className="group/plan w-full cursor-pointer"
              data-testid="plan-current-step"
            />
          }
        >
          <ItemMedia variant="icon">{allDone ? <Check /> : <Circle />}</ItemMedia>
          <ItemContent>
            <ItemTitle className="truncate" title={currentStep?.description ?? plan.goal}>
              {currentStep?.description ?? plan.goal}
            </ItemTitle>
          </ItemContent>
          <ItemActions>
            <Badge variant="secondary">
              {doneCount}/{plan.steps.length}
            </Badge>
            <ChevronDown
              aria-hidden="true"
              className="size-4 shrink-0 text-muted-foreground transition-transform group-aria-expanded/plan:rotate-180"
            />
          </ItemActions>
        </CollapsibleTrigger>
      </Collapsible>
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
