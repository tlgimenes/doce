import { ChevronRight, Terminal } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { Marker, MarkerContent, MarkerIcon } from "@/components/ui/marker";
import { Spinner } from "@/components/ui/spinner";
import type { BashDetail } from "@/lib/ipc";
import { formatTokenCount } from "@/lib/formatTokenCount";
import ViewFullOutput from "./ViewFullOutput";

interface BashWidgetProps {
  detail: BashDetail;
}

const OUTPUT_LINE_CAP = 50;

function truncatedLines(text: string): { shown: string; truncated: boolean } {
  const lines = text.split("\n");
  if (lines.length <= OUTPUT_LINE_CAP) return { shown: text, truncated: false };
  return { shown: lines.slice(0, OUTPUT_LINE_CAP).join("\n"), truncated: true };
}

/**
 * US2/FR-003: command + output shown together, terminal-style — plain
 * monospace text rather than `xterm.js` (research.md § 6: this is a
 * static, already-complete result, not an interactive terminal).
 */
export default function BashWidget({ detail }: BashWidgetProps) {
  // Pending branch: outcome absent means the command is still running
  if (!detail.outcome) {
    return (
      <Collapsible data-testid="bash-widget" defaultOpen>
        <CollapsibleTrigger
          nativeButton={false}
          render={<Marker className="group/marker-row cursor-pointer" />}
        >
          <MarkerIcon>
            <Spinner role="presentation" aria-label={undefined} />
          </MarkerIcon>
          <MarkerContent className="flex min-w-0 flex-col">
            <span data-testid="bash-status" className="shimmer truncate">
              Running…
            </span>
            <span className="truncate text-xs" title={`$ ${detail.command}`}>
              $ {detail.command}
            </span>
          </MarkerContent>
          <ChevronRight
            aria-hidden="true"
            className="ml-auto size-4 shrink-0 transition-transform group-aria-expanded/marker-row:rotate-90"
          />
        </CollapsibleTrigger>
        <CollapsibleContent className="pl-6">
          <pre
            data-testid="bash-command"
            className="overflow-x-auto px-3 py-2 font-mono text-xs whitespace-pre-wrap wrap-break-word text-foreground"
          >
            $ {detail.command}
          </pre>
        </CollapsibleContent>
      </Collapsible>
    );
  }

  if (!detail.outcome.ok) {
    return (
      <Marker data-testid="bash-widget">
        <MarkerIcon>
          <Terminal />
        </MarkerIcon>
        <MarkerContent className="flex min-w-0 flex-col">
          <span data-testid="bash-command" className="truncate" title={`$ ${detail.command}`}>
            <code data-slot="code-inline" className="font-mono text-xs">
              $ {detail.command}
            </code>
          </span>
          <span data-testid="bash-status" className="text-xs">
            Failed to run
          </span>
          <span className="text-xs">{detail.outcome.error}</span>
        </MarkerContent>
        <Badge variant="destructive" className="ml-auto shrink-0">
          Failed
        </Badge>
      </Marker>
    );
  }

  const { exitCode } = detail.outcome;
  const succeeded = exitCode === 0;
  // New rows only carry a bounded preview (stdoutPreview/stderrPreview);
  // legacy rows persisted before the payload-files design still carry the
  // full stdout/stderr inline.
  const stdout = detail.outcome.stdoutPreview ?? detail.outcome.stdout ?? "";
  const stderr = detail.outcome.stderrPreview ?? detail.outcome.stderr ?? "";
  const payloadPath = detail.payloadRef ?? detail.offloadedTo;
  const stdoutTrunc = truncatedLines(stdout);
  const stderrTrunc = truncatedLines(stderr);
  const isEmpty =
    !stdout && !stderr && !payloadPath && !stdoutTrunc.truncated && !stderrTrunc.truncated;

  // Success is the quiet default — only failure earns a collapsed-row
  // badge; exit code and token count live in the expanded panel's footer.
  const statusBadges = !succeeded && <Badge variant="destructive">Failed (exit {exitCode})</Badge>;
  const meta = [
    `exit ${exitCode}`,
    detail.tokenCount != null ? `${formatTokenCount(detail.tokenCount)} tok` : null,
  ]
    .filter(Boolean)
    .join(" · ");

  // Nothing to expand into: render the header-only, non-collapsible frame
  // rather than a Collapsible whose panel would just be empty.
  if (isEmpty) {
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
        <span data-testid="bash-status" className="ml-auto flex shrink-0 items-center gap-2">
          {statusBadges}
        </span>
      </Marker>
    );
  }

  return (
    <Collapsible data-testid="bash-widget">
      <CollapsibleTrigger
        nativeButton={false}
        render={<Marker className="group/marker-row cursor-pointer" />}
      >
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
        <span data-testid="bash-status" className="ml-auto flex shrink-0 items-center gap-2">
          {statusBadges}
          <ChevronRight
            aria-hidden="true"
            className="size-4 shrink-0 transition-transform group-aria-expanded/marker-row:rotate-90"
          />
        </span>
      </CollapsibleTrigger>
      <CollapsibleContent className="pl-6">
        {stdout && (
          <pre
            data-testid="bash-stdout"
            className="overflow-x-auto px-3 py-2 font-mono text-xs whitespace-pre-wrap wrap-break-word text-foreground"
          >
            {stdoutTrunc.shown}
          </pre>
        )}
        {stderr && (
          <pre
            data-testid="bash-stderr"
            className="overflow-x-auto px-3 py-2 font-mono text-xs whitespace-pre-wrap wrap-break-word text-destructive"
          >
            {stderrTrunc.shown}
          </pre>
        )}
        {(stdoutTrunc.truncated || stderrTrunc.truncated) && (
          <p
            className="px-3 py-1 text-xs text-muted-foreground"
            data-testid="bash-output-truncated"
          >
            Output truncated
          </p>
        )}
        {payloadPath && <ViewFullOutput path={payloadPath} />}
        <p data-testid="bash-meta" className="px-3 py-1 text-xs text-muted-foreground">
          {meta}
        </p>
      </CollapsibleContent>
    </Collapsible>
  );
}
