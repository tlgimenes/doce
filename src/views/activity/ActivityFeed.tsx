import type { ReactNode } from "react";

export interface ActivityFeedProps {
  /** A live WorkingCard rendered above every group, if present. */
  working?: ReactNode;
  /** Action-required cards — floated to the top under a counted header. */
  needsYou?: ReactNode[];
  /** FYI / already-handled cards. */
  earlier?: ReactNode[];
  /**
   * Shown when there is nothing in either group — the feed's empty state,
   * which doubles as the login surface (connect cards).
   */
  emptyState?: ReactNode;
}

function GroupHead({ label, count, subtle }: { label: string; count?: number; subtle?: boolean }) {
  return (
    <div className="mb-3 flex items-center gap-2.5">
      <span
        className={`text-xs font-bold tracking-wide uppercase ${
          subtle ? "text-muted-foreground" : "text-foreground"
        }`}
      >
        {label}
      </span>
      {count != null && (
        <span
          data-testid="feed-count-pill"
          className="inline-flex min-w-[18px] justify-center rounded-full bg-primary px-1.5 py-px text-[11px] font-semibold text-primary-foreground"
        >
          {count}
        </span>
      )}
      <span className="h-px flex-1 bg-border" />
    </div>
  );
}

/**
 * Lays out activity cards grouped by "Needs you" (counted) then "Earlier",
 * with an optional live WorkingCard on top. When both groups are empty it
 * renders `emptyState` (the connect surface). Presentational — it arranges
 * cards the caller supplies; it does not own their data.
 */
export default function ActivityFeed({
  working,
  needsYou = [],
  earlier = [],
  emptyState,
}: ActivityFeedProps) {
  const isEmpty = !working && needsYou.length === 0 && earlier.length === 0;

  if (isEmpty) {
    return (
      <div data-testid="activity-feed" className="flex flex-col">
        {emptyState}
      </div>
    );
  }

  return (
    <div data-testid="activity-feed" className="flex flex-col">
      {working}

      {needsYou.length > 0 && (
        <section className="mb-2">
          <GroupHead label="Needs you" count={needsYou.length} />
          <div className="flex flex-col gap-3">{needsYou}</div>
        </section>
      )}

      {earlier.length > 0 && (
        <section className="mt-5">
          <GroupHead label="Earlier" subtle />
          <div className="flex flex-col gap-3">{earlier}</div>
        </section>
      )}
    </div>
  );
}
