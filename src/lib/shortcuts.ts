export interface Shortcut {
  id: string;
  combo: string;
  metaKey: boolean;
  key: string;
  description: string;
  action: () => void;
}

export interface ShortcutHandlers {
  focusInput: () => void;
  newConversation: () => void;
  toggleShortcutsDialog: () => void;
}

// The single source of truth both the global keydown listener and the
// shortcuts dialog read from (FR-010) — a shortcut added here is
// automatically intercepted and automatically listed, nothing to keep in
// sync by hand.
export function buildShortcuts(handlers: ShortcutHandlers): Shortcut[] {
  return [
    {
      id: "focus-input",
      combo: "⌘L",
      metaKey: true,
      key: "l",
      description: "Focus the message input",
      action: handlers.focusInput,
    },
    {
      id: "new-conversation",
      combo: "⌘N",
      metaKey: true,
      key: "n",
      description: "Start a new conversation",
      action: handlers.newConversation,
    },
    {
      id: "show-shortcuts",
      combo: "⌘K",
      metaKey: true,
      key: "k",
      description: "Show keyboard shortcuts",
      action: handlers.toggleShortcutsDialog,
    },
  ];
}
