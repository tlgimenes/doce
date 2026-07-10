import { useEffect, useMemo, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { commands, type Conversation, type SearchResult } from "@/lib/ipc";

interface SearchPanelProps {
  onSelect: (conversationId: string) => void;
  recentConversations?: Conversation[];
}

/**
 * Renders an FTS5 `snippet()` excerpt's `<mark>...</mark>` markers as real
 * `<mark>` elements without `dangerouslySetInnerHTML` — the excerpt is
 * built from the user's own message content, so blindly rendering it as
 * HTML would let arbitrary tags in a past message execute as markup.
 */
function highlightExcerpt(excerpt: string) {
  const parts = excerpt.split(/(<mark>.*?<\/mark>)/g);
  return parts.map((part, i) => {
    const match = part.match(/^<mark>(.*)<\/mark>$/);
    if (match) {
      return (
        <mark key={i} className="bg-primary/30 text-foreground">
          {match[1]}
        </mark>
      );
    }
    return <span key={i}>{part}</span>;
  });
}

/** User Story 6: FTS5-backed search across titles and message content. */
export default function SearchPanel({ onSelect, recentConversations = [] }: SearchPanelProps) {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<SearchResult[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [activeResultIndex, setActiveResultIndex] = useState(-1);
  const latestSearchRequestId = useRef(0);
  const recentResults = useMemo(
    () =>
      [...recentConversations]
        .sort((a, b) => b.updatedAt - a.updatedAt)
        .slice(0, 10)
        .map((conversation) => ({
          conversationId: conversation.id,
          title: conversation.title,
          excerpt: "Recent conversation",
          rank: 0,
        })),
    [recentConversations],
  );

  const runSearch = async (value: string) => {
    setQuery(value);
    setError(null);
    latestSearchRequestId.current += 1;
    const requestId = latestSearchRequestId.current;
    if (!value.trim()) {
      setResults([]);
      setActiveResultIndex(-1);
      setLoading(false);
      return;
    }
    setResults([]);
    setActiveResultIndex(-1);
    setLoading(true);
    try {
      const found = await commands.searchConversations(value);
      if (latestSearchRequestId.current !== requestId) return;
      setResults(found);
    } catch (err) {
      if (latestSearchRequestId.current !== requestId) return;
      setResults([]);
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      if (latestSearchRequestId.current === requestId) {
        setLoading(false);
      }
    }
  };

  const visibleResults = query.trim() ? results : recentResults;
  const activeDescendant =
    activeResultIndex >= 0 ? `search-result-option-${activeResultIndex}` : undefined;

  useEffect(() => {
    setActiveResultIndex((current) => {
      if (visibleResults.length === 0) return -1;
      if (current < 0) return current;
      return Math.min(current, visibleResults.length - 1);
    });
  }, [visibleResults]);

  const moveActiveResult = (direction: "next" | "previous") => {
    if (visibleResults.length === 0) return;

    setActiveResultIndex((current) => {
      if (direction === "next") {
        return current < 0 ? 0 : Math.min(current + 1, visibleResults.length - 1);
      }

      return current < 0 ? visibleResults.length - 1 : Math.max(current - 1, 0);
    });
  };

  const handleInputKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      moveActiveResult("next");
      return;
    }

    if (event.key === "ArrowUp") {
      event.preventDefault();
      moveActiveResult("previous");
      return;
    }

    if (event.key === "Enter" && activeResultIndex >= 0) {
      event.preventDefault();
      onSelect(visibleResults[activeResultIndex].conversationId);
    }
  };

  return (
    <div
      className="flex h-[28rem] max-h-[70vh] min-h-0 w-full flex-col bg-background p-4"
      data-testid="search-panel"
    >
      <div className="mb-3 flex">
        <input
          autoFocus
          className="flex-1 rounded-md border border-border bg-card px-3 py-2"
          aria-label="Search conversations"
          placeholder="Search conversations…"
          value={query}
          onChange={(e) => runSearch(e.target.value)}
          onKeyDown={handleInputKeyDown}
          role="combobox"
          aria-autocomplete="list"
          aria-controls="search-results"
          aria-expanded={visibleResults.length > 0}
          aria-activedescendant={activeDescendant}
          aria-haspopup="listbox"
          data-testid="search-input"
        />
      </div>
      <div id="search-results" role="listbox" className="flex-1 space-y-2 overflow-y-auto">
        {loading && (
          <p className="text-sm text-muted-foreground" data-testid="search-loading">
            Searching
          </p>
        )}
        {error && (
          <p className="text-sm text-destructive" data-testid="search-error">
            {error}
          </p>
        )}
        {visibleResults.map((r, index) => (
          <Button
            key={r.conversationId}
            id={`search-result-option-${index}`}
            variant="secondary"
            role="option"
            aria-selected={activeResultIndex === index}
            className={
              activeResultIndex === index
                ? "h-auto min-h-14 w-full flex-col items-start justify-start gap-0 bg-accent p-3 text-left ring-1 ring-ring"
                : "h-auto min-h-14 w-full flex-col items-start justify-start gap-0 p-3 text-left"
            }
            onClick={() => onSelect(r.conversationId)}
            onMouseMove={() => setActiveResultIndex(index)}
            data-testid="search-result"
          >
            <p className="text-sm font-medium">{r.title}</p>
            <p className="mt-1 text-xs text-muted-foreground">{highlightExcerpt(r.excerpt)}</p>
          </Button>
        ))}
        {query.trim() && !loading && !error && results.length === 0 && (
          <p className="text-sm text-muted-foreground">No results.</p>
        )}
      </div>
    </div>
  );
}
