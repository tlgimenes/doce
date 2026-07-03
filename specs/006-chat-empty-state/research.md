# Phase 0 Research: Chat Empty State Composer

The core product decisions (Home = a real folder scope, remove the old
button, recents + native dialog, "+ New conversation" shows the composer
rather than instant-creating) were resolved via direct interview before
this spec was written — not defaults, confirmed choices. This phase covers
the technical decisions needed to implement them.

## 1. Does "cwd being the chosen directory" require rewriting tool execution?

- **Decision**: No — for this pass, only add the working-directory path to
  `SYSTEM_PROMPT` so the model knows what it's working in. Do not change
  `Bash`'s process working directory or add relative-path resolution to
  `Read`/`Write`/`Edit`/`Glob`/`Grep`.
- **Rationale**: Traced `send_agent_message` → `dispatch::execute` in
  `src-tauri/src/commands/agent.rs` and `agent/mod.rs` directly: today,
  `workspace_id` is looked up and stored, but never read by tool
  execution — `Bash` runs with the process's ambient cwd, and file tools
  require whatever path the model supplies verbatim. This matches the
  constitution's Principle V, which explicitly says tools operate "not
  scoped to the opened workspace folder" — a deliberate v1.0 stance, not
  a bug. Telling the model the directory via the prompt is a one-line,
  low-risk change that makes the model *behave* sensibly (it can
  construct full paths itself, e.g. `~/code/foo/src/main.rs`), without
  touching how any of the five tools actually execute.
- **Alternatives considered**: Making `Bash` run with
  `Command::current_dir(workspace_path)` and having `Read`/`Write`/
  `Edit`/`Glob`/`Grep` resolve relative paths against it — this is the
  *complete* version of "cwd is real," but touches every built-in tool's
  execution path, not just prompting, and is large enough to deserve its
  own spec and its own review of the constitution's "not scoped to the
  workspace folder" language (Principle V would need updating either way,
  but "the model knows the path" and "tools enforce the path" are
  different-sized changes with different risk profiles). Flagged as a
  reasonable, explicit follow-up rather than silently expanded into this
  feature's scope.

## 2. Folder browsing: native OS dialog vs. custom in-app tree

- **Decision**: `@tauri-apps/plugin-dialog`'s folder-open dialog.
- **Rationale**: Confirmed via interview as the intentionally lighter
  choice. The plugin is Tauri's own first-party dialog integration —
  adding it is a `Cargo.toml` + `package.json` dependency plus one plugin
  registration line in `src-tauri/src/lib.rs`, not custom UI code. Neither
  `@tauri-apps/plugin-dialog` nor any equivalent is in the project today
  (checked `package.json` and `Cargo.toml` directly) — this is a new,
  small, first-party dependency, not a heavier fallback.
- **Alternatives considered**: A custom in-app expandable folder tree
  (matching the reference screenshot's "Workspaces > This Mac >" section)
  — rejected via interview; would need a new `list_directory`-style Rust
  command plus recursive tree-state management in the frontend for
  meaningfully less benefit than the OS's own, already-familiar picker.

## 3. Resolving the "Home" default to a real path

- **Decision**: `@tauri-apps/api/path`'s `homeDir()` function, called from
  the frontend when the composer needs to resolve "Home" to an actual
  path (at submit time, or when first rendering the picker's pinned
  "Home" entry).
- **Rationale**: Already part of Tauri's core JS API (no new dependency),
  purpose-built for exactly this ("get the current user's home
  directory" is a standard Tauri capability), and keeps path resolution
  on the frontend where the rest of the workspace-selection UI already
  lives.
- **Alternatives considered**: A new Rust command returning the home
  directory — rejected, `homeDir()` already does this without adding
  backend surface.

## 4. View routing: a conversation's own `workspaceId` vs. a separate `agentMode` flag

- **Decision**: `App.tsx` determines which view to render (the existing
  plain `Chat.tsx` vs. the restructured `Workspace.tsx`) by checking the
  *currently selected conversation's* `workspaceId` (already returned by
  `listConversations`/`Conversation`), not a separate `agentMode` boolean
  disconnected from which conversation is actually open.
- **Rationale**: Today's separate `agentMode` state is *already* a latent
  bug source: selecting a previously-created workspace-scoped conversation
  from the sidebar today renders it via `Chat.tsx` regardless (`onSelect`/
  `onCreated` in `App.tsx` unconditionally set `agentMode: false`), since
  sidebar selection and `agentMode` aren't actually connected to each
  other. Since this feature makes *every new* conversation
  workspace-scoped, that disconnect would become far more visible (most
  conversations, selected from the sidebar, would render wrong). Deriving
  the view from the conversation's own field, once, fixes this as a
  natural consequence rather than as a separate bugfix, and satisfies
  FR-012 for free: old, non-workspace conversations still have
  `workspaceId: null` and still route to `Chat.tsx` exactly as before.
- **Alternatives considered**: Keeping `agentMode` and also fixing
  `onSelect` to set it correctly per-conversation — rejected, this just
  reintroduces two sources of truth (the flag and the data) that have to
  be kept in sync by hand, the exact failure mode the fix above avoids
  entirely by only having one.

## 5. Restructuring `Workspace.tsx`

- **Decision**: `Workspace.tsx` changes from a self-contained "type a
  path, open it, then chat" component into a `conversationId`-driven
  message view — the same shape as `Chat.tsx` (fetch messages via
  `listMessages`, send via `sendAgentMessage`). Its current folder-input
  UI and `openFolder()` logic move into the new `EmptyState.tsx`/
  `FolderPicker.tsx` components, since "pick a folder to start" is now the
  empty state's job, not this view's.
- **Rationale**: Once folder selection lives in the composer, nothing
  should still enter a "pick a folder" flow from inside the conversation
  view itself — that would be a second, redundant path to the same
  outcome, exactly what the interview's "remove the old button" answer
  was about at the view level, not just the button.
- **Alternatives considered**: Leaving `Workspace.tsx` as-is and having it
  receive a pre-opened workspace some other way — rejected, its internal
  state (`pathInput`, its own local conversation list) has no role left
  once creation is centralized in the composer; keeping the dead code path
  around invites exactly the kind of drift already seen elsewhere in this
  codebase between near-duplicate components.

## 6. "+ New conversation" button behavior

- **Decision**: The button calls a new `onNewConversation` prop (handled
  in `App.tsx`) that clears `activeConversationId` (and exits Settings/
  agent-mode-equivalent state), rather than calling
  `commands.createConversation()` itself.
- **Rationale**: Directly satisfies the interview answer ("the button …
  just opens this empty state") and FR-002/SC-005 (no conversation record
  created until an actual first message is sent).
- **Alternatives considered**: None seriously — this is a direct,
  unambiguous translation of the confirmed interview answer.

## 7. Constitution alignment

- **Decision**: Recommend a documentation-only amendment to Principle V
  alongside this feature (see `plan.md`'s Constitution Check), rather than
  treating the principle as unaffected or as a blocker.
- **Rationale**: The constitution's own governance section requires an
  amendment for "reintroducing or further loosening" the no-permission-
  system boundary; making agent mode the default outcome of every new
  conversation is a loosening of its *practical* reach even though no
  code-level restriction is being removed (there was never a restriction —
  it's the *frequency/defaultness* of exposure that changes). Writing this
  down is a process step this feature's implementation shouldn't skip,
  even though the underlying product decision itself isn't in question.
