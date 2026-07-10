import { Search } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { CodeInline } from "@/components/ui/code-block";
import { Empty, EmptyDescription, EmptyHeader } from "@/components/ui/empty";
import { ItemContent, ItemDescription, ItemMedia, ItemTitle } from "@/components/ui/item";
import { WidgetFrame, WidgetFrameContent, WidgetFrameHeader } from "@/components/ui/widget-frame";
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
      <WidgetFrame data-testid="search-widget">
        <WidgetFrameHeader>
          <ItemMedia variant="icon">
            <Search />
          </ItemMedia>
          <ItemContent>
            <ItemTitle>
              {detail.toolName} <CodeInline>{detail.pattern}</CodeInline>
            </ItemTitle>
            <ItemDescription data-testid="search-interrupted">
              Interrupted — the app closed before this search finished
            </ItemDescription>
          </ItemContent>
          <Badge variant="outline">Interrupted</Badge>
        </WidgetFrameHeader>
      </WidgetFrame>
    );
  }

  const count = detail.matches.length;
  const countLabel = isGrep ? `${count} ${count === 1 ? "match" : "matches"}` : `${count} files`;

  return (
    <WidgetFrame collapsible data-testid="search-widget">
      <WidgetFrameHeader>
        <ItemMedia variant="icon">
          <Search />
        </ItemMedia>
        <ItemContent>
          <ItemTitle data-testid="search-summary" title={detail.pattern ?? undefined}>
            {detail.toolName} <CodeInline>{detail.pattern}</CodeInline>
          </ItemTitle>
        </ItemContent>
        <span className="flex items-center gap-2">
          <Badge variant="outline">{countLabel}</Badge>
          {detail.tokenCount != null && (
            <Badge variant="outline">{formatTokenCount(detail.tokenCount)} tok</Badge>
          )}
        </span>
      </WidgetFrameHeader>
      <WidgetFrameContent data-testid="search-results">
        <div className="max-h-80 space-y-2 overflow-y-auto p-3">
          <SearchContext detail={detail} />
          {isGrep ? <GrepResults detail={detail} /> : <GlobResults detail={detail} />}
        </div>
      </WidgetFrameContent>
    </WidgetFrame>
  );
}

function SearchContext({ detail }: { detail: GlobDetail | GrepDetail }) {
  const parts = [
    detail.path ? `path: ${detail.path}` : null,
    detail.toolName === "Grep" && detail.glob ? `glob: ${detail.glob}` : null,
  ].filter(Boolean);

  if (parts.length === 0) return null;

  return (
    <ItemDescription data-testid="search-context">
      <CodeInline>{parts.join(" · ")}</CodeInline>
    </ItemDescription>
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
          <CodeInline>{path}</CodeInline>
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
          <CodeInline>
            {m.path}:{m.lineNumber}: {m.line}
          </CodeInline>
        </li>
      ))}
    </ul>
  );
}
