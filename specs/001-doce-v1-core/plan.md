# Implementation Plan: Doce v1.0 — Zero-Config Local Personal Agent

**Branch**: `001-doce-v1-core` | **Date**: 2026-07-02 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/001-doce-v1-core/spec.md`

**Note**: This template is filled in by the `/speckit-plan` command. See `.specify/templates/plan-template.md` for the execution workflow.

## Summary

Doce v1.0 is a native macOS (Apple Silicon) desktop app that opens directly
into a working local AI agent: on first launch it profiles the host
hardware, auto-downloads a matched local model (no picker, no API key, no
account), and offers two surfaces — a streaming chat assistant and a
per-workspace coding/system agent that reads/edits files and runs shell
commands under an explicit, persistent, plain-language permission model.
Technical approach: a React + TypeScript frontend inside a Tauri 2 webview,
backed by a Rust `src-tauri` process that embeds llama.cpp in-process
(`llama-cpp-2`) for inference, profiles hardware via `sysctl`, downloads and
verifies models over resumable HTTP range requests, runs an agent
orchestrator (built-in tools + MCP via the official `rmcp` client + skills)
with GBNF-grammar-constrained tool calling for non-tool-calling models, and
persists all local state (conversations, workspaces, permission grants,
settings) in a bundled SQLite database via `rusqlite`. Packaged as a signed,
notarized `.dmg` via Tauri's built-in macOS signing/notarization pipeline.

## Technical Context

**Language/Version**: Rust 1.80+ (backend, `src-tauri`); TypeScript 5.x +
React 18 (frontend), orchestrated by Tauri 2.

**Primary Dependencies**: `tauri` 2.x; `llama-cpp-2` (embedded llama.cpp
bindings); `rmcp` (official Rust MCP SDK, client feature); `rusqlite`
(bundled SQLite); `reqwest` (resumable model downloads over HTTP range
requests); `sysctl`/`libc` FFI (macOS hardware profiling); `serde`/
`serde_json` (IPC payloads, registry/config parsing).

**Storage**: Local SQLite via `rusqlite` (conversations, messages,
workspaces, permission grants, MCP server configs, settings) plus the local
filesystem (installed model files, bundled + user skill packs). No remote
storage in v1.0.

**Testing**: `cargo test` (Rust unit tests per backend module) + Rust
integration tests in `src-tauri/tests/` (agent tool-use loop, permission
gate, download resume, against temp workspaces/temp SQLite DBs); Vitest +
React Testing Library (frontend unit/component tests); Tauri's WebDriver-based
e2e (`tauri-driver` + WebDriverIO/Playwright) for full user journeys
(onboarding, chat, agent-mode permission prompts) against a built binary.

**Target Platform**: macOS 13+, Apple Silicon (arm64) only, per constitution
v1 scope discipline.

**Project Type**: Desktop application (Tauri: TypeScript/React frontend +
Rust backend in one repo).

**Performance Goals**: Model download begins within seconds of first launch
with continuously visible progress (SC-002); chat responses begin streaming
promptly after send (no fixed numeric target specified in spec — qualitative
"streams incrementally," per FR-006/User Story 2); agent-mode file
diffs/terminal output surface live, not only on task completion (FR-011).

**Constraints**: No required network calls for core chat/agent functionality
after the initial model download (offline-capable, Principle II); no
telemetry or account-gated functionality (Principle II, FR-017); every
action outside the opened workspace folder or an untrusted shell-command
category MUST be gated by an approval prompt before execution, with zero
silent out-of-scope actions (FR-012, SC-004); trust decisions MUST be
strictly per-workspace (FR-014); hardware-tier → model matching MUST keep
conservative memory headroom to avoid first-run OOM (constitution risk:
Hardware fragmentation).

**Scale/Scope**: Single user, single device, no multi-user or team
concerns; conversation history and workspace state are local-only; agent
mode targets typical individual project folders, not enterprise-scale
monorepos, for v1.0.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Gate | Status |
|---|---|---|
| I. Zero-Config First Run | Onboarding flow (FR-001–FR-004) has no picker/API-key/account step in the default path; model auto-selected via hardware-tier match; override exists only in settings, post-onboarding. | PASS |
| II. Local-By-Default Privacy | All storage is local SQLite/filesystem (Data Model); no network calls other than model download/registry refresh; telemetry setting defaults to off (FR-017, Settings entity). | PASS |
| III. Native macOS Polish | Packaging via Tauri's native macOS signing/notarization pipeline (Research §7) targeting a signed, stapled `.dmg`; Apple Silicon only. | PASS |
| IV. Explicit, Persistent Permissions | PermissionGrant entity + `permission-prompt`/`respond_to_permission_prompt` IPC contract enforce prompt-before-action, per-workspace persistence, and (structurally, via `source` field) a distinct bar for bridged-channel-triggered actions even though no bridge ships in v1.0. | PASS |
| V. Extensibility via MCP and Skills | `rmcp`-based MCP client (`add_mcp_server`/`list_mcp_servers`) and filesystem-based skills loader (`list_skills`) both included in scope. | PASS |
| VI. v1 Scope Discipline | Spec and this plan exclude WhatsApp/other channels, cloud sync, team features, RAG, and non-Apple-Silicon targets; model override is settings-only, never onboarding. | PASS |

No violations — Complexity Tracking table below is empty by design.

*Re-checked after Phase 1 design (data-model.md, contracts/tauri-ipc.md,
quickstart.md): still PASS. The IPC contract has no network-facing surface,
the data model stores nothing off-device, and the permission-prompt contract
matches Principle IV's per-workspace, prompt-before-action requirement
exactly.*

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
src/                        # React + TypeScript frontend (Tauri webview)
├── views/
│   ├── onboarding/          # hardware detection → download progress (User Story 1)
│   ├── chat/                 # streaming conversation, markdown/code, artifacts (User Story 2)
│   ├── workspace/              # file tree, diffs, terminal output, permission prompts (User Story 3/4)
│   └── settings/                 # model override, MCP servers, skills, permission review (User Story 5)
├── components/
├── state/                        # conversation/workspace/permission client state
└── lib/                            # typed wrappers around Tauri `invoke`/`listen` (contracts/tauri-ipc.md)

src-tauri/                  # Rust backend
├── src/
│   ├── inference/            # llama-cpp-2 embedding: model load, context/KV cache, sampling, token streaming
│   ├── hardware/               # sysctl-based hardware profiler → tier matching
│   ├── model_registry/           # versioned hardware-tier → model table, remote refresh
│   ├── downloader/                 # resumable, checksum-verified downloads (reqwest + range requests)
│   ├── agent/                        # tool-use loop orchestrator; GBNF grammar generation for non-tool-calling models
│   ├── permissions/                    # permission engine: workspace-scoped trust store, prompt gating
│   ├── mcp/                              # rmcp-based MCP client
│   ├── skills/                             # filesystem skill-pack discovery/matching
│   ├── storage/                              # rusqlite access layer + migrations
│   └── commands/                               # Tauri IPC command handlers (contracts/tauri-ipc.md)
└── tests/
    ├── contract/                                 # IPC command contract tests
    ├── integration/                                # agent loop, permission gate, download resume
    └── unit/

tests/
├── frontend/                                          # Vitest unit/component tests
└── e2e/                                                  # tauri-driver + WebDriverIO/Playwright user journeys
```

**Structure Decision**: Web-application-shaped layout (frontend + backend)
adapted to a single Tauri desktop app: `src/` is the React/TypeScript
frontend, `src-tauri/` is the Rust backend, matching Tauri's own project
convention rather than a generic library/CLI layout. Module boundaries in
`src-tauri/src/` map directly to the constitution's Technology & Platform
Constraints list (inference, hardware, model_registry, downloader, agent,
permissions, mcp, skills, storage) so each constitution-mandated component
has exactly one home.

## Complexity Tracking

> Fill ONLY if Constitution Check has violations that must be justified

*(No violations — table intentionally omitted.)*
