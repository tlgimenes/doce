# Phase 0 Research: Tool Call Widgets

## 1. Why tool calls render as nothing today (confirmed against real code)

- **Decision**: N/A — this is a factual finding, not a decision, but it sets
  the scope for everything below.
- **Finding**: Traced `send_agent_message` (`src-tauri/src/commands/agent.rs`)
  directly: it inserts exactly one `'user'` message at the start and exactly
  one `'assistant'`/`'text'` message once `run_loop` fully resolves. Every
  intermediate tool call `execute_top_level_tool`/`dispatch::execute` makes
  is used only to keep the model's own conversation context moving
  (`ChatMessage::user("Tool result for {tool_name}: {result}")`) and is then
  discarded — never written to the `messages` table, never emitted as an
  event. `Chat.tsx`/`Workspace.tsx` also don't branch on `contentType` at all
  today; every message renders through one generic markdown bubble. Both
  gaps must close for this feature to do anything.
- **Also confirmed**: the plain (non-agent) `send_message` path
  (`commands/conversations.rs`) never runs a tool loop at all — `tool_call`/
  `tool_result` content types are schema-only there today, exercised solely
  by directly-constructed fixture rows in tests (the status-derivation
  logic — "latest message is a pending `AskUserQuestion` → `requires_action`"
  — is real and tested, but nothing live ever reaches it). This feature's
  actual reachable surface is entirely the agent-mode path.

## 2. Live event streaming vs. persist-then-render-on-completion

- **Decision**: Persist every tool call as real message rows during the
  loop (so the full trace exists once the turn completes), but do **not**
  build general live, mid-turn `agent-activity` event streaming this pass.
  The one exception is `AskUserQuestion` (see § 3) — it structurally
  requires a live event, not just persistence, because the loop has to
  pause and the frontend has to know why *while* the `send_agent_message`
  call is still pending.
- **Rationale**: `send_agent_message` runs its whole tool-use loop
  synchronously today — no channel/streaming plumbing exists for it at all
  (unlike the plain chat path's queued/generating scheduler machinery).
  Making every tool call stream live would mean re-architecting the agent
  loop to run non-blocking with incremental event emission — a
  large, separate lift on the order of the streaming chat path itself, and
  arguably deserves its own follow-up spec rather than being absorbed
  silently into this one (the same "flag it, don't silently expand scope"
  call `006-chat-empty-state`'s plan made for cwd resolution). Critically,
  none of spec.md's acceptance scenarios require a widget to appear *while*
  a later step is still running — "when the edit message appears in the
  conversation, then it renders as a diff" is satisfied by the message
  existing once the turn completes and the frontend re-fetches. The user's
  actual reported problem ("tool calls are not rendering on the UI at
  all") is fully solved by persistence + rendering; live streaming is a
  separate, additive improvement.
- **Consequence, stated plainly**: `Task`/subagent-status widgets (FR-010)
  will only ever be observed in the `"complete"` state in this pass, since
  by the time the frontend can see the message the subagent has already
  finished. The data shape still carries a `state` field so a future
  live-streaming pass doesn't need a schema change — just real-time
  delivery.
- **Alternatives considered**: Full live streaming for every tool
  (matches `001`'s original, more ambitious T059/T060/T062 vision) —
  rejected for this pass as disproportionate to the reported problem;
  flagged as a natural follow-up feature once this one ships.

## 3. `AskUserQuestion` is the one case that needs a real event

- **Decision**: Wire the already-built, already-unit-tested
  `PendingQuestions` registry (`agent/tools/ask_user.rs`) into the live
  dispatch path: register a pending question, emit `ask-user-question`
  (already defined in `001-doce-v1-core`'s `contracts/tauri-ipc.md`, not
  redefined here), and `.await` the oneshot receiver from inside the tool
  dispatch call itself. `answer_user_question` (also already speced, not
  yet implemented) resolves it.
- **Rationale**: Unlike every other tool, `AskUserQuestion`'s entire point
  is interactivity mid-task — the loop must actually stop and wait for a
  real answer, so the frontend needs to know *while* the
  `send_agent_message` promise is still pending, not after. A Tauri event
  is the only way to surface that. This is a narrow, single-purpose
  exception to § 2's "no live streaming" decision, not a reopening of it —
  everything else stays synchronous-then-persisted.
- **What already exists vs. what's new**: `PendingQuestions::register`/
  `answer` and their unit tests are done. New: a `PendingQuestions` managed
  Tauri state (matching `ActiveGenerations`/`InferenceState`'s existing
  pattern), a dispatch arm for `AskUserQuestion` that emits the event and
  awaits the receiver, the `answer_user_question` command, and the
  frontend prompt component.

## 4. Restructuring `dispatch::execute`'s return type

- **Decision**: `dispatch::execute` currently returns one plain
  `String` — already formatted prose for the model to read (e.g. `"exit_code:
  0\nstdout:\n...\nstderr:\n..."` for `Bash`). Change it to return a small
  `ToolOutcome { model_text: String, detail: serde_json::Value }`: `model_text`
  is exactly what's fed back into the conversation today (unchanged
  behavior for the model), `detail` is a tool-shaped, serializable payload
  for the UI (see `data-model.md`).
- **Rationale**: The model-facing text and the UI-facing structured data
  have different needs (the model wants a natural-language-ish result;
  `EditDiffWidget` wants raw `old_string`/`new_string` to diff itself,
  `BashWidget` wants `exit_code`/`stdout`/`stderr` as separate fields, not
  concatenated prose it would have to re-parse). Producing both at the
  single point that already has all the raw data (`dispatch::execute`,
  which already destructures each tool's arguments and calls the real
  tool) is simpler and more robust than having the frontend parse
  `model_text` back apart, or having a second function recompute the same
  information.
- **Alternatives considered**: Parsing `model_text` on the frontend to
  recover structured fields — rejected, fragile (the text format is
  incidental prose for the model, not a contract) and duplicates knowledge
  of each tool's output shape in two places.

## 5. One message row per tool call, or two (`tool_call` + `tool_result`)?

- **Decision**: Two rows, matching `001`'s existing schema distinction
  (`content_type IN ('text', 'tool_call', 'tool_result', 'error')`) rather
  than inventing a new shape — `tool_call` (arguments, inserted first) and
  `tool_result` (the full `ToolOutcome.detail`, including a copy of the
  arguments needed to render its widget standalone). The frontend renders
  the widget from the `tool_result` row alone (self-sufficient payload);
  the paired `tool_call` row is not rendered as its own bubble in this
  synchronous-execution pass (see § 2) — it exists for data-model
  completeness and to be ready for a future live pass where `tool_call`
  can be inserted the moment the call is made and `tool_result` later once
  it resolves, without a schema change then either.
- **Rationale**: Not redefining `001`'s content-type vocabulary (this
  spec's own explicit assumption); two-message-per-invocation also mirrors
  how tool use is generally represented in chat-completion transcripts
  (a call message, then a result message), so it composes cleanly with
  `load_history`'s existing sequence-ordered loading.

## 6. Widget implementation: lightweight custom, not CodeMirror/xterm.js

- **Decision**: Build the diff and terminal-output widgets as small,
  custom, read-only React components — a colored-line diff view (using the
  `diff` npm package's `diffLines` for the actual line-diff algorithm, not
  hand-rolled) and a monospaced stdout/stderr block — rather than using the
  already-installed `@uiw/react-codemirror`/`@xterm/xterm`/`react-xtermjs`/
  `shiki` packages.
- **Rationale**: Those packages are installed but genuinely unused anywhere
  in `src/` today (confirmed via `grep`) — they were brought in anticipating
  `001`'s original, more ambitious "file tree + live code editor + live
  terminal panel" workspace vision, which was explicitly simplified away in
  practice (`001`'s own tasks.md retro: "the workspace view is a minimal
  chat-style vertical slice, not the file-tree/diff-viewer/terminal UI
  originally scoped", further confirmed by `006-chat-empty-state`
  restructuring `Workspace.tsx` into a lean, `Chat.tsx`-shaped message view).
  This spec's own framing is "distinct, compact widgets inside the message
  transcript" (SC-002: recognizable "within a couple of seconds of
  glancing at it") — CodeMirror is a full interactive code *editor* and
  `xterm.js` a full interactive VT100 terminal *emulator*; both are heavy,
  stateful, and built for editing/interacting, a mismatch for small,
  read-only, non-interactive summaries embedded in a scrolling chat list.
  A plain diff-line list and a `<pre>`-style output block match this
  codebase's existing preference for minimal, hand-rolled presentation
  (`Dialog.tsx` on the native `<dialog>` element, not a modal library;
  `Button` as a small Tailwind-styled primitive, not a component-kit
  import) far better than pulling in either dependency's full weight for
  every message bubble.
- **Alternatives considered**: `@uiw/react-codemirror`'s merge/diff
  extension for `EditDiffWidget` — rejected, needs a full editor instance
  per widget (heavy for a list of many small widgets in a scrolling
  transcript, and none of `@codemirror/merge` is even installed —only the
  base editor and a JS-language package are). `@xterm/xterm`/`react-xtermjs`
  for `BashWidget` — rejected, that's an interactive terminal *emulator*
  (cursor control, ANSI parsing, live PTY-style input) for what's actually
  a static, already-complete `exit_code`/`stdout`/`stderr` triple; a
  monospaced block covers the real requirement (FR-003) without the
  dependency. Leaving the unused packages in `package.json` is out of
  scope for this UI-focused spec — removing them is a separate, unrelated
  cleanup this feature doesn't need to block on.
- **New dependency**: `diff` (npm) — small (no transitive weight worth
  noting), the standard JS line-diff library, used only for
  `EditDiffWidget`'s `diffLines(oldString, newString)` call.

## 7. Sharing one widget-dispatch component between `Chat.tsx` and `Workspace.tsx`

- **Decision**: Extract a new `src/components/MessageContent.tsx` that both
  `Chat.tsx` and `Workspace.tsx` render each message through, replacing
  their current independent, duplicated inline JSX for the message body.
  It dispatches on `message.contentType`/`message.toolName`: `"text"` →
  today's `ReactMarkdown` rendering (unchanged); `"tool_result"` → the
  matching widget (`EditDiffWidget`, `BashWidget`, `ReadWidget`,
  `WriteWidget`, `SearchResultsWidget` for `Glob`/`Grep`, `TaskWidget`,
  `AskUserQuestionWidget`), falling back to `UnknownToolWidget` for any
  `toolName` without a dedicated one (FR-011); `"tool_call"` rows render
  nothing standalone in this pass (§ 5); `"error"` → today's existing error
  styling, unchanged.
- **Rationale**: Directly satisfies FR-013 ("one consistent widget system",
  not two independently-maintained ones) by construction — there's only
  one rendering function to maintain, not two copies that can drift, which
  is exactly the kind of near-duplicate-component drift `006`'s own
  research.md flagged as an existing pattern this codebase has been bitten
  by before. `Workspace.tsx` already fetches real per-message data via
  `listMessages` (restructured in `006-chat-empty-state`) — the remaining
  gap for FR-013 is purely "share the rendering function," not "share the
  data source" (that part's already done).
- **Alternatives considered**: A separate `WorkspaceMessageContent`
  component mirroring `Chat.tsx`'s — rejected, directly reintroduces the
  drift FR-013 exists to prevent.

## 8. `AskUserQuestion` answer submission — new command, existing pattern

- **Decision**: `answer_user_question(questionId: string, answer: string[])`
  — a new Tauri command (already specified in `001`'s IPC contract),
  looked up via the new `PendingQuestions` app state, calling
  `.answer(question_id, answer)`. On success, also persists the answer by
  updating the corresponding `tool_result` message row's `content` JSON
  (adding the chosen option(s)) so the prompt shows as already-answered
  on reload (FR-009) rather than only in the live session.
- **Rationale**: Matches the existing `cancel_generation`-style command
  shape (a simple state lookup + action, `bool`/`Result` return) already
  established for `commands/conversations.rs`. Persisting the answer into
  the same row (rather than a third message) keeps one `AskUserQuestion`
  invocation as one widget with an evolving state (`pending` →
  `answered`), matching how `TaskWidget`'s `running`/`complete` states
  work.
