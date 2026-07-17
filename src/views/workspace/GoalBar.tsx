import { useEffect, useState } from "react";
import { Pencil, Target } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { commands } from "@/lib/ipc";

interface GoalBarProps {
  conversationId: string;
}

/**
 * observer-verified completion + goals (final UI task): a compact topbar
 * control to set/edit/clear a conversation's goal — the same value
 * `send_agent_message` loads into `Plan.goal` at the start of the
 * conversation's next turn and the observer checks at `FinishTask`.
 * Mirrors `ContextUsageIndicator`'s styling and "never crash the topbar on
 * a failed invoke" convention (`WorkspaceTopbar.tsx`).
 */
export default function GoalBar({ conversationId }: GoalBarProps) {
  const [goal, setGoal] = useState<string | null>(null);
  // Distinguishes "still loading, render nothing yet" from "resolved (with
  // or without a goal), safe to show the affordance" -- avoids a flash of
  // "Set a goal" before the initial read comes back, same spirit as
  // ContextUsageIndicator withholding the gauge until `usage` exists.
  const [ready, setReady] = useState(false);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setReady(false);
    setEditing(false);

    // Defensive: a failed (or, in a host that hasn't wired this command
    // up, missing) `getConversationGoal` must never crash the topbar --
    // fall back to the unset-goal affordance rather than throwing.
    try {
      commands
        .getConversationGoal(conversationId)
        .then((loaded) => {
          if (cancelled) return;
          setGoal(loaded);
          setReady(true);
        })
        .catch(() => {
          if (!cancelled) {
            setGoal(null);
            setReady(true);
          }
        });
    } catch {
      setGoal(null);
      setReady(true);
    }

    return () => {
      cancelled = true;
    };
  }, [conversationId]);

  const startEditing = () => {
    setDraft(goal ?? "");
    setEditing(true);
  };

  const cancelEditing = () => {
    setEditing(false);
    setDraft("");
  };

  const save = async () => {
    const trimmed = draft.trim();
    const nextGoal = trimmed.length > 0 ? trimmed : null;
    setSaving(true);
    try {
      await commands.setConversationGoal(conversationId, nextGoal);
      setGoal(nextGoal);
      setEditing(false);
    } catch {
      // Leave the editor open with the failed draft in place so the user
      // can retry or cancel -- the topbar itself is unaffected.
    } finally {
      setSaving(false);
    }
  };

  if (!ready) return null;

  if (editing) {
    return (
      <div className="flex min-w-0 shrink-0 items-center gap-1" data-testid="goal-bar-editing">
        <Input
          autoFocus
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") void save();
            if (e.key === "Escape") cancelEditing();
          }}
          placeholder="Set a goal for this conversation"
          className="h-6 w-48 text-xs"
          disabled={saving}
          data-testid="goal-bar-input"
        />
        <Button
          size="xs"
          variant="ghost"
          onClick={() => void save()}
          disabled={saving}
          data-testid="goal-bar-save"
        >
          Save
        </Button>
        <Button
          size="xs"
          variant="ghost"
          onClick={cancelEditing}
          disabled={saving}
          data-testid="goal-bar-cancel"
        >
          Cancel
        </Button>
      </div>
    );
  }

  if (goal) {
    return (
      <Tooltip>
        <TooltipTrigger
          render={
            <button
              type="button"
              onClick={startEditing}
              className="group flex min-w-0 shrink-0 items-center gap-1 text-xs text-muted-foreground hover:text-foreground"
              data-testid="goal-bar-display"
            >
              <Target size={12} className="shrink-0" />
              <span className="max-w-40 truncate">{goal}</span>
              <Pencil
                size={11}
                className="shrink-0 opacity-0 transition-opacity group-hover:opacity-100"
              />
            </button>
          }
        />
        <TooltipContent>{goal}</TooltipContent>
      </Tooltip>
    );
  }

  return (
    <button
      type="button"
      onClick={startEditing}
      className="shrink-0 text-xs text-muted-foreground hover:text-foreground"
      data-testid="goal-bar-set-affordance"
    >
      Set a goal
    </button>
  );
}
