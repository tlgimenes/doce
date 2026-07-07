import Dialog from "@/components/Dialog";
import { Button } from "@/components/ui/button";
import { KeyboardShortcut } from "@/components/ui/KeyboardShortcut";
import { XIcon } from "@phosphor-icons/react";
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
        <div className="mb-3 flex items-center justify-between">
          <h2 className="text-balance text-sm font-medium">Keyboard shortcuts</h2>
          <Button
            variant="ghost"
            size="icon-sm"
            className="text-muted-foreground hover:bg-transparent"
            onClick={onClose}
            data-testid="close-shortcuts-dialog"
            aria-label="Close dialog"
          >
            <XIcon size={16} />
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
              <KeyboardShortcut
                keys={[s.metaKey ? "⌘" : "", s.key.toUpperCase()]}
                data-testid={`shortcut-combo-${s.id}`}
              />
            </li>
          ))}
        </ul>
      </div>
    </Dialog>
  );
}
