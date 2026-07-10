import { Terminal } from "lucide-react";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { CodeBlock, CodeInline } from "@/components/ui/code-block";
import { ItemContent, ItemDescription, ItemMedia, ItemTitle } from "@/components/ui/item";
import { Spinner } from "@/components/ui/spinner";
import { WidgetFrame, WidgetFrameContent, WidgetFrameHeader } from "@/components/ui/widget-frame";
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
      <WidgetFrame collapsible defaultOpen data-testid="bash-widget">
        <WidgetFrameHeader>
          <ItemMedia variant="icon">
            <Terminal />
          </ItemMedia>
          <ItemContent>
            <ItemTitle data-testid="bash-status">
              <Spinner role="presentation" aria-label={undefined} />
              Running…
            </ItemTitle>
          </ItemContent>
        </WidgetFrameHeader>
        <WidgetFrameContent>
          <CodeBlock data-testid="bash-command">$ {detail.command}</CodeBlock>
        </WidgetFrameContent>
      </WidgetFrame>
    );
  }

  if (!detail.outcome.ok) {
    return (
      <WidgetFrame data-testid="bash-widget">
        <WidgetFrameHeader>
          <ItemMedia variant="icon">
            <Terminal />
          </ItemMedia>
          <ItemContent>
            <ItemTitle data-testid="bash-command" title={`$ ${detail.command}`}>
              <CodeInline>$ {detail.command}</CodeInline>
            </ItemTitle>
            <ItemDescription data-testid="bash-status">Failed to run</ItemDescription>
          </ItemContent>
        </WidgetFrameHeader>
        <div className="p-3 pt-0">
          <Alert variant="destructive">
            <AlertDescription>{detail.outcome.error}</AlertDescription>
          </Alert>
        </div>
      </WidgetFrame>
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

  return (
    <WidgetFrame collapsible data-testid="bash-widget">
      <WidgetFrameHeader>
        <ItemMedia variant="icon">
          <Terminal />
        </ItemMedia>
        <ItemContent>
          <ItemTitle data-testid="bash-command" title={`$ ${detail.command}`}>
            <CodeInline>$ {detail.command}</CodeInline>
          </ItemTitle>
        </ItemContent>
        <span className="flex items-center gap-2" data-testid="bash-status">
          <Badge variant={succeeded ? "secondary" : "destructive"}>
            {succeeded ? "Success" : `Failed (exit ${exitCode})`}
          </Badge>
          <Badge variant="outline">
            exit {exitCode}
            {detail.tokenCount != null && ` · ${formatTokenCount(detail.tokenCount)} tok`}
          </Badge>
        </span>
      </WidgetFrameHeader>
      <WidgetFrameContent>
        {stdout && <CodeBlock data-testid="bash-stdout">{stdoutTrunc.shown}</CodeBlock>}
        {stderr && (
          <CodeBlock tone="destructive" data-testid="bash-stderr">
            {stderrTrunc.shown}
          </CodeBlock>
        )}
        {(stdoutTrunc.truncated || stderrTrunc.truncated) && (
          <ItemDescription className="px-3 py-1" data-testid="bash-output-truncated">
            Output truncated
          </ItemDescription>
        )}
        {payloadPath && <ViewFullOutput path={payloadPath} />}
      </WidgetFrameContent>
    </WidgetFrame>
  );
}
