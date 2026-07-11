# doce

doce is a local-first AI coding/system agent for macOS. It runs entirely
on-device — an embedded [llama.cpp](https://github.com/ggml-org/llama.cpp)
model does the inference, chat history and workspace state live in a local
SQLite database, and there is no account, no API key, and no cloud service
in the loop. Opening the app is the entire setup: it profiles the host
Mac, downloads a model sized to its hardware, and drops you into a working
agent session.

This mirrors the Claude Code experience, but self-hosted on your own
machine. There is exactly one mode — every conversation is an agent
session:

- **Agent conversations**: every conversation is tool-enabled and scoped
  to a working folder — the agent reads/writes files and runs shell
  commands in an iterative plan-and-execute tool-use loop (`Read`,
  `Write`, `Edit`, `Bash`, `Glob`, `Grep`, plus `AskUserQuestion` for
  structured clarifying questions and one-level-deep subagent
  delegation), with markdown/code rendering, local persistence, and
  full-text search across past conversations.
- **Extensibility**: an MCP client for connecting arbitrary MCP servers,
  and filesystem-based skill packs (bundled + user-added) the agent pulls
  into context contextually, or that you can invoke explicitly from the
  chat input with `/`.

See `.specify/memory/constitution.md` for the project's governing
principles — in particular:

- **Zero-config first run** (no model picker, no API key, no account on
  first launch).
- **Local-by-default privacy** (no telemetry, nothing leaves the device
  by default).
- **v1.0 has no permission/approval system**: the agent can read, write,
  and execute anywhere on the local filesystem
  without confirmation prompts, not scoped to the opened folder — the one
  exception is a hard-coded block on a small set of catastrophic,
  irreversible shell commands (e.g. recursive home/root deletion). This is
  a deliberate v1.0 trade-off, not an oversight; see Principle V.
- **v1 targets Apple Silicon Macs only** (`macOS 13+`).

One implementation nuance worth knowing up front: the constitution
describes GBNF-grammar-constrained tool calling for models without native
function calling, but as of this writing that path (`T045`/`T056` in
`specs/001-doce-v1-core/tasks.md`) hasn't been built — the agent loop
instead uses a documented JSON tool-call convention plus a parser tolerant
of real model output noise. It works against the shipped model, but it
doesn't _guarantee_ syntactically valid tool calls the way grammar
constraints would. See that file's "Known gaps" section for the full,
current list of such gaps.

## Prerequisites

- **macOS 13+ on Apple Silicon** (`arm64`) — this is the only supported
  platform for v1 (`src-tauri/tauri.conf.json` sets
  `bundle.macOS.minimumSystemVersion` to `13.0`; Intel Macs and other OSes
  are out of scope per the constitution).
- **Rust**, stable toolchain (`src-tauri/Cargo.toml` sets
  `rust-version = "1.80"`; CI installs the current `stable` channel via
  `dtolnay/rust-toolchain`).
- **Node.js 22** (matches `.github/workflows/ci.yml`'s
  `actions/setup-node` configuration) and npm.
- Xcode Command Line Tools, needed to compile Tauri's macOS integration
  and the Metal-accelerated `llama-cpp-2` backend.

## Getting started

```sh
npm install     # also runs `patch-package` via the postinstall hook
npm run tauri dev
```

`npm run tauri dev` builds the Rust backend, starts the Vite dev server
(`beforeDevCommand` in `src-tauri/tauri.conf.json`), and opens the native
window with hot reload. On first launch it will detect your hardware and
download a model — this is the real, multi-gigabyte download, not a mock.

Other useful scripts (see `package.json` for the full/authoritative list):

- `npm run dev` — Vite dev server only, no Tauri window (frontend-only
  iteration).
- `npm run build` — type-checks and builds the frontend bundle.
- `npm run tauri build` — produces a release bundle (see "Build &
  packaging status" below for what this currently does and doesn't include).
- `npm run lint` / `npm run format` / `npm run format:check` — Oxlint /
  Oxfmt.

## Testing

### Frontend unit/component tests

```sh
npm run test        # vitest run, single pass
npm run test:watch  # vitest, watch mode
```

### Backend tests

```sh
cargo test --manifest-path src-tauri/Cargo.toml --all-targets
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
```

### End-to-end tests (WebdriverIO + `@wdio/tauri-service`)

```sh
npm run test:e2e
```

This one command does everything: `test:e2e` has a `pretest:e2e` hook
that runs `tests/e2e/build-for-e2e.sh` first, which stages a wdio-only
Tauri capability file, builds the frontend with the e2e bridge enabled,
and — importantly — builds the Rust binary with `cargo build --features
wdio`. **A plain `cargo build` is not sufficient**: the WebdriverIO bridge
(`tauri-plugin-wdio` / `tauri-plugin-wdio-webdriver`) is gated behind that
`wdio` Cargo feature and won't be present in a normal debug build. Once
built, `tests/e2e/run-e2e.sh` serves the built frontend via `vite preview`
on port 1420 (a debug Tauri build always loads `devUrl`, so this stands in
for it) and hands off to `wdio`.

**Warning — this wipes real local app data by default.** Before each run,
`tests/e2e/wdio.conf.ts` deletes
`~/Library/Application Support/app.doce.desktop` (doce's real macOS
app-data directory) so `onboarding.spec.ts` can exercise a genuine
zero-config first run — this deletes any real model install and chat
history you have locally. If you're iterating locally against a machine
that already has a model installed and don't want to trigger a fresh
multi-gigabyte download on every run, set:

```sh
DOCE_E2E_SKIP_WIPE=1 npm run test:e2e
```

Do **not** rely on `DOCE_E2E_SKIP_WIPE` as your only e2e validation before
shipping — CI, and any full validation pass, always run with the wipe in
place to prove the real first-run path still works. Also note the suite's
Mocha timeout is a generous 12 minutes per test, because it exercises a
real model download, checksum verification, and first load rather than a
mock.

## Project structure

- `src/` — React + TypeScript frontend (Tauri webview): `views/` (chat,
  onboarding, workspace, settings, shortcuts), `components/` (shared
  UI), `state/`, `lib/`.
- `src-tauri/` — Rust backend: `agent/` (tool-use loop, dispatch, built-in
  tools), `inference/` (embedded llama.cpp), `storage/` (SQLite +
  migrations), `mcp/`, `skills/`,
  `hardware/` + `downloader/` + `model_registry/` (zero-config model
  selection), `commands/` (Tauri IPC surface).
- `specs/` — full spec-kit feature history (see below).
- `tests/e2e/` — WebdriverIO end-to-end specs and harness scripts.

## Full design history

This project is built with spec-driven development throughout
(`constitution` → `specify` → `plan` → `tasks` → `implement` for every
feature). `specs/001-doce-v1-core/` is the v1.0 baseline — spec, plan,
research, data model, IPC contracts, task breakdown, and a
`quickstart.md` manual validation walkthrough. Every feature shipped since
(landing page, color theme, tool-call widgets, keyboard shortcuts,
agent-mode-by-default, workspace cwd resolution, a shared design system,
rich chat input, and beyond) has its own `specs/NNN-*/` directory with the
same structure. Start there — not this README — for the authoritative,
feature-by-feature design record, including documented trade-offs and
known gaps.

## Build & packaging status

`npm run tauri build` (targets configured in
`src-tauri/tauri.conf.json`: `dmg` and `app`) produces a working, unsigned
local build today. **Code signing and notarization are not yet wired
up** — that is open work (`T090` in
`specs/001-doce-v1-core/tasks.md`), and `.github/workflows/ci.yml`
currently has no release/signing job, only `rust`, `frontend`, and `e2e`
verification jobs. The constitution's goal of a signed, notarized `.dmg`
distributed via direct download and a Homebrew cask is the target for v1.0
release, not the current state of this repository.
