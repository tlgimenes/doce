import type { BashDetail } from "@/lib/ipc";

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
  if (!detail.outcome.ok) {
    return (
      <div
        className="overflow-hidden rounded-lg border border-destructive/40 bg-destructive/10"
        data-testid="bash-widget"
      >
        <p
          className="border-b border-destructive/40 px-3 py-1.5 font-mono text-xs text-destructive"
          data-testid="bash-status"
        >
          Failed to run
        </p>
        <p className="px-3 py-2 font-mono text-xs text-destructive" data-testid="bash-command">
          $ {detail.command}
        </p>
        <p className="px-3 pb-2 text-sm text-destructive">{detail.outcome.error}</p>
      </div>
    );
  }

  const { exitCode, stdout, stderr } = detail.outcome;
  const succeeded = exitCode === 0;
  const stdoutTrunc = truncatedLines(stdout);
  const stderrTrunc = truncatedLines(stderr);

  return (
    <div className="overflow-hidden rounded-lg border border-border" data-testid="bash-widget">
      <div
        className={`flex items-center justify-between border-b border-border px-3 py-1.5 font-mono text-xs ${
          succeeded
            ? "bg-emerald-500/10 text-emerald-700 dark:text-emerald-400"
            : "bg-destructive/10 text-destructive"
        }`}
        data-testid="bash-status"
      >
        <span>{succeeded ? "Success" : `Failed (exit ${exitCode})`}</span>
        <span>exit {exitCode}</span>
      </div>
      <pre
        className="overflow-x-auto whitespace-pre-wrap break-words bg-card px-3 py-2 font-mono text-xs"
        data-testid="bash-command"
      >
        $ {detail.command}
      </pre>
      {stdout && (
        <pre
          className="overflow-x-auto whitespace-pre-wrap break-words px-3 py-2 font-mono text-xs"
          data-testid="bash-stdout"
        >
          {stdoutTrunc.shown}
        </pre>
      )}
      {stderr && (
        <pre
          className="overflow-x-auto whitespace-pre-wrap break-words px-3 py-2 font-mono text-xs text-destructive"
          data-testid="bash-stderr"
        >
          {stderrTrunc.shown}
        </pre>
      )}
      {(stdoutTrunc.truncated || stderrTrunc.truncated) && (
        <p
          className="border-t border-border px-3 py-1 text-xs text-muted-foreground"
          data-testid="bash-output-truncated"
        >
          Output truncated
        </p>
      )}
    </div>
  );
}
