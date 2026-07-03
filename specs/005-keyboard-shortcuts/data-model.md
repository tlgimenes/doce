# Data Model: Keyboard Shortcuts

This feature has no persisted data — everything here is in-memory,
static, or transient UI state.

## Shortcut (the shared registry entry)

Defined once in `lib/shortcuts.ts`, consumed by both the global `keydown`
listener and `ShortcutsDialog.tsx` — see `research.md` § 5 for why a
single shared source of truth matters here (FR-010).

| Field | Type | Notes |
|-------|------|-------|
| `id` | string | Stable identifier (e.g. `"focus-input"`, `"new-conversation"`, `"show-shortcuts"`) |
| `combo` | string | Human-readable display form for the dialog, e.g. `"⌘L"` |
| `metaKey` | boolean | Always `true` for this feature — every binding requires Cmd |
| `key` | string | The single key in the combo, matched against `KeyboardEvent.key` (lowercased), e.g. `"l"`, `"n"`, `"k"` |
| `description` | string | Short label shown in the shortcuts dialog, e.g. `"Focus the message input"` |
| `action` | function | What running the shortcut does — takes whatever app-level context it needs (e.g. a callback to focus the input, a ref to trigger new-conversation creation, a function to open/close the dialog) |

**Validation rules**:
- `key` values MUST be unique within the registry — two entries claiming
  the same combo is a contradiction the registry itself should prevent,
  not something resolved at runtime.
- Every entry in the registry MUST appear in the shortcuts dialog (FR-010)
  — the dialog renders directly from this array, not a separate list.

## Dialog open state

A single boolean, owned by whichever component mounts `ShortcutsDialog`
(`App.tsx`), toggled by the Cmd+K entry's `action` and by the dialog's own
close affordances (`Escape`, backdrop click, close button). Not
persisted — always starts closed on app launch.

## Active-view context (read, not owned, by this feature)

The Cmd+L handler reads `App.tsx`'s existing `showSettings` / `agentMode`
/ `activeConversationId` state (already defined for view routing) to
decide which input — if any — to focus. This feature does not introduce
new state for this; see `research.md` § 4.
