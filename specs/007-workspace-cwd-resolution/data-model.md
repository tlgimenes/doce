# Data Model: Workspace Working-Directory Resolution

No schema changes. This documents the one new in-memory field and the
resolution rule it drives — the actual verification is `cargo test`
cases (per `plan.md`'s Testing note), not a manual data model to walk
through, so this file also carries the worked examples a `quickstart.md`
would normally hold.

## `AgentContext.cwd` (new field, in-memory only)

| Field | Type | Notes |
|-------|------|-------|
| `cwd` | `Option<PathBuf>` | The conversation's workspace path, resolved once per `send_agent_message` call; `None` for a conversation with no `workspace_id` |

**Resolution rule** (used identically for `Read`/`Write`/`Edit`'s
`file_path` and `Glob`/`Grep`'s `path`, via the shared helper from
`research.md` § 2):

| `cwd` | `given` path | Result |
|-------|-------------|--------|
| `Some(dir)` | relative | `dir.join(given)` |
| `Some(dir)` | absolute | `given`, unchanged (FR-004) |
| `None` | relative | `given`, unchanged — resolves against the process's ambient directory exactly as today (FR-005) |
| `None` | absolute | `given`, unchanged |

**`Bash`**: `cwd` is passed straight to the process-spawn's
working-directory option when `Some`; when `None`, spawning is unchanged
from today (no working-directory option set).

**`Glob`/`Grep`'s omitted-path default**: when the model's tool call
includes no `path` argument, `dispatch.rs`'s fallback becomes `cwd`'s
path when `Some`, and `"."` (today's behavior) when `None`.

## Worked examples (the acceptance scenarios, made concrete)

Conversation scoped to `/Users/alex/code/widget-app`:

| Tool call from the model | Resolves to |
|---|---|
| `Bash({"command": "ls ."})` | Lists `/Users/alex/code/widget-app` |
| `Bash({"command": "pwd"})` | Prints `/Users/alex/code/widget-app` |
| `Write({"file_path": "notes.md", ...})` | Creates `/Users/alex/code/widget-app/notes.md` |
| `Write({"file_path": "/tmp/scratch.md", ...})` | Creates `/tmp/scratch.md` — unaffected, absolute path (FR-004) |
| `Grep({"pattern": "TODO"})` (no `path`) | Searches within `/Users/alex/code/widget-app` |

Conversation with no workspace (`cwd` is `None`):

| Tool call from the model | Resolves to |
|---|---|
| `Bash({"command": "pwd"})` | Whatever the app process's own ambient directory is — identical to today (FR-005) |
| `Write({"file_path": "notes.md", ...})` | Same location it would land in today, unchanged |
