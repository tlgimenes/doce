import type { GlobDetail, GrepDetail } from "@/lib/ipc";
import { formatTokenCount } from "@/lib/formatTokenCount";

interface SearchResultsWidgetProps {
  detail: GlobDetail | GrepDetail;
}

/** US4/FR-007: a match list for Glob (filenames) and Grep (file:line:content), not an undifferentiated data dump. */
export default function SearchResultsWidget({ detail }: SearchResultsWidgetProps) {
  const isGrep = detail.toolName === "Grep";

  return (
    <div
      className="rounded-lg border border-border bg-card p-3 text-sm"
      data-testid="search-widget"
    >
      <p className="mb-1 font-mono text-xs text-muted-foreground">
        {detail.toolName} {detail.pattern}
        {detail.tokenCount != null && <span> · {formatTokenCount(detail.tokenCount)} tok</span>}
      </p>
      {isGrep ? (
        detail.matches.length === 0 ? (
          <p className="text-xs text-muted-foreground" data-testid="search-no-matches">
            No matches found
          </p>
        ) : (
          <ul className="space-y-0.5 font-mono text-xs">
            {detail.matches.map((m, i) => (
              <li key={i} data-testid="search-match" className="truncate">
                {m.path}:{m.lineNumber}: {m.line}
              </li>
            ))}
          </ul>
        )
      ) : detail.matches.length === 0 ? (
        <p className="text-xs text-muted-foreground" data-testid="search-no-matches">
          No files matched
        </p>
      ) : (
        <ul className="space-y-0.5 font-mono text-xs">
          {detail.matches.map((path, i) => (
            <li key={i} data-testid="search-match" className="truncate">
              {path}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
