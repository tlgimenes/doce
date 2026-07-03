# Phase 0 Research: Workspace Working-Directory Resolution

No `[NEEDS CLARIFICATION]` markers — the two rounds of interview that
grounded this spec resolved every open question, including explicitly
ruling out the enforcement/sandboxing interpretation of the original ask.
This phase covers the remaining implementation-shape decisions.

## 1. Where does the resolved `cwd` live while threading through the call chain?

- **Decision**: A new field, `cwd: Option<PathBuf>`, on `AgentContext`
  (`src-tauri/src/agent/mod.rs`), resolved once in `send_agent_message`
  and read by both the top-level loop and the `Task` tool's nested
  subagent loop.
- **Rationale**: `AgentContext` already exists specifically to carry
  per-run context that both the top-level and subagent loops need
  (today: whether this run *is* a subagent, for the one-level-nesting
  cap). Adding `cwd` here means there is exactly one place that
  determines "what folder is this agent run working in," read
  identically by every caller — directly satisfying FR-006 (subagent
  inheritance) as a natural consequence of the data's location, not a
  separately-remembered rule two call sites have to both get right.
- **Alternatives considered**: Passing `cwd: Option<&Path>` as its own
  explicit parameter through `run_loop`/`execute_top_level_tool`/
  `dispatch::execute` — rejected, this is the same information carried a
  second, parallel way alongside `AgentContext`, which is exactly the
  "two sources of truth that can drift" shape a previous fix in this
  codebase (the `Chat.tsx`/`Workspace.tsx` message-testid asymmetry) had
  to clean up after the fact. Putting it on the context that's already
  shared avoids creating that risk in the first place.

## 2. A shared path-resolution helper vs. inlining the join logic per tool

- **Decision**: One small function, `resolve_against(cwd: Option<&Path>, given: &Path) -> PathBuf`
  (or equivalent), in `dispatch.rs`, used for `Read`/`Write`/`Edit`'s
  `file_path` argument and for `Glob`/`Grep`'s `path` argument.
- **Rationale**: The rule is identical in all four places ("if `given` is
  relative and `cwd` is `Some`, join them; otherwise use `given` as
  given") — writing it once and calling it four times is safer than
  writing the same three-line conditional four times, especially given
  this exact codebase's own recent, direct experience with near-identical
  logic silently drifting between copies.
- **Alternatives considered**: Each tool function doing its own
  resolution inline — rejected for the duplication reason above.

## 3. `Glob`/`Grep`'s default path: change the tool functions, or change the dispatcher's default?

- **Decision**: `search::glob_search`/`search::grep`'s signatures stay
  exactly as they are (`base: &Path`, required) — only
  `dispatch::execute`'s current hardcoded fallback (`.unwrap_or(".")`,
  used when the model's tool call omits a `path` argument) changes, to
  fall back to the resolved `cwd` when one is known, and `"."` only when
  it isn't (preserving today's behavior for workspace-less conversations
  per FR-005).
- **Rationale**: `glob_search`/`grep` are already correctly designed
  around "operate relative to whatever base you're given" — the actual
  gap is entirely in what `dispatch.rs` decides that base *is* by
  default. Changing the dispatcher's default is a smaller, more precisely
  targeted fix than adding cwd-awareness to functions that don't
  otherwise need it.
- **Alternatives considered**: Threading `cwd` into `search.rs` directly
  — rejected as unnecessary; the dispatcher is already the single place
  that decides what "no path given" defaults to.

## 4. Resolving the workspace path: once per turn, or once per tool call?

- **Decision**: Once, at the start of `send_agent_message`, before the
  loop begins — not re-queried on every individual tool call within that
  turn.
- **Rationale**: The conversation's `workspace_id` (and thus its
  `Workspace.path`) cannot change mid-turn — there's no user-facing way
  to reassign a conversation's folder while the agent is actively working
  (per `006-chat-empty-state`, the folder is fixed at conversation
  creation). One lookup per turn is both correct and avoids redundant
  database round-trips per tool call within a multi-tool-call turn.
- **Alternatives considered**: Re-resolving per tool call — rejected as
  unnecessary work with no behavioral difference, given the path cannot
  change within a turn.

## 5. Fallback for conversations with no workspace

- **Decision**: `cwd` resolves to `None` for such a conversation (a
  simple `LEFT JOIN`-shaped lookup, or an `Option`-returning query), and
  every changed function's `None` branch is exactly today's existing
  behavior (relative paths resolve against the process's own ambient
  directory, `Bash` spawns without `current_dir`, `Glob`/`Grep` default
  to `"."`) — verified by construction, not by adding a separate
  conditional path to test.
- **Rationale**: Directly satisfies FR-005 and SC-005 — "no change in
  behavior" is easiest to guarantee when the no-workspace case takes
  literally the same code path as before this feature existed, rather
  than a new "unscoped" branch that has to be kept behaviorally identical
  to the old code by hand.
