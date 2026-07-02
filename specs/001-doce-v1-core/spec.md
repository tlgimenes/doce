# Feature Specification: Doce v1.0 — Zero-Config Local Personal Agent

**Feature Branch**: `001-doce-v1-core`

**Created**: 2026-07-02

**Status**: Draft

**Input**: User description: "Create the baseline specification for Doce v1.0 using the existing design document at docs/superpowers/specs/2026-07-02-doce-design.md as the source of truth. Doce is a fully local, zero-config personal AI agent for macOS — the Claude Desktop + Claude Code experience, running entirely on-device via an embedded llama.cpp, with no API keys, no cloud dependency, and no setup beyond opening the app. Scope: v1.0 launch only — onboarding, chat mode, agent mode, MCP client and skills, sandboxed-workspace permissions. WhatsApp bridging and other channels are v1.1+ and explicitly deferred, not silently dropped. Target users: people who want a personal AI agent that acts on their own Mac without cloud dependency, API key management, or account setup, evaluated against OpenClaw and Enclave AI."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Open the app and start talking, with zero setup (Priority: P1)

A new user downloads and opens Doce for the first time on their Mac. Without
entering any API key, creating an account, or picking a model, the app
detects their hardware, downloads a suitable local model, and lets them start
a conversation.

**Why this priority**: This is the entire differentiator versus every
existing local-agent option (OpenClaw requires a CLI + onboarding flow;
hosted assistants require accounts/API keys). If this journey fails or adds
friction, the product's core value proposition fails with it.

**Independent Test**: Can be fully tested by launching the app on a fresh
machine with no prior configuration and confirming the user reaches a working
chat conversation without being asked for credentials, a model choice, or an
account.

**Acceptance Scenarios**:

1. **Given** a fresh install on a Mac that meets minimum hardware
   requirements, **When** the user opens the app for the first time, **Then**
   the app detects hardware capabilities and begins downloading a matched
   model automatically, with visible progress and no model-selection prompt.
2. **Given** the model download has completed, **When** the user types a
   message, **Then** the app streams a response with no further setup steps
   required.
3. **Given** the model download is interrupted (e.g. network drop), **When**
   the user reopens the app, **Then** the download resumes from where it left
   off rather than restarting.
4. **Given** a user wants a different model than the one auto-selected,
   **When** they open settings, **Then** they can override the model
   selection, but this option is never presented during first run.

---

### User Story 2 - Chat with the local assistant (Priority: P1)

A user has an ongoing conversation with Doce for everyday questions, writing
help, or general assistance — the Claude-Desktop-equivalent experience.

**Why this priority**: Chat is the baseline surface every user touches first
and most often; agent mode builds on the same runtime but chat alone must
stand on its own as a useful, trustworthy local assistant.

**Independent Test**: Can be fully tested by having a multi-turn conversation
and confirming responses stream, render code/markdown correctly, and persist
across app restarts, without opening any workspace folder.

**Acceptance Scenarios**:

1. **Given** an open conversation, **When** the assistant responds, **Then**
   the response streams incrementally rather than appearing all at once.
2. **Given** a response contains code or formatted text, **When** it is
   rendered, **Then** markdown and code blocks display with correct
   formatting (syntax highlighting, copy affordance).
3. **Given** a prior conversation exists, **When** the user reopens the app,
   **Then** their chat history is available locally.

---

### User Story 3 - Turn a folder into a coding/system agent (Priority: P1)

A user opens a project folder in Doce. The app becomes an agent that can read
and edit files and run shell commands in that workspace to complete a task
the user describes, iterating in a tool-use loop — the Claude-Code-equivalent
experience.

**Why this priority**: Acting on the user's machine (not just chatting) is
the second half of Doce's core differentiation versus passive local
assistants like Enclave AI; without it, Doce is not meaningfully different
from a local chatbot.

**Independent Test**: Can be fully tested by opening a sample project folder,
describing a small code change, and confirming the agent reads relevant
files, proposes/applies edits, and can run a shell command, all scoped to
that folder.

**Acceptance Scenarios**:

1. **Given** a workspace folder is open, **When** the user describes a task,
   **Then** the agent reads relevant files, plans steps, and takes actions
   (file edits, shell commands) inside that folder without additional setup.
2. **Given** the underlying model lacks native tool-calling support, **When**
   the agent needs to invoke a tool, **Then** the app still produces valid,
   structured tool calls (via grammar-constrained generation) so the loop
   works regardless of model capability.
3. **Given** an in-progress agent task, **When** the user views the workspace
   view, **Then** they can see file diffs and terminal output as the agent
   works, not just a final result.

---

### User Story 4 - Approve or deny actions with persistent, per-workspace trust (Priority: P1)

While the agent is working, it needs to take an action outside the explicit
boundaries already trusted (e.g. writing outside the opened folder, or
running a new category of shell command). The user is asked for a
plain-language approval before the agent proceeds.

**Why this priority**: This is the safety mechanism that makes "grant an
agent file and shell access" acceptable to a non-technical user; without it,
agent mode is either unsafe or unusable.

**Independent Test**: Can be fully tested by triggering an agent action that
falls outside the current workspace's trusted scope and confirming the app
blocks the action until the user explicitly approves it, and that choosing
"always allow" prevents the same prompt from reappearing for that workspace.

**Acceptance Scenarios**:

1. **Given** the agent wants to act outside the opened workspace folder,
   **When** it attempts the action, **Then** the app pauses and shows a
   plain-language approval prompt describing the action before it executes.
2. **Given** the user selects "always allow this" for an action kind in a
   workspace, **When** the agent later requests the same kind of action in
   that same workspace, **Then** it proceeds without re-prompting.
3. **Given** a trust decision was granted in one workspace, **When** the
   agent operates in a different workspace, **Then** that decision does not
   carry over — each workspace has its own trust state.

---

### User Story 5 - Extend the agent with MCP servers and skills (Priority: P2)

A user connects an external MCP server or adds a skill pack to extend what
the agent can do, beyond the built-in tool set.

**Why this priority**: Extensibility matters for power users and for parity
with the Claude Desktop/Code ecosystem, but a useful zero-config experience
must work fully without it — hence P2, not P1.

**Independent Test**: Can be fully tested by connecting one MCP server and
adding one skill pack, then confirming the agent can discover and use both
during a task.

**Acceptance Scenarios**:

1. **Given** a user adds an MCP server in settings, **When** the agent runs a
   task that could use that server's tools, **Then** those tools are
   available to the agent's tool-use loop.
2. **Given** a skill pack is present (bundled or user-added), **When** the
   agent works on a task matching that skill's purpose, **Then** the skill is
   discovered and pulled into context automatically, without manual
   selection by the user.

---

### Edge Cases

- What happens when the user's Mac does not meet the minimum hardware tier
  for any bundled model? The app must communicate this clearly rather than
  downloading a model that will fail or perform unusably.
- How does the system handle a workspace folder that is deleted or moved
  while the agent has pending trust grants for it?
- What happens if the agent proposes a shell command that would affect files
  outside any workspace (e.g. a command with an absolute path elsewhere)? This
  must be treated as an outside-workspace action requiring approval, not
  silently allowed because the command itself was typed inside the workspace.
- How does the app behave if the remote model-tier config is unreachable on a
  later launch (after the first model is already installed)? The existing
  local model must remain usable; only the recommendation table refresh is
  affected.
- What happens when two agent actions are queued and the user denies the
  first approval prompt — does the agent stop, retry differently, or ask
  again?

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The app MUST, on first launch, detect host hardware
  characteristics (RAM, chip generation, unified memory, disk space) without
  requiring any user input.
- **FR-002**: The app MUST automatically select and download a local model
  matched to the detected hardware tier, with no model-selection screen shown
  in the default first-run path.
- **FR-003**: The app MUST show download progress and MUST resume an
  interrupted download rather than restarting it, verifying integrity via
  checksum before use.
- **FR-004**: The app MUST NOT require an API key, account creation, or sign-in
  at any point in the default first-run flow.
- **FR-005**: Users MUST be able to override the auto-selected model from
  settings after first run.
- **FR-006**: The app MUST provide a chat surface with streaming responses
  and markdown/code rendering.
- **FR-007**: The app MUST persist chat history locally across app restarts.
- **FR-008**: The app MUST let a user open a folder to enter agent mode
  scoped to that folder.
- **FR-009**: In agent mode, the app MUST support reading files, editing
  files, and running shell commands inside the opened workspace as part of an
  iterative tool-use loop.
- **FR-010**: The app MUST support models without native tool-calling by
  constraining generation (e.g. grammar-based constraints) to produce valid
  structured tool calls.
- **FR-011**: The app MUST show the user in-progress agent activity (file
  diffs, terminal output) as it happens, not only a final summary.
- **FR-012**: The app MUST require explicit, plain-language user approval
  before the agent takes an action outside the currently opened workspace
  folder, or before it uses a shell command category not yet trusted in that
  workspace.
- **FR-013**: The app MUST offer an "always allow this" option on approval
  prompts and MUST persist that trust decision per workspace, so the same
  action kind does not re-prompt in that workspace.
- **FR-014**: Trust decisions MUST be scoped per workspace and MUST NOT
  transfer to a different workspace.
- **FR-015**: The app MUST provide an MCP client so users can connect
  external MCP servers, making their tools available to the agent's tool-use
  loop.
- **FR-016**: The app MUST support filesystem-based skill packs (bundled and
  user-added) that the agent discovers and applies contextually without
  requiring manual selection per task.
- **FR-017**: The app MUST NOT transmit telemetry or usage data off-device by
  default, and MUST NOT require an account for any v1.0 functionality.
- **FR-018**: The app MUST store chat history, workspace state, and
  permission grants locally on-device.
- **FR-019**: The app's distributed build MUST be signed and notarized for
  macOS.
- **FR-020**: v1.0 MUST NOT include any bridged messaging channel (WhatsApp
  or otherwise); this is explicitly deferred to a later release, not silently
  omitted.

### Key Entities

- **Workspace**: A folder the user has opened for agent mode. Holds its own
  permission/trust state, independent of other workspaces.
- **Conversation**: A chat thread (either standalone chat or tied to a
  workspace's agent session), containing ordered messages and streamed
  assistant responses, persisted locally.
- **Model**: A local inference model matched to a hardware tier; has a
  source, quantization, expected footprint, and capability tags (e.g.
  tool-calling, coding-focused).
- **Permission Grant**: A persisted trust decision for an action kind within
  a specific workspace (e.g. "run shell commands of category X," "write
  outside workspace folder"), including whether it was a one-time or
  "always allow" grant.
- **Skill**: A filesystem-based capability pack (bundled default or
  user-added) with metadata the agent uses to decide contextual relevance.
- **MCP Server Connection**: A user-configured external tool server the agent
  can call into during a tool-use loop.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A first-time user reaches a working chat conversation within a
  single first-run flow with zero required text-entry steps (no API key, no
  account form, no model picker interaction).
- **SC-002**: On a supported Mac with a typical broadband connection, the
  app completes hardware detection and begins model download within seconds
  of first launch, with visible progress throughout.
- **SC-003**: An interrupted model download resumes successfully in 100% of
  tested interruption scenarios (app quit, network drop) without requiring a
  full restart of the download.
- **SC-004**: In agent mode, 100% of actions that fall outside the current
  workspace folder or an untrusted command category trigger an approval
  prompt before execution — zero silent out-of-scope actions.
- **SC-005**: Once a user grants "always allow" for an action kind in a
  workspace, that same action kind does not re-prompt in that workspace for
  the remainder of the session and across restarts.
- **SC-006**: A user can connect at least one MCP server and have its tools
  available to the agent without restarting the app.
- **SC-007**: No network request containing user conversation content or
  usage telemetry is made without an explicit, separate user opt-in (verified
  by inspection of outbound traffic during standard chat/agent use).

## Assumptions

- "Supported Mac" for v1.0 means Apple Silicon only, per project scope
  discipline; Intel Macs and non-macOS platforms are out of scope.
- A curated, versioned hardware-tier → model table ships with the app and is
  periodically refreshed from a remote config; the refresh affects future
  recommendations only and never invalidates an already-installed, working
  model.
- "Typical broadband connection" for SC-002 assumes a connection capable of
  downloading a multi-gigabyte model file in a reasonable time; no specific
  minimum bandwidth is mandated by this spec.
- Model marketplace/picker UI exists only as a secondary, settings-level
  override path in v1.0, never as an onboarding step, per project scope
  discipline.
- WhatsApp and other channel bridging, cloud sync, team/multi-user features,
  and RAG over arbitrary personal document stores are out of scope for this
  spec and tracked separately for a future release, per project scope
  discipline.
- Minimum viable "shell command category" granularity for approval prompts
  (FR-012/FR-013) is decided during planning/design, not fixed by this spec.
