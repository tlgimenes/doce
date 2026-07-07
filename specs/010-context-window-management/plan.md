# Implementation Plan: Context Window Management for Chat and Agent Mode

**Branch**: `010-context-window-management` | **Date**: 2026-07-04 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/010-context-window-management/spec.md`

## Summary

doce's local model runs with a hardcoded 2048-token context window and zero budget awareness: `load_history()` loads a conversation's entire history unbounded on every turn, and agent-mode tool results (especially Bash output) are appended verbatim with no cap. This feature adds real, client-side context-window management — since llama.cpp has no server-side equivalent of the Anthropic API's context-editing/compaction/memory-tool primitives, everything is reimplemented in the Tauri app itself: (1) live per-conversation token accounting surfaced to the UI, (2) a two-tier compaction pipeline (cheap tool-result clearing, then model-driven summarization) that runs pre-flight before a generation is submitted, (3) size-based offloading of oversized tool outputs to disk with preview+pointer retrieval via the existing `Read` tool, and (4) a minimal, always-visible chat-UI indicator (Claude-Desktop-style, not Claude-Code-CLI's dense breakdown) plus an inline transcript notice when compaction occurs.

## Technical Context

**Language/Version**: Rust 2021 (src-tauri, Tauri 2.x) + TypeScript 5 / React 19 (src, Vite)

**Primary Dependencies**: `llama-cpp-2` 0.1.150 (tokenization + inference), `rusqlite` (SQLite storage), `tauri` + `tauri-specta` (IPC/event codegen), `tokio` (async), Zustand (frontend state), `@tanstack/react-query` (request/response data)

**Storage**: SQLite (existing `messages`/`conversations`/`settings` tables via `src-tauri/src/storage/`), one new migration widening `messages.content_type`

**Testing**: `cargo test` (pure-function unit tests for the compaction/threshold algorithms, following the existing `prefill_chunks`-style precedent in `inference/mod.rs`), `vitest` (frontend unit tests for the new store/indicator), existing `wdio` e2e suite (extended, not required to block this feature)

**Target Platform**: macOS (Apple Silicon), Tauri desktop webview — matches existing project constraints

**Project Type**: Desktop app (Tauri: Rust backend + React frontend) — existing single-project layout, no new project structure needed

**Performance Goals**: Token counting + usage computation must be cheap enough to run synchronously before every generation without perceptible delay (tokenization only, no model forward pass, well under 50ms for conversations within budget); the summarization tier is the one exception and is expected to take as long as a normal short generation (it *is* one)

**Constraints**: Must work entirely offline/on-device (Constitution Principle II — no telemetry, no data leaves the device); must not block the UI thread; must not silently drop a user's in-flight message if compaction is running concurrently (FR-017); the whole feature operates within a small absolute token budget (2048 tokens total) so thresholds are tuned in that regime, not scaled from Claude Code's 200K–1M-token figures

**Scale/Scope**: Single active conversation compaction at a time (consistent with the existing single-flight scheduler); no multi-conversation batch compaction; no cross-session memory (explicitly out of scope per spec)

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-checked after Phase 1 design.*

- **Principle I (Zero-Config First Run)**: Not touched — no onboarding, model-picker, or first-run flow changes. New settings (thresholds) ship with working defaults; the feature is invisible/inert until a conversation grows, no setup required. **PASS**.
- **Principle II (Local-By-Default Privacy)**: All new state (context usage, compaction notices, offloaded tool-output files) stays in the existing local SQLite database and the existing app-data directory. No new network calls, no telemetry. The summarization tier calls the *same already-loaded local model*, not a remote service. **PASS**.
- **Principle III (Native macOS Polish)**: The new indicator is a small in-window UI element following existing design-system conventions (see `specs/008-shared-design-system`); no new window chrome or platform-specific surface. **PASS**.
- **Principle IV (Extensibility via MCP and Skills)**: Not touched. Tool-output offloading applies uniformly to built-in tools; MCP tool results flow through the same `dispatch`/`ChatMessage` path and get the same treatment for free. **PASS**.
- **Principle V (v1 Scope Discipline)**: No onboarding/model-marketplace/permission-system changes. Reusing the existing unrestricted `Read` tool against an app-data-directory path (rather than inventing new file access) is consistent with the already-accepted v1.0 posture of unscoped filesystem access documented in Principle V — this feature does not expand that posture, it merely exercises it. **PASS**.
- **Technology & Platform Constraints**: Uses only the existing stack (Rust/llama.cpp/Tauri/React/SQLite) — no new dependency introduced. **PASS**.
- **Development Workflow**: This plan follows `/speckit-specify` → `/speckit-plan` → `/speckit-tasks` → `/speckit-implement` in order, per the mandated workflow. No onboarding/model-selection/telemetry surface is touched, so the Principle I/II plan-gate check above is the relevant one and it passes.

No violations — Complexity Tracking table is not needed.

## Project Structure

### Documentation (this feature)

```text
specs/010-context-window-management/
├── plan.md                                    # This file
├── research.md                                # Phase 0 output
├── data-model.md                               # Phase 1 output
├── quickstart.md                               # Phase 1 output
├── contracts/
│   └── context-window-management.md            # Phase 1 output
├── checklists/
│   └── requirements.md                         # Already produced by /speckit-specify
└── tasks.md                                    # Phase 2 output (/speckit-tasks, not this command)
```

### Source Code (repository root)

```text
src-tauri/src/
├── inference/
│   └── mod.rs                     # MODIFIED: add CONTEXT_WINDOW_TOKENS const, count_tokens()
├── context/                       # NEW module
│   ├── mod.rs                     #   ContextUsage/ContextState, compute_usage(), maybe_compact()
│   │                              #   (tier-1 clearing + tier-2 summarization orchestration),
│   │                              #   ContextSettings (reads warn/compact/hard-limit thresholds
│   │                              #   from the existing settings table)
│   └── offload.rs                 #   tool-output size check, write-to-file, preview substitution
├── storage/
│   ├── conversations.rs           # MODIFIED: load_history -> load_history_annotated
│   │                              #   (returns content_type-tagged HistoryMessage, splices in
│   │                              #   persisted summaries at context_notice rows)
│   └── migrations/
│       └── 0004_context_notice_content_type.sql   # NEW: widen content_type CHECK, same
│                                                    #   table-rebuild pattern as migration 0003
├── commands/
│   ├── context.rs                 # NEW: get_context_usage, compact_conversation commands +
│   │                              #   ContextUsageUpdate event struct
│   ├── conversations.rs           # MODIFIED: send_message gains InferenceState param, runs
│   │                              #   pre-flight compaction, emits context-usage-update
│   ├── agent.rs                   # MODIFIED: send_agent_message runs pre-flight compaction,
│   │                              #   emits context-usage-update per loop iteration
│   ├── settings.rs                # UNCHANGED (existing get_settings/update_setting reused as-is)
│   └── mod.rs                     # MODIFIED: register new commands + event in collect_commands!/
│                                  #   collect_events!
├── agent/
│   ├── mod.rs                     # MODIFIED: run_loop calls context::offload before pushing an
│   │                              #   oversized tool result into messages
│   └── dispatch.rs                # UNCHANGED (offload happens at the run_loop call site, not
│                                  #   inside individual tool implementations)
└── (no changes to scheduler/ — pre-flight compaction runs before scheduler.submit(), so the
    scheduler itself stays unaware of context management, consistent with §24 of
    specs/001-doce-v1-core/research.md, which already treats a "turn's messages" as an opaque
    payload the scheduler just forwards to InferenceEngine::generate)

src/
├── state/
│   └── contextUsageStore.ts       # NEW: Zustand store, mirrors conversationStreamStore.ts
├── components/
│   ├── ContextUsageIndicator.tsx  # NEW: shared indicator, used by both Chat.tsx and
│   │                              #   Workspace.tsx (same discipline as MessageContent.tsx,
│   │                              #   spec 004 FR-013/SC-006)
│   └── MessageContent.tsx         # MODIFIED: dispatch new 'context_notice' content_type to a
│                                  #   small inline notice renderer (not a tool widget)
├── views/chat/
│   ├── Chat.tsx                   # MODIFIED: render indicator, wire "Compact now" action,
│   │                              #   subscribe to context-usage-update
│   └── tool-widgets/
│       ├── BashWidget.tsx         # MODIFIED: show "view full output" when offloaded
│       └── ReadWidget.tsx         # UNCHANGED (already has a truncated-notice affordance)
├── views/workspace/
│   └── Workspace.tsx              # MODIFIED: same indicator wiring as Chat.tsx
└── lib/
    ├── ipc.ts                     # MODIFIED: new event wrapper, ToolResultDetail gains an
    │                              #   offloaded/fullOutputPath field
    └── bindings.ts                # REGENERATED by tauri-specta (not hand-edited) once the new
                                   #   Rust commands/events/types above compile
```

**Structure Decision**: Existing single Tauri-app layout (`src-tauri/` + `src/`) is reused as-is — this feature adds one new backend module (`context/`) alongside the existing `inference/`, `agent/`, `scheduler/`, `storage/`, `commands/` modules, and one new frontend store + one new shared component alongside the existing `state/`/`components/`/`views/` structure. No new project, package, or build target is introduced.

## Complexity Tracking

*No Constitution Check violations — table intentionally omitted.*
