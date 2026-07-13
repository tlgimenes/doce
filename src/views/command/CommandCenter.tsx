import { useEffect, useState, type ReactNode } from "react";
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
  /** The action's existing product icon (e.g. the sidebar's Plus for New
   * Agent) — actions without one render label-only. */
  icon?: ReactNode;
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
  const [query, setQuery] = useState("");

  useEffect(() => {
    if (!open) {
      setQuery("");
    }
  }, [open]);

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
      contentClassName="w-[34rem]"
    >
      <div className="w-full" data-testid="command-center">
        <Command
          label="Command search"
          className="rounded-lg border border-border/70 bg-popover p-0"
        >
          <CommandInput
            autoFocus
            placeholder="Type a command or search"
            value={query}
            onValueChange={setQuery}
          />
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
                  {/* Iconless actions get an icon-sized spacer so every
                      label starts at the same x. */}
                  {action.icon ?? <span aria-hidden="true" className="size-4 shrink-0" />}
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
