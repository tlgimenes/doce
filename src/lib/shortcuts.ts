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
  openSearch: () => void;
  openCommandCenter: () => void;
  toggleWidgetGallery: () => void;
}

// The single source of truth both the global keydown listener and the
// shortcuts dialog read from (FR-010) — a shortcut added here is
// automatically intercepted and automatically listed, nothing to keep in
// sync by hand.
export function buildShortcuts(handlers: ShortcutHandlers): Shortcut[] {
  return [
    {
      id: "focus-input",
      combo: "Cmd+L",
      metaKey: true,
      key: "l",
      description: "Focus composer",
      action: handlers.focusInput,
    },
    {
      id: "new-conversation",
      combo: "Cmd+N",
      metaKey: true,
      key: "n",
      description: "New Agent",
      action: handlers.newConversation,
    },
    {
      id: "search-conversations",
      combo: "Cmd+F",
      metaKey: true,
      key: "f",
      description: "Search conversations",
      action: handlers.openSearch,
    },
    {
      id: "open-command-center",
      combo: "Cmd+K",
      metaKey: true,
      key: "k",
      description: "Open command center",
      action: handlers.openCommandCenter,
    },
    {
      id: "show-widget-gallery",
      combo: "Cmd+D",
      metaKey: true,
      key: "d",
      description: "Open widget gallery",
      action: handlers.toggleWidgetGallery,
    },
  ];
}
