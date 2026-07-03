# Feature Specification: Workspace Working-Directory Resolution

**Feature Branch**: `007-workspace-cwd-resolution`

**Created**: 2026-07-03

**Status**: Draft

**Input**: User description: "Agent tools currently resolve relative (or unspecified/default) paths against the Tauri process's own ambient working directory — an arbitrary location from the user's perspective, not the folder the user chose for that conversation. Fix: when a shell command runs (Bash), it must spawn with the conversation's chosen folder as its working directory. When a file read/write/edit is requested with a relative path, or a search (Glob/Grep) is requested without an explicit path, it must resolve against that same chosen folder instead of the ambient process directory. This is NOT a security/enforcement feature — no path validation, no rejection, no throwing for out-of-bounds access. Absolute paths continue to work exactly as they do today, unrestricted, preserving 001-doce-v1-core's FR-009 and the constitution's Principle V in full. A conversation with no associated folder must see no change in behavior. A subagent spawned from within a workspace-scoped conversation must inherit the same working-directory resolution as its parent."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Shell commands run in the chosen folder (Priority: P1)

While working in a conversation scoped to a specific folder, the agent
runs a shell command using a relative reference (for example, listing the
current directory, or running a build command that assumes it's already
in the right place). The command executes as if a terminal had been
opened directly in the folder the user chose — not in some other,
unrelated location.

**Why this priority**: Shell commands are the most frequent way this gap
shows up, and the easiest to verify directly (run `ls .` or `pwd` and
check the answer).

**Independent Test**: Can be fully tested by starting a conversation
scoped to a known folder and having the agent run a command like `ls .`
or `pwd`, then confirming the output matches that folder, not the app's
own process location.

**Acceptance Scenarios**:

1. **Given** a conversation is scoped to a specific folder, **When** the
   agent runs a shell command with a relative reference (e.g. `ls .`),
   **Then** the output reflects the contents of that folder.
2. **Given** the same conversation, **When** the agent runs a command
   that reports the current directory (e.g. `pwd`), **Then** it reports
   the chosen folder's path.

---

### User Story 2 - File operations without an explicit path land in the chosen folder (Priority: P2)

The agent reads, writes, or edits a file using a relative path, or
without specifying an explicit path at all. The operation happens inside
the folder the user chose for that conversation, not wherever the app
process happened to be running from.

**Why this priority**: The direct example that surfaced this gap — a
file write with no explicit path landing somewhere unrelated to the
conversation. Slightly lower priority than Story 1 only because it's a
narrower slice of the same underlying fix.

**Independent Test**: Can be fully tested by having the agent write a
file using a relative filename (no absolute path) in a folder-scoped
conversation, then confirming the file appears inside that folder.

**Acceptance Scenarios**:

1. **Given** a conversation is scoped to a specific folder, **When** the
   agent writes a new file using a relative path, **Then** the file is
   created inside that folder.
2. **Given** the same conversation, **When** the agent reads or edits a
   file using a relative path, **Then** it operates on the file inside
   that folder.
3. **Given** the agent instead provides a full, absolute path to any of
   these operations, **When** it runs, **Then** it behaves exactly as it
   does today — this feature changes nothing about absolute paths.

---

### User Story 3 - Searching without an explicit path searches the chosen folder (Priority: P3)

The agent searches for files by name or content without specifying a
starting folder. The search happens within the folder the user chose for
that conversation, not the app's own ambient location.

**Why this priority**: The narrowest slice of the same fix — search tools
already accept an explicit base path today; this is about what they
default to when one isn't given.

**Independent Test**: Can be fully tested by having the agent search for
files without specifying a path in a folder-scoped conversation, and
confirming results come from within that folder.

**Acceptance Scenarios**:

1. **Given** a conversation is scoped to a specific folder, **When** the
   agent searches for files by name or content without specifying a
   starting path, **Then** results come from within that folder.

---

### Edge Cases

- What happens in a conversation with no associated folder (one that
  predates this feature, if any still exist)? No change — tools behave
  exactly as they do today for such a conversation.
- What happens when the agent delegates part of a task to a subagent
  (the `Task` tool) from within a folder-scoped conversation? The
  subagent's own tool calls resolve relative paths against the same
  chosen folder as its parent — this fix isn't bypassed just by
  delegating.
- What happens when the agent provides an absolute path, anywhere on the
  filesystem, to any tool? Nothing changes — it's honored exactly as it
  is today. This feature does not add any validation, restriction, or
  rejection of any path.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: When a shell command runs within a conversation scoped to a
  folder, it MUST execute with that folder as its working directory.
- **FR-002**: When a file read, write, or edit is requested with a
  relative path within a folder-scoped conversation, it MUST resolve
  against that folder.
- **FR-003**: When a file or content search is requested without an
  explicit starting path within a folder-scoped conversation, it MUST
  default to searching within that folder.
- **FR-004**: An absolute path provided to any tool MUST continue to work
  exactly as it does today — this feature MUST NOT add any validation,
  restriction, or rejection of any path, in or out of the chosen folder.
- **FR-005**: A conversation with no associated folder MUST see no change
  in tool behavior.
- **FR-006**: A subagent spawned from within a folder-scoped conversation
  MUST resolve relative paths against the same folder as its parent
  conversation.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Running a relative-path shell command in a folder-scoped
  conversation reflects that folder's contents, verified 100% of the
  time across repeated checks.
- **SC-002**: Reading, writing, or editing a file with a relative path in
  a folder-scoped conversation operates on that folder, 100% of the
  time.
- **SC-003**: Searching without an explicit path in a folder-scoped
  conversation returns results from that folder, 100% of the time.
- **SC-004**: Providing an absolute path to any tool produces identical
  behavior before and after this feature ships — zero behavior change,
  confirming no restriction was introduced.
- **SC-005**: Conversations without an associated folder show zero
  behavior change after this feature ships.

## Assumptions

- This feature adds no access restriction, validation, or enforcement of
  any kind — confirmed directly via interview, not a default guess. It
  only changes what a relative or unspecified path resolves against.
  Absolute paths, wherever they point, continue to work exactly as they
  do today, preserving `001-doce-v1-core`'s FR-009 and the constitution's
  Principle V ("not scoped to the opened workspace folder") in full —
  this is an additive fix to previously-undefined behavior, not a
  reversal of either.
- "The conversation's chosen folder" is the workspace path already
  associated with that conversation (via `006-chat-empty-state`'s
  composer, or the pre-existing folder-opening flow for older
  conversations).
- No OS-level sandboxing is in scope — confirmed directly via interview.
  Setting the correct starting working directory for shell commands (via
  the standard working-directory option already available when spawning
  a process) is sufficient; a command that itself navigates elsewhere or
  references an absolute path is unaffected by this feature, exactly as
  today.
