import { useEffect, useRef, useState } from "react";
import { homeDir } from "@tauri-apps/api/path";
import { open } from "@tauri-apps/plugin-dialog";
import { Folder } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Command, CommandInput, CommandItem, CommandList } from "@/components/ui/command";
import { commands, type FolderSearchResult, type Workspace } from "@/lib/ipc";
import type { FolderTarget } from "@/views/chat/EmptyState";

const FOLDER_SEARCH_LIMIT = 10;
const FOLDER_SEARCH_MIN_QUERY_LENGTH = 2;

export interface FolderPickerProps {
  currentPath: string;
  onSelect: (target: FolderTarget) => void;
  onDismiss: () => void;
}

const formatDisplayLabel = (path: string, homePath: string | null) => {
  if (!homePath) return path;
  const normalizedHome =
    homePath.endsWith("/") && homePath.length > 1 ? homePath.slice(0, -1) : homePath;
  if (path === normalizedHome || path === `${normalizedHome}/`) return "Home";
  if (path.startsWith(`${normalizedHome}/`)) {
    return `~${path.slice(normalizedHome.length)}`;
  }
  return path;
};

/**
 * 006-chat-empty-state (US2/US3): recents + search + native-browse popover
 * for `EmptyState.tsx`'s folder-target selector. "Home" is synthesized
 * client-side (data-model.md's Recent Folders List) rather than being a
 * `Workspace` row, and is represented through the user's own folder path
 * (which is intentionally removed from results so Home is never duplicated).
 *
 * Built on the shared cmdk-based Command primitives: cmdk owns row
 * highlighting and Arrow/Enter keyboard navigation; `shouldFilter` is off
 * because rows come pre-filtered from the backend (`searchFolders`) rather
 * than from client-side matching.
 */
export default function FolderPicker({ currentPath, onSelect, onDismiss }: FolderPickerProps) {
  const [homePath, setHomePath] = useState<string | null>(null);
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  const [searchResults, setSearchResults] = useState<FolderSearchResult[]>([]);
  const [filter, setFilter] = useState("");
  const containerRef = useRef<HTMLDivElement>(null);
  const searchSeq = useRef(0);

  useEffect(() => {
    homeDir().then(setHomePath);
    commands.listWorkspaces().then(setWorkspaces);
  }, []);

  useEffect(() => {
    const needle = filter.trim();
    const isFullPathSearch = needle.startsWith("/") || needle.startsWith("~");

    if (!isFullPathSearch && needle.length < FOLDER_SEARCH_MIN_QUERY_LENGTH) {
      setSearchResults([]);
      return;
    }

    const requestId = ++searchSeq.current;
    commands
      .searchFolders(needle, FOLDER_SEARCH_LIMIT)
      .then((page) => {
        if (requestId !== searchSeq.current) return;
        setSearchResults(page.folders);
      })
      .catch(() => {
        if (requestId !== searchSeq.current) return;
        setSearchResults([]);
      });
  }, [filter]);

  useEffect(() => {
    function onPointerDown(e: MouseEvent) {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        onDismiss();
      }
    }
    function onKeyDown(e: KeyboardEvent) {
      if (e.key === "Escape") onDismiss();
    }
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [onDismiss]);

  const needle = filter.trim().toLowerCase();
  const showSearchResults =
    needle.startsWith("/") ||
    needle.startsWith("~") ||
    needle.length >= FOLDER_SEARCH_MIN_QUERY_LENGTH;
  const filterMode = filter.trim();
  const isPathMode = filterMode.startsWith("/") || filterMode.startsWith("~");
  const rows = (showSearchResults ? searchResults : workspaces).filter(
    (candidate) => !homePath || candidate.path !== homePath,
  );

  const getSearchResultLabel = (row: Workspace | FolderSearchResult) => {
    if (!isPathMode) return { prefix: "", suffix: row.displayName };

    const userPrefix = filterMode;

    if (homePath && filterMode.startsWith("~")) {
      const expandedPrefix = filterMode === "~" ? homePath : `${homePath}${filterMode.slice(1)}`;
      if (row.path.startsWith(expandedPrefix)) {
        return {
          prefix: userPrefix,
          suffix: row.path.slice(expandedPrefix.length),
        };
      }
    }

    if (row.path.startsWith(filterMode)) {
      return {
        prefix: userPrefix,
        suffix: row.path.slice(filterMode.length),
      };
    }

    return { prefix: "", suffix: row.displayName };
  };

  // FR-008: browsing isn't limited to the recent list. A cancelled dialog
  // (`null`) leaves the current target untouched — it does not even close
  // the picker, mirroring "opening or changing the picker alone MUST NOT
  // create or modify anything" (FR-009).
  const browse = async () => {
    const picked = await open({ directory: true });
    if (!picked) return;
    const displayLabel = formatDisplayLabel(picked, homePath);
    onSelect({ kind: "browsed", path: picked, displayLabel });
  };

  return (
    <div
      ref={containerRef}
      data-testid="folder-picker"
      className="absolute -mt-1 z-10 w-72 rounded-md border border-border shadow-md"
    >
      <Command shouldFilter={false} loop label="Folder picker" className="rounded-md">
        <CommandInput
          autoFocus
          value={filter}
          onValueChange={setFilter}
          placeholder="Filter folders…"
          data-testid="folder-picker-filter"
        />
        <CommandList className="max-h-64 p-1">
          {rows.map((w) => {
            const path = w.path;
            const displayName = w.displayName;
            const displayLabel = getSearchResultLabel(w);
            const isCurrent = currentPath === path;

            return (
              <CommandItem
                key={path}
                value={path}
                aria-current={isCurrent}
                data-checked={isCurrent}
                onSelect={() =>
                  onSelect({
                    kind: "recent",
                    path,
                    displayLabel: formatDisplayLabel(path, homePath),
                  })
                }
                data-testid="folder-picker-item"
                title={path}
              >
                <Folder className="text-muted-foreground" />
                <span className="min-w-0 truncate">
                  {displayLabel.prefix ? (
                    <>
                      <span className="font-semibold">{displayLabel.prefix}</span>
                      {displayLabel.suffix ? <span>{displayLabel.suffix}</span> : null}
                    </>
                  ) : (
                    displayName
                  )}
                </span>
              </CommandItem>
            );
          })}
        </CommandList>
        <div className="p-1 pt-0">
          <Button
            variant="ghost"
            size="sm"
            className="w-full justify-start px-2 font-normal text-muted-foreground"
            onClick={browse}
            data-testid="folder-picker-browse"
          >
            Browse…
          </Button>
        </div>
      </Command>
    </div>
  );
}
