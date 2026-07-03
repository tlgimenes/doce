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
  decoding (needed for FR-014) is a fast-moving llama.cpp feature area.
- Shelling out to `llama-server` (HTTP subprocess) — simpler integration, but
  reintroduces a process-management and packaging burden the constitution's
  architecture explicitly rejects ("not a spawned subprocess").

## 2. Grammar-constrained tool calling (FR-014)

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
for chat history, workspaces, and settings, with a small hand-rolled
migration runner (versioned `.sql` files applied in order at startup)
rather than an ORM. Since `rusqlite` is synchronous and the backend runs on
tokio, all access goes through `tokio-rusqlite` (proxies `rusqlite` calls
through a single dedicated background thread with an async API) rather
than calling `rusqlite` directly from `async fn` command handlers.

**Rationale**: `rusqlite`'s bundled feature avoids requiring a system SQLite
install, which matters for a zero-config app. The data model (Section: Key
Entities in spec.md) is small enough that an ORM would add indirection
without benefit; direct SQL keeps query behavior (e.g. conversation/message
lookups on the chat hot path) easy to reason about. `tokio-rusqlite` keeps
that simplicity while avoiding blocking a tokio worker thread on every
query, which would otherwise starve concurrent command handling (e.g. a
slow query blocking token-streaming events).

**Alternatives considered**: `sqlx` — adds async-over-SQLite complexity and
a compile-time query-checking workflow not needed at this scale; `sled` /
embedded KV stores — rejected, relational queries (conversation ↔ messages,
workspace ↔ conversations) fit SQL better than a KV model; calling
`rusqlite` directly wrapped in ad hoc `tokio::task::spawn_blocking` calls at
every call site — rejected in favor of `tokio-rusqlite`'s single dedicated
thread, which avoids repeating that boilerplate and bounds SQLite access to
one connection without needing a separate pool crate (e.g. `r2d2` +
`r2d2_sqlite`) at this scale.

**Schema conventions** (see `data-model.md`'s "Schema conventions" for the
full statement): primary keys are UUIDv7 stored as `TEXT` rather than
random UUIDv4, so inserts land roughly in B-tree order on the
highest-volume table (`Message`) instead of scattering — most of the
locality benefit of an `INTEGER PRIMARY KEY` (rowid) without giving up a
single ID format usable across the IPC boundary too. Timestamps are
`INTEGER` Unix epoch milliseconds (compact, sorts correctly, trivial in
Rust) rather than `TEXT` ISO 8601. Every connection sets `PRAGMA
journal_mode = WAL` and `PRAGMA foreign_keys = ON` explicitly — the latter
is off by default in SQLite for backward compatibility, and this schema
has real FK relationships (`Conversation.workspace_id`,
`Conversation.spawned_by_conversation_id`, `Message.conversation_id`) that
depend on it being on. Migrations are tracked via SQLite's built-in
`PRAGMA user_version` rather than a hand-rolled tracking table — one less
thing to invent when SQLite already ships the exact mechanism needed.

**Alternatives considered (schema conventions)**: random UUIDv4 — rejected
per the B-tree locality argument above; `TEXT` ISO 8601 timestamps —
more human-readable when inspecting the `.sqlite` file directly with the
`sqlite3` CLI, but rejected in favor of the more compact, idiomatic
`INTEGER` epoch representation; a hand-rolled `schema_migrations` tracking
table — rejected as redundant given `PRAGMA user_version` already exists
for exactly this purpose.

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
workspace," "send message") and Tauri's event system
(`emit`/`listen`) for streaming: token-by-token chat output, model download
progress, and live agent activity (file diffs, terminal output) as they
happen (FR-017).

**Rationale**: This is Tauri's native pattern for backend→frontend push and
avoids introducing a second transport (e.g. a local WebSocket server) for
streaming.

**Alternatives considered**: An embedded local HTTP/WebSocket server between
the Rust backend and the webview — rejected as unnecessary; Tauri's IPC
already covers both request/response and streaming needs in-process.

## 9. Testing strategy

**Decision**:
- Rust backend: `cargo test` for unit tests per module (inference, hardware
  profiler, downloader, scheduler, storage); integration tests in
  `src-tauri/tests/` that exercise the agent tool-use loop and the
  scheduler's queueing/priority behavior against a temporary workspace
  directory and an in-memory/temp SQLite database. Concrete scheduler
  scenarios required (not left implicit, per adversarial review): (1) a
  focused-conversation flip mid-queue correctly reprioritizes already-
  queued requests; (2) a subagent's requests inherit and track its
  spawning conversation's priority as focus changes; (3) canceling a
  request removes only that request, leaving others queued/running
  untouched; (4) a subagent that hits its 30-turn cap (FR-016) actually
  stops and returns a result rather than continuing. Download-resume
  tests (FR-003/SC-003) use a local mock HTTP server (`wiremock` crate)
  configured to serve a partial response then drop the connection,
  giving deterministic, repeatable interruption — not a real network
  fault, which cannot be a reliable CI dependency. A direct SQL-level test
  (insert a message into a subagent-run conversation, assert it's absent
  from `messages_fts`) verifies the FTS5 sync-trigger exclusion (§26)
  without going through the full `search_conversations` command, so a
  broken trigger can't be masked by application-level filtering.
- Frontend: Vitest + React Testing Library for component/unit tests.
- End-to-end: WebdriverIO with `@wdio/tauri-service` to drive full user
  journeys (onboarding, chat, agent mode) against a built app binary. Each
  numbered scenario in `quickstart.md` is a spec: the numbered steps and
  "Expected outcome" lines map directly to a WDIO spec file's
  actions/assertions (one spec file per quickstart section) — quickstart.md
  is not a separate manual-only checklist, it's the source these e2e specs
  are written from, so it stays in sync by construction rather than by
  discipline. Exotic concurrency claims (scheduler priority-switching,
  subagent isolation, the deadlock-free-by-construction argument) are
  verified by the Rust integration tests above, not by e2e — e2e is
  comparatively slow and flaky for timing-sensitive assertions, and the
  claims are backend-internal, not user-observable UI behavior.

**Rationale**: Matches the constitution's Development Workflow expectation
that onboarding behavior (Principles I and II) is verifiable, not just
unit-tested in isolation; e2e coverage is the only way to prove
frontend+backend contracts (e.g. the queued/running indicator described in
`contracts/tauri-ipc.md`'s `generation-queue-update` event) actually hold
end-to-end, not just on one side of the IPC boundary. `@wdio/tauri-service`
specifically matters because Doce is macOS-only (constitution Principle V):
Apple provides no WebDriver for embedded `WKWebView` (unlike Windows'
WebView2 or Linux's WebKitGTK, which `tauri-driver` itself can drive
directly), so plain `tauri-driver` cannot run e2e tests on our target
platform at all. `@wdio/tauri-service` works around this by running an
embedded WebDriver server inside the app itself, which is cross-platform
including macOS.

**Alternatives considered**: Plain `tauri-driver` — rejected outright, no
macOS support. Playwright — rejected; Playwright automates its own bundled
WebKit build, which is a different engine build than the system `WKWebView`
Doce actually ships in, so it would validate the wrong rendering/JS engine
rather than the one users run. Skipping e2e in favor of Rust-only
integration tests — rejected because streaming/queueing UX is a
frontend+backend contract that unit/integration tests on one side alone
cannot fully verify.

## 10. Frontend build tooling

**Decision**: Vite as the frontend build tool (Tauri has no bundler of its
own — it's frontend-agnostic and just serves whatever static output the
configured `frontendDist`/`devUrl` points at; Vite is the officially
recommended default for SPA frameworks and what `create-tauri-app`'s React
template scaffolds). React 19 with the React Compiler enabled, via
`@vitejs/plugin-react` v6 + `reactCompilerPreset()` + `@rolldown/plugin-babel`
+ `babel-plugin-react-compiler`, with the babel plugin ordered before the
`react()` plugin in the Vite config.

**Rationale**: Vite is the path of least resistance for a Tauri + React app
and has first-class Tauri documentation support. The React Compiler removes
a class of manual `useMemo`/`useCallback` optimization work; as of
`@vitejs/plugin-react` v6 (paired with Vite 8), the plugin dropped its
built-in Babel (moved JSX transform/Fast Refresh to oxc/Rust for speed), so
Babel-based tools like the React Compiler now require the separate
`@rolldown/plugin-babel` package rather than the old inline
`react({ babel: {...} })` option.

**Alternatives considered**: Webpack — slower dev server, no particular
benefit for a greenfield Tauri app; skipping the React Compiler — rejected,
it's a low-cost, low-risk addition to a new codebase (nothing to migrate)
and removes a recurring class of manual-memoization bugs.

## 11. Styling and design tokens

**Decision**: Tailwind CSS v4 (latest v4.3.x line), integrated via the
official `@tailwindcss/vite` plugin. Design tokens (color, radius,
typography) are defined as CSS custom properties in a `@theme` block —
Tailwind v4's native, CSS-first token mechanism, not a
`tailwind.config.js` theme object. Dark/light mode uses a `@custom-variant
dark (&:where(.dark, .dark *));` class-based variant (Tailwind v4 removed
the old `darkMode: 'class' | 'media'` config key), with token values
overridden inside a `.dark { ... }` block. The app defaults to following
the OS appearance (read via Tauri's window theme API) and toggles the
`.dark` class accordingly; an explicit user override is persisted in the
`Settings` entity (see `data-model.md`) rather than only tracking OS state.

**Rationale**: Tailwind v4's CSS-first token model is a natural fit for a
CSS-variable-based design system, and the official Vite plugin is simpler
than the v3-era PostCSS pipeline. Defaulting to OS appearance with a
persisted override matches how native Mac apps handle appearance and
directly serves constitution Principle III (Native macOS Polish).

**Alternatives considered**: CSS-in-JS (e.g. styled-components, vanilla-extract)
— rejected, adds runtime or build-step cost Tailwind's utility-class model
avoids, and doesn't pair as naturally with the Base UI + shadcn/ui pattern
chosen for accessible primitives (below), which assumes Tailwind classes.

## 12. Accessible UI primitives

**Decision**: Base UI (not Radix UI) as the unstyled, accessible primitive
layer, used via the shadcn/ui pattern — component recipes copied into the
repo (`src/components/ui/`) and styled with the Tailwind design tokens
above, rather than installing a pre-styled component kit.

**Rationale**: Radix UI's release pace has visibly slowed since its
acquisition by WorkOS, while Base UI (built with involvement from Radix's
original author) reached a stable v1.0 in December 2025 and is the
better-maintained option going forward; shadcn/ui officially added Base UI
as a supported primitive layer in February 2026. For a project starting
now, Base UI is the fresher long-term bet. Accessible primitives matter
concretely here, not just as web best practice: proper focus management,
keyboard navigation, and ARIA roles are what `WKWebView` actually propagates
to macOS's accessibility APIs (VoiceOver), which is part of what
"native macOS polish" (Principle III) means in practice.

**Alternatives considered**: Radix UI — still viable and has more existing
examples/tutorials today, but the slower maintenance trajectory is a poor
bet for a new codebase; React Aria / Headless UI — also credible unstyled-
primitive options, but Base UI's shadcn/ui integration (matching the
existing ecosystem's component patterns and semantic token naming) was the
deciding factor.

## 13. Typed IPC bindings

**Decision**: `tauri-specta` generates TypeScript types and a typed
`invoke` wrapper directly from `#[tauri::command]` function signatures in
the Rust backend. The Rust command signatures are the source of truth;
`contracts/tauri-ipc.md` remains the human-readable design reference but is
not hand-synced against the generated bindings.

**Rationale**: Removes an entire class of frontend/backend drift bugs
(renamed fields, changed types) that a hand-maintained TypeScript IPC layer
would otherwise be prone to, at effectively no runtime cost since it's a
build-time codegen step.

**Alternatives considered**: Hand-written TypeScript types mirroring the
Rust command signatures — rejected as ongoing manual-sync maintenance with
no compile-time guarantee they still match.

## 14. Client-side data and state management

**Decision**: TanStack Query wraps all `invoke` (command) calls — workspace
list, model list, MCP server config, skills list, settings — giving
caching, loading/error state, and refetching for free. The Tauri event
streams (`assistant-token`, `model-install-progress`, `agent-activity`,
`generation-queue-update`) are push-based, not request/response, so they
don't fit TanStack Query; they're held in small Zustand stores scoped to
their actual lifetime: a global `useGenerationQueueStore` (a conversation
list can show a "queued"/"running" badge for any conversation, not just the
one currently open, so this can't be scoped to one component subtree), a
`useConversationStreamStore` scoped to the active conversation, and a
`useWorkspaceActivityStore` scoped to the open workspace.
`model-install-progress` is onboarding-only and not shared elsewhere, so it
stays as local component state rather than a fourth store.

**Rationale**: TanStack Query + Zustand is a well-established pairing:
Query owns anything shaped like a request/response resource, Zustand owns
push-driven/ephemeral UI state. Using Zustand's selector-based subscriptions
(rather than React Context) avoids re-rendering unrelated components on
every streamed token, which would otherwise happen at the streaming
event's frequency (potentially tens of times per second).

**Alternatives considered**: Hand-rolled `useSyncExternalStore` modules
instead of Zustand — technically equivalent (it's what Zustand wraps
internally) but rejected in favor of Zustand's small, well-tested,
ergonomic API rather than re-implementing the same pattern three times;
React Context for the event streams — rejected due to the re-render-storm
risk under high-frequency token streaming described above.

## 15. Chat content rendering

**Decision**: `react-markdown` renders assistant/user message content
(markdown, code blocks) in the chat surface (FR-006), with `shiki` as the
syntax highlighter for code blocks (via a `rehype`/`remark` plugin in the
`react-markdown` pipeline).

**Rationale**: `react-markdown` is the standard, well-maintained React
markdown renderer and composes cleanly with a plugin pipeline rather than
requiring a bespoke parser. `shiki` uses real TextMate grammars (the same
highlighting engine VS Code itself uses), so chat code blocks are
highlighted with the same accuracy/detail as the workspace view's code
viewer (§16), rather than two visually inconsistent highlighting styles in
the same app.

**Alternatives considered**: `rehype-highlight` (highlight.js-based) —
lighter-weight, but less accurate than TextMate-grammar-based highlighting
and would look visually inconsistent next to the CodeMirror 6-based
workspace view.

## 16. Workspace code/diff viewer

**Decision**: CodeMirror 6 for the workspace view's file content and diff
display (FR-017, User Story 3).

**Rationale**: For a Tauri desktop app, bundle size matters far less than
for a web app (shipped once, run locally, not re-transferred per session),
so this was a closer call than it would be on the web — but CodeMirror 6's
modular architecture, near-instant init, and strong accessibility track
record won out over Monaco's heavier, more monolithic footprint for what
v1.0 actually needs (file viewing + diffs, not a full IDE feature set like
IntelliSense or a minimap).

**Alternatives considered**: Monaco Editor — the VS Code editor engine
itself, with a built-in diff view and IDE-grade features out of the box;
rejected in favor of CodeMirror 6's lighter, more purpose-fitted footprint
for v1.0's actual requirements, revisitable later if agent-mode grows
IDE-like feature needs (e.g. IntelliSense-style completions) that Monaco
would provide out of the box.

## 17. Icon set

**Decision**: Phosphor Icons.

**Rationale**: Larger, more stylistically flexible family (multiple
weights) than the shadcn/ui-ecosystem default (`lucide-react`); acceptable
tradeoff since Base UI + the shadcn/ui *pattern* (copied component recipes)
doesn't hard-depend on any specific icon set the way a pre-styled component
kit might.

**Alternatives considered**: `lucide-react` — the more common default
pairing with shadcn/ui recipes, would require zero icon-swap effort if
copying recipes verbatim; Heroicons — natural Tailwind Labs pairing, but
less commonly the default in shadcn/ui examples. Both remain reasonable if
Phosphor's weight/style options prove unnecessary in practice.

## 18. Frontend linting and formatting

**Decision**: Oxlint (linter) + Oxfmt (formatter), used directly as
standalone tools — not the full Vite+ unified CLI/task-runner.

**Rationale**: Both are built on Oxc, the same Rust-based engine already in
play via `@vitejs/plugin-react` v6's oxc-based JSX transform (§10), so this
introduces no new tooling ecosystem — just faster (~50–100x vs ESLint,
~30x vs Prettier), ESLint-/Prettier-compatible drop-ins from the same
team as Vite and Vitest (VoidZero, now Cloudflare-owned as of June 2026).
The broader Vite+ CLI (package-manager/monorepo orchestration, unified
task running) was deliberately not adopted: it was only open-sourced in
March 2026, and its added scope (workspace/monorepo tooling) isn't needed
for this single-app repo — adopting just Oxlint/Oxfmt captures the speed
and ecosystem-consistency benefit without the exposure to a very new,
broader tool's churn.

**Alternatives considered**: Biome — also a fast, unified Rust-based
lint+format tool with a strong track record; a reasonable alternative if
Oxlint/Oxfmt prove less mature in practice than expected. ESLint + Prettier
— the established combo with the largest plugin ecosystem, rejected here
in favor of the faster Oxc-based tools given no specialized lint-rule
package currently required that only ESLint's ecosystem provides.

## 19. Workspace terminal/shell-output rendering

**Decision**: `xterm.js` (via a maintained React wrapper, e.g.
`react-xtermjs`), used in read-only/log mode — the agent runs shell
commands itself (per FR-009), so this renders streamed stdout/stderr with
ANSI color/formatting support; it is not a general-purpose interactive
shell the user types into directly.

**Rationale**: `xterm.js` is the de facto standard for ANSI-aware terminal
rendering on the web (it's what VS Code's own integrated terminal uses),
with the largest ecosystem and most battle-tested track record — a
reasonable priority for a v1.0 launch dependency. A newer alternative,
`wterm` (Zig/WASM core, smaller footprint, more native DOM
selection/accessibility semantics), is worth revisiting later if
accessibility gaps surface in practice, but is too new/unproven to bet a
launch dependency on today.

**Alternatives considered**: `wterm` — rejected for now due to project
maturity, not technical merit; a bespoke plain-text `<pre>` renderer with
manual ANSI-code stripping — rejected, would lose color/formatting fidelity
agent output commonly relies on (e.g. `git diff`, colored test output).

## 20. Settings form handling

**Decision**: TanStack Form for the settings screens (model override,
MCP server add/edit, telemetry opt-in).

**Rationale**: Since TanStack Query is already the client-data layer
(§14), TanStack Form keeps a consistent mental model across data-fetching
and form state rather than introducing a second library family. Its
per-field reactive subscriptions (only the changed field's subscribers
re-render) also fit the same re-render-conscious approach already applied
to streaming state (Zustand, §14) and the React Compiler (§10).

**Alternatives considered**: React Hook Form — still the more
conventional 2026 default and arguably lower-risk for a team not already
deep in the TanStack ecosystem; a legitimate alternative if TanStack Form's
comparatively smaller adoption base becomes a practical problem (fewer
examples/community answers than RHF).

## 21. Streaming channel (inference/agent loop → Tauri events)

**Decision**: A bounded `tokio::sync::mpsc` channel between the
token-generation/agent-loop task (producer) and the task that calls
`app_handle.emit(...)` for streaming events (consumer) — one channel per
in-flight conversation/agent-activity stream.

**Rationale**: `mpsc` is tokio's standard single/multi-producer,
single-consumer primitive and is the natural fit for this shape (one
generation loop pushing tokens, one emitter task draining them). Bounding
the channel applies backpressure on the producer if the frontend/event
system falls behind, rather than letting an unbounded queue grow
unboundedly in memory during a long generation or a slow frontend.

**Alternatives considered**: Unbounded `mpsc` — rejected, no backpressure
means a stalled consumer could grow memory usage indefinitely during a long
agent run; `tokio::sync::broadcast` — unnecessary, there is exactly one
consumer per stream (the emitter task), not a fan-out case.

## 22. GBNF grammar generation from tool schemas

**Decision**: The `gbnf` crate (Rust, JSON-Schema → GBNF grammar
conversion) generates the per-turn grammar described in §2, built from the
JSON schemas of the currently available tool set (built-in + MCP + skills).

**Rationale**: An existing, purpose-built Rust crate for exactly this
conversion avoids hand-writing and maintaining a JSON-Schema-to-GBNF
compiler, which is a meaningfully complex piece of logic. Accepted with
its real limitations named below rather than assumed away, given the
crate is small and still experimental.

**Known limitations to carry into implementation (corrected — a review
pass found the original wording had conflated two different things)**:
- **llama.cpp's own reference JSON-Schema-to-grammar converter** does not
  support mixing plain properties with `anyOf`/`oneOf` in the same schema
  type, and `minimum`/`maximum` constraints only apply to
  `"type": "integer"`, not `"number"` (tracked upstream in
  ggml-org/llama.cpp #7703 for the `anyOf`/`oneOf` gap).
- **The Rust `gbnf` crate itself is a separate, smaller, more experimental
  dependency** with its own, stricter limitations: no numeric-bound
  support at all (no `minimum`/`maximum`/fixed-length constraints of any
  kind, for any type — not even the integer-only support the llama.cpp
  reference converter has), and a known issue with underscore characters
  in property names. The latter is the bigger practical risk for Doce
  specifically: MCP tool schemas commonly use `snake_case` parameter names
  (e.g. `file_path`), so this could affect a meaningful fraction of
  real-world MCP tools, not just an edge case.
- Some MCP server tool schemas may hit either limitation; the agent
  orchestrator will need a schema-simplification/normalization step before
  grammar generation for affected tools, or those tools will be unusable
  with non-tool-calling models until upstream support improves. This is a
  real v1.0 constraint, not a hypothetical edge case — tracked here so
  `/speckit-tasks` creates a task for the fallback/normalization behavior,
  sized against the `gbnf` crate's actual (narrower) capability rather
  than the previously-assumed one.

**Alternatives considered**: Hand-rolled JSON-Schema-to-GBNF generator —
rejected as unnecessary given a maintained crate already exists for this
exact need. `jsonschema2gbnf` (a separate, more feature-complete converter
in this space) — worth evaluating as a fallback if the `gbnf` crate's
narrower capability (above) proves too limiting in practice; not switched
to now since `gbnf` is already the chosen dependency and the gap is
narrow enough to work around with normalization rather than a dependency
swap.

## 23. Model registry format and remote refresh

**Decision**: A single versioned JSON document (`registry.json`), bundled
inside the app (so first-run model matching never depends on network
access, per FR-002) and periodically refreshed from a remote URL on
subsequent launches. Shape:

```json
{
  "schema_version": 1,
  "updated_at": "2026-07-02T00:00:00Z",
  "tiers": [
    {
      "tier_id": "apple-silicon-16gb",
      "min_ram_gb": 16,
      "chip_families": ["M1", "M2"],
      "models": [
        {
          "model_id": "...",
          "source_url": "https://huggingface.co/...",
          "quantization": "Q4_K_M",
          "sha256": "...",
          "capability_tags": ["tool-calling", "coding-focused"],
          "priority": 1
        }
      ]
    }
  ]
}
```

`priority` allows a fallback candidate within a tier if the top choice's
download/verification fails. On each launch, the app attempts a background
fetch of the remote registry; if it parses successfully, has a
`schema_version` the running app understands, and has a newer `updated_at`
than the cached copy, it replaces the locally cached registry for *future*
model-selection decisions only — it never invalidates or re-triggers
re-matching for an already-installed, working model (per the constitution's
"Model-table maintenance" risk mitigation and spec.md's Assumptions).

**Rationale**: Bundling a fallback copy guarantees onboarding (FR-001/
FR-002) never blocks on network access to the registry itself (only the
subsequent model *download* needs network). Versioning via
`schema_version` lets a future incompatible registry format ship without
breaking older installed app versions that fetch it (they simply ignore an
unrecognized `schema_version` and keep using their bundled/cached copy).

**Validation**: The registry is parsed and structurally validated (schema
shape, required fields present) before use; each model's `sha256` remains
the actual integrity guarantee for what gets installed and run — the
registry document itself is not cryptographically signed in v1.0. This was
considered (e.g. ed25519/minisign) and deliberately deferred: it's data
describing where to fetch a model and its expected checksum, not executable
code, and the per-model checksum already prevents a corrupted/tampered
*model file* from being installed; signing the registry itself is a
reasonable v1.1+ hardening candidate rather than a launch blocker.

## 24. Inference scheduling across concurrent conversations/agent tasks (FR-024–FR-028)

**Decision**: Single-flight generation — exactly one model generation runs
system-wide at any moment, owned by one inference worker with one loaded
`LlamaModel`/`LlamaContext` — fed by a **strict, dynamic focus-based
priority rule** rather than a static request-type tier: whichever
conversation is currently focused/viewed in the UI is served first;
everything else (a different chat conversation, or an agent task not
currently being viewed) is served in arrival order once the focused
conversation has no pending request. Priority is not a property fixed on
a request when it's created — it's evaluated against whichever
conversation is currently focused at the moment the worker picks its next
item, via a `set_focused_conversation` signal the frontend sends whenever
the active view changes (see `contracts/tauri-ipc.md`). A request's
effective priority can therefore drop if the user navigates away from its
conversation before it's serviced. There is deliberately **no
anti-starvation/aging mechanism**: background work is served
opportunistically, in the gaps when the focused conversation has nothing
pending, not on a guaranteed schedule (see Rationale). Agent-mode work is
still submitted **per turn** (one LLM call within the tool-use loop), not
as one long-running job: after a turn completes, the orchestrator
resubmits the next turn to the back of the queue rather than looping
tightly against the worker — this is what lets a request for a *different*
conversation actually get a chance between an agent task's turns, whether
or not that agent task happens to be focused. Cancellation is a
`tokio_util::sync::CancellationToken` checked between decode steps, so both
queued (not-yet-started) and in-flight generations can be stopped; a
canceled in-flight generation keeps whatever partial output it already
produced (FR-028). The worker also caps llama.cpp's thread count below the
machine's full core count (informed by the hardware-tier match, §6) to
leave headroom for the OS/UI even during heavy generation. When a request
can't start immediately, the frontend is notified via the queue-status
events in `contracts/tauri-ipc.md` so a queued conversation is visibly
"queued," not indistinguishable from frozen.

Subagents (§25) require no special-case logic here: every Generation
Request carries a `priority_conversation_id` — for a normal conversation's
own turn this is just its own id, and for a subagent's turn it's the id of
whichever conversation spawned it. The priority check above ("is this
request's conversation the one currently focused") is really "is this
request's `priority_conversation_id` the one currently focused," which
makes a subagent's priority automatically track its spawning conversation's
focus state without the scheduler needing to know subagents exist as a
distinct concept at all.

**Rationale**: llama.cpp's alternative — true parallel multi-sequence
decoding via its slot/continuous-batching model (as `llama-server` uses
with `--parallel`) — requires pre-allocating KV cache sized for
`n_ctx × n_seq_max` up front, reserved whether or not those slots are
actually busy. That's an acceptable trade for a server with a known GPU
budget; for a personal Mac that may have as little as 8–16GB of unified
memory, reserving memory for N parallel conversation slots directly
conflicts with the conservative-headroom goal behind the hardware→model
tiering table (Apple Silicon generations vary widely in usable unified
memory, and the tiering table exists specifically to avoid first-run OOM
across that spread). Single-flight generation is also just simpler and
safer to reason about: there is fundamentally one generation loop
consuming CPU/GPU at any moment, which is itself the mechanism that
guarantees the app can't "overtake" the machine — no separate resource
governor is needed on top of it, beyond the thread-count cap.

Focus-based priority (rather than a static request-type tier such as
"chat sends beat agent turns") was chosen because it matches what actually
needs to feel responsive: the thing the user is looking at right now,
whether that's a chat or an agent task mid-run in a workspace view — not a
fixed rule that a chat message always outranks agent work even in a
conversation the user has since navigated away from.

Dropping the anti-starvation/aging mechanism (rather than keeping a
"force through one background item every N focused items" rule) is a
deliberate simplification, accepted with its real failure mode named
rather than hidden: if the user keeps the focused conversation
continuously busy with zero gaps between a response finishing and the
next request being ready, background work can be delayed for as long as
that continues. In practice this is a narrow case — real conversational
usage has pauses (reading a response, composing the next message) during
which the model is idle and background work is serviced regardless of
priority — so the risk is judged acceptable against the complexity an
aging/fairness mechanism would add. See spec.md's Assumptions and Edge
Cases for the user-facing statement of this trade-off.

**Alternatives considered**: True parallel decoding (llama.cpp's native
multi-slot/continuous-batching model) — rejected for v1.0 due to the
memory pre-allocation cost described above, revisitable later if hardware
tiers prove to have consistent headroom for it; mid-generation preemption
(interrupting a turn partway through rather than chunking at turn
boundaries) — rejected as unnecessary complexity, since turn-level
chunking already provides enough interleaving opportunity without needing
to save/restore partial in-flight generation state; a static request-type
priority tier (chat sends always above agent turns, regardless of what's
focused) — rejected in favor of focus-based priority per the rationale
above; an anti-starvation/aging rule on top of focus-based priority —
considered and deliberately dropped for simplicity, accepting the narrow
continuous-use starvation case described above rather than adding
scheduling bookkeeping to prevent it.

**KV-cache context-switching (resolved — was flagged as an open
verification item, a review pass confirmed the answer)**: When the
queue's next item belongs to a different conversation than the one
currently loaded in the context, the fastest approach is to save/restore
that conversation's KV cache state rather than reprocessing its full
history from scratch. `llama-cpp-2`'s `LlamaContext` cleanly exposes this
at the per-sequence level — `state_seq_save_file`/`state_seq_load_file`,
plus `state_seq_get_size_ext`/`state_seq_get_data_ext`/
`state_seq_set_data_ext` for in-memory (non-file) save/restore — no
lower-level FFI needed. (The older whole-context `save_session_file`/
`load_session_file` are deprecated in favor of `state_save_file`/
`state_load_file` and the sequence-scoped variants above; use the
sequence-scoped ones, since Doce's design already keys work by
conversation/sequence.) This is no longer a spike — it's a known
implementation path, though it still carries the memory-vs-throughput
tension noted below.

**Residual tension worth flagging (not fully resolved)**: retaining
multiple conversations' saved KV-cache state in memory to make switching
between them cheap works against the same conservative-memory-headroom
goal (§6, the hardware-tier table) that motivated rejecting true parallel
decoding in the first place — the two are in real tension on the 8–16GB
tier. A reasonable default is to only retain the *single* most-recently-
active conversation's saved state (a size-1 cache) and reprocess on
switch for anything older, rather than retaining state for every open
conversation; the exact retention policy is an implementation-time
tuning decision, not fixed by this research note.

## 25. Subagent architecture (FR-015, FR-016)

**Decision**: A subagent is not a separate execution engine — it's another
instance of the same `agent/` tool-use-loop orchestrator, run against a
fresh, isolated context (its own system prompt + the spawning agent's task
prompt, no parent conversation history, a restricted tool subset) instead
of an ongoing user conversation. This mirrors the Claude Agent SDK's own
documented subagent isolation model exactly: only the subagent's final
result is returned into the spawning agent's context; its intermediate
tool calls and reasoning are never shown to the user or the parent
(FR-015, SC-008).

Rather than inventing a parallel "subagent run" entity, a subagent's
context is stored as another row in the existing `Conversation`/`Message`
schema (`data-model.md`), with `spawned_by_conversation_id` set to
whichever conversation spawned it. This keeps the implementation honest to
"it's not agents, it's LLM loops" — a subagent literally *is* another
conversation, just one the orchestrator opened instead of the user, and
excluded from `list_conversations`' default result rather than modeled as
a different kind of thing. Because it's a real, persisted conversation, a
subagent run is durable and (in principle) resumable/inspectable later,
matching the Claude Agent SDK's own resumable-subagent-by-ID behavior —
even though v1.0 does not expose an IPC surface for a user to manually
resume or inspect one (see Out of scope, below).

Subagent turns are scheduled exactly like any other Generation Request
(§24) — no special-case priority tier. A subagent's requests carry
`priority_conversation_id = spawned_by_conversation_id` (the *spawning*
conversation's id, not the subagent's own), so a subagent's scheduling
priority always tracks whether its spawning conversation is currently
focused, and updates dynamically the same way any other request's priority
does when focus changes.

**Nesting**: capped at exactly one level (FR-016) — the agent orchestrator
does not expose the subagent-spawning tool to a conversation that is
itself a subagent run. This is enforced by the orchestrator (which tool
set it hands to a given conversation's loop), not by a database
constraint on `spawned_by_conversation_id`.

**Turn budget (revised after adversarial review)**: capped at 30 turns by
default (FR-016), matching the Claude Agent SDK's own `AgentDefinition
.maxTurns` convention (commonly recommended around 20–30). The original
draft of this design left this unbounded; an adversarial critique pass
flagged that subagents are effectively a second, invisible way to reach an
already-unrestricted, unconfirmed agent, and unbounded turns had no limit
on how much unsupervised work a single invocation could compound. The cap
is a simple counter in the orchestrator — once reached, the subagent stops
and returns whatever it has, rather than continuing indefinitely. This
does not reopen or resolve the broader no-permission-system decision
(still accepted per constitution Principle V); it specifically bounds the
one part of the subagent design that made that risk worse without limit.

**No-deadlock constraint (implementation-critical)**: the single inference
worker (§24) MUST remain a pure queue consumer — it pulls the next
Generation Request, runs it, and pulls the next one, and must never itself
block waiting on a subagent. "Parent awaits child" is ordinary
orchestration-level async suspension: the parent's own LLM call already
completed (producing the tool-call that requests a subagent) before tool
execution — including spawning and awaiting a subagent — begins, so the
await is a suspended Rust task blocked on a channel (e.g. a `tokio::sync::
oneshot` the subagent signals on completion), not a held queue slot or a
blocked worker thread. Every conversation and every running subagent is a
*producer* submitting Generation Requests into the same queue; the worker
is the sole *consumer*, oblivious to who is awaiting what. A producer can
only ever block on its own result, never on the consumer's availability —
which is what makes this deadlock-free by construction, provided the
implementation actually keeps the worker's queue-draining loop and the
orchestrator's per-conversation/per-subagent await logic on separate async
tasks rather than accidentally coupling them.

**Rationale**: Reusing the existing tool-use-loop orchestrator and
Conversation/Message schema avoids building a second, parallel execution
and storage model for what is functionally identical work (an LLM
tool-calling loop). Inheriting priority from the spawning conversation
(rather than a flat "subagents are always priority 1" rule considered
earlier in this conversation) requires no scheduler-side special-casing at
all — §24's existing priority check already does the right thing once
`priority_conversation_id` is populated correctly at spawn time.

**Alternatives considered**: A flat, subagent-specific priority tier
(always priority 1, regardless of the spawning conversation's focus state)
— rejected once it became clear that inheriting the spawning conversation's
own dynamic priority is strictly simpler (no new tier, no new scheduler
logic) and more correct (a subagent spawned by a conversation the user has
since navigated away from shouldn't keep elevated priority just because it
happened to start while that conversation was focused). A dedicated
"SubagentRun" entity separate from `Conversation` — rejected in favor of
reusing the existing schema, since a subagent is architecturally identical
to a conversation's own agent loop. Claude Agent SDK-style `maxTurns`
enforcement — considered, deliberately deferred (see Turn budget above).

**Out of scope for v1.0**: no IPC command exposes listing, inspecting, or
manually resuming a subagent run — `contracts/tauri-ipc.md`'s
`agent-activity` event gains a coarse `subagent-status` kind so the
workspace view can show that delegation is happening (e.g. "Delegating a
sub-task…") without exposing the subagent's intermediate steps, matching
what Claude Code's own Agent View shows (a coarse "Working/Waiting"
indicator, not a live sub-transcript).

## 26. Local chat search (FR-029, FR-030)

**Decision**: SQLite FTS5, via two external-content virtual tables
(`messages_fts` over `Message.content`, `conversations_fts` over
`Conversation.title`) synced from their source tables by triggers — see
`data-model.md`'s "Search" section for the concrete table definitions.
Ranking uses FTS5's built-in `bm25()`; the highlighted excerpt uses FTS5's
built-in `snippet()`. The sync triggers exclude any row belonging to a
conversation with `spawned_by_conversation_id IS NOT NULL`, so a
subagent's messages are never indexed and can never appear in a search
result (FR-030, SC-009) — the same isolation boundary already enforced on
`list_conversations` (§25), just applied one layer earlier so it can't be
bypassed by searching instead of listing.

**Rationale**: FTS5 is bundled with SQLite (no extra dependency beyond
what `rusqlite` already provides, assuming the `fts5` feature flag is
enabled at compile time) and its external-content pattern avoids
duplicating message/title text into a second store — the FTS index is
purely derived, rebuildable from the source tables if ever needed.
Built-in ranking and snippet generation mean no custom relevance-scoring
or excerpt-highlighting logic needs to be written or maintained.

**Alternatives considered**: A naive `LIKE '%query%'` scan — no relevance
ranking, no highlighting, and a full table scan on every search with no
index to accelerate it, unacceptable even at modest chat-history scale;
an external search engine (e.g. embedding a small vector/text search
library) — unnecessary complexity and a new dependency for a feature
FTS5 already covers well at this scale; duplicating searchable text into a
denormalized table instead of FTS5's external-content mode — rejected,
external-content avoids the duplication for no loss of functionality.

## 27. Built-in tool set (FR-009, FR-010)

**Decision**: Doce's agent tool-use loop exposes exactly the following
built-in tools, matching Claude Code's own tool names and parameter
shapes (per the earlier tool-signature research this session, cross-
verified against directly-observed Claude Code tool schemas):

| Tool | Parameters |
|---|---|
| `Read` | `file_path` (required, absolute path), `offset`, `limit` |
| `Write` | `file_path` (required), `content` (required) |
| `Edit` | `file_path` (required), `old_string` (required, unique match unless `replace_all`), `new_string` (required), `replace_all` (default false) |
| `Bash` | `command` (required), `description`, `timeout` (default 120000ms, max 600000ms), `run_in_background` (default false); hard-blocks a small catastrophic-command denylist before execution, see §29 |
| `Glob` | `pattern` (required), `path` (default cwd); sorted by mtime, capped at 100 results, does not respect `.gitignore` |
| `Grep` | `pattern` (required, ripgrep regex), `glob`, `type`, `multiline`, `output_mode` (`files_with_matches` default \| `content` \| `count`); respects `.gitignore` |
| `AskUserQuestion` | `header`, `question`, `options[]` (`label`, `description`), `multiSelect` — pauses the tool-use loop and surfaces a structured clarifying question to the user |

**Rationale**: The standing project decision (from an earlier session) is
exact tool-set parity with Claude Code, not just "a similar coding agent
tool set" — matching names and parameter shapes means anything written
about Claude Code's tool behavior (prompting patterns, documentation,
user intuition from using Claude Code itself) transfers directly to Doce.
`AskUserQuestion` specifically also now backs the `requires_action`
conversation status (§28) — its exact name matters because the status
computation checks for it by name (`Message.tool_name = 'AskUserQuestion'`,
`data-model.md`).

**`AskUserQuestion`'s pause/resume mechanic**: unlike the other built-in
tools, this one suspends the agent's tool-use loop rather than resolving
immediately. When the loop encounters an `AskUserQuestion` call, it emits
`ask-user-question` (`contracts/tauri-ipc.md`) and awaits the frontend's
`answer_user_question` command — an ordinary suspended async task, the
same shape as a subagent await (§25): the turn that produced this tool
call has already completed generating, so nothing about this holds the
single inference worker hostage. Once the user answers, that answer is
appended as a tool result and the loop resubmits its next turn to the
scheduler like any other turn.

**Alternatives considered**: A simplified/renamed ad hoc question tool
(e.g. the originally-proposed `AskUser`) — rejected in favor of exact
parity per the standing decision; a smaller built-in tool set (e.g.
folding `Glob`/`Grep` into `Bash`-driven `find`/`grep`, as one Claude Code
deployment variant does) — rejected, since exact parity was the explicit
goal, not "a deployment variant of it."

## 28. Conversation status and title generation (FR-011, FR-012)

**Decision**: `Conversation.title` is generated by truncating the user's
first message to a fixed maximum length (~60 characters) at a word
boundary — no model inference involved. `Conversation.status` is a
computed value (`data-model.md`), never stored, evaluated against the
conversation's latest **assistant-authored** message (never the user's
own last message) in this order: `in_progress` if a Generation Request is
currently active/queued for the conversation (checked against the
scheduler's live state, §24); else `failed` if that message has
`content_type = 'error'`; else `requires_action` if it's an
`AskUserQuestion` tool call (`Message.tool_name = 'AskUserQuestion'`) or
its last text segment ends in a `?` outside any `https?://\S+` match;
else `done`.

**Rationale**: Truncation avoids spending an extra Generation Request
(competing for scheduler priority, §24) purely on cosmetic metadata for
every new conversation — cheap and instant. Computing `status` live
rather than caching it avoids a stale-cache invalidation problem: the
inputs (scheduler state, latest message) already have a single source of
truth elsewhere, so a cached column would just be a second place the same
fact could drift out of sync. Checking only the *trailing* character
(after stripping URL matches), not "contains a `?` anywhere," is what
correctly handles a message like "Not sure if tabs or spaces? I used
spaces." — a rhetorical question already resolved in the same message,
which an earlier "contains" draft of this rule would have wrongly flagged
as `requires_action`.

**Known residual gap**: the `https?://\S+` strip only catches URLs with an
explicit scheme. A message ending in a bare, scheme-less domain-plus-query
string (e.g. "...see example.com/search?") would still be misread as a
trailing question mark. This is a narrow edge case — accepted as-is for
v1.0 rather than building a heavier bare-domain-matching heuristic for it.

**Alternatives considered**: An LLM-generated title (like ChatGPT/Claude's
own auto-titling) — rejected per explicit product decision (simplicity,
no extra inference cost); persisting `status` as a stored column updated
by triggers or application code on every relevant write — rejected as
more moving parts than recomputing it at read time for what is, at this
scale, a cheap query.

## 29. Destructive-command denylist (FR-013) — added after adversarial review

**Decision**: The `Bash` tool hard-blocks a small, fixed set of
catastrophic, irreversible command patterns outright — no prompt, no
override, not a permission gate — before they ever reach the shell.
Starting set (exact patterns to be finalized during implementation, not
fixed by this research note): recursive deletion of the user's home
directory or filesystem root (`rm -rf ~`, `rm -rf /`, and equivalent glob
forms), whole-disk erase commands (`diskutil eraseDisk`, `dd` targeting a
raw disk device), and disk-partition-table manipulation. This is a
hardcoded denylist checked before command execution, living in the
`agent/` module alongside the `Bash` tool implementation (`plan.md`).

**Rationale**: An adversarial security critique of this design correctly
pointed out that "no permission system" (accepted, per constitution
Principle V) and "no safeguard against catastrophic irreversible actions"
are two different things being conflated. The no-permission-system
decision trades away *friction* for every action; it was never meant to
also trade away protection against a narrow class of actions that are
irrecoverable even by a technically savvy user (no undo, no trash, full
data loss). A hardcoded, unconditional block on a handful of well-known
catastrophic patterns doesn't reintroduce any of the friction the
no-permission-system decision was rejecting — it never prompts, never
asks, and can't be worked around — so it's additive safety with zero cost
to the "always enable changing anything" experience for the other 99.9%
of actions.

**Alternatives considered**: A broader "risky command" classifier (e.g. an
LLM-based or heuristic risk classifier flagging a wider range of commands)
— rejected, that's a permission/approval system by another name (a
judgment call gating execution) and reopens exactly the friction question
already settled; relying on the no-permission-system decision alone with
no denylist — rejected per the review finding above, since it leaves the
single most catastrophic and least recoverable failure mode completely
unmitigated for free.

## 30. Continuous integration

**Decision**: A single GitHub Actions workflow (`.github/workflows/ci.yml`)
runs on every `push` and `pull_request`, on `macos-26` runners (Apple
Silicon-native — matches the constitution's Apple Silicon-only target;
`macos-14` is being deprecated in 2026, so it's deliberately not used).
Three jobs, run in parallel where they don't depend on each other:
- **`rust`**: `cargo test` (all unit + integration tests, including the
  `wiremock`-based download tests and the FTS5 trigger-exclusion test)
  plus `cargo clippy -- -D warnings` and `cargo fmt --check`.
- **`frontend`**: `npm ci` then Vitest (unit/component tests), plus
  `oxlint`/`oxfmt --check` (§18).
- **`e2e`**: depends on `rust`/`frontend` passing first; builds the Tauri
  app (`cargo tauri build --debug` or equivalent) and runs the WebdriverIO
  suite (`@wdio/tauri-service`) against it, covering every `quickstart.md`
  scenario per §9.

**Rationale**: "Run on every commit" only has teeth if it's actually
enforced, not just possible to run locally — a required CI check on every
push/PR is what makes the test suite a gate rather than a suggestion.
Splitting into three jobs lets `rust`/`frontend` fail fast (seconds to a
couple minutes) before paying the cost of a full Tauri build + e2e run
(the slowest job, matching the testing-strategy rationale that e2e is
comparatively expensive and reserved for what unit/integration tests
can't cover). Apple Silicon runners are required rather than optional —
Doce is arm64-only, and Rosetta-emulated x64 runners wouldn't reflect the
actual target architecture.

**Alternatives considered**: A single monolithic job running everything
sequentially — rejected, slower feedback loop (a lint failure would only
surface after waiting for a full Tauri build). `macos-14` — rejected, in
its deprecation window and being phased out this year. Running e2e on
every push including feature branches with no gating — accepted as-is
(this is the intended behavior, not something rejected); the only gate is
`rust`/`frontend` passing first, to avoid burning e2e time on commits that
don't even compile.

## Resolved unknowns summary

Items 1–30 above are fully resolved; the item 22 (GBNF schema-mixing
limitation, corrected below) and item 24 (KV-cache session-state API,
resolved below) each carry implementation-time notes tracked in their
sections above rather than left implicit. No open follow-ups remain
otherwise for Phase 1.

## Critique Decisions (adversarial review, 8 parallel critics: duplication,
correctness, security, performance, testing, architecture, scope,
documentation — before `/speckit-tasks`)

**Adopted:**
- Fixed 3 stale FR cross-references in `spec.md`'s Assumptions
  (renumbering artifacts) and one imprecise citation in User Story 6.
- Corrected the `gbnf` crate write-up (§22): the min/max-integer-only
  limitation belongs to llama.cpp's reference converter, not `gbnf`
  itself, which has no numeric-bound support at all plus an
  underscore-in-property-name issue — a bigger real risk for `snake_case`
  MCP tool params.
- Resolved the KV-cache session-state open question (§24):
  `llama-cpp-2` does expose per-sequence save/restore
  (`state_seq_save_file`/`state_seq_load_file`); converted from a spike to
  a known path, while adding the memory-vs-throughput tension the
  performance critic raised as a named, not-fully-resolved residual note.
- Added an `error` `content_type` to `Message` (data-model.md) — two
  critics independently found `failed`-status detection had nothing
  concrete to check for.
- Resolved the dual source of truth for "active model" — `Model.is_active`
  is authoritative; removed the ambiguous "model override choice" mention
  from `Settings`.
- Fixed the `requires_action` rule's internal contradiction ("contains a
  `?` anywhere" vs. "ends in a `?`") in favor of "ends in," and restricted
  status computation to the latest **assistant-authored** message (not
  the user's own last message, which could itself end in "?").
- Added a subagent turn cap (30, default) — the scope/security critics
  both flagged that subagents are, in effect, an invisible second way to
  reach an already-unrestricted agent, with nothing bounding how much
  unsupervised work one invocation could do.
- Added a narrow, hardcoded catastrophic-command denylist to `Bash` (§29)
  — orthogonal to the no-permission-system decision (a hard block, not a
  prompt), addressing the security critic's point that "no permission
  system" and "no protection against unrecoverable disasters" are
  different things that had been conflated.

**Rejected (with reason):**
- **Keeping everything in v1.0 scope** (scope critic proposed deferring
  subagents and/or search to v1.1) — rejected for now. The specific
  compounding risk the scope critic raised about subagents (invisible,
  unbounded) is what the new turn cap directly addresses; search is
  low-risk, cheap, and already well-isolated (FTS5, explicit subagent
  exclusion). This was decided in the user's absence using the
  recommended default from an interview they didn't get to answer — worth
  revisiting explicitly if they'd have chosen differently.
- **A broader risk-classifier for shell commands** (an alternative to the
  narrow denylist) — rejected as a permission/approval system by another
  name, reopening exactly the friction question already settled.

**Adapted (partially addressed, not fully — documented as residual risk
rather than deferred or silently dropped):**
- **Prompt injection via tool results** (security critic) — genuinely
  unsolved; no code or architecture change adopted, but naming it here
  ensures it isn't mistaken for "considered and dismissed." A concrete
  mitigation (e.g. tagging tool-origin content distinctly in the prompt)
  is a candidate implementation-time task, not a v1.0 spec decision.
- **`list_conversations` latest-message-per-conversation query shape**
  (performance critic's N+1 concern) — not solved here; flagged as an
  implementation-time query-design decision (window function vs. a
  trigger-maintained `last_message_id`), not fixed in this document.
- **GBNF grammar recompilation cost per turn** (performance critic) — no
  caching-by-tool-set-hash added to this document; worth an
  implementation-time optimization pass, not blocking `/speckit-tasks`.
- **`contracts/tauri-ipc.md` staleness relative to generated `tauri-specta`
  bindings** (architecture critic) — no CI check specified; the existing
  wording already says the doc is a design reference, not hand-synced,
  which is judged sufficient for now.
- Testing critic's points about naming a concrete fault-injection
  mechanism for download-interruption tests, and enumerating specific
  Rust scheduler test scenarios — left for `/speckit-tasks` to define as
  actual test tasks, not this design document's job.

**Not addressed at all (explicitly out of scope for this pass):** at-rest
SQLite encryption, macOS TCC interaction with unrestricted file access,
MCP server provenance/sandboxing, FTS5 query-string sanitization, and the
broader "should every casual feature addition require a constitution
amendment" process question the scope critic raised. None of these are
disputed as valid — they're deprioritized relative to the items above
given limited review time, and worth a follow-up pass.
