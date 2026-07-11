import { ChevronRight, Terminal } from "lucide-react";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import {
  Item,
  ItemActions,
  ItemContent,
  ItemDescription,
  ItemMedia,
  ItemTitle,
} from "@/components/ui/item";
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
      <Collapsible
        data-slot="widget-frame"
        defaultOpen
        className="overflow-hidden rounded-lg border border-border bg-card text-sm"
        data-testid="bash-widget"
      >
        <CollapsibleTrigger
          nativeButton={false}
          render={
            <Item
              data-slot="widget-frame-header"
              size="xs"
              className="group/widget-frame w-full cursor-pointer rounded-none hover:bg-accent"
            />
          }
        >
          <ItemMedia variant="icon">
            <Terminal />
          </ItemMedia>
          <ItemContent>
            <ItemTitle data-testid="bash-status">
              <Spinner role="presentation" aria-label={undefined} />
              Running…
            </ItemTitle>
          </ItemContent>
          <ChevronRight
            aria-hidden="true"
            data-slot="widget-frame-chevron"
            className="ml-auto size-4 shrink-0 text-muted-foreground transition-transform group-aria-expanded/widget-frame:rotate-90"
          />
        </CollapsibleTrigger>
        <CollapsibleContent data-slot="widget-frame-content" className="border-t border-border">
          <pre
            data-slot="code-block"
            data-tone="default"
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
      <div
        data-slot="widget-frame"
        className="overflow-hidden rounded-lg border border-border bg-card text-sm"
        data-testid="bash-widget"
      >
        <Item data-slot="widget-frame-header" size="xs" className="w-full">
          <ItemMedia variant="icon">
            <Terminal />
          </ItemMedia>
          <ItemContent>
            <ItemTitle data-testid="bash-command" title={`$ ${detail.command}`}>
              <code data-slot="code-inline" className="font-mono text-xs">
                $ {detail.command}
              </code>
            </ItemTitle>
            <ItemDescription data-testid="bash-status">Failed to run</ItemDescription>
          </ItemContent>
        </Item>
        <div className="p-3 pt-0">
          <Alert variant="destructive">
            <AlertDescription>{detail.outcome.error}</AlertDescription>
          </Alert>
        </div>
      </div>
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

  // Nothing to expand into: render the header-only, non-collapsible frame
  // rather than a Collapsible whose panel would just be empty.
  if (isEmpty) {
    return (
      <div
        data-slot="widget-frame"
        className="overflow-hidden rounded-lg border border-border bg-card text-sm"
        data-testid="bash-widget"
      >
        <Item data-slot="widget-frame-header" size="xs" className="w-full">
          <ItemMedia variant="icon">
            <Terminal />
          </ItemMedia>
          <ItemContent>
            <ItemTitle data-testid="bash-command" title={`$ ${detail.command}`}>
              <code data-slot="code-inline" className="font-mono text-xs">
                $ {detail.command}
              </code>
            </ItemTitle>
          </ItemContent>
          <ItemActions data-testid="bash-status">
            <Badge variant={succeeded ? "secondary" : "destructive"}>
              {succeeded ? "Success" : `Failed (exit ${exitCode})`}
            </Badge>
            <Badge variant="outline">
              exit {exitCode}
              {detail.tokenCount != null && ` · ${formatTokenCount(detail.tokenCount)} tok`}
            </Badge>
          </ItemActions>
        </Item>
      </div>
    );
  }

  return (
    <Collapsible
      data-slot="widget-frame"
      className="overflow-hidden rounded-lg border border-border bg-card text-sm"
      data-testid="bash-widget"
    >
      <CollapsibleTrigger
        nativeButton={false}
        render={
          <Item
            data-slot="widget-frame-header"
            size="xs"
            className="group/widget-frame w-full cursor-pointer rounded-none hover:bg-accent"
          />
        }
      >
        <ItemMedia variant="icon">
          <Terminal />
        </ItemMedia>
        <ItemContent>
          <ItemTitle data-testid="bash-command" title={`$ ${detail.command}`}>
            <code data-slot="code-inline" className="font-mono text-xs">
              $ {detail.command}
            </code>
          </ItemTitle>
        </ItemContent>
        <ItemActions data-testid="bash-status">
          <Badge variant={succeeded ? "secondary" : "destructive"}>
            {succeeded ? "Success" : `Failed (exit ${exitCode})`}
          </Badge>
          <Badge variant="outline">
            exit {exitCode}
            {detail.tokenCount != null && ` · ${formatTokenCount(detail.tokenCount)} tok`}
          </Badge>
        </ItemActions>
        <ChevronRight
          aria-hidden="true"
          data-slot="widget-frame-chevron"
          className="ml-auto size-4 shrink-0 text-muted-foreground transition-transform group-aria-expanded/widget-frame:rotate-90"
        />
      </CollapsibleTrigger>
      <CollapsibleContent data-slot="widget-frame-content" className="border-t border-border">
        {stdout && (
          <pre
            data-slot="code-block"
            data-tone="default"
            data-testid="bash-stdout"
            className="overflow-x-auto px-3 py-2 font-mono text-xs whitespace-pre-wrap wrap-break-word text-foreground"
          >
            {stdoutTrunc.shown}
          </pre>
        )}
        {stderr && (
          <pre
            data-slot="code-block"
            data-tone="destructive"
            data-testid="bash-stderr"
            className="overflow-x-auto px-3 py-2 font-mono text-xs whitespace-pre-wrap wrap-break-word text-destructive"
          >
            {stderrTrunc.shown}
          </pre>
        )}
        {(stdoutTrunc.truncated || stderrTrunc.truncated) && (
          <ItemDescription className="px-3 py-1" data-testid="bash-output-truncated">
            Output truncated
          </ItemDescription>
        )}
        {payloadPath && <ViewFullOutput path={payloadPath} />}
      </CollapsibleContent>
    </Collapsible>
  );
}
