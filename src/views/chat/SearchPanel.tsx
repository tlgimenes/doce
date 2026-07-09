import { useMemo, useState } from "react";
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
    if (!value.trim()) {
      setResults([]);
      setLoading(false);
      return;
    }
    setLoading(true);
    try {
      const found = await commands.searchConversations(value);
      setResults(found);
    } catch (err) {
      setResults([]);
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  };

  const visibleResults = query.trim() ? results : recentResults;

  return (
    <div
      className="flex h-[28rem] max-h-[70vh] min-h-0 w-full flex-col bg-background p-4"
      data-testid="search-panel"
    >
      <div className="mb-3 flex">
        <input
          autoFocus
          className="flex-1 rounded-md border border-border bg-card px-3 py-2"
          placeholder="Search conversations…"
          value={query}
          onChange={(e) => runSearch(e.target.value)}
          data-testid="search-input"
        />
      </div>
      <div className="flex-1 space-y-2 overflow-y-auto">
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
        {visibleResults.map((r) => (
          <Button
            key={r.conversationId}
            variant="secondary"
            className="w-full flex-col items-start justify-start gap-0 p-3 text-left"
            onClick={() => onSelect(r.conversationId)}
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
