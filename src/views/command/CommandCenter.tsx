import { useMemo, useState, type KeyboardEvent } from "react";
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

function matchesActionQuery(action: CommandCenterAction, query: string) {
  const normalizedQuery = query.trim().toLowerCase();
  if (!normalizedQuery) return true;

  return [action.label, action.id, action.shortcut ?? ""].some((candidate) =>
    candidate.trim().toLowerCase().includes(normalizedQuery),
  );
}

export default function CommandCenter({ open, onOpenChange, actions }: CommandCenterProps) {
  const [query, setQuery] = useState("");

  const runAction = (action: CommandCenterAction) => {
    action.run();
    onOpenChange(false);
  };

  const visibleEnabledActions = useMemo(
    () => actions.filter((action) => !action.disabled && matchesActionQuery(action, query)),
    [actions, query],
  );

  const handleInputKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key !== "Enter") return;

    const action = visibleEnabledActions[0];
    if (!action) return;

    event.preventDefault();
    runAction(action);
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
          className="rounded-lg border border-border/70 bg-popover p-0"
          value={query}
          onValueChange={setQuery}
        >
          <CommandInput
            autoFocus
            aria-label="Command search"
            placeholder="Type a command or search"
            onKeyDown={handleInputKeyDown}
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
