# Implementation Plan: Rich Chat Input

**Branch**: `009-rich-chat-input` | **Date**: 2026-07-03 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/009-rich-chat-input/spec.md`

**Note**: This template is filled in by the `/speckit-plan` command. See `.specify/templates/plan-template.md` for the execution workflow.

## Summary

Replace the three separate plain `<textarea>`/`<input>` chat inputs (`EmptyState.tsx`, `Chat.tsx`, `Workspace.tsx`) with one shared Tiptap-based rich input, adding three capabilities none of them have today: large pastes collapse into an expandable "<pasted N lines>" chip, image/file attachments become a chip with hover preview (visible only locally — never sent to the model), and typing "/" in an agent-mode surface opens a picker of locally-installed skills that performs real context injection into that turn. The architecture directly reuses the proven patterns from `~/code/mesh/apps/mesh`'s Tiptap chat input (atom-node chips, ref-based mutable editor config, Floating UI-positioned suggestion popups) for everything that already exists there, and designs fresh (no mesh precedent) for paste-collapse and skill mentions. Sent messages persist their structure so history re-renders the same chips, extending 004-tool-call-widgets' precedent of a `content_type`-discriminated JSON `content` column.

## Technical Context

**Language/Version**: TypeScript ^6 / React 19 (frontend, primary surface for this feature); Rust (backend, `src-tauri` — small additions: message persistence, skill-content resolution, model-text derivation)

**Primary Dependencies**: New — `@tiptap/core`, `@tiptap/react`, `@tiptap/starter-kit`, `@tiptap/suggestion`, `@tiptap/pm`, `@floating-ui/react` (exact versions pinned in research.md). Existing, reused as-is — React 19, Tailwind 4 (`src/components/ui/button.tsx`'s token conventions), `@tauri-apps/plugin-dialog` (already installed for `FolderPicker.tsx`), `@phosphor-icons/react`.

**Storage**: Existing local SQLite (`src-tauri/src/storage/`). Adds `'rich_text'` as a new `messages.content_type` (alongside existing `'text'`/`'tool_call'`/`'tool_result'`/`'error'`), used only for messages containing at least one non-plain-text segment — a plain-text-only message still persists exactly as today (`content_type='text'`, no JSON wrapper), so the common case has zero storage/search-relevance impact.

**Testing**: Three tiers (research.md — empirically verified against doce's exact pinned versions, and matching the split `~/code/mesh` itself uses for its own Tiptap input): pure-logic Vitest unit tests for doc↔JSON conversion, jsdom component tests for structural/rendering correctness (with three new polyfills in `src/test/setup.ts`), and WDIO e2e for real caret navigation/popup positioning. `cargo test` for the backend.

**Target Platform**: Existing — macOS (Apple Silicon), Tauri v2 desktop shell.

**Project Type**: Desktop app (single repo, `src/` frontend + `src-tauri/` backend — existing structure, no new project boundary).

**Performance Goals**: No new numeric target; input responsiveness (typing, pasting, opening the skill picker) must feel immediate — same bar as the plain `<textarea>` it replaces. The editor-recreation-avoidance pattern (ref-based mutable config, `setEditable` instead of remount — research.md) exists specifically to protect this.

**Constraints**: Local-only, per Principle II — image bytes, attachment bytes, and skill content never leave the device (inference is local; "never sent to the model" in this feature's FRs means never included in the local model's prompt, not a network boundary). The local model's context window (2048 tokens, `src-tauri/src/inference/mod.rs`) is a real, already-encountered limit (fixed for prompt overflow earlier this session) — informs the decision to keep image/attachment bytes out of the model-facing text entirely rather than "send now, revisit later."

**Scale/Scope**: Single-user local desktop app; no concurrency or multi-tenant concerns beyond what already exists.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

- **Principle I (Zero-Config First Run)**: Not touched — no onboarding/model-picker change. **PASS.**
- **Principle II (Local-By-Default Privacy)**: Directly reinforced, not just complied with — FR-009 explicitly keeps image bytes out of what's sent to the model, and skill content / pasted text / attachment bytes are read from and rendered against the local filesystem/SQLite only, matching the existing local-inference/local-storage posture. **PASS.**
- **Principle III (Native macOS Polish)**: The native-file-dialog requirement (FR-006, confirmed via interview) is a direct expression of this principle over a web-styled fake picker. **PASS.**
- **Principle IV (Extensibility via MCP and Skills)**: This feature is the first real implementation of "the agent loop discovers and pulls [skills] into context contextually" — previously `list_skills` fed only a Settings display list with zero connection to `send_agent_message`. This closes a gap between the constitution's existing promise and the code, rather than introducing new scope. **PASS, and notably strengthens alignment.**
- **Principle V (v1 Scope Discipline)**: No change to tool-access scope, permission model, or onboarding/telemetry — out of the Development Workflow's mandatory-recheck triggers. **PASS.**

No violations. No Complexity Tracking entries required.

**Re-checked post-design (Phase 1)**: the one new backend surface Phase 0 research surfaced beyond the original description — `read_attached_file`, reading an arbitrary local path's bytes — reads only a path the user explicitly picked via the native OS dialog (or dropped/pasted directly), the same trust boundary the existing `Read` tool already operates under with no permission system (Principle V, already an accepted v1.0 posture). Doesn't introduce a new class of filesystem access, just a narrowly-scoped read of a user-selected file. **PASS, no change to the pre-Phase-0 assessment.**

## Project Structure

### Documentation (this feature)

```text
specs/009-rich-chat-input/
├── plan.md              # This file (/speckit-plan command output)
├── research.md          # Phase 0 output (/speckit-plan command)
├── data-model.md        # Phase 1 output (/speckit-plan command)
├── quickstart.md        # Phase 1 output (/speckit-plan command)
├── contracts/           # Phase 1 output (/speckit-plan command)
└── tasks.md             # Phase 2 output (/speckit-tasks command - NOT created by /speckit-plan)
```

### Source Code (repository root)

```text
src/
├── views/
│   └── chat/
│       ├── rich-input/                    # NEW — the shared editor, replacing all 3 raw inputs
│       │   ├── RichInput.tsx              # useEditor() setup, ref-based mutable config, Enter/Shift+Enter
│       │   ├── RichInput.test.tsx
│       │   ├── serialize.ts               # editor doc -> RichMessageContent (segments) at send time
│       │   ├── serialize.test.ts
│       │   ├── UserMessageContent.tsx     # NEW read-only rendering path for content_type='rich_text'
│       │   ├── UserMessageContent.test.tsx
│       │   └── extensions/
│       │       ├── pasted-text-node.tsx   # NEW atom node: collapse/expand chip (no mesh precedent)
│       │       ├── pasted-text-node.test.tsx
│       │       ├── attachment-node.tsx    # atom node: image/file chip + hover preview (mesh-derived)
│       │       ├── attachment-node.test.tsx
│       │       ├── skill-mention.tsx      # "/" suggestion: @tiptap/suggestion + Floating UI (mesh-derived)
│       │       └── skill-mention.test.tsx
│       ├── EmptyState.tsx                 # MODIFIED — composer uses RichInput
│       ├── Chat.tsx                       # MODIFIED — plain-mode input uses RichInput (no skill mention)
│       └── ...
├── views/
│   └── workspace/
│       └── Workspace.tsx                  # MODIFIED — agent input uses RichInput
├── components/
│   └── MessageContent.tsx                 # MODIFIED — dispatches content_type='rich_text' to UserMessageContent
└── lib/
    └── ipc.ts                             # MODIFIED — RichMessageContent/RichTextSegment types, updated command signatures

src-tauri/
├── src/
│   ├── skills/
│   │   └── mod.rs                         # MODIFIED — read a named skill's SKILL.md content by name
│   ├── agent/
│   │   └── rich_content.rs                # NEW — RichMessageContent/RichTextSegment (serde), model-text expansion
│   ├── commands/
│   │   ├── agent.rs                       # MODIFIED — send_agent_message accepts rich content, expands at send time
│   │   ├── conversations.rs               # MODIFIED — send_message (plain-chat path) gets the same treatment
│   │   └── attachments.rs                 # NEW — read_attached_file(path) -> {data, mimeType, name} (research.md:
                                            # no @tauri-apps/plugin-fs installed; a purpose-built command instead
                                            # of adding a new plugin dependency + capability surface)
│   └── storage/
│       ├── conversations.rs               # MODIFIED — load_history expands content_type='rich_text' rows
│       └── migrations/
│           └── 0003_rich_text_content_type.sql   # NEW — table-rebuild migration adding 'rich_text' to the CHECK constraint
└── capabilities/
    └── default.json                       # UNCHANGED — research.md confirms "dialog:default" already grants
                                            # dialog:allow-open for file (non-directory) mode; the new
                                            # read_attached_file command is a plain Tauri command, no plugin
                                            # permission of its own
```

**Structure Decision**: Single project, existing `src/` (frontend) + `src-tauri/` (backend) split — no new top-level directory. The new `src/views/chat/rich-input/` directory mirrors the existing `src/views/chat/tool-widgets/` convention from `004-tool-call-widgets` (one directory per related-widget-family, colocated tests, a shared dispatch point feeding into `MessageContent.tsx`). Backend changes are additive and small: one new module (`rich_content.rs`) for the shared segment type + expansion logic (used by both `send_agent_message` and the plain-chat `send_message`, and by `load_history` for replay), plus a table-rebuild migration since SQLite can't alter a `CHECK` constraint in place.

## Complexity Tracking

*No Constitution Check violations — this section is not applicable.*
