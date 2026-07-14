import { useEffect, useState } from "react";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Marker,
  MarkerContent,
  MarkerIcon,
} from "@/components/ui/marker";
import {
  MessageScroller,
  MessageScrollerContent,
  MessageScrollerItem,
  MessageScrollerProvider,
  MessageScrollerViewport,
} from "@/components/ui/message-scroller";
import { commands, events, type PlanSnapshot } from "@/lib/ipc";

const PLAN_VISIBLE_ROWS = 3;
const PLAN_ROW_HEIGHT_REM = 1.25;

interface PlanTrackerProps {
  conversationId: string;
}

/**
 * The live plan/todo tracker (spec:
 * docs/superpowers/specs/2026-07-09-plan-tracker-design.md): docks above
 * the composer as a compact task list. Live-turn chrome only — appears
 * when the agent creates a plan, follows plan-update events, recovers
 * across reloads via get_active_plan, and unmounts immediately when the
 * turn ends (plan: null). The list preserves plan order and scrolls after
 * three rows.
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

  return (
    <div className="px-4">
      <PlanTrackerCard plan={plan} />
    </div>
  );
}

/**
 * The presentational plan card, split from the live container above so the
 * WidgetGallery (Cmd+D) can render it from mock snapshots — the place to
 * iterate on its look without driving a real agent turn.
 */
export function PlanTrackerCard({ plan }: { plan: PlanSnapshot }) {
  const doneCount = plan.steps.filter((step) => step.done).length;
  const queuedCount = plan.steps.length - doneCount;
  const maxListHeight = `${PLAN_VISIBLE_ROWS * PLAN_ROW_HEIGHT_REM}rem`;

  return (
    <div className="mx-auto w-full max-w-xl" data-testid="plan-tracker">
      <div
        className="px-2.5 py-0 text-xs tabular-nums text-muted-foreground"
        data-testid="plan-status"
      >
        <span>
          {queuedCount > 0 ? (
            <>{doneCount} done · {queuedCount} queued</>
          ) : (
            <>{doneCount} completed</>
          )}
        </span>
      </div>
      <MessageScrollerProvider>
        <MessageScroller
          className="w-full"
          data-testid="plan-task-scroller"
          style={{ maxHeight: maxListHeight }}
        >
          <MessageScrollerViewport data-testid="plan-task-viewport">
            <MessageScrollerContent className="min-h-0 gap-0">
              {plan.steps.map((step, index) => {
                const isCurrent = index === plan.currentStepIndex;

                return (
                  <MessageScrollerItem key={index}>
                    <Marker
                      className={
                        isCurrent
                          ? "gap-1.5 px-2.5 py-0 text-foreground"
                          : "gap-1.5 px-2.5 py-0 text-muted-foreground"
                      }
                      data-current={isCurrent ? "true" : undefined}
                      data-state={step.done ? "done" : "todo"}
                      data-testid="plan-step"
                    >
                      <MarkerIcon>
                        <Checkbox
                          checked={step.done}
                          className="size-3.5 shrink-0"
                          disabled
                        />
                      </MarkerIcon>
                      <MarkerContent className="min-w-0">
                        <span
                          className={
                            step.done
                              ? "block min-w-0 truncate line-through"
                              : "block min-w-0 truncate"
                          }
                          title={step.description}
                        >
                          {step.description}
                        </span>
                      </MarkerContent>
                    </Marker>
                  </MessageScrollerItem>
                );
              })}
            </MessageScrollerContent>
          </MessageScrollerViewport>
        </MessageScroller>
      </MessageScrollerProvider>
    </div>
  );
}
