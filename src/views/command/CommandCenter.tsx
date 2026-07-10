import Dialog from "@/components/Dialog";
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandShortcut,
} from "@/components/ui/command";

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
  const runAction = (action: CommandCenterAction) => {
    action.run();
    onOpenChange(false);
  };

  return (
    <Dialog
      open={open}
      onClose={() => onOpenChange(false)}
      title="Command center"
      description="Run application actions."
    >
      <div className="w-[34rem] max-w-[90vw]" data-testid="command-center">
        <Command className="rounded-lg border border-border/70 bg-popover p-0">
          <CommandInput autoFocus placeholder="Type a command or search" />
          <CommandList className="max-h-80 p-1">
            <CommandEmpty>No matching actions.</CommandEmpty>
            <CommandGroup heading="Actions">
          {actions.map((action) => (
            <CommandItem
              key={action.id}
              value={action.label}
              keywords={[action.id, action.shortcut ?? ""]}
              disabled={action.disabled}
              onSelect={() => runAction(action)}
            >
              <span>{action.label}</span>
              {action.shortcut ? <CommandShortcut>{action.shortcut}</CommandShortcut> : null}
            </CommandItem>
          ))}
            </CommandGroup>
          </CommandList>
        </Command>
      </div>
    </Dialog>
  );
}
