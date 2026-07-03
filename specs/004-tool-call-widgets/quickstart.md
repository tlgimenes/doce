# Quickstart: Tool Call Widgets

Validates against `spec.md`'s acceptance scenarios. Requires an
agent-mode (workspace-scoped) conversation — per `006-chat-empty-state`,
start one via the empty-state composer.

## Automated validation

```bash
cargo test    # dispatch::execute's ToolOutcome/detail shapes, PendingQuestions wiring
npx vitest run  # MessageContent.tsx's per-widget rendering, one test file per widget
```

Should cover, at minimum (see `tasks.md` for the exact breakdown):
- Each of `Read`/`Write`/`Edit`/`Bash`/`Glob`/`Grep`/`Task` produces a
  `tool_call` + `tool_result` message pair with the shape `data-model.md`
  defines, for both success and failure outcomes.
- `MessageContent.tsx` renders the correct widget for each `toolName`, a
  failed-edit state for `Edit`'s `ok: false`, a failure state for `Bash`'s
  non-zero exit, and the fallback widget for an unrecognized `toolName`.
- `AskUserQuestion`: registering emits `ask-user-question`;
  `answer_user_question` resolves the pending call, updates the persisted
  row, and a second call against the same `questionId` errors rather than
  silently succeeding.
- `Chat.tsx` and `Workspace.tsx` both render the exact same widget output
  for an identical message, via the shared `MessageContent.tsx` (FR-013/
  SC-006) — no independent rendering logic in either view.

## Manual validation (in the running app, real model, real tools)

1. **User Story 1 (diff)**: start a workspace conversation, ask the agent
   to edit a real file (e.g. "add a comment to the top of X"). Confirm the
   resulting message renders as a labeled diff — file path, added/removed
   lines visually distinguished — not plain text. Ask it to edit with an
   `old_string` that doesn't exist in the file; confirm a failed-edit state,
   not an empty/misleading diff.
2. **User Story 2 (shell)**: ask the agent to run a shell command (e.g.
   `ls` or a real build/test command). Confirm the command and its output
   render together, monospaced, distinct from prose, with success visually
   clear. Ask it to run a command that fails (e.g. `exit 1` or a bad path);
   confirm the failure is visible without reading the output text closely.
   Ask it to run something with long output; confirm it's truncated/
   collapsed, not an unbroken wall of text.
3. **User Story 3 (clarifying question)**: give the agent a genuinely
   ambiguous task likely to trigger `AskUserQuestion` (small local models
   may not reliably choose to ask — if it doesn't, this can also be
   validated by triggering the tool call directly in a test harness).
   Confirm a real interactive prompt appears with clickable options;
   confirm clicking one resumes the task; confirm reloading the
   conversation still shows which option was chosen, with no further
   input accepted.
4. **User Story 4 (other tools)**: ask the agent to read a file, write a
   new file, and search the codebase by name or content. Confirm each
   renders as its own distinct, compact widget — not a raw dump, not
   indistinguishable from a plain reply. Ask it to delegate to a subagent
   (`"delegate this to a subagent"`); confirm only a running/complete
   status shows, never the subagent's own intermediate tool calls (check
   the parent conversation specifically — the subagent's own conversation,
   visible via its own row, is expected to show its own tool activity
   normally).
5. **Fallback**: exercise a tool name the widget system doesn't recognize
   (e.g. temporarily register a dummy tool, or inspect the DB directly and
   insert a `tool_result` row with an unknown `tool_name`); confirm it
   still renders legibly rather than breaking the message list.
6. **FR-013 regression**: compare the same conversation's rendering
   between selecting it from the sidebar (`Workspace.tsx`) and — if a
   pre-existing plain conversation with a stray tool-call message exists —
   `Chat.tsx`; confirm identical widget output.
