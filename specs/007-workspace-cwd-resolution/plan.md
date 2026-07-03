# Implementation Plan: Workspace Working-Directory Resolution

**Branch**: `007-workspace-cwd-resolution` | **Date**: 2026-07-03 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/007-workspace-cwd-resolution/spec.md`

**Note**: This template is filled in by the `/speckit-plan` command. See `.specify/templates/plan-template.md` for the execution workflow.

## Summary

`send_agent_message` (`src-tauri/src/commands/agent.rs`) resolves the
conversation's workspace path once, at the start of the call, and threads
it down through `execute_top_level_tool` into `dispatch::execute` (and
into the `Task` tool's nested subagent `run_loop`, so subagents inherit
it too — FR-006). `dispatch::execute` gains one new parameter,
`cwd: Option<&Path>`. A single new helper resolves a given path against
it (join if relative, pass through unchanged if absolute) — used by
`Read`/`Write`/`Edit` directly, and by `Glob`/`Grep`'s existing `base`
default (currently hardcoded to `"."`) so that default becomes the
workspace path when one is known. `Bash` gains a `cwd: Option<&Path>`
parameter, applied via the working-directory option already available
when spawning a process — one line in `bash::run`. No IPC, schema, or
frontend change; this is entirely internal to `src-tauri/src/agent/`.

## Technical Context

**Language/Version**: Rust (backend only — no frontend changes)

**Primary Dependencies**: None new — `std::path`/`std::process::Command`'s existing `current_dir` are part of the standard library already in use

**Storage**: No schema change — resolves the existing `conversations.workspace_id` → `workspaces.path` join (both already present, used by other commands today) once per `send_agent_message` call

**Testing**: `cargo test` — extend the existing unit tests in `fs.rs`/`bash.rs`/`search.rs`/`dispatch.rs` (all already have `tempdir()`-based test infrastructure) with cwd-aware cases; the user's own suggested test case ("running `ls .` returns the chosen folder") maps directly to a `dispatch.rs` integration-style unit test

**Target Platform**: macOS desktop (Tauri backend) — same as the rest of the app

**Project Type**: Backend-only change to the existing single Tauri + Rust backend

**Performance Goals**: Negligible — one additional indexed lookup (conversation → workspace → path) per agent turn, not per tool call

**Constraints**:
- MUST NOT add any path validation, containment check, or rejection —
  confirmed via interview this is explicitly out of scope. The only new
  logic is "join if relative, else use as given."
- MUST NOT change behavior for a conversation with no `workspace_id`
  (`cwd` resolves to `None`, and every call site's existing
  behavior — resolve against the process's own ambient directory — is
  preserved exactly by falling back to today's code path).
- MUST propagate to subagents (`Task` tool), not just the top-level loop.

**Scale/Scope**: One new helper function, one changed function signature
(`dispatch::execute`) threaded through two call sites (top-level +
subagent) plus the five per-tool functions it dispatches to

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

- **I. Zero-Config First Run** — N/A.
- **II. Local-By-Default Privacy** — PASS. No new data leaves the device;
  this only changes which local path a relative reference resolves
  against.
- **III. Native macOS Polish** — N/A. Backend-only, no UI surface.
- **IV. Extensibility via MCP and Skills** — N/A.
- **V. v1 Scope Discipline** — **PASS, explicitly re-verified, not just
  assumed.** Unlike `006-chat-empty-state` (which does flag a real
  constitution tension elsewhere), this feature adds no restriction of
  any kind: FR-004 and SC-004 both require that an absolute path continues
  to work identically before and after this ships. `001-doce-v1-core`'s
  FR-009 ("without restricting these actions to the opened workspace
  folder") and the constitution's Principle V remain entirely true after
  this feature — it fills in previously-*undefined* behavior for relative
  paths, it does not narrow anything that's defined today. No amendment
  needed.

No violations. Complexity Tracking is empty.

## Project Structure

### Documentation (this feature)

```text
specs/007-workspace-cwd-resolution/
├── plan.md              # This file (/speckit-plan command output)
├── research.md          # Phase 0 output (/speckit-plan command)
├── data-model.md        # Phase 1 output (/speckit-plan command)
└── tasks.md             # Phase 2 output (/speckit-tasks command - NOT created by /speckit-plan)
```

No `quickstart.md`/`contracts/` — this feature adds no external interface
and no new user-facing flow to walk through independently of the code
itself; `data-model.md`'s worked examples (below) serve the same
verification purpose a quickstart normally would, since every scenario is
a `cargo test` case, not a manual UI walkthrough.

### Source Code (repository root)

```text
src-tauri/src/
├── agent/
│   ├── mod.rs                    # MODIFIED: AgentContext gains a `cwd: Option<PathBuf>` field;
│   │                             #   run_loop threads it to execute_tool as today
│   ├── dispatch.rs                # MODIFIED: execute() gains `cwd: Option<&Path>`; adds the
│   │                              #   shared resolve_against() helper; Glob/Grep's "." default
│   │                              #   becomes cwd when known
│   └── tools/
│       ├── fs.rs                  # MODIFIED: read/write/edit take `cwd: Option<&Path>`,
│       │                          #   resolve relative file_path against it
│       ├── bash.rs                # MODIFIED: run() takes `cwd: Option<&Path>`, sets
│       │                          #   Command::current_dir when present
│       └── search.rs              # UNCHANGED signatures (already take `base: &Path`) — only
│                                  #   dispatch.rs's default for that argument changes
└── commands/
    └── agent.rs                   # MODIFIED: send_agent_message resolves workspace_id -> path
                                    #   once, passes it into AgentContext for both the top-level
                                    #   loop and the Task tool's nested subagent loop
```

**Structure Decision**: Everything lives in the existing
`src-tauri/src/agent/` module tree — no new files, no new module. The
resolved `cwd` is carried on `AgentContext` (which already exists
specifically to carry per-run context like subagent-nesting depth)
rather than as a separate parameter threaded everywhere by hand, so the
top-level loop and the `Task` tool's nested subagent loop pick it up
identically with no risk of one path forgetting to pass it — directly
satisfying FR-006.

## Complexity Tracking

> **Fill ONLY if Constitution Check has violations that must be justified**

No violations — this section is intentionally empty.

## Post-Phase 1 Constitution Re-check

Re-evaluated after `data-model.md` was drafted: the resolved design adds
one struct field (`AgentContext.cwd`), one helper function, and small
signature changes to existing functions — no new capability, no new
restriction, no new external surface. The Constitution Check verdict
above (PASS on all applicable principles, no amendment needed) holds
unchanged.
