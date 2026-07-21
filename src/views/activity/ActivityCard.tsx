import type { ReactNode } from "react";
import { Check, FolderOpen, ScrollText, Send, Sparkles } from "lucide-react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

/** A proposed calendar slot; exactly one is expected to be `selected`. */
export interface TimeSlot {
  label: string;
  selected?: boolean;
}

interface ActivityCardCommon {
  /** Source/type glyph shown in the card's logo well. */
  logo: ReactNode;
  /** What the agent did, in plain words ("Draft reply — Re: Q3 roadmap"). */
  title: string;
  /** Secondary line ("to Sarah Chen", "45 min · 3 people free"). */
  meta?: string;
  /** The task that produced this card — rendered as a provenance chip. */
  provenance?: string;
  /** Relative time, e.g. "4m". */
  timestamp: string;
  /** Unread → accent border + dot; clears when the user acts or opens it. */
  unread?: boolean;
  /** Open the conversation that produced the card (provenance chip click). */
  onOpenProvenance?: () => void;
}

/** Email draft awaiting a Send. */
export interface DraftActivity extends ActivityCardCommon {
  kind: "draft";
  /** The drafted body, shown as an editable-looking preview. */
  bodyPreview?: string;
  onSend?: () => void;
  onEdit?: () => void;
  onDiscard?: () => void;
}

/** Calendar hold awaiting a Confirm. */
export interface EventActivity extends ActivityCardCommon {
  kind: "event";
  slots: TimeSlot[];
  onConfirm?: () => void;
  onOtherTime?: () => void;
  onDismiss?: () => void;
}

/** FYI file result with an optional "Open". */
export interface FileActivity extends ActivityCardCommon {
  kind: "file";
  onOpen?: () => void;
  onDismiss?: () => void;
}

/** FYI shell/command result with an optional "View log". */
export interface ShellActivity extends ActivityCardCommon {
  kind: "shell";
  onViewLog?: () => void;
  onDismiss?: () => void;
}

export type ActivityCardProps = DraftActivity | EventActivity | FileActivity | ShellActivity;

function CardShell({
  card,
  preview,
  actions,
}: {
  card: ActivityCardCommon;
  preview?: ReactNode;
  actions: ReactNode;
}) {
  return (
    <div
      data-testid="activity-card"
      className={cn(
        "relative rounded-xl border bg-card p-3.5 shadow-sm",
        card.unread ? "border-emerald-500/40" : "border-border",
      )}
    >
      {card.unread && (
        <span
          data-testid="activity-card-unread"
          aria-label="Unread"
          className="absolute top-4 -left-1 size-2 rounded-full bg-emerald-500 ring-3 ring-background"
        />
      )}

      <div className="mb-2.5 flex items-center gap-3">
        <span className="grid size-8 shrink-0 place-items-center rounded-lg border border-border bg-muted text-foreground">
          {card.logo}
        </span>
        <div className="min-w-0 flex-1">
          <div className="text-sm leading-tight font-semibold">{card.title}</div>
          <div className="mt-0.5 flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-muted-foreground">
            {card.meta && <span>{card.meta}</span>}
            {card.provenance && (
              <button
                type="button"
                onClick={card.onOpenProvenance}
                data-testid="activity-card-provenance"
                className="inline-flex items-center gap-1 rounded-full border border-border px-1.5 py-px text-muted-foreground transition-colors hover:text-foreground"
              >
                <Sparkles className="size-2.5" />
                {card.provenance}
              </button>
            )}
          </div>
        </div>
        <span className="shrink-0 text-xs text-muted-foreground">{card.timestamp}</span>
      </div>

      {preview}

      <div className="flex items-center gap-2">{actions}</div>
    </div>
  );
}

/**
 * A typed tool-call result surfaced as an actionable card. Each `kind`
 * carries its own preview + actions; the primary action is always the
 * human commit (Send / Confirm). Presentational — handlers are supplied by
 * the caller and default to no-ops in the gallery.
 */
export default function ActivityCard(props: ActivityCardProps) {
  switch (props.kind) {
    case "draft":
      return (
        <CardShell
          card={props}
          preview={
            props.bodyPreview ? (
              <div className="mb-3 rounded-lg border border-border bg-muted/50 px-3 py-2 text-xs leading-relaxed text-muted-foreground">
                {props.bodyPreview}
              </div>
            ) : undefined
          }
          actions={
            <>
              <Button type="button" size="sm" onClick={props.onSend}>
                <Send />
                Send
              </Button>
              <Button type="button" variant="outline" size="sm" onClick={props.onEdit}>
                Edit
              </Button>
              <span className="flex-1" />
              <Button type="button" variant="ghost" size="sm" onClick={props.onDiscard}>
                Discard
              </Button>
            </>
          }
        />
      );

    case "event": {
      const selected = props.slots.find((slot) => slot.selected) ?? props.slots[0];
      return (
        <CardShell
          card={props}
          preview={
            <div className="mb-3 flex flex-wrap gap-2">
              {props.slots.map((slot) => (
                <span
                  key={slot.label}
                  data-testid="activity-card-slot"
                  data-selected={slot.selected ? "true" : undefined}
                  className={cn(
                    "rounded-md border bg-muted/50 px-2.5 py-1 text-xs",
                    slot.selected
                      ? "border-foreground font-semibold text-foreground"
                      : "border-border text-foreground",
                  )}
                >
                  {slot.label}
                </span>
              ))}
            </div>
          }
          actions={
            <>
              <Button type="button" size="sm" onClick={props.onConfirm}>
                <Check />
                {selected ? `Confirm ${selected.label}` : "Confirm"}
              </Button>
              <Button type="button" variant="outline" size="sm" onClick={props.onOtherTime}>
                Other time
              </Button>
              <span className="flex-1" />
              <Button type="button" variant="ghost" size="sm" onClick={props.onDismiss}>
                Dismiss
              </Button>
            </>
          }
        />
      );
    }

    case "file":
      return (
        <CardShell
          card={props}
          actions={
            <>
              {props.onOpen && (
                <Button type="button" variant="outline" size="sm" onClick={props.onOpen}>
                  <FolderOpen />
                  Open
                </Button>
              )}
              <span className="flex-1" />
              <Button type="button" variant="ghost" size="sm" onClick={props.onDismiss}>
                Dismiss
              </Button>
            </>
          }
        />
      );

    case "shell":
      return (
        <CardShell
          card={props}
          actions={
            <>
              {props.onViewLog && (
                <Button type="button" variant="outline" size="sm" onClick={props.onViewLog}>
                  <ScrollText />
                  View log
                </Button>
              )}
              <span className="flex-1" />
              <Button type="button" variant="ghost" size="sm" onClick={props.onDismiss}>
                Dismiss
              </Button>
            </>
          }
        />
      );
  }
}
