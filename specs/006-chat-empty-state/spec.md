# Feature Specification: Chat Empty State Composer

**Feature Branch**: `006-chat-empty-state`

**Created**: 2026-07-03

**Status**: Draft

**Input**: User description: "Currently, the empty state renders 'Start a new conversation, or [Open a folder (agent mode)]'. Replace it with a rich composer that, when submitted, creates a new conversation immediately with the typed text as its first message. Above the input, a folder-target selector (defaulting to 'Home') expands into a rich picker for navigating folders and selecting the working folder for the new conversation. Confirmed via interview: every conversation created this way is ALWAYS a tool-enabled agent-mode conversation scoped to a working folder — Home is itself a folder selection (the user's home directory), not an opt-out. The existing separate 'Open a folder (agent mode)' button is removed. The folder picker offers a searchable list of previously used folders plus a native OS folder-browse option (not a custom in-app file-tree browser). The sidebar's '+ New conversation' button no longer creates a conversation immediately — it now shows this same empty-state composer, and the conversation is only actually created once the user submits a first message."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Start working by typing, no separate "create" step (Priority: P1)

A user lands on the empty state — either because no conversation is
selected yet, or because they clicked "+ New conversation" — and sees a
message composer instead of plain text. They type what they want to work
on and submit. A new, tool-enabled conversation is created immediately,
scoped to whatever folder the selector currently shows (Home by default),
with their typed message as its first turn.

**Why this priority**: This is the core simplification being asked for —
collapsing "create an empty conversation, then type into it" into one
step. It delivers the entire point of this feature even before the folder
picker is considered.

**Independent Test**: Can be fully tested by landing on the empty state,
typing a message without touching the folder selector, submitting, and
confirming a new conversation exists with that message as its first turn
and tool access enabled.

**Acceptance Scenarios**:

1. **Given** the user is on the empty state with the folder selector
   showing its default, **When** they type a message and submit, **Then**
   a new conversation is created, scoped to the home folder, with their
   message as the first turn.
2. **Given** the user clicks "+ New conversation" while another
   conversation is active, **When** the composer appears, **Then** no new
   conversation has been created yet — only submitting a message creates
   one.
3. **Given** the user has changed the folder selector to a different
   folder before typing, **When** they submit their message, **Then** the
   new conversation is scoped to that folder, not the default.

---

### User Story 2 - Pick which folder to work in (Priority: P2)

Before submitting, the user clicks the folder-target selector (showing
"Home" by default) and sees a list of folders they've used before, most
recently used first, with the current selection indicated. They can
filter the list by typing, and pick a different folder as the target for
the conversation they're about to start.

**Why this priority**: Lets a returning user jump back into a known
project quickly; secondary to the core "type and go" simplification in
Story 1, which already works with the default folder alone.

**Independent Test**: Can be fully tested by opening the folder selector,
confirming previously used folders appear ordered most-recent-first, and
confirming picking one and then submitting a message scopes the new
conversation to it.

**Acceptance Scenarios**:

1. **Given** the user has previously worked in several folders, **When**
   they open the folder selector, **Then** those folders appear listed,
   most recently used first, with the currently selected target visually
   indicated.
2. **Given** the folder list is open, **When** the user types to filter
   it, **Then** only matching folders remain visible.
3. **Given** the user picks a folder from the list, **When** they later
   submit their message, **Then** the new conversation is scoped to that
   folder.
4. **Given** the folder selector is open, **When** the user dismisses it
   (clicking elsewhere or pressing Escape) without picking anything,
   **Then** the previously selected target is unchanged.

---

### User Story 3 - Browse to a folder not used before (Priority: P3)

From the folder selector, the user chooses to browse the filesystem
directly (rather than picking from recent folders) and selects any folder
on their Mac as the target.

**Why this priority**: Necessary for completeness — a user's very first
project, or one they haven't opened before, won't appear in recents — but
used less often than returning to an already-known folder.

**Independent Test**: Can be fully tested by opening the folder selector,
choosing to browse the filesystem, picking a folder that has never been
used before, and confirming it becomes the selected target.

**Acceptance Scenarios**:

1. **Given** the folder selector is open, **When** the user chooses to
   browse the filesystem, **Then** they can navigate to and select any
   folder on their Mac.
2. **Given** the user cancels the filesystem browser without selecting
   anything, **When** it closes, **Then** the previously selected target
   is unchanged.
3. **Given** the user selects a folder that has never been used before,
   **When** they submit their message, **Then** the new conversation is
   scoped to that folder and it now appears in future recent-folder lists.

---

### Edge Cases

- What happens when the user submits with the default "Home" target
  untouched? A conversation scoped to the user's actual home folder is
  created — this always succeeds since the home folder always exists.
- What happens if the user changes the folder selection but doesn't
  submit a message? The new selection is remembered for the next submit;
  nothing is created until an actual message is sent.
- What happens if there are no previously used folders yet (a fresh
  install)? The picker shows only the Home entry — no error state.
- What happens if a previously used folder has since been deleted or
  moved? Selecting it and submitting surfaces an error the same way
  opening a missing folder already does today — not a new failure mode.
- What happens to a conversation that already existed before this feature
  shipped? It continues to be viewed and behave exactly as it does today —
  this feature governs how new conversations are created, not a migration
  of existing ones.
- What happens if the user clicks "+ New conversation" while a
  conversation is already active? The active conversation is deselected
  and the composer appears; nothing about the previously active
  conversation changes.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The app MUST present a message composer — not static text —
  wherever the current empty-state placeholder appears, including when
  the user clicks "+ New conversation."
- **FR-002**: Clicking "+ New conversation" MUST NOT create a conversation
  immediately; it MUST show the composer, deselecting whatever
  conversation was previously active.
- **FR-003**: Submitting non-empty text in the composer MUST create
  exactly one new conversation, scoped to the folder currently shown in
  the folder-target selector, with the typed text sent as that
  conversation's first message, as a single action.
- **FR-004**: Every conversation created via this composer MUST be a
  tool-enabled (agent-mode) conversation scoped to a working folder —
  there is no path from this composer to an unscoped or tools-disabled
  conversation.
- **FR-005**: The composer MUST show a folder-target selector, defaulting
  to the user's home folder ("Home"), when not otherwise changed.
- **FR-006**: Clicking the folder-target selector MUST open a picker
  listing folders the user has previously used, ordered most-recently-used
  first, with the currently selected target visually indicated.
- **FR-007**: The picker MUST let the user filter the recent-folders list
  by typing.
- **FR-008**: The picker MUST offer a way to browse the filesystem and
  select any folder, not limited to the recent list.
- **FR-009**: The folder shown in the selector at the moment of submit
  (Home, a picked recent folder, or a browsed folder) MUST become the
  working folder of the conversation created by that submit — opening or
  changing the picker alone MUST NOT create or modify any conversation.
- **FR-010**: The previously separate "Open a folder (agent mode)" entry
  point MUST be removed — folder selection is now part of the composer.
- **FR-011**: The picker MUST be dismissible without changing the current
  selection (clicking elsewhere or pressing Escape).
- **FR-012**: This feature MUST NOT change how conversations that already
  existed before it shipped are viewed or behave.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A user can go from the empty state (or clicking "+ New
  conversation") to a working, tool-enabled conversation using exactly
  one text entry and one submit action.
- **SC-002**: A user can return to any of their recently used folders in
  2 clicks or fewer (open the picker, click the entry).
- **SC-003**: A user can select any folder on their Mac from the same
  picker, not only previously used ones.
- **SC-004**: 100% of conversations created via this composer are scoped
  to a working folder — zero conversations are created without one.
- **SC-005**: Clicking "+ New conversation" never creates a stored
  conversation the user hasn't actually sent a first message in — zero
  "phantom empty conversations" result from clicking it and changing your
  mind.

## Assumptions

- Every conversation created from this composer onward is tool-enabled
  and scoped to a working folder; "Home" is itself a folder selection
  (the user's home directory), not an opt-out of folder-scoping.
  Confirmed directly via interview, not a default guess.
- Conversations already in the database before this feature ships
  (including any without a folder scope) continue to be viewable and
  behave exactly as they do today — this feature is scoped to how new
  conversations get created, not a migration of historical data.
- "Recently used folders" are sourced from the app's own record of
  previously opened folders, not the operating system's general
  recent-files list.
- Browsing for a folder outside the recent list uses the operating
  system's native folder-picker dialog, not a custom in-app file-tree
  browser — confirmed via interview as the intentionally lighter-weight
  choice over building a full in-app folder tree.
