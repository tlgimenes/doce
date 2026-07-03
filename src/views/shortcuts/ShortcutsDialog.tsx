import Dialog from "@/components/Dialog";
import { Button } from "@/components/ui/button";
import type { Shortcut } from "@/lib/shortcuts";

export interface ShortcutsDialogProps {
  open: boolean;
  onClose: () => void;
  shortcuts: Shortcut[];
}

// Renders directly from the shared shortcuts registry (FR-010) — never a
// separate hardcoded description list that could drift from what's
// actually bound (research.md § 5).
export default function ShortcutsDialog({ open, onClose, shortcuts }: ShortcutsDialogProps) {
  return (
    <Dialog open={open} onClose={onClose}>
      <div className="w-80 p-4" data-testid="shortcuts-dialog">
        <div className="mb-3 flex items-center justify-between">
          <h2 className="text-balance text-sm font-medium">Keyboard shortcuts</h2>
          <Button
            variant="ghost"
            size="sm"
            className="p-0 text-muted-foreground underline hover:bg-transparent"
            onClick={onClose}
            data-testid="close-shortcuts-dialog"
          >
            Close
          </Button>
        </div>
        <ul className="space-y-2">
          {shortcuts.map((s) => (
            <li
              key={s.id}
              className="flex items-center justify-between text-sm"
              data-testid="shortcut-item"
            >
              <span className="text-muted-foreground">{s.description}</span>
              <kbd className="rounded border border-border bg-muted px-1.5 py-0.5 font-mono text-xs">
                {s.combo}
              </kbd>
            </li>
          ))}
        </ul>
      </div>
    </Dialog>
  );
}
