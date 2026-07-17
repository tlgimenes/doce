<div align="center">

<img src="site/assets/logo.png" alt="doce" width="128" />

# doce

**Your own AI coding agent — fully local, zero-config, on your Mac.**

No account. No API key. No cloud. Open the app and start building.

[![Download](https://img.shields.io/github/v/release/tlgimenes/doce?label=Download&logo=apple&color=black)](https://github.com/tlgimenes/doce/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/macOS%2013%2B-Apple%20Silicon-lightgrey?logo=apple)](https://github.com/tlgimenes/doce/releases/latest)
[![CI](https://github.com/tlgimenes/doce/actions/workflows/ci.yml/badge.svg)](https://github.com/tlgimenes/doce/actions/workflows/ci.yml)
[![Built with Rust + Tauri](https://img.shields.io/badge/built%20with-Rust%20%2B%20Tauri-dea584?logo=rust)](https://tauri.app)
[![Website](https://img.shields.io/badge/website-doce-black?logo=github)](https://tlgimenes.github.io/doce/)

### [⬇&nbsp; Download the latest release](https://github.com/tlgimenes/doce/releases/latest) &nbsp;·&nbsp; [🌐&nbsp; Website](https://tlgimenes.github.io/doce/)

</div>

---

## What is doce?

doce is a local-first AI coding agent for macOS. It runs **entirely on your
machine** — an embedded [llama.cpp](https://github.com/ggml-org/llama.cpp) model
does the inference, your chat history and workspace state live in a local SQLite
database, and nothing leaves your device. Opening the app is the whole setup: it
profiles your Mac, downloads a model sized to its hardware, and drops you
straight into a working agent session.

Think of the Claude Code experience — but self-hosted, private, and entirely
yours.

## Why doce?

- 🔒 **Private by default** — no telemetry, no account, no cloud. Your code and
  conversations never leave your Mac.
- ⚡ **Zero-config** — no model picker, no API key, no setup wizard. Open it and
  go.
- 🛠️ **A real agent** — every conversation is tool-enabled: it reads, writes, and
  edits files and runs shell commands in an iterative plan-and-execute loop.
- 🧩 **Extensible** — connect any MCP server, and drop in skill packs the agent
  pulls into context automatically (or invoke with `/`).
- 🍎 **Native & fast** — a small Tauri app built in Rust. No Electron, no browser
  tab.

## Install

1. [**Download the latest `.dmg`**](https://github.com/tlgimenes/doce/releases/latest).
2. Open it and drag **doce** into Applications.
3. Launch it — on first run it picks and downloads a model for your hardware,
   then you're in.

> [!NOTE]
> The app isn't code-signed yet, so on first launch macOS may warn about an
> "unidentified developer." Right-click the app → **Open** → **Open**.

**Requirements:** Apple Silicon Mac, macOS 13+.

## Features

- **Agent conversations** — every conversation is scoped to a working folder; the
  agent uses `Read`, `Write`, `Edit`, `Bash`, `Glob`, `Grep`, `AskUserQuestion`,
  and one-level subagent delegation in a tool-use loop — with markdown/code
  rendering, local persistence, and full-text search across past conversations.
- **MCP client** — connect arbitrary MCP servers to extend what the agent can do.
- **Skill packs** — filesystem-based skills (bundled + your own) the agent pulls
  in contextually, or that you invoke explicitly with `/`.

## Privacy & design principles

- **Zero-config first run** — no model picker, no API key, no account.
- **Local by default** — no telemetry; nothing leaves the device by default.
- **No permission prompts (v1)** — the agent can read, write, and execute across
  the local filesystem without confirmation, except a hard-coded block on a few
  catastrophic, irreversible shell commands (e.g. recursive home/root deletion).
  A deliberate v1 trade-off, not an oversight.
- **Apple Silicon, macOS 13+** for v1.

See [`.specify/memory/constitution.md`](.specify/memory/constitution.md) for the
full governing principles.

## Development

Prerequisites: **Rust** (stable), **Node.js 22** + npm, and Xcode Command Line
Tools (to compile Tauri's macOS integration and the Metal-accelerated backend).

```sh
npm install        # also runs patch-package via postinstall
npm run tauri dev  # builds the Rust backend, starts Vite, opens the native window
```

On first launch it detects your hardware and downloads a model — the real,
multi-gigabyte download, not a mock.

Handy scripts (see `package.json` for the authoritative list):

```sh
npm run dev            # Vite dev server only (frontend-only iteration)
npm run build          # type-check + build the frontend bundle
npm run tauri build    # produce a release bundle (.dmg + .app)
npm run lint           # oxlint
npm run format:check   # oxfmt --check
npm run test           # vitest (frontend unit/component tests)
```

Backend checks:

```sh
cargo test   --manifest-path src-tauri/Cargo.toml --all-targets
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo fmt    --manifest-path src-tauri/Cargo.toml --all -- --check
```

The full agent/model integration tests live behind the `bench` cargo feature
(`--features bench`); end-to-end tests drive the real app via WebdriverIO
(`npm run test:e2e`).

## Releases

Every merge to `main` builds and publishes a fresh release automatically, so
[`releases/latest`](https://github.com/tlgimenes/doce/releases/latest) — and the
Download button above — always tracks the newest build.

## Contributing

Issues and pull requests are welcome. Please run the frontend and backend checks
above (`npm run test`, `npm run lint`, `npm run format:check`, and the `cargo`
checks) before opening a PR — the same checks CI runs.

## Contributors

[![Contributors](https://contrib.rocks/image?repo=tlgimenes/doce)](https://github.com/tlgimenes/doce/graphs/contributors)

Created and maintained by [**@tlgimenes**](https://github.com/tlgimenes).

## License

[MIT](LICENSE) © 2026 Tiago Gimenes
