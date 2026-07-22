import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

export interface PreviewCardProps {
  /** Kind glyph, matching the real ActivityCard logo well. */
  logo: ReactNode;
  /** What will appear here once the agent starts working. */
  title: string;
  /** Secondary line describing the action you'll approve. */
  meta: string;
  /**
   * `false` (default): a dashed, dimmed placeholder — nothing has happened
   * yet and no service is connected. `true`: a service is connected, so the
   * card brightens to a solid "waiting for the first action" state. Either
   * way it is inert and non-actionable — a promise of what's coming, never a
   * real result.
   */
  ready?: boolean;
}

/**
 * A ghost card in the empty-state Stream: it shows the shape of the work the
 * agent will surface (a draft to send, a hold to confirm) before any real
 * card exists. Decorative — marked `aria-hidden` so assistive tech reads the
 * feed's lead line instead of announcing activity that hasn't happened.
 */
export default function PreviewCard({ logo, title, meta, ready = false }: PreviewCardProps) {
  return (
    <div
      data-testid="preview-card"
      data-ready={ready ? "true" : undefined}
      aria-hidden="true"
      className={cn(
        "flex items-center gap-3 rounded-xl border p-3.5 transition-all duration-500",
        ready
          ? "border-border bg-card opacity-90 shadow-sm"
          : "border-dashed border-border bg-card/40 opacity-55",
      )}
    >
      <span
        className={cn(
          "grid size-8 shrink-0 place-items-center rounded-lg border bg-muted text-muted-foreground",
          ready ? "border-border" : "border-dashed border-border",
        )}
      >
        {logo}
      </span>
      <div className="min-w-0 flex-1">
        <div className="text-sm font-medium">{title}</div>
        <div className="text-xs text-muted-foreground">{meta}</div>
      </div>
      <span className="shrink-0 text-[11px] font-medium tracking-wide text-muted-foreground uppercase">
        {ready ? "waiting" : "soon"}
      </span>
    </div>
  );
}
