# Doce — Design Spec

Date: 2026-07-02
Status: Draft, pending user review

## One-liner

A fully local, zero-config personal AI agent for macOS — the Claude Desktop + Claude Code experience, running entirely on-device via an embedded llama.cpp, with no API keys, no cloud dependency, and no setup beyond opening the app.

## Problem / market context

OpenClaw (github.com/openclaw/openclaw) proved the demand for a personal AI agent that acts on your own machine — it grew from ~9K to 60K+ GitHub stars in days. But it is not zero-config: installation is a CLI one-liner plus an `openclaw onboard` flow, it defaults toward hosted LLMs (Claude/GPT) with local inference as a manual power-user path, and it bridges into many messaging channels (WhatsApp, Telegram, Discord, Slack, Signal, iMessage), which is both its biggest draw and its biggest attack surface (inbound messages from arbitrary contacts can reach an autonomous agent with system access). The friction is real enough that third parties are already selling native-macOS wrappers around it (LocalClaw, a $49 "native control center" for managing local models/setup; MacClaw, a Spotlight-style hotkey client) — evidence that "OpenClaw's power, without OpenClaw's setup," is an underserved need even within its own ecosystem.

Enclave AI (enclaveai.app) occupies the adjacent niche — private, local, offline, zero-account, native macOS/iOS app — but it is a passive chat assistant with voice and personas. It does not edit files, run shell commands, control the system, or support MCP/tool use.

**The gap: nobody combines "agent that actually acts on your machine" with "genuinely zero-config, local-by-default, native macOS app."** That is Doce.

## Positioning

*"OpenClaw proved people want a personal agent that does things on their machine. Doce believes it shouldn't need a config file, an API key, or an account — it should open and just work, entirely on your Mac."*

## Goals

- Zero-config first run: open the app, it detects your hardware, downloads an appropriate local model, and you're talking to it. No model picker, no API key entry, no account.
- One app, two surfaces: a chat assistant (Claude Desktop-like) and an autonomous coding/system agent (Claude Code-like) that can read/edit files and run shell commands inside a workspace you open.
- Extensible via MCP (connect external tool servers) and skills (filesystem-based capability packs, matched contextually).
- Fully private by default: no telemetry, no accounts, nothing leaves the device unless the user explicitly opts into a bridged channel (WhatsApp, later).
- Native macOS polish: signed, notarized, feels like a first-class Mac app, not a ported CLI tool.

## Non-goals (v1)

- Windows/Linux support.
- A model marketplace/picker UI as the primary flow (advanced users can override the auto-selected model in settings, but it is not the default path).
- Cloud sync, team/multi-user features, hosted anything.
- RAG over arbitrary personal document stores.
- Full OpenClaw-style channel breadth (Telegram/Discord/Slack/Signal/iMessage) — explicitly deferred past WhatsApp until it validates demand.

## Product spec

### Onboarding

On first launch, the app profiles the host machine (RAM, chip generation, unified memory, disk space) and matches it against a bundled hardware-tier → model table (GGUF, quantization, source, expected footprint, capability tags such as "tool-calling," "coding-focused"). It downloads the matched model automatically (resumable, checksum-verified, progress shown) with no model-selection screen in the default path. The table is fetched from a remote config on subsequent launches so recommendations improve without an app update. Advanced users can override the selected model from settings, but this is not surfaced during onboarding.

### Chat mode

Streaming conversation, markdown/code rendering, artifacts. This is the Claude-Desktop-equivalent surface.

### Agent mode

Opening a folder turns the app into a coding/system agent for that workspace: it reads and edits files, runs shell commands, and iterates in a tool-use loop (built-in tools + MCP tools + skills) against that workspace. Models without native tool-calling are constrained via GBNF grammars to produce structured tool calls.

### MCP and skills

Doce ships an MCP client so users can connect arbitrary MCP servers, matching the extensibility model of Claude Desktop/Code. Skills are filesystem-based `SKILL.md`-style capability packs (bundled defaults + user-added) that the agent loop discovers and pulls into context contextually, the same pattern used by Claude's own skill system.

### Safety and permissions

The agent operates freely inside a workspace folder the user has explicitly opened. The first time it wants to take an action outside that folder, or run a given kind of shell command, it triggers a plain-language approval prompt with an "always allow this" option — the same shape as Claude Code's permission model, simplified for a non-technical audience. Trust decisions persist per workspace, so approved action kinds don't re-prompt.

Actions triggered by a bridged channel (WhatsApp, see below) are held to a stricter bar than actions triggered from local chat: an inbound message can never cause a shell command or file write without a live, in-app approval on the Mac itself. This exists specifically because inbound messages are the one place a stranger could attempt prompt injection against an otherwise fully local, single-user agent.

### WhatsApp bridging (v1.1, not launch-blocking)

The single highest-reach "meet the user where they are" channel, shipped as a fast-follow after v1.0 rather than in the initial launch, so no new protocol/crypto subsystem sits in the launch critical path. Implemented via the `whatsapp-rust` crate (a Rust-native reimplementation of the WhatsApp Web multi-device protocol, modeled on Baileys/whatsmeow, with full Signal Protocol E2E support), linked via QR code as a companion device — the same linking mechanism WhatsApp's own multi-device feature uses, just not through Meta's official Business API.

This is an unofficial protocol reimplementation and using it violates WhatsApp's Terms of Service; it carries a documented, non-hypothetical risk of the linked number being flagged or suspended (OpenClaw's own issue tracker has reports of reconnect loops triggering ban detection). Doce must disclose this risk clearly and un-skippably during WhatsApp setup — not buried in a ToS — and should suggest linking a secondary number for anything but casual, low-risk use.

Other channels (iMessage, Telegram, Discord, ...) are explicitly deferred until WhatsApp validates demand for bridging at all.

## Architecture

**Frontend:** React + TypeScript inside a Tauri webview.
- Chat view (streaming, markdown/code, artifacts)
- Workspace/agent view (file tree, diffs, terminal output panel, permission prompts)
- First-run flow (hardware detection → download progress → done, no configuration screens)
- Settings (model override, MCP servers, skills, permission review, WhatsApp linking once v1.1 ships)

**Backend (Rust):**
- *Inference engine* — llama.cpp embedded via Rust bindings (not a spawned subprocess); manages model load, context/KV cache, sampling, and streams tokens to the frontend via Tauri events.
- *Hardware profiler* — detects RAM/GPU/chip generation, maps to a tier.
- *Model registry* — versioned tier→model table, remotely refreshable.
- *Downloader* — resumable, checksum-verified pulls from Hugging Face.
- *Agent orchestrator* — the tool-use loop across built-in tools, MCP tools, and skills; GBNF-grammar-constrained tool calling for models without native function calling.
- *Permission engine* — the sandboxed-workspace policy above, persisted per workspace, with the stricter bridged-channel bar.
- *MCP client* and *skills loader*.
- *WhatsApp bridge* (v1.1) — `whatsapp-rust`, running in-process in the Rust backend rather than requiring a bundled Node runtime (a structural advantage over OpenClaw's Node/Baileys architecture).
- *Storage* — local SQLite (chat history, workspaces, settings, permission grants). No telemetry, no accounts.

**Packaging:** Tauri bundler → signed and notarized `.dmg`, Apple Silicon first. Distributed via direct download and Homebrew cask, not the Mac App Store (shell execution and arbitrary file access are incompatible with App Sandbox).

## Phased roadmap

- **v1.0 (launch):** onboarding, chat mode, agent mode, MCP client, skills, sandboxed-workspace permissions. No channel bridging.
- **v1.1 (fast-follow, weeks after launch):** WhatsApp bridging, opt-in, with mandatory risk disclosure.
- **Later (unscheduled):** additional channels (iMessage, Telegram, Discord, ...) if WhatsApp validates the model; Windows/Linux; open-core monetization (device sync, team features).

## Risks

- **WhatsApp ToS/ban risk** — see above; mitigated by clear disclosure and defaulting to the strictest permission bar for channel-triggered actions, not by hiding the risk.
- **Model-table maintenance** — the hardware→model mapping needs to stay current as new small/efficient models ship; remote-refreshable table mitigates but doesn't eliminate this as an ongoing content-ops burden.
- **Embedded llama.cpp bindings** — tighter integration than shelling out to `llama-server`, but more maintenance burden to track upstream llama.cpp changes; accepted trade-off for v1.
- **Hardware fragmentation** — Apple Silicon generations vary widely in usable unified memory; the tiering table needs conservative headroom to avoid OOM on first run, which is the worst possible first impression for a zero-config product.

## Naming

**Doce** — Portuguese for "sweet." Ties directly to the positioning: the product should be sweet/simple, not bitter/complicated to set up. Short, easy to type, distinct from the field's English names (Ollama itself sets precedent for a non-English evocative name working in this space). A collision check (GitHub, npm, crates.io) found no meaningful conflicts — only a dormant, unrelated 7-year-old npm package. Two known trade-offs, accepted: non-Portuguese speakers will likely mispronounce it on first read (worth a phonetic note in branding, e.g. "Doce (DOH-see)"), and "doce" means "twelve" in Spanish rather than "sweet," which may read as a mixed signal to part of the Latin American audience. Domain/trademark availability has not been verified through an authoritative registrar/trademark search — that check is still outstanding before the name is fully final.

## Appendix: Marketing (light sketch)

Launch on r/LocalLLaMA and Hacker News ("Show HN") the same day, Product Hunt as a follow-up. The core marketing asset is a screen recording of first run: open the app, it detects the Mac, downloads a model, and the user is already working with an autonomous agent — no clicks in between. README leads with that GIF and a comparison table against OpenClaw and Enclave AI on the "zero-config + agentic + local-by-default + MCP/skills" axis. Fast issue response and "good first issue" labels in the first weeks matter disproportionately for star momentum.

## Appendix: Funding (light sketch)

No hosting costs at scale — inference is fully local, a structural advantage over any cloud-dependent competitor. Phase 0: GitHub Sponsors / Open Collective, stay lean. Phase 1 (post-traction): open-core — MIT/Apache-2.0 core forever free, monetizing only things that inherently need infrastructure (encrypted cross-device sync of a user's own data, team-shared MCP/skill registries, an optional hosted relay for teams pooling a GPU), none of which compromises the local-first privacy pitch. If traction is strong, this is a plausible pre-seed pitch: "the agent OpenClaw proved people want, without the setup."
