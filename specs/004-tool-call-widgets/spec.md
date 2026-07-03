# Feature Specification: Tool Call Widgets

**Feature Branch**: `004-tool-call-widgets`

**Created**: 2026-07-03

**Status**: Draft

**Input**: User description: "Give each agent tool call in the chat/workspace UI its own distinct visual widget, instead of every message (plain text replies, tool calls, and tool results alike) rendering through the same generic markdown bubble as it does today. Grounding from the existing 001-doce-v1-core spec and current code: the Message data model already has a content_type field (text | tool_call | tool_result | error) and a tool_name field for exactly this purpose, but the backend agent loop currently collapses every tool call/result into one plain-text transcript and only ever returns/persists the final text answer. The built-in tools are Read, Write, Edit, Bash, Glob, Grep, Task (subagent delegation), and AskUserQuestion. This feature should specify distinct widgets for at least Edit (a real diff view), Bash (terminal-style command + output), Read/Write (file reference cards), Glob/Grep (search results list), Task (a running/complete subagent status indicator that never reveals the subagent's intermediate tool calls), and AskUserQuestion (an interactive clickable prompt) — plus a graceful fallback widget for any tool without dedicated handling, and the Workspace view consuming the same real per-message data Chat does instead of its own disconnected list. Depends on 001-doce-v1-core's existing data model and IPC contracts rather than redefining them."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - See a file edit as a diff, not raw text (Priority: P1)

While the agent is working in agent mode, it edits a file. Instead of the
change appearing as a wall of plain text or raw data the user has to parse
by eye, they see a proper before/after diff — added and removed lines
visually distinguished — so they can verify what changed at a glance.

**Why this priority**: Editing files is the core value proposition of
agent mode; a legible diff is the single highest-value piece of this
feature, and delivering it alone already makes agent-mode activity far
easier to trust and verify than today's undifferentiated text.

**Independent Test**: Can be fully tested by having the agent edit a file
and confirming the resulting message renders as a labeled diff (file path
plus added/removed lines distinguishable from each other), not as a plain
paragraph or raw structured data.

**Acceptance Scenarios**:

1. **Given** the agent successfully edits a file, **When** the edit
   message appears in the conversation, **Then** it renders as a diff
   showing the file path and the specific lines added and removed.
2. **Given** an edit the agent attempted fails (e.g., the target text
   wasn't found), **When** that result appears, **Then** it's shown as a
   failed edit rather than an empty or misleading diff.

---

### User Story 2 - See shell commands and their output clearly (Priority: P2)

The agent runs a shell command as part of a task. The user sees the exact
command that ran and its output rendered like a terminal — monospaced,
with standard output and error output distinguishable, and whether it
succeeded or failed clear at a glance — instead of the command and its
output blended into ordinary prose.

**Why this priority**: Shell commands are the other most common, highest-
value tool a coding agent uses; after diffs, this is the next biggest
legibility win.

**Independent Test**: Can be fully tested by having the agent run a shell
command and confirming the message renders the command and its output in
a distinct, terminal-like presentation, with success/failure visible
without reading the output text itself.

**Acceptance Scenarios**:

1. **Given** the agent runs a shell command that succeeds, **When** the
   result appears, **Then** the command and its output are shown together,
   clearly distinguished from plain conversation text.
2. **Given** a command fails (non-zero exit), **When** the result appears,
   **Then** the failure is visually apparent without the user needing to
   read the output text closely.
3. **Given** a command produces very long output, **When** it's displayed,
   **Then** it's truncated or collapsed rather than expanding the
   conversation into an unbroken wall of text.

---

### User Story 3 - Answer the agent's clarifying questions inline (Priority: P3)

Mid-task, the agent needs to ask the user a clarifying question rather
than guessing. The user sees a real interactive prompt — the question,
its header, and clickable options (single- or multi-select as
appropriate) — and can answer with a click, after which the agent's task
resumes using that answer.

**Why this priority**: This closes a currently-nonfunctional gap (the
agent can't actually pause and ask today) — valuable, but only relevant
in tasks ambiguous enough to need it, making it lower-frequency than
Stories 1-2.

**Independent Test**: Can be fully tested by having the agent ask a
clarifying question and confirming the user can select an option (or
options) via the rendered prompt, with the task visibly continuing
afterward.

**Acceptance Scenarios**:

1. **Given** the agent asks a clarifying question with several options,
   **When** the prompt appears, **Then** the user can select an option by
   clicking it, without typing a free-text reply.
2. **Given** a question allows selecting more than one option, **When**
   the user is choosing, **Then** the prompt clearly indicates multiple
   selections are allowed.
3. **Given** the user has answered a question, **When** they view that
   message afterward, **Then** it shows which option(s) they chose and no
   longer accepts new input for that question.

---

### User Story 4 - Recognize other tool activity at a glance (Priority: P4)

The agent reads a file, writes a new file, searches the codebase by name
or content, or delegates part of the task to a subagent. Each of these
shows as its own compact, recognizable widget — not raw data, and not
indistinguishable from a plain-text reply. Delegating to a subagent shows
only that a sub-task is running or complete, never the subagent's own
intermediate tool calls. Any tool call without a dedicated widget still
renders in a sane, readable fallback rather than breaking the
conversation view.

**Why this priority**: These fill out full coverage of the built-in tool
set and improve consistency, but individually each is a smaller
legibility win than Stories 1-3, and the fallback case is a safety net
rather than everyday value.

**Independent Test**: Can be fully tested by having the agent read a file,
write a file, search the codebase, and delegate to a subagent, and
confirming each renders as its own distinct widget; and by exercising a
tool with no dedicated widget and confirming it still renders legibly.

**Acceptance Scenarios**:

1. **Given** the agent reads a file, **When** that message appears,
   **Then** it's shown as a compact file reference, not a plain-text
   dump of the file's contents.
2. **Given** the agent writes a new file, **When** that message appears,
   **Then** it's shown distinctly from a file edit and from a plain
   reply.
3. **Given** the agent searches for files or file content, **When** the
   results appear, **Then** they're shown as a list of matches, not a raw
   data dump.
4. **Given** the agent delegates a sub-task, **When** that activity
   appears, **Then** only a running/complete status is shown — the
   subagent's own intermediate tool calls never appear in the parent
   conversation.
5. **Given** a tool call for a tool without a dedicated widget, **When**
   that message appears, **Then** it still renders legibly (tool name
   plus its input/output) rather than being blank, broken, or silently
   dropped.

---

### Edge Cases

- What happens when a tool call itself failed (an unrecoverable error, not
  just a tool reporting an unsuccessful result)? The relevant widget must
  show a failure state appropriate to that tool rather than pretending it
  succeeded.
- What happens when a file read, write, or search returns a very large
  result? It must be truncated or made collapsible rather than dominating
  the conversation.
- What happens if the user tries to answer a clarifying-question prompt
  that has already been answered, or that a later part of the
  conversation has moved past? The prompt must not accept a second, stale
  answer.
- What happens when a tool call's recorded arguments are incomplete (e.g.
  optional fields omitted)? The corresponding widget must degrade
  gracefully rather than showing broken or missing content.
- What happens in the regular chat view (not agent/workspace mode) if a
  tool-call message somehow appears there? It must render with the same
  widget system as the workspace view, not a separate/inconsistent
  treatment, since both views share the same underlying message data.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The chat/workspace UI MUST render a message differently
  based on its recorded content type and tool name, rather than every
  message (plain-text replies, tool calls, and tool results alike)
  rendering through one identical generic presentation.
- **FR-002**: A file edit MUST render as a diff showing the affected file
  path and the specific lines added and removed, distinguished from each
  other.
- **FR-003**: A shell command MUST render with the command and its output
  shown together in a distinct, monospaced presentation, with success and
  failure visually distinguishable and standard output distinguishable
  from error output.
- **FR-004**: Shell command output and file read/search results that
  exceed a reasonable length MUST be truncated or made collapsible rather
  than rendered in full inline.
- **FR-005**: A file read MUST render as a compact file reference (at
  minimum, the file path), distinct from a file write and from a plain
  reply.
- **FR-006**: A file write MUST render distinctly from a file edit and
  from a plain reply.
- **FR-007**: A file or content search MUST render as a list of matches
  (files and/or matching locations), not as an undifferentiated data dump.
- **FR-008**: A clarifying question from the agent MUST render as an
  interactive prompt with selectable options; the user MUST be able to
  respond by selecting an option (or options, when multiple selection is
  allowed) rather than typing free text.
- **FR-009**: Once a clarifying-question prompt has been answered, it
  MUST display which option(s) were chosen and MUST NOT accept a further
  answer for that same question.
- **FR-010**: Subagent delegation MUST render only a running/complete
  status indicator; the subagent's own intermediate tool calls MUST NOT
  appear in the parent conversation, preserving the existing constraint
  from `001-doce-v1-core` (FR-015/SC-008) that subagent activity stays
  invisible to the parent conversation.
- **FR-011**: Any tool call for a tool without a dedicated widget MUST
  still render legibly (at minimum, the tool's name and its input/output),
  rather than rendering blank, broken, or being silently dropped.
- **FR-012**: A tool call or result that represents a failure MUST be
  visually distinguishable from one that succeeded, in every widget type
  that can fail.
- **FR-013**: The workspace (agent-mode) view MUST render messages from
  the same underlying per-message data as the regular chat view, so both
  views use one consistent widget system rather than the workspace view
  maintaining its own separate, disconnected message list.
- **FR-014**: This feature MUST NOT change what actions the agent is
  allowed to take or when — it changes only how existing tool activity is
  displayed.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: 100% of tool-call and tool-result messages across the
  built-in tool set (file read, file write, file edit, shell command,
  search, subagent delegation, clarifying question) render in a widget
  visually distinct from a plain-text reply.
- **SC-002**: A user can identify what kind of tool activity just
  happened (edit vs. shell command vs. search vs. question, etc.) within
  a couple of seconds of glancing at it, without reading the full content
  closely.
- **SC-003**: A user can answer a clarifying-question prompt with a
  single click, with zero instances of the agent's task remaining stalled
  waiting on a reply the UI never let the user give.
- **SC-004**: 100% of tool calls for tools without a dedicated widget
  still render legibly rather than breaking the conversation view or
  disappearing silently, verified by exercising an unrecognized tool
  name.
- **SC-005**: Zero instances of a subagent's intermediate tool calls
  appearing in the parent conversation (regression guard on the existing
  `001-doce-v1-core` constraint).
- **SC-006**: The workspace view and the regular chat view produce
  identical widget output for the same underlying message, verified
  across every supported tool type.

## Assumptions

- This feature builds on `001-doce-v1-core`'s existing data model
  (`Message.content_type`, `Message.tool_name`) and IPC contracts (the
  `agent-activity` and `ask-user-question` events already defined in that
  feature's `contracts/tauri-ipc.md`) rather than redefining them. Where
  those existing contracts are currently unwired end-to-end (tracked as
  known gaps in `001-doce-v1-core`'s `tasks.md`: T058/T061 for
  `AskUserQuestion`, T059/T060/T062 for live agent-activity streaming),
  completing that wiring is treated as necessary supporting work for this
  feature, not a redefinition of what was already specified.
- The built-in tool set for this pass is exactly the one already
  implemented: `Read`, `Write`, `Edit`, `Bash`, `Glob`, `Grep`, `Task`, and
  `AskUserQuestion`. Any future additional tool is covered by the
  fallback widget (FR-011) until it earns a dedicated one.
- This feature is purely about presentation. It does not add, remove, or
  restrict any tool or capability, and does not change the v1.0
  no-permission-system decision (`001-doce-v1-core` constitution
  Principle V) — nothing here asks for confirmation before a tool runs.
- "Distinct widget" means visually and structurally distinguishable
  presentation (layout, labeling, typography treatment) — it does not
  mandate any particular color scheme; the product's existing neutral
  gray design system applies.
