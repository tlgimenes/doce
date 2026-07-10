import Dialog from "@/components/Dialog";
import { Button } from "@/components/ui/button";
import { KeyboardShortcut } from "@/components/ui/KeyboardShortcut";
import { X } from "lucide-react";
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
      <div className="w-full p-4" data-testid="shortcuts-dialog">
        <div className="mb-4 flex items-center justify-between">
          <h2 className="text-sm font-semibold">Keyboard shortcuts</h2>
          <Button
            variant="ghost"
            size="icon-sm"
            className="text-muted-foreground hover:bg-accent"
            onClick={onClose}
            data-testid="close-shortcuts-dialog"
            aria-label="Close dialog"
          >
            <X size={16} />
          </Button>
        </div>
        <ul className="space-y-1">
          {shortcuts.map((s) => (
            <li
              key={s.id}
              className="flex items-center justify-between rounded-md px-2 py-1.5 text-sm"
              data-testid="shortcut-item"
            >
              <span className="text-muted-foreground">{s.description}</span>
              <KeyboardShortcut keys={s.combo.split("+")} data-testid={`shortcut-combo-${s.id}`} />
            </li>
          ))}
        </ul>
      </div>
    </Dialog>
  );
}
