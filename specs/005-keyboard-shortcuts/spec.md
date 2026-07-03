# Feature Specification: Keyboard Shortcuts

**Feature Branch**: `005-keyboard-shortcuts`

**Created**: 2026-07-03

**Status**: Draft

**Input**: User description: "Add global keyboard shortcuts to the product: Cmd+L focuses the primary message input (the chat input in regular chat mode, or the task input in agent/workspace mode); Cmd+N creates a new conversation and switches to it, matching the existing '+ New conversation' sidebar action; Cmd+K opens a dialog listing every available keyboard shortcut and what it does. These are the app's first global (not input-scoped) keyboard shortcuts and first modal dialog — there is no existing hotkey handling or dialog component in the codebase today. Platform is macOS only. The shortcuts must work regardless of what currently has keyboard focus (including while typing in a text input), must not break any existing keyboard behavior, and the shortcuts dialog must be dismissible via Escape and a visible close control."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Jump straight into typing (Priority: P1)

While using the app — regardless of what's currently focused, including
while already typing somewhere else — the user presses Cmd+L and keyboard
focus immediately moves to the primary message input, ready to type.

**Why this priority**: The single most common action in this app is
composing a message; removing the need to reach for the mouse to click
into the input is the highest-frequency, highest-value shortcut of the
three.

**Independent Test**: Can be fully tested by focusing something else
entirely (or nothing), pressing Cmd+L, and confirming the message input is
now focused and ready for typing.

**Acceptance Scenarios**:

1. **Given** the user is viewing a conversation with focus anywhere else on
   the page, **When** they press Cmd+L, **Then** keyboard focus moves to
   the message input.
2. **Given** the user is already typing in the message input, **When**
   they press Cmd+L, **Then** focus remains in the message input (no
   disruptive effect).
3. **Given** the user is in agent/workspace mode, **When** they press
   Cmd+L, **Then** focus moves to that mode's own task input.

---

### User Story 2 - Start a new conversation from the keyboard (Priority: P2)

At any point in the app, the user presses Cmd+N and a new conversation is
created and immediately becomes the active one, exactly as if they'd
clicked the existing "+ New conversation" button.

**Why this priority**: Starting fresh is a frequent action, but slightly
less frequent than simply focusing the input to continue an existing
thread — hence second priority.

**Independent Test**: Can be fully tested by pressing Cmd+N from anywhere
in the app and confirming a new, empty conversation is created and
becomes the active view.

**Acceptance Scenarios**:

1. **Given** the user is viewing any conversation, **When** they press
   Cmd+N, **Then** a new conversation is created and becomes the active
   one, matching what clicking "+ New conversation" already does.
2. **Given** the user is looking at Settings or agent/workspace mode,
   **When** they press Cmd+N, **Then** the app switches back to the new
   conversation's chat view.

---

### User Story 3 - Discover what shortcuts exist (Priority: P3)

The user presses Cmd+K and a dialog appears listing every available
keyboard shortcut alongside a short description of what each one does.
They can dismiss it with Escape or a visible close control and return
exactly to what they were doing.

**Why this priority**: This is a discoverability/reference aid rather
than a direct-action shortcut — valuable, especially for a new user, but
used less often once the other two are learned.

**Independent Test**: Can be fully tested by pressing Cmd+K from anywhere
in the app and confirming a dialog appears listing all shortcuts, then
confirming it can be dismissed via Escape and via a visible control.

**Acceptance Scenarios**:

1. **Given** the user is anywhere in the app, **When** they press Cmd+K,
   **Then** a dialog appears listing every available shortcut and what it
   does, including Cmd+K itself.
2. **Given** the shortcuts dialog is open, **When** the user presses
   Escape, **Then** the dialog closes.
3. **Given** the shortcuts dialog is open, **When** the user clicks a
   visible close control, **Then** the dialog closes.
4. **Given** the shortcuts dialog is open, **When** the user presses
   Cmd+K again, **Then** the dialog closes (does not stack or reopen).

---

### Edge Cases

- What happens if Cmd+L is pressed while no message input exists in the
  current view (e.g., Settings is open, or no conversation/workspace
  session exists yet)? It has no effect — there is nothing to focus.
- What happens if Cmd+N is pressed while the user has an unsent draft in
  the message input? The draft is not preserved, consistent with how
  switching conversations already clears the input today.
- What happens if Cmd+L or Cmd+N is pressed while the shortcuts dialog is
  open? The dialog captures keyboard focus; those shortcuts do not act on
  the conversation until the dialog is dismissed.
- What happens to unrelated keys and combinations (e.g., normal typing,
  Enter-to-send, copy/paste)? They continue to work exactly as before —
  none of this feature's shortcuts intercepts any key combination other
  than its own three.
- What happens if a shortcut is added or changed in the future? The
  shortcuts dialog must reflect the actual current set, not a
  description that can drift out of sync with reality.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The app MUST let the user move keyboard focus to the
  primary message input of whichever view is currently active (the chat
  input in chat mode, or the task input in agent/workspace mode) via a
  single keyboard shortcut (Cmd+L), from anywhere in the app.
- **FR-002**: The focus shortcut MUST have no effect when the active view
  has no message input to focus.
- **FR-003**: The app MUST let the user create a new conversation and
  switch to it via a single keyboard shortcut (Cmd+N), from anywhere in
  the app, matching the existing "+ New conversation" action.
- **FR-004**: The app MUST let the user open a dialog listing every
  available keyboard shortcut and a short description of what each does,
  via a single keyboard shortcut (Cmd+K), from anywhere in the app.
- **FR-005**: The shortcuts dialog MUST be dismissible both via the
  Escape key and via a visible close control.
- **FR-006**: Pressing the shortcuts-dialog shortcut again while it is
  already open MUST close it rather than stacking another instance.
- **FR-007**: All three shortcuts MUST take effect regardless of what
  currently has keyboard focus, including while the user is actively
  typing in a text input.
- **FR-008**: None of these shortcuts MUST interfere with any existing
  keyboard behavior (typing, text editing, Enter-to-send, copy/paste, or
  any other key/combination not explicitly assigned here).
- **FR-009**: While the shortcuts dialog is open, the other two shortcuts
  MUST NOT act on the conversation until the dialog is dismissed.
- **FR-010**: The list shown in the shortcuts dialog MUST accurately
  reflect the actual current set of available shortcuts at all times, not
  a description that can go stale as shortcuts are added or changed.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A user can move focus to the message input from anywhere in
  the app using a single keypress, with no mouse interaction required.
- **SC-002**: A user can start a new conversation from anywhere in the
  app using a single keypress, with no mouse interaction required.
- **SC-003**: A user unfamiliar with the shortcuts can discover all of
  them in one interaction (opening the dialog), without consulting
  outside documentation.
- **SC-004**: 100% of existing keyboard behavior (typing, text editing,
  Enter-to-send) continues to work unchanged after this feature ships.
- **SC-005**: All three shortcuts work correctly regardless of which
  element currently holds keyboard focus, except while the shortcuts
  dialog itself is open.

## Assumptions

- The app targets macOS only (v1 scope), so "Cmd" refers to the physical
  Command key; no Windows/Linux equivalent bindings are in scope.
- "The primary message input" means the regular chat view's input when in
  chat mode, or the agent/workspace view's task input when in agent mode —
  whichever is currently the active view's main text entry point.
- Cmd+N always creates a new regular chat conversation (not a new
  agent/workspace session), matching the existing "+ New conversation"
  sidebar action exactly, including switching away from Settings or
  agent/workspace mode to show it.
- These are the app's first global (not input-scoped) keyboard shortcuts
  and first modal dialog; no existing interaction pattern is being
  changed, only added.
- Customizing or rebinding these shortcuts is out of scope for this pass —
  they are fixed.
