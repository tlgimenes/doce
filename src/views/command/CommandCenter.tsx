import Dialog from "@/components/Dialog";
import { Button } from "@/components/ui/button";
import { KeyboardShortcut } from "@/components/ui/KeyboardShortcut";
import { cn } from "@/lib/cn";

export interface CommandCenterAction {
  id: string;
  label: string;
  shortcut?: string;
  disabled?: boolean;
  run: () => void;
}

interface CommandCenterProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  actions: CommandCenterAction[];
}

export default function CommandCenter({ open, onOpenChange, actions }: CommandCenterProps) {
  return (
    <Dialog open={open} onClose={() => onOpenChange(false)}>
      <div className="w-[34rem] max-w-[90vw] p-2" data-testid="command-center">
        <div className="px-2 py-2 text-xs font-medium uppercase text-muted-foreground">
          Actions
        </div>
        <div className="space-y-1">
          {actions.map((action) => (
            <Button
              key={action.id}
              type="button"
              variant="ghost"
              className={cn("h-9 w-full justify-between px-2", action.disabled && "opacity-50")}
              disabled={action.disabled}
              onClick={() => {
                action.run();
                onOpenChange(false);
              }}
            >
              <span>{action.label}</span>
              {action.shortcut ? <KeyboardShortcut keys={action.shortcut.split("+")} /> : null}
            </Button>
          ))}
        </div>
      </div>
    </Dialog>
  );
}
