# Implementation Plan: Tool Call Widgets

**Branch**: `004-tool-call-widgets` | **Date**: 2026-07-03 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/004-tool-call-widgets/spec.md`

**Note**: This template is filled in by the `/speckit-plan` command. See `.specify/templates/plan-template.md` for the execution workflow.

## Summary

Today every agent-mode message — plain replies, tool calls, and tool
results alike — renders through the same generic markdown bubble, and
worse, the backend never even persists individual tool activity: only the
final answer survives once a turn completes. This plan closes both gaps:
`dispatch::execute` returns a structured `ToolOutcome` (unchanged
model-facing text, plus a tool-shaped JSON detail) so every tool call and
its result become real `tool_call`/`tool_result` message rows; a new
shared `MessageContent.tsx` (used by both `Chat.tsx` and `Workspace.tsx`,
closing FR-013) dispatches each message to a small, purpose-built widget —
a real diff for `Edit`, a terminal-style block for `Bash`, compact cards
for `Read`/`Write`, a match list for `Glob`/`Grep`, a status indicator for
`Task`, an interactive prompt for `AskUserQuestion`, and a legible
fallback for anything else. `AskUserQuestion` additionally gets the one
piece of live event wiring this feature adds (`ask-user-question` +
`answer_user_question`), since its whole point is pausing the loop for a
real answer — everything else renders once its turn completes and the
frontend re-fetches, deliberately not the full live `agent-activity`
streaming `001-doce-v1-core` originally scoped (see `research.md` §§ 2-3
for why that split is the right cut here, not a shortcut).

## Technical Context

**Language/Version**: TypeScript/React 19 (frontend, primary) + Rust (backend — `dispatch::execute`'s return type, `AskUserQuestion` wiring, message persistence)

**Primary Dependencies**: `diff` (npm, new — `EditDiffWidget`'s line-diff algorithm, research.md § 6); everything else uses existing, already-installed dependencies (`react-markdown`, this project's own `Button`/design-system primitives from `008-shared-design-system`). Deliberately does **not** add `@uiw/react-codemirror`/`@xterm/xterm`/`react-xtermjs`/`shiki` to real use despite being pre-installed (research.md § 6)

**Storage**: No schema migration — `messages.content_type`/`messages.tool_name` already support exactly this (`001-doce-v1-core`); this feature is the first to actually populate `tool_call`/`tool_result` rows from live code (data-model.md)

**Testing**: `cargo test` for `dispatch::execute`'s `ToolOutcome`/detail shapes and `PendingQuestions` wiring (extends this project's existing `#[cfg(test)]` convention, no real model needed — pure data-shape and dispatch-path tests); Vitest + Testing Library for `MessageContent.tsx` and each widget (matches this project's established frontend test culture)

**Target Platform**: macOS desktop (Tauri) — same as the rest of the app

**Project Type**: Frontend-primary widget system + a backend data-shape change to the existing single Tauri + React app; no new service

**Performance Goals**: No new performance-sensitive path — tool execution itself is unchanged (same `fs`/`bash`/`search` calls); this feature only changes what gets persisted and how it renders

**Constraints**:
- **Scope decision, flagged not silently narrowed**: this feature does **not** implement `001`'s originally-specified general `agent-activity` live event stream (file-diff/shell-output/subagent-status kinds firing mid-turn) — every tool result becomes visible once the whole turn completes and the frontend re-fetches `list_messages`, not while a later step in the same turn is still running. `AskUserQuestion` is the sole exception, because it structurally requires a live event (the loop pauses; the frontend must know why while the command call is still pending). Full live streaming for every tool call is a materially larger, separate architectural change (an already-synchronous loop becoming incremental/event-driven throughout) that deserves its own follow-up rather than silent scope expansion here — see research.md § 2 and Complexity Tracking below.
- **Depends on `001-doce-v1-core`'s already-specified-but-unwired surface**: `PendingQuestions` (`agent/tools/ask_user.rs`) and the `ask-user-question`/`agent-activity` event shapes, `answer_user_question` command shape — this plan implements the parts of that surface this feature actually needs (`PendingQuestions` wiring, `ask-user-question`, `answer_user_question`) and explicitly does not implement `agent-activity` (previous bullet).
- Must not change what actions the agent is allowed to take or when (FR-014) — this is a presentation-and-persistence change; `dispatch::execute`'s actual tool behavior (what `fs`/`bash`/`search` do) is untouched.
- Must not regress `001`'s existing subagent-isolation guarantee (FR-015/SC-008) — a subagent's own tool activity persists under its own conversation row exactly as today; this feature adds no new visibility into it from the parent.

**Scale/Scope**: One backend return-type change threaded through `dispatch::execute`'s six match arms (`Read`/`Write`/`Edit`/`Bash`/`Glob`/`Grep`) plus a seventh new `AskUserQuestion` arm, one new Tauri command, one new managed state (`PendingQuestions`), one shared frontend dispatch component plus seven widget components, one new small npm dependency

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

- **I. Zero-Config First Run** — N/A. Doesn't touch onboarding/first-launch.
- **II. Local-By-Default Privacy** — PASS. No new network calls, no telemetry; purely local rendering of already-local tool-execution data.
- **III. Native macOS Polish** — PASS. Widgets are small, native-feeling presentational components matching this app's existing minimal Tailwind styling (research.md § 6's rejection of heavyweight editor/terminal libraries is itself in service of this — a lighter, more integrated feel than embedding a full code editor/terminal per message).
- **IV. Extensibility via MCP and Skills** — N/A. This feature covers the seven built-in tools; MCP/skill-invoked tools without a dedicated widget fall through to the fallback widget (FR-011) like any other unrecognized tool name, not a gap this feature needs to close specially.
- **V. v1 Scope Discipline** — PASS, no change to the no-permission-system posture. This feature changes only how already-permitted tool activity is *displayed* (FR-014) — it doesn't add, remove, gate, or confirm any action the agent can already take. No constitution amendment needed.

No violations. Complexity Tracking records the one scope decision worth
justifying explicitly (deferring general `agent-activity` live streaming).

## Project Structure

### Documentation (this feature)

```text
specs/004-tool-call-widgets/
├── plan.md              # This file (/speckit-plan command output)
├── research.md          # Phase 0 output (/speckit-plan command)
├── data-model.md         # Phase 1 output (/speckit-plan command)
├── quickstart.md         # Phase 1 output (/speckit-plan command)
├── contracts/            # Phase 1 output (/speckit-plan command)
└── tasks.md              # Phase 2 output (/speckit-tasks command - NOT created by /speckit-plan)
```

### Source Code (repository root)

```text
src-tauri/
├── Cargo.toml                          # UNCHANGED — no new Rust crate needed
├── src/
│   ├── agent/
│   │   ├── dispatch.rs                 # MODIFIED: execute() returns ToolOutcome{model_text, detail}
│   │   │                                #   instead of String; new AskUserQuestion match arm
│   │   ├── tools/
│   │   │   └── ask_user.rs             # UNCHANGED — PendingQuestions already implemented/tested
│   │   └── subagent.rs                 # UNCHANGED
│   └── commands/
│       ├── agent.rs                    # MODIFIED: persists tool_call/tool_result message pairs
│       │                                #   per dispatch call; manages PendingQuestions state;
│       │                                #   new answer_user_question command
│       └── conversations.rs            # UNCHANGED — insert_message-shaped inserts, reused pattern

src/
├── lib/
│   └── ipc.ts                          # MODIFIED: ToolResultDetail discriminated union (per tool),
│                                        #   answer_user_question binding, ask-user-question event
├── components/
│   └── MessageContent.tsx              # NEW: shared per-message dispatch (FR-013), used by both
│   └── MessageContent.test.tsx         # NEW
└── views/
    ├── chat/
    │   ├── Chat.tsx                    # MODIFIED: renders via MessageContent instead of inline JSX
    │   └── tool-widgets/
    │       ├── EditDiffWidget.tsx      # NEW
    │       ├── BashWidget.tsx          # NEW
    │       ├── ReadWidget.tsx          # NEW
    │       ├── WriteWidget.tsx         # NEW
    │       ├── SearchResultsWidget.tsx # NEW — Glob + Grep
    │       ├── TaskWidget.tsx          # NEW
    │       ├── AskUserQuestionWidget.tsx # NEW
    │       ├── UnknownToolWidget.tsx   # NEW — fallback (FR-011)
    │       └── *.test.tsx              # NEW, one per widget above
    └── workspace/
        └── Workspace.tsx               # MODIFIED: renders via MessageContent instead of inline JSX
```

**Structure Decision**: Widgets live under `src/views/chat/tool-widgets/`
(not `src/components/`) since they're specifically about this app's tool
data shapes (matching `006-chat-empty-state`'s precedent of placing
domain-specific UI under `views/`, reserving `components/` for
generic, reusable primitives like `Dialog.tsx`). `MessageContent.tsx`
itself lives in `src/components/` since — unlike the widgets it
dispatches to — it's the one piece both `Chat.tsx` and `Workspace.tsx`
genuinely share as a primitive rendering function, matching `Timer.tsx`'s
existing placement there. No backend crate/module additions — the new
`AskUserQuestion` dispatch arm and `ToolOutcome` type extend
`agent/dispatch.rs` in place, matching every prior feature's pattern of
extending this file's `execute()` rather than introducing a new
sub-module per tool.

## Complexity Tracking

> **Fill ONLY if Constitution Check has violations that must be justified**

No Constitution Check violations. The one deliberate scope decision this
plan makes — not implementing `001`'s general `agent-activity` live event
stream — isn't a constitution violation (nothing in the constitution
requires it), but is recorded here for the same reason Complexity Tracking
exists: so a reviewer sees it was a considered cut, not a missed spec item.

| Decision | Why Needed | Simpler/Fuller Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| Defer `001`'s general `agent-activity` live streaming (every tool call visible mid-turn); implement only `AskUserQuestion`'s live event, everything else via persist-then-render-on-completion | Solves the actually-reported problem (tool calls render nothing, ever) with a contained backend change (`dispatch::execute`'s return type); the full live version needs the agent loop to become incremental/event-driven throughout, a materially larger, separate architectural change | Building full live streaming now was considered and rejected for this pass — no acceptance scenario in spec.md requires a widget to appear *while* a later step is still running, so the larger change wouldn't deliver additional required value here, only unrequested scope; flagged as a natural, well-scoped follow-up feature instead |
