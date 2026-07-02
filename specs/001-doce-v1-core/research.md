# Phase 0 Research: Doce v1.0 — Zero-Config Local Personal Agent

## 1. llama.cpp Rust bindings

**Decision**: `llama-cpp-2` (crate, MIT/Apache-2.0, wraps llama.cpp via
bindgen), embedded directly in the `src-tauri` Rust backend rather than
shelling out to `llama-server` or bundling a subprocess.

**Rationale**: `llama-cpp-2` mirrors llama.cpp's C API closely and stays
current with upstream by design, which matches the constitution's accepted
trade-off ("tighter integration, more maintenance burden to track upstream").
In-process embedding avoids a subprocess boundary for streaming tokens,
keeps model/context lifetime under direct Rust ownership (needed for the
per-workspace agent orchestrator), and avoids bundling a second binary in the
signed/notarized `.dmg`.

**Alternatives considered**:
- `llama_cpp` (edgenai) — higher-level, more ergonomic async API, but less
  tightly tracked to upstream llama.cpp; rejected because grammar-constrained
  decoding (needed for FR-010) is a fast-moving llama.cpp feature area.
- Shelling out to `llama-server` (HTTP subprocess) — simpler integration, but
  reintroduces a process-management and packaging burden the constitution's
  architecture explicitly rejects ("not a spawned subprocess").

## 2. Grammar-constrained tool calling (FR-010)

**Decision**: Use llama.cpp's native GBNF grammar sampler (exposed through
`llama-cpp-2`'s grammar bindings) to constrain generation to a JSON tool-call
schema when the loaded model lacks native function-calling support. The
agent orchestrator generates a per-turn GBNF grammar from the currently
available tool set (built-in + MCP + skills-declared tools) rather than
maintaining one static grammar.

**Rationale**: GBNF is llama.cpp's built-in grammar mechanism, avoiding a
second constrained-decoding dependency. Generating the grammar per-turn from
the live tool set keeps it correct as MCP servers/skills are added or removed
without redeploying a static schema.

**Alternatives considered**: A separate constrained-decoding library
(e.g. `outlines`-style) — rejected, adds a second decoding path to keep in
sync with llama.cpp's own sampler and duplicates functionality llama.cpp
already ships.

## 3. MCP client

**Decision**: `rmcp` (crate, the official `modelcontextprotocol/rust-sdk`
Rust implementation), used in client mode (`features = ["client"]`) over
tokio async, to connect to user-configured external MCP servers.

**Rationale**: Official SDK maintained by the Model Context Protocol project
itself; using it over a third-party reimplementation minimizes protocol-drift
risk as MCP evolves.

**Alternatives considered**: Hand-rolled JSON-RPC client against the MCP
spec — rejected as unnecessary maintenance burden given an official,
actively maintained SDK exists.

## 4. Local storage

**Decision**: `rusqlite` (bundled SQLite, no separate system dependency)
for chat history, workspaces, settings, and permission grants, with a
small hand-rolled migration runner (versioned `.sql` files applied in order
at startup) rather than an ORM.

**Rationale**: `rusqlite`'s bundled feature avoids requiring a system SQLite
install, which matters for a zero-config app. The data model (Section: Key
Entities in spec.md) is small enough that an ORM would add indirection
without benefit; direct SQL keeps query behavior (esp. permission-grant
lookups on the agent's hot path) easy to reason about.

**Alternatives considered**: `sqlx` — adds async-over-SQLite complexity and
a compile-time query-checking workflow not needed at this scale; `sled` /
embedded KV stores — rejected, relational queries (conversation ↔ messages,
workspace ↔ permission grants) fit SQL better than a KV model.

## 5. Resumable, checksum-verified model downloads

**Decision**: `reqwest` with HTTP `Range` requests for resume, writing to a
`.part` file alongside a small sidecar metadata file (expected size, SHA-256,
bytes-downloaded-so-far); verify the full SHA-256 digest against the model
registry's published checksum before renaming `.part` to the final model
file. Source: Hugging Face model repositories (per constitution).

**Rationale**: Range-request resume is the standard mechanism Hugging Face's
CDN supports; a `.part` + sidecar pattern is simple, dependency-free, and
survives app restarts and network drops (spec FR-003/SC-003) without a
dedicated download-manager crate.

**Alternatives considered**: A dedicated resumable-download crate — surveyed
options are either unmaintained or add more surface than the simple
range-request pattern requires; rejected in favor of the minimal
`reqwest`-based approach.

## 6. Hardware profiling (macOS)

**Decision**: Query `sysctl` (via the `sysctl` crate or direct `libc` FFI
calls to `sysctlbyname`) for chip identifier, physical/unified memory, and
core counts; combine with `std::fs` disk-space queries. Map the result
against the bundled hardware-tier → model table (FR-002).

**Rationale**: `sysctl` is the standard, dependency-light way to read
hardware facts on macOS and needs no elevated privileges — consistent with
zero-config (Principle I): no permission prompt is needed just to profile
the machine.

**Alternatives considered**: Shelling out to `system_profiler` — slower
(spawns a subprocess, parses text/plist output) for information `sysctl`
already exposes programmatically.

## 7. Signing, notarization, and distribution

**Decision**: Use Tauri's built-in macOS signing/notarization pipeline
(`tauri build --bundles dmg` with `APPLE_CERTIFICATE`, `APPLE_ID`,
`APPLE_PASSWORD` / notarytool credentials set as environment variables),
producing a signed, stapled `.dmg`. Distribute via direct download (GitHub
Releases) and a Homebrew cask pointing at the same release artifact.

**Rationale**: This is Tauri v2's first-party, documented flow — no bespoke
signing tooling needed. It directly satisfies constitution Principle III
(native, signed, notarized) and the Technology & Platform Constraints
section's packaging requirement.

**Alternatives considered**: Manual `codesign`/`notarytool` scripting
outside Tauri's pipeline — rejected as redundant; Tauri's bundler already
wraps this correctly when the required environment variables are present.

## 8. Frontend/backend IPC and streaming

**Decision**: Tauri `invoke` commands for request/response calls (e.g. "open
workspace," "grant permission") and Tauri's event system
(`emit`/`listen`) for streaming: token-by-token chat output, model download
progress, and live agent activity (file diffs, terminal output) as they
happen (FR-011).

**Rationale**: This is Tauri's native pattern for backend→frontend push and
avoids introducing a second transport (e.g. a local WebSocket server) for
streaming.

**Alternatives considered**: An embedded local HTTP/WebSocket server between
the Rust backend and the webview — rejected as unnecessary; Tauri's IPC
already covers both request/response and streaming needs in-process.

## 9. Testing strategy

**Decision**:
- Rust backend: `cargo test` for unit tests per module (inference, hardware
  profiler, downloader, permission engine, storage); integration tests in
  `src-tauri/tests/` that exercise the agent tool-use loop and permission
  gate against a temporary workspace directory and an in-memory/temp SQLite
  database.
- Frontend: Vitest + React Testing Library for component/unit tests.
- End-to-end: Tauri's WebDriver-based e2e support (`tauri-driver` +
  WebDriverIO/Playwright over the WebDriver protocol) to drive full user
  journeys (onboarding, chat, agent-mode permission prompts) against a built
  app binary.

**Rationale**: Matches the constitution's Development Workflow expectation
that permission-prompt and onboarding behavior (Principles I and IV) is
verifiable, not just unit-tested in isolation; e2e coverage is the only way
to prove the approval-prompt gate (FR-012/FR-013, SC-004/SC-005) actually
blocks an action end-to-end.

**Alternatives considered**: Skipping e2e in favor of Rust-only integration
tests — rejected because the permission-prompt UX (plain-language prompt,
"always allow" persistence) is a frontend+backend contract that unit/
integration tests on one side alone cannot fully verify.

## Resolved unknowns summary

All items originally flagged as candidates for `NEEDS CLARIFICATION` in the
Technical Context are resolved above; none remain open for Phase 1.
