import type { ReactNode } from "react";
import { ArrowUp, Plus } from "lucide-react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

export interface ConnectionChip {
  label: string;
  /** Optional leading glyph (e.g. the Google "G"). */
  icon?: ReactNode;
  /** Live (connected) → green dot; otherwise a muted dot. */
  live?: boolean;
}

export interface FeedComposerProps {
  placeholder?: string;
  /** Connected services shown as chips with a live dot. */
  connections?: ConnectionChip[];
  onAdd?: () => void;
  onSend?: () => void;
}

/**
 * The top-pinned command bar: a rounded input placeholder, a "+"
 * affordance, the live connection chips, and a send button. Presentational
 * only — this renders a static placeholder, not a live editor.
 */
export default function FeedComposer({
  placeholder = "Ask doce, or describe a task…",
  connections = [],
  onAdd,
  onSend,
}: FeedComposerProps) {
  return (
    <div
      data-testid="feed-composer"
      className="rounded-2xl border border-border bg-card p-3 shadow-sm"
    >
      <div className="px-0.5 pt-0.5 pb-2.5 text-sm text-muted-foreground">{placeholder}</div>
      <div className="flex items-center gap-2">
        <Button
          type="button"
          variant="ghost"
          size="icon-sm"
          aria-label="Add context"
          onClick={onAdd}
        >
          <Plus />
        </Button>

        <div className="flex min-w-0 flex-wrap items-center gap-1.5">
          {connections.length === 0 ? (
            <span className="inline-flex items-center gap-1.5 rounded-full border border-border px-2 py-0.5 text-xs text-muted-foreground">
              <span className="size-1.5 rounded-full bg-muted-foreground/40" />
              No connections
            </span>
          ) : (
            connections.map((chip) => (
              <span
                key={chip.label}
                data-testid="composer-connection-chip"
                className="inline-flex items-center gap-1.5 rounded-full border border-border px-2 py-0.5 text-xs text-foreground"
              >
                {chip.icon}
                {chip.label}
                <span
                  className={cn(
                    "size-1.5 rounded-full",
                    chip.live ? "bg-emerald-500" : "bg-muted-foreground/40",
                  )}
                />
              </span>
            ))
          )}
        </div>

        <span className="flex-1" />

        <Button type="button" size="icon-sm" aria-label="Send" onClick={onSend}>
          <ArrowUp />
        </Button>
      </div>
    </div>
  );
}
