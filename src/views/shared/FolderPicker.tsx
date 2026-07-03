import { useEffect, useRef, useState } from "react";
import { homeDir } from "@tauri-apps/api/path";
import { open } from "@tauri-apps/plugin-dialog";
import { commands, type Workspace } from "@/lib/ipc";
import type { FolderTarget } from "@/views/chat/EmptyState";

export interface FolderPickerProps {
  currentPath: string;
  onSelect: (target: FolderTarget) => void;
  onDismiss: () => void;
}

/**
 * 006-chat-empty-state (US2/US3): recents + search + native-browse popover
 * for `EmptyState.tsx`'s folder-target selector. "Home" is synthesized
 * client-side (data-model.md's Recent Folders List) rather than being a
 * `Workspace` row, and is always shown regardless of the filter — it's
 * pinned, not a recent entry.
 */
export default function FolderPicker({ currentPath, onSelect, onDismiss }: FolderPickerProps) {
  const [homePath, setHomePath] = useState<string | null>(null);
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  const [filter, setFilter] = useState("");
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    homeDir().then(setHomePath);
    commands.listWorkspaces().then(setWorkspaces);
  }, []);

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
  const filtered = needle
    ? workspaces.filter(
        (w) => w.displayName.toLowerCase().includes(needle) || w.path.toLowerCase().includes(needle),
      )
    : workspaces;

  // FR-008: browsing isn't limited to the recent list. A cancelled dialog
  // (`null`) leaves the current target untouched — it does not even close
  // the picker, mirroring "opening or changing the picker alone MUST NOT
  // create or modify anything" (FR-009).
  const browse = async () => {
    const picked = await open({ directory: true });
    if (!picked) return;
    const displayLabel = picked.split("/").filter(Boolean).pop() ?? picked;
    onSelect({ kind: "browsed", path: picked, displayLabel });
  };

  return (
    <div
      ref={containerRef}
      data-testid="folder-picker"
      className="absolute z-10 w-72 rounded-md border border-border bg-card p-2 shadow-lg"
    >
      <input
        className="mb-2 w-full rounded-md border border-border bg-background px-2 py-1 text-sm"
        value={filter}
        onChange={(e) => setFilter(e.target.value)}
        placeholder="Filter folders…"
        data-testid="folder-picker-filter"
      />
      <ul className="max-h-64 space-y-0.5 overflow-y-auto">
        {homePath && (
          <li>
            <button
              type="button"
              className="w-full rounded px-2 py-1 text-left text-sm hover:bg-muted"
              aria-current={currentPath === homePath}
              onClick={() => onSelect({ kind: "home", path: homePath, displayLabel: "Home" })}
              data-testid="folder-picker-home"
            >
              Home
            </button>
          </li>
        )}
        {filtered.map((w) => (
          <li key={w.id}>
            <button
              type="button"
              className="w-full truncate rounded px-2 py-1 text-left text-sm hover:bg-muted"
              aria-current={currentPath === w.path}
              onClick={() => onSelect({ kind: "recent", path: w.path, displayLabel: w.displayName })}
              data-testid="folder-picker-item"
              title={w.path}
            >
              {w.displayName}
            </button>
          </li>
        ))}
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
