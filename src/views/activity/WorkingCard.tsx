import { Spinner } from "@/components/ui/spinner";
import { Button } from "@/components/ui/button";

export interface WorkingCardProps {
  /** The live task line, e.g. "Triaging inbox — 11 of 14 read". */
  title: string;
  /** Quiet sub-line, e.g. "reading with the local model · nothing left your Mac". */
  detail?: string;
  onStop?: () => void;
}

/**
 * A live, in-progress task pinned at the top of the feed: spinner + what's
 * happening + a Stop. Presentational — the caller drives the copy and
 * `onStop`.
 */
export default function WorkingCard({ title, detail, onStop }: WorkingCardProps) {
  return (
    <div
      data-testid="working-card"
      className="mb-3 flex items-center gap-3 rounded-xl border border-border bg-card p-3.5 shadow-sm"
    >
      <Spinner className="size-4 text-foreground" />
      <div className="min-w-0 flex-1">
        <div className="truncate text-sm">{title}</div>
        {detail && <div className="truncate text-xs text-muted-foreground">{detail}</div>}
      </div>
      <Button type="button" variant="ghost" size="sm" onClick={onStop}>
        Stop
      </Button>
    </div>
  );
}
