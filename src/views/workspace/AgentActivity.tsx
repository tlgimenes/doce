import { useEffect, useState } from "react";
import {
  ChevronDown,
  CircleCheck,
  CornerDownRight,
  Pencil,
  Sparkles,
  Target,
  Trash2,
} from "lucide-react";

import { Checkbox } from "@/components/ui/checkbox";
import { cn } from "@/lib/cn";
import { formatTokenCount } from "@/lib/formatTokenCount";
import { commands, events, type PlanSnapshot } from "@/lib/ipc";
import type { TurnTokenTotals } from "./transcriptTurns";

/**
 * The reasoning line currently shown on the thinking row: the latest
 * non-empty line of the model's think block. Content after `</think>` is the
 * tool-call tail — grammar syntax, not reasoning — so the row reverts to
 * hidden once thinking closes. `null` means "nothing to show yet".
 *
 * (Lifted verbatim from the old StreamingStatus, whose behavior this
 * consolidates — see AgentActivity.test.tsx for the cases it must satisfy.)
 */
export function currentThinkingLine(stream: string): string | null {
  // Reasoning ends at whichever comes first: the think close OR a tool
  // call opening — a generation that skips thinking goes straight into
  // grammar-forced call syntax (`<function name=…` / `<tool_call>`), which
  // must never render as "thinking".
  for (const marker of ["</think>", "<tool_call>", "<function"]) {
    if (stream.includes(marker)) return null;
  }
  const lines = stream
    .replace("<think>", "")
    .split("\n")
    .map((line) => line.trim())
    // A line still starting with "<" is a partially-sampled marker (the
    // model emits tags token by token) — suppress rather than flicker "<fun"
    // for a frame. Everything else shows verbatim: the ticker is a window
    // into the model, not a censor.
    .filter((line) => line !== "" && !line.startsWith("<"));
  return lines.length > 0 ? lines[lines.length - 1] : null;
}

export function formatElapsedMs(elapsedMs: number): string {
  return `${(Math.max(0, elapsedMs) / 1000).toFixed(1)}s`;
}

/** The index of the step the model is currently on — the snapshot's own
 * pointer when it has one (mid-step), otherwise the first not-yet-done step
 * (Planning state, `currentStepIndex: null`). `-1` when everything is done. */
function currentStepIndex(plan: PlanSnapshot): number {
  if (plan.currentStepIndex != null) return plan.currentStepIndex;
  return plan.steps.findIndex((step) => !step.done);
}

interface GoalControls {
  /** The active conversation goal, or `null` if none is set. */
  current: string | null;
  /** Observer-confirmed as met: rendered muted with a check, no edit/delete. */
  achieved: boolean;
  /** Load the goal back into the composer for editing (goal mode + prefill). */
  onEdit: () => void;
  /** Clear the goal (persist `null`). */
  onDelete: () => void;
}

interface WorkingState {
  /** A turn is in flight — show the live indicator (dot + chron + tokens). */
  active: boolean;
  /** Pre-formatted elapsed label (e.g. "12.3s"), or `null` when idle. */
  elapsedLabel: string | null;
  /** Live in/out token totals for the in-flight turn, or `null`. */
  tokens: TurnTokenTotals | null;
  /** The model's current reasoning line, or `null` when not reasoning. */
  thinkingLine: string | null;
}

interface AgentActivityProps {
  conversationId: string;
  goal: GoalControls;
  streaming: {
    active: boolean;
    startedAt: number | null;
    tokens: TurnTokenTotals | null;
    stream: string;
  };
}

/**
 * The single agent-activity status line docked above the composer, replacing
 * the three previously-stacked widgets (plan tracker, working status, goal
 * banner) with one coherent surface (spec:
 * docs/superpowers/specs/2026-07-19-agent-activity-status-line-design.md):
 *
 * - a collapsed pill: `primary · progress · working` where the PRIMARY slot
 *   grows to fill the line and shows the goal, or — with no goal — the current
 *   todo, or nothing;
 * - a thinking row above it that streams the model's live reasoning;
 * - an expandable panel below it with the full plan checklist and the goal's
 *   edit/delete controls.
 *
 * This container owns the live plan subscription and the elapsed-time ticker;
 * the presentational {@link AgentActivityView} is what the WidgetGallery
 * (Cmd+D) renders from mock snapshots.
 */
export default function AgentActivity({ conversationId, goal, streaming }: AgentActivityProps) {
  const [plan, setPlan] = useState<PlanSnapshot | null>(null);

  // Plan subscription (moved verbatim from the old PlanTracker): recover an
  // in-flight plan on mount, then follow plan-update events; a `plan: null`
  // event ends the turn and drops the plan immediately.
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
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
        setPlan(payload.plan);
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

  // Elapsed-time ticker — only runs while a turn is active, so an idle
  // conversation (goal/plan shown but nothing streaming) doesn't re-render on
  // a timer.
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!streaming.active) return;
    setNow(Date.now());
    const intervalId = window.setInterval(() => setNow(Date.now()), 100);
    return () => window.clearInterval(intervalId);
  }, [streaming.active]);

  const elapsedMs = streaming.active ? now - (streaming.startedAt ?? now) : 0;

  return (
    <AgentActivityView
      plan={plan}
      goal={goal}
      working={{
        active: streaming.active,
        elapsedLabel: streaming.active ? formatElapsedMs(elapsedMs) : null,
        tokens: streaming.tokens,
        thinkingLine: streaming.active ? currentThinkingLine(streaming.stream) : null,
      }}
    />
  );
}

/**
 * Presentational status line — all layout, no subscriptions. Split from the
 * container above so the WidgetGallery can drive it from static snapshots.
 */
export function AgentActivityView({
  plan,
  goal,
  working,
}: {
  plan: PlanSnapshot | null;
  goal: GoalControls;
  working: WorkingState;
}) {
  const [expanded, setExpanded] = useState(false);

  const steps = plan?.steps ?? [];
  const hasPlan = steps.length > 0;
  const doneCount = steps.filter((step) => step.done).length;
  const curIndex = plan ? currentStepIndex(plan) : -1;
  const currentStep = curIndex >= 0 ? steps[curIndex] : undefined;

  // The strip only exists when there's something live to show.
  if (!hasPlan && goal.current == null && !working.active) return null;

  // Editable goal controls or a plan checklist give the pill something to
  // reveal; a working-only line has no expander.
  const canExpand = hasPlan || (goal.current != null && !goal.achieved);
  const open = expanded && canExpand;

  return (
    <div className="px-4" data-testid="agent-activity">
      <div className="mx-auto w-full max-w-xl py-2">
        {/* Thinking row — the model's live reasoning, above the pill. Present
            only while the model is actually reasoning (gone during a tool
            call). */}
        {working.thinkingLine != null && (
          <div className="flex min-w-0 items-center gap-1.5 px-3 py-0.5 text-xs text-muted-foreground">
            <Sparkles size={12} className="shrink-0" />
            <span className="shimmer shrink-0 font-medium">Thinking</span>
            <span
              aria-hidden="true"
              className="min-w-0 flex-1 truncate italic"
              data-testid="agent-thinking-stream"
            >
              {working.thinkingLine}
            </span>
          </div>
        )}

        {/* The pill. */}
        <div
          className={cn(
            "flex items-center gap-2 border border-border bg-card px-3 py-1.5 text-xs",
            open ? "rounded-t-lg border-b-transparent" : "rounded-full",
          )}
          data-testid={hasPlan ? "plan-tracker" : undefined}
        >
          {/* Primary slot — grows to fill the line. Goal, else current todo,
              else empty. */}
          {goal.current != null ? (
            <span className="flex min-w-0 flex-1 items-center gap-1.5">
              {goal.achieved ? (
                <CircleCheck size={13} className="shrink-0 text-muted-foreground" />
              ) : (
                <Target size={13} className="shrink-0" />
              )}
              <span
                className={cn(
                  "min-w-0 flex-1 truncate font-medium",
                  goal.achieved && "font-normal text-muted-foreground",
                )}
                title={goal.current}
                data-testid="agent-activity-goal"
              >
                {goal.current}
              </span>
            </span>
          ) : currentStep ? (
            <span className="flex min-w-0 flex-1 items-center gap-1.5">
              <CornerDownRight size={13} className="shrink-0 text-muted-foreground" />
              <span
                className="min-w-0 flex-1 truncate font-medium"
                title={currentStep.description}
                data-testid="agent-activity-current-todo"
              >
                {currentStep.description}
              </span>
            </span>
          ) : (
            <span className="flex-1" />
          )}

          {/* Progress — mini bar + done/total. */}
          {hasPlan && (
            <>
              <span className="h-4 w-px shrink-0 bg-border" />
              <span
                className="flex shrink-0 items-center gap-1.5 tabular-nums text-muted-foreground"
                data-testid="plan-status"
              >
                <span className="h-1 w-8 overflow-hidden rounded-full bg-muted">
                  <span
                    className="block h-full rounded-full bg-foreground"
                    style={{ width: `${(doneCount / steps.length) * 100}%` }}
                  />
                </span>
                <span className="font-mono">
                  {doneCount}/{steps.length}
                </span>
              </span>
            </>
          )}

          {/* Working — the pulsing filled circle, chron, and token totals,
              justified to the right edge. */}
          {working.active && (
            <>
              <span className="h-4 w-px shrink-0 bg-border" />
              <span className="flex shrink-0 items-center gap-1.5" data-testid="agent-thinking">
                <span
                  role="status"
                  aria-atomic="true"
                  aria-label="Working"
                  className="flex items-center"
                  data-testid="agent-thinking-status"
                >
                  <span className="size-2 animate-pulse rounded-full bg-foreground" />
                  <span className="sr-only">Working</span>
                </span>
                {working.elapsedLabel != null && (
                  <span
                    aria-live="off"
                    className="font-mono tabular-nums"
                    data-testid="agent-thinking-timer"
                  >
                    {working.elapsedLabel}
                  </span>
                )}
                {/* Zero-valued directions stay hidden — "↓ 0" is noise while
                    the first generation is still running. */}
                {working.tokens && (working.tokens.input > 0 || working.tokens.output > 0) && (
                  <span
                    aria-live="off"
                    className="font-mono tabular-nums text-muted-foreground"
                    data-testid="agent-thinking-tokens"
                  >
                    {working.tokens.input > 0 && <>↑ {formatTokenCount(working.tokens.input)}</>}
                    {working.tokens.input > 0 && working.tokens.output > 0 && " "}
                    {working.tokens.output > 0 && <>↓ {formatTokenCount(working.tokens.output)}</>}
                  </span>
                )}
              </span>
            </>
          )}

          {/* Expander — only when there's a plan or editable goal to reveal. */}
          {canExpand && (
            <button
              type="button"
              onClick={() => setExpanded((prev) => !prev)}
              className="-mr-1 shrink-0 rounded p-0.5 text-muted-foreground hover:text-foreground"
              aria-expanded={open}
              aria-label={open ? "Collapse activity" : "Expand activity"}
              data-testid="agent-activity-expander"
            >
              <ChevronDown size={14} className={cn("transition-transform", open && "rotate-180")} />
            </button>
          )}
        </div>

        {/* Expanded panel — goal controls + the full plan checklist. */}
        {open && (
          <div
            className="rounded-b-lg border border-t-0 border-border bg-card px-2 pb-1.5"
            data-testid="agent-activity-panel"
          >
            {goal.current != null && !goal.achieved && (
              <div className="flex items-center gap-2 px-1.5 py-1 text-xs">
                <span className="min-w-0 flex-1 truncate text-muted-foreground">
                  <span className="font-medium text-foreground">Goal</span> {goal.current}
                </span>
                <button
                  type="button"
                  onClick={goal.onEdit}
                  className="shrink-0 rounded p-1 text-muted-foreground hover:bg-muted hover:text-foreground"
                  aria-label="Edit goal"
                  data-testid="agent-activity-goal-edit"
                >
                  <Pencil size={12} />
                </button>
                <button
                  type="button"
                  onClick={goal.onDelete}
                  className="shrink-0 rounded p-1 text-muted-foreground hover:bg-destructive/10 hover:text-destructive"
                  aria-label="Delete goal"
                  data-testid="agent-activity-goal-delete"
                >
                  <Trash2 size={12} />
                </button>
              </div>
            )}
            {steps.map((step, index) => {
              const isCurrent = index === curIndex;
              return (
                <div
                  key={index}
                  className={cn(
                    "flex items-center gap-1.5 px-1.5 py-0.5 text-xs",
                    isCurrent ? "text-foreground" : "text-muted-foreground",
                  )}
                  data-current={isCurrent ? "true" : undefined}
                  data-state={step.done ? "done" : "todo"}
                  data-testid="plan-step"
                >
                  <Checkbox checked={step.done} className="size-3.5 shrink-0" disabled />
                  <span
                    className={cn("min-w-0 flex-1 truncate", step.done && "line-through")}
                    title={step.description}
                  >
                    {step.description}
                  </span>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
