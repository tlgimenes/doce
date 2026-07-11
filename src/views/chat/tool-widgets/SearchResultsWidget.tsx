import { ChevronRight, Search } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { Empty, EmptyDescription, EmptyHeader } from "@/components/ui/empty";
import {
  Item,
  ItemActions,
  ItemContent,
  ItemDescription,
  ItemMedia,
  ItemTitle,
} from "@/components/ui/item";
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
      <div
        data-slot="widget-frame"
        className="overflow-hidden rounded-lg border border-border bg-card text-sm"
        data-testid="search-widget"
      >
        <Item data-slot="widget-frame-header" size="xs" className="w-full">
          <ItemMedia variant="icon">
            <Search />
          </ItemMedia>
          <ItemContent>
            <ItemTitle title={detail.pattern ?? undefined}>
              {detail.toolName}{" "}
              <code data-slot="code-inline" className="font-mono text-xs">
                {detail.pattern}
              </code>
            </ItemTitle>
            <ItemDescription data-testid="search-interrupted">
              Interrupted — the app closed before this search finished
            </ItemDescription>
          </ItemContent>
          <Badge variant="outline">Interrupted</Badge>
        </Item>
      </div>
    );
  }

  const count = detail.matches.length;
  const countLabel = isGrep ? `${count} ${count === 1 ? "match" : "matches"}` : `${count} files`;

  return (
    <Collapsible
      data-slot="widget-frame"
      className="overflow-hidden rounded-lg border border-border bg-card text-sm"
      data-testid="search-widget"
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
          <Search />
        </ItemMedia>
        <ItemContent>
          <ItemTitle data-testid="search-summary" title={detail.pattern ?? undefined}>
            {detail.toolName}{" "}
            <code data-slot="code-inline" className="font-mono text-xs">
              {detail.pattern}
            </code>
          </ItemTitle>
        </ItemContent>
        <ItemActions>
          <Badge variant="outline">{countLabel}</Badge>
          {detail.tokenCount != null && (
            <Badge variant="outline">{formatTokenCount(detail.tokenCount)} tok</Badge>
          )}
        </ItemActions>
        <ChevronRight
          aria-hidden="true"
          data-slot="widget-frame-chevron"
          className="ml-auto size-4 shrink-0 text-muted-foreground transition-transform group-aria-expanded/widget-frame:rotate-90"
        />
      </CollapsibleTrigger>
      <CollapsibleContent
        data-slot="widget-frame-content"
        className="border-t border-border"
        data-testid="search-results"
      >
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
    <ItemDescription data-testid="search-context">
      <code data-slot="code-inline" className="font-mono text-xs">
        {parts.join(" · ")}
      </code>
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
