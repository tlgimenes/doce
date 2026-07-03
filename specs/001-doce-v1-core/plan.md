# Implementation Plan: Doce v1.0 — Zero-Config Local Personal Agent

**Branch**: `001-doce-v1-core` | **Date**: 2026-07-02 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/001-doce-v1-core/spec.md`

**Note**: This template is filled in by the `/speckit-plan` command. See `.specify/templates/plan-template.md` for the execution workflow.

## Summary

Doce v1.0 is a native macOS (Apple Silicon) desktop app that opens directly
into a working local AI agent: on first launch it profiles the host
hardware, auto-downloads a matched local model (no picker, no API key, no
account), and offers two surfaces — a streaming chat assistant and a
coding/system agent that reads/edits files and runs shell commands with no
permission or approval gating (an explicit v1.0 simplification, not scoped
to the opened workspace folder — see constitution Principle V).
Technical approach: a React + TypeScript frontend inside a Tauri 2 webview,
backed by a Rust `src-tauri` process that embeds llama.cpp in-process
(`llama-cpp-2`) for inference, profiles hardware via `sysctl`, downloads and
verifies models over resumable HTTP range requests, runs an agent
orchestrator (built-in tools + MCP via the official `rmcp` client + skills)
with GBNF-grammar-constrained tool calling for non-tool-calling models, and
persists all local state (conversations, workspaces, settings) in a bundled
SQLite database via `rusqlite`. Packaged as a signed, notarized `.dmg` via
Tauri's built-in macOS signing/notarization pipeline.

## Technical Context

**Language/Version**: Rust 1.80+ (backend, `src-tauri`); TypeScript 5.x +
React 18 (frontend), orchestrated by Tauri 2.

**Primary Dependencies**:
- *Backend (`src-tauri`)*: `tauri` 2.x; `tokio` (async runtime, incl.
  `sync::mpsc` bounded channels for inference/agent-loop → Tauri-event
  streaming) + `tokio-util` (`CancellationToken` for generation
  cancellation); `llama-cpp-2` (embedded llama.cpp bindings); `gbnf`
  (JSON-Schema → GBNF grammar generation for FR-014); `rmcp` (official Rust
  MCP SDK, client feature); `rusqlite` (`bundled`, `fts5` features) +
  `tokio-rusqlite` (bundled SQLite with FTS5 search, async-safe access);
  `reqwest` (resumable model downloads over HTTP range
  requests; also fetches the remote model registry, see `research.md`
  §23); `sysctl`/`libc` FFI (macOS hardware profiling); `serde`/
  `serde_json` (payload/registry/config parsing); `tauri-specta` (generates
  typed TS bindings from `#[tauri::command]` signatures); `wiremock`
  (dev-dependency, deterministic download-interruption tests, `research.md`
  §9).
- *Frontend (`src`)*: Vite + `@vitejs/plugin-react` v6 (build tool); React
  19 with the React Compiler (`reactCompilerPreset()` +
  `@rolldown/plugin-babel` + `babel-plugin-react-compiler`); TypeScript;
  Tailwind CSS v4.3.x via `@tailwindcss/vite` (design tokens as CSS custom
  properties, class-based dark mode via `@custom-variant dark`); Base UI +
  the shadcn/ui pattern (accessible, unstyled primitives styled with
  Tailwind tokens); TanStack Query (wraps `invoke` command calls); Zustand
  (scoped stores for the four Tauri event streams); TanStack Form (settings
  screens); `react-markdown` + `shiki` (chat content rendering with
  TextMate-grammar syntax highlighting); CodeMirror 6 (workspace-view
  code/diff viewer); `xterm.js` (`react-xtermjs`) in read-only/log mode
  (workspace-view shell-output rendering); Phosphor Icons; Oxlint + Oxfmt
  (lint/format, standalone Oxc-based tools, not the full Vite+ CLI).

**Storage**: Local SQLite via `rusqlite`/`tokio-rusqlite` (conversations,
messages, workspaces, MCP server configs, settings) plus the local
filesystem (installed model files, bundled + user skill packs). No remote
storage in v1.0.

**Testing**: `cargo test` (Rust unit tests per backend module) + Rust
integration tests in `src-tauri/tests/` (agent tool-use loop, scheduler
priority/cancellation/subagent-cap scenarios, `wiremock`-driven download
resume, FTS5 trigger exclusion, against temp workspaces/temp SQLite DBs);
Vitest + React Testing Library (frontend unit/component tests); WebdriverIO
+ `@wdio/tauri-service` (embedded WebDriver server, the only e2e approach
that supports macOS — plain `tauri-driver` and Playwright do not, see
`research.md` §9) for full user journeys (onboarding, chat, agent mode),
one WDIO spec per `quickstart.md` section, against a built binary. All of
the above run in CI on every push/PR via GitHub Actions (`research.md`
§30, `.github/workflows/ci.yml`).

**Target Platform**: macOS 13+, Apple Silicon (arm64) only, per constitution
v1 scope discipline.

**Project Type**: Desktop application (Tauri: TypeScript/React frontend +
Rust backend in one repo).

**Performance Goals**: Model download begins within seconds of first launch
with continuously visible progress (SC-002); chat responses begin streaming
promptly after send (no fixed numeric target specified in spec — qualitative
"streams incrementally," per FR-006/User Story 2); agent-mode file
diffs/terminal output surface live, not only on task completion (FR-017).

**Constraints**: No required network calls for core chat/agent functionality
after the initial model download (offline-capable, Principle II); no
telemetry or account-gated functionality (Principle II, FR-020); agent-mode
file and shell actions execute without any confirmation/approval gating,
not scoped to the opened workspace folder (FR-013 — an explicit v1.0
simplification, see constitution Principle V); hardware-tier → model
matching MUST keep conservative memory headroom to avoid first-run OOM (a
known risk given Apple Silicon's wide variance in usable unified memory
across generations).

**Scale/Scope**: Single user, single device, no multi-user or team
concerns; conversation history and workspace state are local-only; agent
mode targets typical individual project folders, not enterprise-scale
monorepos, for v1.0.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Gate | Status |
|---|---|---|
| I. Zero-Config First Run | Onboarding flow (FR-001–FR-004) has no picker/API-key/account step in the default path; model auto-selected via hardware-tier match; override exists only in settings, post-onboarding. | PASS |
| II. Local-By-Default Privacy | All storage is local SQLite/filesystem (Data Model); no network calls other than model download/registry refresh; telemetry setting defaults to off (FR-020, Settings entity). | PASS |
| III. Native macOS Polish | Packaging via Tauri's native macOS signing/notarization pipeline (Research §7) targeting a signed, stapled `.dmg`; Apple Silicon only. | PASS |
| IV. Extensibility via MCP and Skills | `rmcp`-based MCP client (`add_mcp_server`/`list_mcp_servers`) and filesystem-based skills loader (`list_skills`) both included in scope. | PASS |
| V. v1 Scope Discipline | Spec and this plan exclude WhatsApp/other channels, cloud sync, team features, RAG, and non-Apple-Silicon targets; model override is settings-only, never onboarding; the no-permission-system simplification (FR-013) is documented here and in `spec.md`'s Assumptions as an explicit, revisit-before-v1.1 trade-off, not a silent omission. | PASS |

No violations — Complexity Tracking table below is empty by design.

*Re-checked after Phase 1 design (data-model.md, contracts/tauri-ipc.md,
quickstart.md): still PASS. The IPC contract has no network-facing surface
and the data model stores nothing off-device. Re-checked again after the
permission system was removed (constitution v2.0.0): still PASS — the
removal is itself Principle V-compliant (an explicitly documented v1.0
scope decision with a named revisit trigger), not an undocumented gap.*

## Project Structure

### Documentation (this feature)

```text
specs/001-doce-v1-core/
├── plan.md              # This file (/speckit-plan command output)
├── research.md          # Phase 0 output (/speckit-plan command)
├── data-model.md         # Phase 1 output (/speckit-plan command)
├── quickstart.md         # Phase 1 output (/speckit-plan command)
├── contracts/             # Phase 1 output (/speckit-plan command)
│   └── tauri-ipc.md
└── tasks.md               # Phase 2 output (/speckit-tasks command - NOT created by /speckit-plan)
```

### Source Code (repository root)

```text
.github/
└── workflows/
    └── ci.yml                # rust/frontend/e2e jobs on every push + PR (research.md §30)

src/                        # React + TypeScript frontend (Tauri webview)
├── views/
│   ├── onboarding/          # hardware detection → download progress (User Story 1)
│   ├── chat/                 # streaming conversation, markdown/code, artifacts (User Story 2)
│   ├── workspace/              # file tree, diffs, terminal output (User Story 3)
│   └── settings/                 # model override, MCP servers, skills (User Story 4)
├── components/
├── state/                        # conversation/workspace client state
└── lib/                            # typed wrappers around Tauri `invoke`/`listen` (contracts/tauri-ipc.md)

src-tauri/                  # Rust backend
├── src/
│   ├── inference/            # llama-cpp-2 embedding: single model/context owner, sampling, token streaming
│   ├── scheduler/           # single-flight generation queue: focus-based dynamic priority, turn-chunking, cancellation, thread-headroom
│   ├── hardware/               # sysctl-based hardware profiler → tier matching
│   ├── model_registry/           # versioned hardware-tier → model table, remote refresh
│   ├── downloader/                 # resumable, checksum-verified downloads (reqwest + range requests)
│   ├── agent/                        # tool-use loop orchestrator (submits per-turn work to scheduler; unrestricted file/shell actions, no approval gate; can spawn one level of isolated, turn-capped subagent runs — see research.md §25); built-in tools Read/Write/Edit/Bash/Glob/Grep/AskUserQuestion (research.md §27, exact Claude Code parity); Bash hard-blocks a small catastrophic-command denylist (research.md §29); GBNF grammar generation for non-tool-calling models
│   ├── mcp/                              # rmcp-based MCP client
│   ├── skills/                             # filesystem skill-pack discovery/matching
│   ├── storage/                              # rusqlite access layer + migrations
│   └── commands/                               # Tauri IPC command handlers (contracts/tauri-ipc.md)
└── tests/
    ├── contract/                                 # IPC command contract tests
    ├── integration/                                # agent loop, scheduler priority/cancellation/subagent-cap, wiremock download resume, FTS5 trigger exclusion (research.md §9)
    └── unit/

tests/
├── frontend/                                          # Vitest unit/component tests
└── e2e/                                                  # WebdriverIO + @wdio/tauri-service; one spec file per quickstart.md section
```

**Structure Decision**: Web-application-shaped layout (frontend + backend)
adapted to a single Tauri desktop app: `src/` is the React/TypeScript
frontend, `src-tauri/` is the Rust backend, matching Tauri's own project
convention rather than a generic library/CLI layout. Module boundaries in
`src-tauri/src/` map directly to the constitution's Technology & Platform
Constraints list (inference, hardware, model_registry, downloader, agent,
mcp, skills, storage) so each constitution-mandated component has exactly
one home; there is deliberately no `permissions/` module, since Doce v1.0
ships with no permission/approval system (constitution Principle V).
`scheduler/` was added alongside `inference/` (not
folded into it) because it's a distinct concern — request queuing,
priority, fairness, and cancellation — that both `agent/`'s tool-use loop
and direct chat sends depend on equally; keeping it separate from the raw
model/context management in `inference/` avoids conflating "how to run the
model" with "whose turn it is to run."

## Complexity Tracking

> Fill ONLY if Constitution Check has violations that must be justified

*(No violations — table intentionally omitted.)*
