import { Terminal } from "lucide-react";
import { Marker, MarkerContent, MarkerIcon } from "@/components/ui/marker";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type { BashDetail } from "@/lib/ipc";
import { formatTokenCount } from "@/lib/formatTokenCount";

interface BashWidgetProps {
  detail: BashDetail;
}

/**
 * US2/FR-003: a single terminal-style command line — the command in
 * monospace with brief muted exit/token info after it. Output is never
 * rendered in the transcript; the command line is the record of what ran.
 */
export default function BashWidget({ detail }: BashWidgetProps) {
  // Pending branch: outcome absent means the command is still running —
  // the shimmering command IS the running signal.
  if (!detail.outcome) {
    return (
      <Marker data-testid="bash-widget">
        <MarkerIcon>
          <Terminal />
        </MarkerIcon>
        <MarkerContent
          data-testid="bash-command"
          className="min-w-0 truncate"
          title={`$ ${detail.command}`}
        >
          <span data-testid="bash-status" className="shimmer">
            <code data-slot="code-inline" className="font-mono text-xs">
              $ {detail.command}
            </code>
          </span>
        </MarkerContent>
      </Marker>
    );
  }

  if (!detail.outcome.ok) {
    // Dispatch-level rejection (e.g. a denylisted command): the command
    // never ran, so there's no exit code — a single line with a discreet
    // amber "denied" note; the backend's rejection reason lives in its
    // tooltip.
    return (
      <Marker data-testid="bash-widget">
        <MarkerIcon>
          <Terminal />
        </MarkerIcon>
        <MarkerContent
          data-testid="bash-command"
          className="min-w-0 truncate"
          title={`$ ${detail.command}`}
        >
          <code data-slot="code-inline" className="font-mono text-xs">
            $ {detail.command}
          </code>
        </MarkerContent>
        <Tooltip>
          <TooltipTrigger
            render={
              <span
                data-testid="bash-denied"
                className="shrink-0 self-end text-xs text-amber-600 dark:text-amber-400"
              />
            }
          >
            denied
          </TooltipTrigger>
          <TooltipContent data-testid="bash-denied-tooltip">{detail.outcome.error}</TooltipContent>
        </Tooltip>
      </Marker>
    );
  }

  const { exitCode } = detail.outcome;
  const succeeded = exitCode === 0;

  return (
    <Marker data-testid="bash-widget">
      <MarkerIcon>
        <Terminal />
      </MarkerIcon>
      <MarkerContent
        data-testid="bash-command"
        className="min-w-0 truncate"
        title={`$ ${detail.command}`}
      >
        <code data-slot="code-inline" className="font-mono text-xs">
          $ {detail.command}
        </code>
      </MarkerContent>
      <span data-testid="bash-meta" className="shrink-0 self-end text-xs text-muted-foreground">
        {/* Success is the quiet default — a failure just paints the exit
            code segment in danger, nothing louder. */}
        <span data-testid="bash-exit" className={succeeded ? undefined : "text-destructive"}>
          exit {exitCode}
        </span>
        {detail.tokenCount != null && <> · {formatTokenCount(detail.tokenCount)} tok</>}
      </span>
    </Marker>
  );
}
