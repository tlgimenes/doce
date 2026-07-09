import type { GlobDetail, GrepDetail } from "@/lib/ipc";
import { formatTokenCount } from "@/lib/formatTokenCount";
import ToolDisclosure from "./ToolDisclosure";

interface SearchResultsWidgetProps {
  detail: GlobDetail | GrepDetail;
}

/** US4/FR-007: a match list for Glob (filenames) and Grep (file:line:content), not an undifferentiated data dump. */
export default function SearchResultsWidget({ detail }: SearchResultsWidgetProps) {
  const isGrep = detail.toolName === "Grep";

  if (detail.interrupted) {
    return (
      <div
        className="rounded-lg border border-border bg-card p-3 text-sm"
        data-testid="search-widget"
      >
        <p className="mb-1 font-mono text-xs text-muted-foreground">
          {detail.toolName} {detail.pattern}
          {detail.tokenCount != null && <span> · {formatTokenCount(detail.tokenCount)} tok</span>}
        </p>
        <p className="text-xs text-amber-600 dark:text-amber-400" data-testid="search-interrupted">
          Interrupted — the app closed before this search finished
        </p>
      </div>
    );
  }

  const count = detail.matches.length;
  const countLabel = isGrep ? `${count} ${count === 1 ? "match" : "matches"}` : `${count} files`;
  const tokenLabel =
    detail.tokenCount != null ? ` · ${formatTokenCount(detail.tokenCount)} tok` : "";

  return (
    <ToolDisclosure
      testId="search-widget"
      summaryTestId="search-summary"
      bodyTestId="search-results"
      summary={
        <span className="font-mono text-xs text-muted-foreground">
          {detail.toolName} {detail.pattern} · {countLabel}
          {tokenLabel}
        </span>
      }
      bodyClassName="space-y-2"
    >
      <SearchContext detail={detail} />
      {isGrep ? <GrepResults detail={detail} /> : <GlobResults detail={detail} />}
    </ToolDisclosure>
  );
}

function SearchContext({ detail }: { detail: GlobDetail | GrepDetail }) {
  const parts = [
    detail.path ? `path: ${detail.path}` : null,
    detail.toolName === "Grep" && detail.glob ? `glob: ${detail.glob}` : null,
  ].filter(Boolean);

  if (parts.length === 0) return null;

  return (
    <p className="font-mono text-xs text-muted-foreground" data-testid="search-context">
      {parts.join(" · ")}
    </p>
  );
}

function GlobResults({ detail }: { detail: GlobDetail }) {
  if (detail.matches.length === 0) {
    return (
      <p className="text-xs text-muted-foreground" data-testid="search-no-matches">
        No files matched
      </p>
    );
  }

  return (
    <ul className="space-y-0.5 font-mono text-xs">
      {detail.matches.map((path, i) => (
        <li key={i} data-testid="search-match" className="truncate">
          {path}
        </li>
      ))}
    </ul>
  );
}

function GrepResults({ detail }: { detail: GrepDetail }) {
  if (detail.matches.length === 0) {
    return (
      <p className="text-xs text-muted-foreground" data-testid="search-no-matches">
        No matches found
      </p>
    );
  }

  return (
    <ul className="space-y-0.5 font-mono text-xs">
      {detail.matches.map((m, i) => (
        <li key={i} data-testid="search-match" className="truncate">
          {m.path}:{m.lineNumber}: {m.line}
        </li>
      ))}
    </ul>
  );
}
