import { ChevronRight, Search } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { Empty, EmptyDescription, EmptyHeader } from "@/components/ui/empty";
import { Marker, MarkerContent, MarkerIcon } from "@/components/ui/marker";
import type { GlobDetail, GrepDetail } from "@/lib/ipc";
import { formatTokenCount } from "@/lib/formatTokenCount";

interface SearchResultsWidgetProps {
  detail: GlobDetail | GrepDetail;
}

/** US4/FR-007: a match list for Glob (filenames) and Grep (file:line:content), not an undifferentiated data dump. */
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
  const countLabel = isGrep ? `${count} ${count === 1 ? "match" : "matches"}` : `${count} files`;

  return (
    <Collapsible data-testid="search-widget">
      <CollapsibleTrigger
        nativeButton={false}
        render={<Marker className="group/marker-row cursor-pointer" />}
      >
        <MarkerIcon>
          <Search />
        </MarkerIcon>
        <MarkerContent
          data-testid="search-summary"
          className="min-w-0 truncate"
          title={detail.pattern ?? undefined}
        >
          {detail.toolName}{" "}
          <code data-slot="code-inline" className="font-mono text-xs">
            {detail.pattern}
          </code>
        </MarkerContent>
        <span className="ml-auto flex shrink-0 items-center gap-2">
          <Badge variant="outline">{countLabel}</Badge>
          {detail.tokenCount != null && (
            <Badge variant="outline">{formatTokenCount(detail.tokenCount)} tok</Badge>
          )}
          <ChevronRight
            aria-hidden="true"
            className="size-4 shrink-0 transition-transform group-aria-expanded/marker-row:rotate-90"
          />
        </span>
      </CollapsibleTrigger>
      <CollapsibleContent className="pl-6" data-testid="search-results">
        <div className="max-h-80 space-y-2 overflow-y-auto p-3">
          <SearchContext detail={detail} />
          {isGrep ? <GrepResults detail={detail} /> : <GlobResults detail={detail} />}
        </div>
      </CollapsibleContent>
    </Collapsible>
  );
}

function SearchContext({ detail }: { detail: GlobDetail | GrepDetail }) {
  const parts = [
    detail.path ? `path: ${detail.path}` : null,
    detail.toolName === "Grep" && detail.glob ? `glob: ${detail.glob}` : null,
  ].filter(Boolean);

  if (parts.length === 0) return null;

  return (
    <p data-testid="search-context" className="text-xs text-muted-foreground">
      <code data-slot="code-inline" className="font-mono text-xs">
        {parts.join(" · ")}
      </code>
    </p>
  );
}

function GlobResults({ detail }: { detail: GlobDetail }) {
  if (detail.matches.length === 0) {
    return (
      <Empty data-testid="search-no-matches">
        <EmptyHeader>
          <EmptyDescription>No files matched</EmptyDescription>
        </EmptyHeader>
      </Empty>
    );
  }

  return (
    <ul className="space-y-0.5">
      {detail.matches.map((path, i) => (
        <li key={i} data-testid="search-match" className="truncate">
          <code data-slot="code-inline" className="font-mono text-xs">
            {path}
          </code>
        </li>
      ))}
    </ul>
  );
}

function GrepResults({ detail }: { detail: GrepDetail }) {
  if (detail.matches.length === 0) {
    return (
      <Empty data-testid="search-no-matches">
        <EmptyHeader>
          <EmptyDescription>No matches found</EmptyDescription>
        </EmptyHeader>
      </Empty>
    );
  }

  return (
    <ul className="space-y-0.5">
      {detail.matches.map((m, i) => (
        <li key={i} data-testid="search-match" className="truncate">
          <code data-slot="code-inline" className="font-mono text-xs">
            {m.path}:{m.lineNumber}: {m.line}
          </code>
        </li>
      ))}
    </ul>
  );
}
