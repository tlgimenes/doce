---

description: "Task list template for feature implementation"
---

# Tasks: Workspace Working-Directory Resolution

**Input**: Design documents from `/specs/007-workspace-cwd-resolution/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md (all present; no `contracts/`/`quickstart.md` — see `plan.md`'s Project Structure for why)

**Tests**: Included — unlike this project's frontend features, every Rust module this feature touches (`fs.rs`, `bash.rs`, `search.rs`, `dispatch.rs`) already has real `#[cfg(test)]` coverage with `tempdir()`-based cases; this feature extends that existing pattern rather than introducing a new testing approach.

**Organization**: Tasks are grouped by user story to enable independent implementation and testing of each story.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on an incomplete task)
- **[Story]**: Which user story this task belongs to (US1, US2, US3)
- Include exact file paths in descriptions

## Path Conventions

Backend-only change, entirely within the existing `src-tauri/src/agent/` module tree — no new files:

```text
src-tauri/src/
├── agent/
│   ├── mod.rs                 # AgentContext gains `cwd`
│   ├── dispatch.rs             # execute() gains `cwd`; resolve_against() helper lives here
│   └── tools/
│       ├── fs.rs                # read/write/edit gain `cwd`
│       ├── bash.rs              # run() gains `cwd`
│       └── search.rs            # UNCHANGED — only dispatch.rs's default changes
└── commands/
    └── agent.rs                # send_agent_message resolves workspace path once
```

Note: `dispatch.rs`'s `execute()` function is touched by all three user stories (one match arm each — `"Bash"`, `"Read"`/`"Write"`/`"Edit"`, `"Glob"`/`"Grep"`). Those specific edits are kept sequential regardless of story, even though the stories themselves are otherwise independent, since they're edits to the same function body in the same file.

---

## Phase 1: Foundational (Blocking Prerequisites)

**Purpose**: The shared data-carrier and helper every story's tool-level change depends on

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [X] T001 Add a `cwd: Option<PathBuf>` field to `AgentContext` in `src-tauri/src/agent/mod.rs`, threaded through its `top_level()`/`subagent()` constructors (defaulting to `None` where not supplied) — this is the single place both the top-level loop and the `Task` tool's nested subagent loop read from, satisfying FR-006 by construction rather than by two call sites separately remembering to pass it
- [X] T002 [P] Add a `resolve_against(cwd: Option<&Path>, given: &Path) -> PathBuf` helper function in `src-tauri/src/agent/dispatch.rs` (per `research.md` § 2): returns `cwd.join(given)` when `cwd` is `Some` and `given` is relative, otherwise returns `given` unchanged
- [X] T003 In `send_agent_message` (`src-tauri/src/commands/agent.rs`), resolve the conversation's workspace path once at the start of the call (join `conversations.workspace_id` → `workspaces.path`; `None` if the conversation has no workspace), and populate it into the `AgentContext` used for both the top-level `run_loop` call and the `Task` tool's nested subagent `run_loop` call in `execute_top_level_tool` (depends on T001)

**Checkpoint**: Foundation ready — each story's tool-level change can now proceed independently

---

## Phase 2: User Story 1 - Shell commands run in the chosen folder (Priority: P1) 🎯 MVP

**Goal**: Bash commands execute with the conversation's chosen folder as their working directory.

**Independent Test**: Start a conversation scoped to a known folder, have the agent run `ls .` or `pwd`, confirm the output reflects that folder.

### Tests for User Story 1

- [X] T004 [P] [US1] Add a unit test in `src-tauri/src/agent/tools/bash.rs` asserting `run("pwd", None, Some(dir))` reports `dir`'s path — the user's own suggested test case (write this first; it will fail until T005 lands)
- [X] T005 [P] [US1] Add a unit test in `src-tauri/src/agent/dispatch.rs` asserting `execute()` with a `Bash` `{"command": "ls ."}` call and a populated `cwd` returns that directory's contents (an integration-style test through the dispatcher, not just the `bash` module directly)

### Implementation for User Story 1

- [X] T006 [US1] Add a `cwd: Option<&Path>` parameter to `bash::run()` in `src-tauri/src/agent/tools/bash.rs`; apply it via the `Command` builder's working-directory option when `Some` (depends on T004 existing as a failing test)
- [X] T007 [US1] Update `dispatch::execute()`'s `"Bash"` match arm in `src-tauri/src/agent/dispatch.rs` to read `cwd` from the call's context and pass it through to `bash::run()` (depends on T001, T006)

**Checkpoint**: User Story 1 is fully functional and testable independently — this alone is the MVP (the user's own cited example, `ls .`/`pwd`, now works correctly).

---

## Phase 3: User Story 2 - File operations without an explicit path land in the chosen folder (Priority: P2)

**Goal**: Reading, writing, or editing a file with a relative path resolves against the conversation's chosen folder.

**Independent Test**: In a folder-scoped conversation, have the agent write a file using a relative filename, confirm it appears inside that folder.

### Tests for User Story 2

- [X] T008 [P] [US2] ~~Add unit tests in `fs.rs`~~ **Deviation**: `fs::read`/`write`/`edit` never gained a `cwd` parameter (see T010) — resolution happens once in `dispatch.rs` before calling them, so there's nothing cwd-aware in `fs.rs` left to test. Coverage moved to `dispatch.rs`: `resolve_against`'s 4 pure unit tests cover every relative/absolute × Some/None combination the logic can hit, plus `us2_relative_write_lands_inside_the_given_cwd` proves the real wiring end-to-end for one tool (Read/Edit share the identical one-line call shape, so this isn't duplicated three times)
- [X] T009 [P] [US2] Add a unit test in `src-tauri/src/agent/dispatch.rs` confirming an absolute `file_path` given to `Read`/`Write`/`Edit` is used unchanged regardless of `cwd` (FR-004 regression guard)

### Implementation for User Story 2

- [X] T010 [US2] ~~Add a `cwd` parameter to `fs::read`/`write`/`edit`~~ **Deviation (simpler than planned)**: left `fs.rs`'s signatures untouched entirely. `dispatch.rs` resolves each `file_path` through `resolve_against()` *before* calling `fs::read`/`write`/`edit`, so those functions never need to know `cwd` exists — fewer signature changes, same behavior, one less thing to keep in sync
- [X] T011 [US2] Update `dispatch::execute()`'s `"Read"`/`"Write"`/`"Edit"` match arms in `src-tauri/src/agent/dispatch.rs` to resolve `file_path` via `resolve_against(cwd, ...)` and pass the resolved path to the corresponding `fs::` function (depends on T001, T002; sequential with T007/T014 since all three touch `dispatch.rs`'s `execute()`)

**Checkpoint**: User Stories 1 and 2 both work independently.

---

## Phase 4: User Story 3 - Searching without an explicit path searches the chosen folder (Priority: P3)

**Goal**: `Glob`/`Grep` calls that omit a `path` argument default to the conversation's chosen folder.

**Independent Test**: In a folder-scoped conversation, have the agent search for files without specifying a path, confirm results come from that folder.

### Tests for User Story 3

- [X] T012 [US3] Add a unit test in `src-tauri/src/agent/dispatch.rs` confirming a `Glob` call with no `path` argument and a populated `cwd` searches within that directory (end-to-end, via `execute()`) — plus a pure `resolve_optional_base(Some(cwd), None) == cwd` test covering the same logic in isolation for both `Glob` and `Grep` (they share the exact same default-resolution call)
- [X] T013 [US3] ~~Add a filesystem-based `cwd: None` test~~ **Deviation**: implemented as a pure test instead — `resolve_optional_base(None, None) == PathBuf::from(".")` — rather than mutating the real process cwd via `std::env::set_current_dir` and asserting against real directory contents, which would race against every other test running concurrently in the same `cargo test` process (a real bug caught while writing this, not just a style choice)

### Implementation for User Story 3

- [X] T014 [US3] Update `dispatch::execute()`'s `"Glob"`/`"Grep"` match arms in `src-tauri/src/agent/dispatch.rs` to call a new shared `resolve_optional_base(cwd, path_arg)` helper: `cwd`'s path when the model omits `path` and `cwd` is known, `"."` when neither is known — as today — otherwise `resolve_against()` on the explicit value. Factored into one helper rather than duplicating the same match block in both arms (per `research.md` § 3, `search.rs`'s function signatures are unchanged) (depends on T001; sequential with T007/T011 since all touch `dispatch.rs`'s `execute()`)

**Checkpoint**: All three user stories are independently functional — the full feature is complete.

---

## Phase 5: Polish & Cross-Cutting Concerns

**Purpose**: Whole-feature verification, including the one behavior this feature must NOT introduce

- [X] T015 Run `cargo test` for the full `src-tauri` workspace and confirm every test — old and new — passes (99 passed, 0 failed; also ran `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings`, matching CI's `ci.yml` exactly — both clean, one unrelated pre-existing clippy lint in `agent/mod.rs` fixed along the way)
- [X] T016 Manually validated live, real model, real app — added `tests/e2e/specs/workspace-cwd-resolution.spec.ts`: opens a real temp folder as a workspace, asks the agent to run `ls .` via the Bash tool, asserts the reply contains a marker file that only exists in that temp folder. **Passed** against the real installed model (`qwen3-4b-instruct-2507-q4_k_m`) in ~2m37s. (File-write-with-relative-path and absolute-path-unaffected scenarios were not separately driven live — covered by the dispatch-level tests instead, which exercise the identical `execute()` entry point `send_agent_message` calls into.)
- [ ] T017 Manually validate subagent inheritance (FR-006) — not run live this pass (would need a task deliberately designed to force delegation, which the model doesn't reliably do on request); the code path is unit-tested (`AgentContext::subagent().with_cwd(...)` in `commands/agent.rs`) but not exercised through a live subagent spawn

---

## Dependencies & Execution Order

### Phase Dependencies

- **Foundational (Phase 1)**: No dependencies — BLOCKS all user stories
- **User Stories (Phase 2-4)**: All depend on Foundational; each story's own test-then-implementation sequence is independent of the others, **except** that the `dispatch.rs` implementation edits (T007, T011, T014) touch the same function and must be done one at a time regardless of which story they belong to
- **Polish (Phase 5)**: Depends on all three user stories being complete

### Within Each User Story

- Tests (T004/T005, T008/T009, T012/T013) before their corresponding implementation tasks — written first, expected to fail, then made to pass
- `bash.rs`/`fs.rs` tool-level changes (T006, T010) before their `dispatch.rs` wiring (T007, T011)

### Parallel Opportunities

- T002 (Foundational helper) can run in parallel with T001 (different files: `dispatch.rs` vs `mod.rs`) — T003 depends on T001 only, not T002, so it doesn't block on T002 either
- T004 (bash.rs test) and T005 (dispatch.rs test) — different files, parallel
- T008 (fs.rs tests) and T009 (dispatch.rs test) — different files, parallel
- T006 (bash.rs) and T010 (fs.rs) — different files, could be worked on in parallel by different people once Foundational is done, even though they're different stories
- **Not parallel**: T007, T011, T014 all edit `dispatch.rs`'s `execute()` — regardless of story, these three are done one at a time
- **Not parallel**: T012 and T013 — same file (`dispatch.rs`), sequential

---

## Parallel Example: Foundational Phase

```bash
# T001 and T002 touch different files and have no dependency on each other:
Task: "Add cwd field to AgentContext in src-tauri/src/agent/mod.rs"
Task: "Add resolve_against() helper in src-tauri/src/agent/dispatch.rs"
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Foundational (CRITICAL — blocks all stories)
2. Complete Phase 2: User Story 1 (Bash cwd)
3. **STOP and VALIDATE**: Run the user's own suggested test — `ls .`/`pwd` in a folder-scoped conversation — both as a `cargo test` case and manually in the running app
4. This alone resolves the most visible/frequent instance of the gap

### Incremental Delivery

1. Complete Foundational → shared `cwd` plumbing ready
2. Add User Story 1 → validate → Bash commands behave correctly
3. Add User Story 2 → validate → file read/write/edit behave correctly
4. Add User Story 3 → validate → search defaults behave correctly
5. Finish with Polish (full `cargo test` run, manual app validation, subagent-inheritance check)

---

## Notes

- [P] tasks touch different files and have no unmet dependencies
- [Story] label maps each task to its user story for traceability
- Tests are written first per story and expected to fail until their
  paired implementation task lands — this project's existing Rust test
  culture (every touched module already has `tempdir()`-based coverage)
  makes this the natural fit here, unlike this project's frontend
  features which validate via manual browser checks instead
- Commit after each task or logical group
- Stop at any checkpoint to validate a story independently
