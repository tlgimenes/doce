import { useEffect, useRef, useState } from "react";
import { homeDir } from "@tauri-apps/api/path";
import { open } from "@tauri-apps/plugin-dialog";
import { FolderSimpleIcon } from "@phosphor-icons/react";
import {
  commands,
  type FolderSearchResult,
  type Workspace,
} from "@/lib/ipc";
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
  const normalizedHome = homePath.endsWith("/") && homePath.length > 1 ? homePath.slice(0, -1) : homePath;
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
 */
export default function FolderPicker({ currentPath, onSelect, onDismiss }: FolderPickerProps) {
  const [homePath, setHomePath] = useState<string | null>(null);
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  const [searchResults, setSearchResults] = useState<FolderSearchResult[]>([]);
  const [filter, setFilter] = useState("");
  const [selectedIndex, setSelectedIndex] = useState(-1);
  const containerRef = useRef<HTMLDivElement>(null);
  const filterInputRef = useRef<HTMLInputElement>(null);
  const searchSeq = useRef(0);

  useEffect(() => {
    homeDir().then(setHomePath);
    commands.listWorkspaces().then(setWorkspaces);
  }, []);

  useEffect(() => {
    filterInputRef.current?.focus();
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
    needle.startsWith("/") || needle.startsWith("~") || needle.length >= FOLDER_SEARCH_MIN_QUERY_LENGTH;
  const filterMode = filter.trim();
  const isPathMode = filterMode.startsWith("/") || filterMode.startsWith("~");
  const rows = (showSearchResults ? searchResults : workspaces).filter(
    (candidate) => !homePath || candidate.path !== homePath,
  );

  useEffect(() => {
    if (rows.length === 0) {
      setSelectedIndex(-1);
      return;
    }
    setSelectedIndex((current) => {
      if (current < 0 || current >= rows.length) return 0;
      return current;
    });
  }, [rows]);

  const getSelected = () => {
    if (selectedIndex < 0 || selectedIndex >= rows.length) return undefined;
    return rows[selectedIndex];
  };

  const selectCurrent = () => {
    const selected = getSelected();
    if (!selected) return;
    onSelect({
      kind: "recent",
      path: selected.path,
      displayLabel: formatDisplayLabel(selected.path, homePath),
    });
  };

  const moveSelection = (delta: number) => {
    if (rows.length === 0) return;
    setSelectedIndex((current) => {
      const base = current < 0 ? 0 : current;
      return (base + delta + rows.length) % rows.length;
    });
  };

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
      className="absolute -mt-1 z-10 w-72 rounded-2xl border border-border bg-card p-2 shadow-lg"
    >
      <input
        ref={filterInputRef}
        className="mb-2 w-full rounded-md bg-transparent px-2 py-1 text-sm outline-none focus-visible:outline-none"
        value={filter}
        onChange={(e) => setFilter(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "ArrowDown") {
            e.preventDefault();
            moveSelection(1);
            return;
          }
          if (e.key === "ArrowUp") {
            e.preventDefault();
            moveSelection(-1);
            return;
          }
          if (e.key !== "Enter") return;
          if (selectedIndex < 0 || rows.length === 0) return;
          e.preventDefault();
          selectCurrent();
        }}
        placeholder="Filter folders…"
        data-testid="folder-picker-filter"
      />
      <ul className="max-h-64 space-y-0.5 overflow-y-auto">
        {/* 008-shared-design-system exemption (applies to all three buttons
            in this popover): compact icon+label list rows and a full-width
            list-item-styled action, not standard button shapes — the
            shared Button component's variants don't have a natural fit for
            a dense picker list; the hand-tuned look here is intentionally
            kept rather than migrated (FR-008 exemption, per T018). */}
        {rows.map((w, index) => {
          const path = w.path;
          const displayName = w.displayName;
          const displayLabel = getSearchResultLabel(w);
          const isSelected = selectedIndex === index;

          return (
            <li key={path}>
              <button
                type="button"
                className={`flex w-full items-center gap-0 truncate rounded px-2 py-1 text-left text-sm hover:bg-muted ${
                  isSelected ? "bg-muted/80 font-medium" : ""
                }`}
                aria-current={currentPath === path}
                aria-selected={isSelected}
                onClick={() => {
                  setSelectedIndex(index);
                  onSelect({ kind: "recent", path, displayLabel: formatDisplayLabel(path, homePath) });
                }}
                data-testid="folder-picker-item"
                title={path}
              >
                <FolderSimpleIcon className="mr-1.5 shrink-0 text-muted-foreground" size={15} />
                {displayLabel.prefix ? (
                  <>
                    <span className="font-semibold">{displayLabel.prefix}</span>
                    {displayLabel.suffix ? <span>{displayLabel.suffix}</span> : null}
                  </>
                ) : (
                  displayName
                )}
              </button>
            </li>
          );
        })}
      </ul>
      <button
        type="button"
        className="mt-1 w-full rounded px-2 py-1 text-left text-sm text-muted-foreground hover:bg-muted"
        onClick={browse}
        data-testid="folder-picker-browse"
      >
        Browse…
      </button>
    </div>
  );
}
