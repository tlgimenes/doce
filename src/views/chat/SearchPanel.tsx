import { useState } from "react";
import { commands, type SearchResult } from "@/lib/ipc";

interface SearchPanelProps {
  onSelect: (conversationId: string) => void;
  onClose: () => void;
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
export default function SearchPanel({ onSelect, onClose }: SearchPanelProps) {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<SearchResult[]>([]);

  const runSearch = async (value: string) => {
    setQuery(value);
    if (!value.trim()) {
      setResults([]);
      return;
    }
    const found = await commands.searchConversations(value);
    setResults(found);
  };

  return (
    <div className="absolute inset-0 z-10 flex flex-col bg-background p-4" data-testid="search-panel">
      <div className="mb-3 flex gap-2">
        <input
          autoFocus
          className="flex-1 rounded-md border border-border bg-card px-3 py-2"
          placeholder="Search conversations…"
          value={query}
          onChange={(e) => runSearch(e.target.value)}
          data-testid="search-input"
        />
        <button className="rounded-md border border-border px-3 py-2 text-sm" onClick={onClose}>
          Close
        </button>
      </div>
      <div className="flex-1 space-y-2 overflow-y-auto">
        {results.map((r) => (
          <button
            key={r.conversationId}
            className="block w-full rounded-md border border-border p-3 text-left hover:bg-muted"
            onClick={() => onSelect(r.conversationId)}
            data-testid="search-result"
          >
            <p className="text-sm font-medium">{r.title}</p>
            <p className="mt-1 text-xs text-muted-foreground">{highlightExcerpt(r.excerpt)}</p>
          </button>
        ))}
        {query.trim() && results.length === 0 && (
          <p className="text-sm text-muted-foreground">No results.</p>
        )}
      </div>
    </div>
  );
}
