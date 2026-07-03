# Implementation Plan: Chat Empty State Composer

**Branch**: `006-chat-empty-state` | **Date**: 2026-07-03 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/006-chat-empty-state/spec.md`

**Note**: This template is filled in by the `/speckit-plan` command. See `.specify/templates/plan-template.md` for the execution workflow.

## Summary

Replaces the static empty-state placeholder with a message composer plus a
folder-target selector (defaulting to "Home"), and makes "+ New
conversation" show that same composer instead of instant-creating a row.
Submitting creates the workspace (if not already known), the conversation,
and sends the first message in one sequence — using entirely **existing**
Tauri commands (`open_workspace` → `create_conversation` →
`send_agent_message`), no new backend command needed for that path. Because
every new conversation is now always workspace-scoped, `App.tsx`'s view
routing changes from a separate `agentMode` boolean to reading the
*selected conversation's own* `workspaceId` — which requires restructuring
`Workspace.tsx` from a self-contained "pick a folder, then chat" component
into a `conversationId`-driven view matching `Chat.tsx`'s existing shape
(its folder-opening responsibility moves to the new composer). The folder
picker's "browse anything else" path needs one new dependency
(`@tauri-apps/plugin-dialog`, native OS folder picker) — everything else is
existing capability, recomposed.

Two things discovered while researching this that materially affect scope
are called out explicitly below rather than silently decided: (1) today's
agent tool execution does **not** actually use a conversation's workspace
path as a working directory — `workspace_id` is presently a label only; and
(2) making unrestricted tool access the *default* for every new
conversation (not an explicit opt-in, as it is today) measurably expands
the blast radius of the constitution's existing no-permission-system
decision.

## Technical Context

**Language/Version**: TypeScript/React 19 (frontend, primary) + Rust (backend, one narrow addition — see Constraints)

**Primary Dependencies**: `@tauri-apps/plugin-dialog` (new — native folder-browse dialog) and its Rust counterpart `tauri-plugin-dialog`; `@tauri-apps/api/path` (already available via the Tauri JS API, used to resolve "Home" to the real home directory) — no other new dependencies

**Storage**: No schema changes — `conversations.workspace_id` and the `workspaces` table already exist and already support everything this feature needs (verified directly against `src-tauri/src/commands/conversations.rs` and `workspaces.rs`)

**Testing**: Vitest + Testing Library for the new composer/picker components and the `App.tsx`/`ConversationList.tsx` routing changes; existing Rust unit tests extended only if the system-prompt cwd addition (see Constraints) is implemented

**Target Platform**: macOS desktop (Tauri) — same as the rest of the app

**Project Type**: Frontend-primary change to the existing single Tauri + React app, with one small, narrowly-scoped backend addition

**Performance Goals**: No new performance-sensitive path; conversation creation is already an existing, acceptable-latency operation (workspace insert + conversation insert + one model turn)

**Constraints**:
- **Discovered gap, decision made**: `dispatch::execute` (the tool-call handler) never reads a conversation's `workspace_id` today — `Bash` runs with the process's ambient working directory, and `Read`/`Write`/`Edit`/`Glob`/`Grep` require whatever path the model supplies, with no relative-path resolution against any workspace. Per the constitution (Principle V: tools operate "not scoped to the opened workspace folder"), this was a deliberate v1.0 choice, not an oversight. **Decision for this feature**: include the *cheap* half of fixing this — the selected folder's path is added to the system prompt (`SYSTEM_PROMPT`) so the model is told what directory it's working in and can produce sensible paths itself — but **not** the deeper half (making `Bash` actually run with that `current_dir`, or having Rust resolve relative paths against it), which is a separate, large-enough change (touching every tool, not just prompting) to warrant its own follow-up spec rather than being absorbed silently here. Flagged, not deferred without saying so.
- **Constitution tension, flagged not silently resolved**: Principle V's no-permission-system decision was written and rationalized around agent mode being a *deliberate, secondary* action (explicitly "opening a workspace"). This feature makes agent mode (unrestricted read/write/execute, no confirmation) the *default and only* path for every new conversation. That's a real increase in the practical blast radius of an existing accepted risk, not just a UI change — see Constitution Check below for the recommended resolution (a documentation-only constitution amendment, not a blocked feature).
- Must not change how already-existing (pre-feature) conversations behave (FR-012) — the `workspaceId`-based view routing naturally satisfies this, since old unscoped conversations still route to the existing plain view.

**Scale/Scope**: One new composer component, one new folder-picker component, one small Rust addition (system-prompt cwd line), a routing change in `App.tsx`, a responsibility split in `Workspace.tsx` (folder-opening moves out, message-view-by-`conversationId` becomes its whole job), and a behavior change in `ConversationList.tsx`'s "+ New conversation" button

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

- **I. Zero-Config First Run** — N/A. Doesn't touch onboarding/first-launch.
- **II. Local-By-Default Privacy** — PASS. No new network calls or telemetry; the native folder dialog is a local OS picker, nothing leaves the device.
- **III. Native macOS Polish** — PASS, and an improvement: the native OS folder-picker dialog (rather than a custom in-app tree) is itself the more "native feeling" choice, consistent with this principle.
- **IV. Extensibility via MCP and Skills** — N/A.
- **V. v1 Scope Discipline** — **Flagged, not blocked.** This feature makes
  the no-permission-system, unrestricted-tool-access experience the
  *default* outcome of every new conversation, where today it requires the
  deliberate, secondary "Open a folder (agent mode)" action. The
  principle's own rationale ("while local chat/agent use is the only way
  to reach the agent" trading off safety for v1.0 simplicity) was written
  assuming agent mode is opt-in, not the default. Per this constitution's
  own governance rules ("reintroducing or further loosening any of them
  requires an explicit constitution amendment, not an ad hoc feature
  decision"), **recommendation: this feature should ship alongside a
  documentation-only constitution amendment** updating Principle V/its
  rationale to explicitly acknowledge that agent-mode-by-default (not just
  agent-mode-available) is the accepted v1.0 posture — a paragraph-level
  update, not a redesign. This is the user's explicit, direct decision
  (confirmed via interview, not assumed), so the recommendation here is
  procedural (write it down where the constitution requires it to be
  written down), not a request to reconsider the decision itself.

No other violations. Complexity Tracking records the one accepted
trade-off above (system-prompt-only cwd, not full path-resolution).

## Project Structure

### Documentation (this feature)

```text
specs/006-chat-empty-state/
├── plan.md              # This file (/speckit-plan command output)
├── research.md          # Phase 0 output (/speckit-plan command)
├── data-model.md        # Phase 1 output (/speckit-plan command)
├── quickstart.md        # Phase 1 output (/speckit-plan command)
├── contracts/           # Phase 1 output (/speckit-plan command) — documents the
│                         # existing-command orchestration sequence, not a new command
└── tasks.md             # Phase 2 output (/speckit-tasks command - NOT created by /speckit-plan)
```

### Source Code (repository root)

```text
src/
├── App.tsx                                # MODIFIED: routes by the active conversation's own
│                                           #   workspaceId instead of a separate agentMode flag
├── lib/
│   └── ipc.ts                             # UNCHANGED — existing commands cover this feature
├── views/
│   ├── chat/
│   │   ├── ConversationList.tsx           # MODIFIED: "+ New conversation" no longer calls
│   │   │                                  #   createConversation() itself — it tells App.tsx to
│   │   │                                  #   show the empty state instead
│   │   ├── Chat.tsx                       # UNCHANGED (still the view for non-workspace conversations)
│   │   └── EmptyState.tsx                 # NEW: the composer + folder-target selector
│   ├── workspace/
│   │   └── Workspace.tsx                  # MODIFIED (restructured): becomes a conversationId-driven
│   │                                      #   message view like Chat.tsx; its old "pick a folder"
│   │                                      #   responsibility moves into EmptyState.tsx
│   └── shared/
│       └── FolderPicker.tsx               # NEW: the recents + search + native-browse popover,
│                                          #   used by EmptyState.tsx

src-tauri/
├── Cargo.toml                              # MODIFIED: add tauri-plugin-dialog
├── src/lib.rs                              # MODIFIED: register the dialog plugin
└── src/agent/mod.rs                        # MODIFIED (narrow): SYSTEM_PROMPT gains the
                                             #   working-directory line when one is known
```

**Structure Decision**: All new UI lives in `src/views/` alongside the
existing chat/workspace/settings views, matching the project's existing
per-feature-folder convention. `FolderPicker.tsx` is placed under a new
`views/shared/` rather than `components/` (unlike the generic `Timer.tsx`
or `Dialog.tsx` primitives) since it's not a generic UI primitive — it's
specifically about Doce's workspace/recents concept, not reusable outside
this domain. The Rust change is deliberately narrow (one prompt-string
addition, one plugin registration) — no new command, no schema change.

## Complexity Tracking

> **Fill ONLY if Constitution Check has violations that must be justified**

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| Agent-mode-by-default (no permission system) becomes the norm for every new conversation, not an opt-in | Directly, explicitly requested by the user via interview ("ONLY and ALWAYS agent mode") — this is a product decision, not an implementation shortcut | A "plain chat, no tools" fallback path was considered (see `research.md` § 1) and explicitly rejected by the user in the interview that grounded this spec |

## Post-Phase 1 Constitution Re-check

Re-evaluated after `data-model.md`, `contracts/`, and `quickstart.md` were
drafted: the design adds no new persisted fields (existing `workspace_id`
already covers it), no new external service, and the one Rust change is a
single prompt-string addition, not new tool-execution power. The
Constitution Check verdicts above hold unchanged, including the
recommended documentation-only amendment for Principle V.
