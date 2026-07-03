# Phase 0 Research: Keyboard Shortcuts

No `[NEEDS CLARIFICATION]` markers were carried into this phase — the
three bindings and their behavior were fully specified by the user. The
open questions this phase resolves are purely technical: how to implement
a global shortcut system and a modal dialog in a codebase that has neither
today.

## 1. Global shortcut handling: hand-rolled listener vs. a hotkey library vs. native Tauri menu accelerators

- **Decision**: A single hand-rolled `keydown` listener attached at
  `window` in `App.tsx`, driven by the shared shortcut registry
  (`lib/shortcuts.ts`).
- **Rationale**: Only three fixed bindings, no rebinding/customization in
  scope (per spec Assumptions) — a library earns its keep when shortcuts
  are numerous, dynamic, or user-configurable, none of which apply here.
  ~20 lines of `event.metaKey && event.key === "..."` checks, with
  `preventDefault()`, is simpler to read, debug, and test than learning a
  library's API for the same outcome. This matches the project's existing
  pattern of avoiding a dependency for something this small (no hotkey
  library exists in `package.json` today).
- **Alternatives considered**: `react-hotkeys-hook`/`tinykeys`/`mousetrap`
  — rejected, adds a dependency for three `if` checks. Native Tauri menu
  accelerators (Rust-side `Menu`/`MenuItem` with an `accelerator` string,
  wired to emit an event the frontend listens for) — this is the more
  "native macOS" answer for Cmd+N specifically (a real `File` menu entry
  showing `⌘N`, standard OS discoverability) but is a materially bigger
  change: new Rust menu-building code, IPC event wiring back into React
  state, and it doesn't naturally cover Cmd+L (browsers' own
  address-bar-focus shortcuts are JS-only too, not menu items) or Cmd+K's
  "show a dialog" behavior particularly well either. Flagged in `plan.md`'s
  Constitution Check as an explicitly-deferred, not silently-skipped,
  native-polish improvement for a future pass.

## 2. Modal dialog: native `<dialog>` element vs. a hand-rolled div overlay vs. a dialog library (e.g. Radix)

- **Decision**: The native HTML `<dialog>` element, opened via
  `.showModal()`.
- **Rationale**: This is the app's first modal, so correctness (focus
  trapped inside while open, focus restored to whatever triggered it on
  close, `Escape` closes it, correct `role="dialog"`/modal semantics
  exposed to assistive tech) matters and is notoriously easy to get subtly
  wrong by hand. WebKit (the engine Tauri uses on macOS) has supported
  `<dialog>`'s modal behavior since Safari 15.4 — focus-trapping,
  `Escape`-to-close (native `cancel` event), and top-layer stacking are
  all provided by the browser itself, satisfying FR-005 and the relevant
  parts of FR-009 with zero custom focus-management code. Backdrop-click-
  to-close isn't automatic and needs a small (~5 line) click handler
  checking whether the click landed on the `<dialog>` element itself
  (the standard pattern for this element) — one specific, well-known gap,
  not a broad set of hand-rolled behaviors to get right.
- **Alternatives considered**: A hand-rolled `<div>` overlay with manual
  `tabindex` focus-trapping — rejected, this is exactly the category of
  code that's easy to get subtly wrong (focus escaping the trap, focus not
  restored on close, missed `aria-modal`) and the native element already
  solves it correctly. `@radix-ui/react-dialog` (or similar) — also a
  reasonable choice and would work well, but adds a new dependency to
  solve a problem the platform's own `<dialog>` element already solves for
  a single, simple modal; not adopted for this pass since nothing here
  needs Radix's extra composability (nested dialogs, custom positioning,
  animation primitives).

## 3. Sharing "create a new conversation" between the sidebar button and Cmd+N

- **Decision**: `ConversationList` exposes its existing `createNew` logic
  via `useImperativeHandle` on a `forwardRef`; `App.tsx` holds the ref and
  calls `.createNew()` from the Cmd+N handler — the exact same function the
  "+ New conversation" button already calls internally.
- **Rationale**: The alternative — lifting `commands.createConversation()`
  up into `App.tsx` and passing the result down — would duplicate the same
  API call and state-update logic in two places (or require restructuring
  which component owns the conversations list), for a change whose whole
  point (per FR-003) is "do exactly what the existing button already
  does." An imperative ref keeps the single implementation where it
  already lives and is already covered by `ConversationList.test.tsx`,
  while giving `App.tsx` a one-line way to trigger it.
- **Alternatives considered**: Duplicating the creation call directly in
  `App.tsx`'s keydown handler — rejected, violates FR-003's "matching the
  existing action exactly" more easily if the two copies drift, and is the
  same kind of duplication a previous change in this codebase already
  learned to avoid (see this feature area's history of two near-identical
  message-rendering blocks silently drifting on test coverage).

## 4. Determining "the primary message input" for Cmd+L

- **Decision**: The Cmd+L handler in `App.tsx` reasons from the app's own
  existing view-state (`showSettings`, `agentMode`, `activeConversationId`)
  — the same state that already decides what `App.tsx` renders — to decide
  whether to focus `[data-testid="chat-input"]`, `[data-testid="agent-input"]`,
  or do nothing.
- **Rationale**: `App.tsx` already has this state; reusing it to decide
  "what's the active view" is more reliable than blindly querying the DOM
  for either input and focusing whichever exists, which could theoretically
  match a stale/hidden element during a transition and doesn't reflect the
  actual state machine already governing what's rendered.
- **Alternatives considered**: Ref-forwarding the actual `<input>` element
  up from `Chat.tsx`/`Workspace.tsx` through props — rejected as more
  invasive (both components would need forwardRef restructuring) for
  equivalent behavior, since a plain `document.querySelector` on the
  already-stable, already-used-elsewhere `data-testid` attributes reaches
  the same element with far less code once the view-state check confirms
  which one *should* be present.

## 5. One shared shortcut registry, not two descriptions of the same thing

- **Decision**: `lib/shortcuts.ts` exports one array —
  `{ combo: "Cmd+L", description: "Focus the message input", ... }` per
  shortcut — read by both the `keydown` listener (to know what to
  intercept) and `ShortcutsDialog.tsx` (to render the list).
- **Rationale**: Directly satisfies FR-010 ("the list … MUST accurately
  reflect the actual current set … not a description that can go stale").
  Any future shortcut added to the registry automatically appears in the
  dialog and is automatically intercepted — there is only one place to
  update, not two to keep in sync by hand.
- **Alternatives considered**: A hardcoded JSX list in the dialog component
  separate from the `if` checks in the listener — rejected, this is
  exactly the drift FR-010 is written to prevent.
