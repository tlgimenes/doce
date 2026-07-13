import { Search } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Marker, MarkerContent, MarkerIcon } from "@/components/ui/marker";
import type { GlobDetail, GrepDetail } from "@/lib/ipc";
import { formatTokenCount } from "@/lib/formatTokenCount";

interface SearchResultsWidgetProps {
  detail: GlobDetail | GrepDetail;
}

/**
 * US4/FR-007: an outcome sentence for Glob and Grep ("Found 12 matches
 * for `useChat`"), not a data dump — brief muted token info after the
 * line, the pattern in the hover title.
 */
export default function SearchResultsWidget({ detail }: SearchResultsWidgetProps) {
  const isGrep = detail.toolName === "Grep";

  if (detail.interrupted) {
    return (
      <Marker data-testid="search-widget">
        <MarkerIcon>
          <Search />
        </MarkerIcon>
        <MarkerContent className="flex min-w-0 flex-col">
          <span className="truncate" title={detail.pattern ?? undefined}>
            {detail.toolName}{" "}
            <code data-slot="code-inline" className="font-mono text-xs">
              {detail.pattern}
            </code>
          </span>
          <span data-testid="search-interrupted" className="text-xs">
            Interrupted — the app closed before this search finished
          </span>
        </MarkerContent>
        <Badge variant="outline" className="ml-auto shrink-0">
          Interrupted
        </Badge>
      </Marker>
    );
  }

  const count = detail.matches.length;

  return (
    <Marker data-testid="search-widget">
      <MarkerIcon>
        <Search />
      </MarkerIcon>
      <MarkerContent
        data-testid="search-summary"
        className="min-w-0 truncate"
        title={detail.pattern ?? undefined}
      >
        {isGrep ? (
          count === 0 ? (
            <>
              No matches for{" "}
              <code data-slot="code-inline" className="font-mono text-xs">
                {detail.pattern}
              </code>
            </>
          ) : (
            <>
              Found {count} {count === 1 ? "match" : "matches"} for{" "}
              <code data-slot="code-inline" className="font-mono text-xs">
                {detail.pattern}
              </code>
            </>
          )
        ) : count === 0 ? (
          "No files matched"
        ) : (
          `Found ${count} ${count === 1 ? "file" : "files"}`
        )}
      </MarkerContent>
      {detail.tokenCount != null && (
        <span data-testid="search-meta" className="shrink-0 self-end text-xs text-muted-foreground">
          {formatTokenCount(detail.tokenCount)} tok
        </span>
      )}
    </Marker>
  );
}
