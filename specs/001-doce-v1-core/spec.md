# Feature Specification: doce v1.0 — Zero-Config Local Personal Agent

**Feature Branch**: `001-doce-v1-core`

**Created**: 2026-07-02

**Status**: Draft

**Input**: User description: "Create the baseline specification for doce v1.0 using the existing design document at docs/superpowers/specs/2026-07-02-doce-design.md as the source of truth. doce is a fully local, zero-config personal AI agent for macOS — the Claude Desktop + Claude Code experience, running entirely on-device via an embedded llama.cpp, with no API keys, no cloud dependency, and no setup beyond opening the app. Scope: v1.0 launch only — onboarding, chat mode, agent mode, MCP client and skills, sandboxed-workspace permissions. WhatsApp bridging and other channels are v1.1+ and explicitly deferred, not silently dropped. Target users: people who want a personal AI agent that acts on their own Mac without cloud dependency, API key management, or account setup, evaluated against OpenClaw and Enclave AI."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Open the app and start talking, with zero setup (Priority: P1)

A new user downloads and opens doce for the first time on their Mac. Without
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

A user has an ongoing conversation with doce for everyday questions, writing
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

A user opens a project folder in doce. The app becomes an agent that can read
and edit files and run shell commands to complete a task the user describes,
iterating in a tool-use loop — the Claude-Code-equivalent experience. The
opened folder is the agent's working project context, not a restriction on
where it can act.

**Why this priority**: Acting on the user's machine (not just chatting) is
the second half of doce's core differentiation versus passive local
assistants like Enclave AI; without it, doce is not meaningfully different
from a local chatbot.

**Independent Test**: Can be fully tested by opening a sample project folder,
describing a small code change, and confirming the agent reads relevant
files, proposes/applies edits, and can run a shell command, with no approval
prompt interrupting the flow.

**Acceptance Scenarios**:

1. **Given** a workspace folder is open, **When** the user describes a task,
   **Then** the agent reads relevant files, plans steps, and takes actions
   (file edits, shell commands) without additional setup or confirmation
   prompts, not limited to that folder.
2. **Given** the underlying model lacks native tool-calling support, **When**
   the agent needs to invoke a tool, **Then** the app still produces valid,
   structured tool calls (via grammar-constrained generation) so the loop
   works regardless of model capability.
3. **Given** an in-progress agent task, **When** the user views the workspace
   view, **Then** they can see file diffs and terminal output as the agent
   works, not just a final result.
4. **Given** the agent attempts a catastrophic, irreversible command (e.g.
   recursive deletion of the home directory or a whole-disk erase),
   **When** it tries to run that command, **Then** the app blocks it
   outright with no prompt and no way to override — the one narrow
   exception to "no confirmation prompts" (FR-013, SC-011).

---

### User Story 4 - Extend the agent with MCP servers and skills (Priority: P2)

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

### User Story 5 - Keep working across multiple chats and agent tasks without the app freezing (Priority: P2)

A user has more than one conversation or agent task active at the same
time (e.g. a background agent task running in one workspace while chatting
in a different conversation). Local inference is comparatively slow and a
single embedded model can only generate for one conversation at a time, so
the app queues the underlying work rather than attempting it all at once —
but stays responsive and visibly shows what's queued rather than appearing
frozen or silently dropping a message.

**Why this priority**: This is what makes it safe to use doce across
multiple chats/tasks at once without it appearing broken or monopolizing
the machine. It's P2 rather than P1 because a single active conversation
(User Stories 2/3) must work correctly on its own regardless of this
mechanism — this story is about behavior under concurrent load, not core
functionality.

**Independent Test**: Can be fully tested by opening two conversations,
sending a message to each in quick succession, and confirming: (a) the
app's UI stays responsive throughout, (b) the second message is visibly
shown as queued until the first completes, (c) both eventually receive a
response.

**Acceptance Scenarios**:

1. **Given** a generation is in progress for one conversation, **When** the
   user sends a message in a different conversation, **Then** the new
   message is queued and shown as queued rather than silently dropped or
   making the app appear frozen.
2. **Given** the user is actively viewing a conversation, **When** that
   conversation and another conversation (or an agent task not currently
   being viewed) both have a pending generation request, **Then** the
   viewed conversation's request is served first.
3. **Given** a background conversation or agent task has a queued request,
   **When** the focused conversation has no pending request of its own,
   **Then** the background request is served during that gap rather than
   waiting for the user to stop using the focused conversation entirely.
4. **Given** an in-progress or queued generation, **When** the user cancels
   it, **Then** it stops (or is removed from the queue) without affecting
   other queued or running work.

---

### User Story 6 - Find something from a past conversation (Priority: P2)

A user remembers discussing something in a past conversation but not which
one. They search by keyword and find the relevant conversation(s), ranked
by relevance, with a highlighted excerpt showing the match.

**Why this priority**: Valuable once a user has accumulated conversation
history, but not required for the zero-config first-run value proposition
or for a single active conversation to work correctly — hence P2.

**Independent Test**: Can be fully tested by having several conversations
with distinct topics, searching for a keyword unique to one of them, and
confirming that conversation is returned with a relevant excerpt.

**Acceptance Scenarios**:

1. **Given** multiple conversations exist with different content, **When**
   the user searches for a keyword appearing in one of them, **Then** that
   conversation is returned with a highlighted excerpt showing where the
   match occurred.
2. **Given** a keyword appears in a conversation's title but not its
   message content (or vice versa), **When** the user searches for it,
   **Then** the conversation is still found.
3. **Given** a subagent run occurred as part of some conversation, **When**
   the user searches for a keyword that appears only in that subagent's
   internal messages, **Then** no result surfaces that content — search
   respects the same isolation boundary as the rest of the app (FR-015,
   enforced for search specifically by FR-030).

---

### User Story 7 - See at a glance which conversations need attention (Priority: P2)

A user has multiple conversations going (chat and/or agent-mode). Each
shows an auto-generated title and a status indicator, so the user can
tell without opening it whether it finished normally, is waiting on them
to answer something, or failed.

**Why this priority**: Becomes valuable as soon as a user has more than
one or two conversations going; not required for the zero-config
first-run value proposition or for a single conversation to work
correctly — hence P2.

**Independent Test**: Can be fully tested by driving conversations into
each of the three terminal outcomes (a normal finish, an agent asking a
clarifying question, and an induced failure) and confirming each shows
the correct status without opening the conversation.

**Acceptance Scenarios**:

1. **Given** a new conversation, **When** the user sends their first
   message, **Then** the conversation's title is generated by truncating
   that message — no additional model inference is used.
2. **Given** a conversation's most recent turn completes normally with no
   trailing question, **When** its status is displayed, **Then** it shows
   as `done`.
3. **Given** a conversation's most recent turn ends with an
   `AskUserQuestion` tool call, or its last text ends in a real question
   mark not part of a URL, **When** its status is displayed, **Then** it
   shows as `requires_action`.
4. **Given** a conversation's most recent turn ends in an unrecoverable
   error, **When** its status is displayed, **Then** it shows as `failed`.
5. **Given** a conversation currently has an active or queued generation
   request, **When** its status is displayed, **Then** it reflects that
   ongoing work (`in_progress`) rather than jumping ahead to a terminal
   outcome that hasn't happened yet.

---

### Edge Cases

- What happens when the user's Mac does not meet the minimum hardware tier
  for any bundled model? The app must communicate this clearly rather than
  downloading a model that will fail or perform unusably.
- How does the app behave if the remote model-tier config is unreachable on a
  later launch (after the first model is already installed)? The existing
  local model must remain usable; only the recommendation table refresh is
  affected.
- What happens if the user keeps the focused conversation continuously busy
  (sending a new message the instant each response finishes, with no gap)?
  Background requests (other conversations, or an agent task not currently
  being viewed) may be delayed for as long as that continues. This is an
  accepted trade-off of prioritizing whatever the user is actively looking
  at, not a guaranteed-fair scheduling promise — see Assumptions.
- What happens when the user cancels a generation that has already produced
  partial output? The partial output must remain visible/saved rather than
  discarded, with a clear indication that it was stopped early.

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
- **FR-008**: The app MUST let a user open a folder to enter agent mode,
  using that folder as the working project context for the conversation.
- **FR-009**: In agent mode, the app MUST support the following built-in
  tools as part of an iterative tool-use loop, matching Claude Code's own
  tool set exactly (name and purpose): `Read` (read a file), `Write`
  (create/overwrite a file), `Edit` (targeted in-place edit), `Bash` (run
  a shell command), `Glob` (find files by name pattern), and `Grep`
  (search file contents) — without restricting these actions to the
  opened workspace folder.
- **FR-010**: The agent's tool-use loop MUST support an `AskUserQuestion`
  tool (matching Claude Code's own tool of the same name), letting the
  agent pause and present the user with a structured clarifying question
  and selectable options mid-task.
- **FR-011**: Each conversation MUST have a status, computed live rather
  than stored: `in_progress` whenever a generation request is currently
  active or queued for it; otherwise, based on its most recently completed
  turn (considering only the assistant's own messages, not the user's) —
  `failed` if that turn ended in an unrecoverable error; `requires_action`
  if that turn's last message is an `AskUserQuestion` tool call, or its
  last text content ends in a `?` that is not part of a URL; `done`
  otherwise.
- **FR-012**: The app MUST generate a conversation's title by truncating
  the user's first message to a fixed maximum length at a word boundary —
  no additional model inference is used for title generation.
- **FR-013**: The app MUST perform agent-mode file and shell actions without
  requiring user confirmation or approval; doce v1.0 has no permission/
  approval system gating agent actions. The one narrow exception: the app
  MUST hard-block a small, fixed set of catastrophic, irreversible shell
  command patterns (e.g. recursive deletion of the user's home directory
  or root, whole-disk erase commands) outright, with no prompt and no way
  to override — this is a safety rail against unrecoverable disasters, not
  a permission gate, and does not reintroduce approval friction for
  anything else.
- **FR-014**: The app MUST support models without native tool-calling by
  constraining generation (e.g. grammar-based constraints) to produce valid
  structured tool calls.
- **FR-015**: The agent's tool-use loop MUST support spawning a subagent —
  an isolated instance of the same tool-use loop, with its own fresh
  context (no parent conversation history) and a restricted tool subset —
  whose intermediate tool calls and reasoning are not shown to the user;
  only its final result is returned into the spawning agent's context.
- **FR-016**: A subagent MUST NOT itself spawn a further subagent; nesting
  is limited to one level. A subagent MUST also be capped at a fixed
  maximum number of turns (default 30); once reached, it MUST stop and
  return whatever result it has to the spawning conversation rather than
  continuing indefinitely.
- **FR-017**: The app MUST show the user in-progress agent activity (file
  diffs, terminal output) as it happens, not only a final summary.
- **FR-018**: The app MUST provide an MCP client so users can connect
  external MCP servers, making their tools available to the agent's tool-use
  loop.
- **FR-019**: The app MUST support filesystem-based skill packs (bundled and
  user-added) that the agent discovers and applies contextually without
  requiring manual selection per task.
- **FR-020**: The app MUST NOT transmit telemetry or usage data off-device by
  default, and MUST NOT require an account for any v1.0 functionality.
- **FR-021**: The app MUST store chat history and workspace state locally
  on-device.
- **FR-022**: The app's distributed build MUST be signed and notarized for
  macOS.
- **FR-023**: v1.0 MUST NOT include any bridged messaging channel (WhatsApp
  or otherwise); this is explicitly deferred to a later release, not silently
  omitted.
- **FR-024**: The app MUST run at most one model generation at a time
  system-wide, regardless of how many conversations, agent tasks, or
  subagents are active, so local inference cannot saturate the machine's
  resources.
- **FR-025**: When a message, agent turn, or subagent turn cannot start
  generating immediately because another generation is in progress, the
  app MUST queue it and MUST visibly indicate its queued state to the user
  rather than appearing unresponsive or silently dropping it.
- **FR-026**: Whichever conversation the user is currently viewing MUST be
  prioritized ahead of all other queued generation requests, whether those
  belong to another chat conversation, an agent task not currently being
  viewed, or a subagent spawned by any of those; a subagent's generation
  requests MUST be scheduled at the same priority as the conversation that
  spawned it, so that priority changes dynamically if the user changes
  focus. Other queued requests MUST be served once the viewed conversation
  (and any of its subagents) has no pending request of its own.
- **FR-027**: An in-progress agent task or subagent MUST be broken into
  per-turn units of inference work rather than submitted as one
  uninterruptible job, so other queued work can be serviced between turns.
- **FR-028**: The user MUST be able to cancel an in-progress or queued
  generation; canceling one item MUST NOT affect other queued or running
  work, and any partial output already produced MUST remain visible rather
  than being discarded.
- **FR-029**: The app MUST let a user search across their conversation
  history (titles and message content) and return ranked, relevant
  results with a highlighted excerpt showing the match.
- **FR-030**: Search results MUST NOT include content from subagent-run
  conversations, consistent with their isolation from the user (FR-015).

### Key Entities

- **Workspace**: A folder the user has opened for agent mode; provides the
  working project context for that agent session (file tree, conversation
  association) — it is not a permission or security boundary in v1.0.
- **Conversation**: A chat thread (either standalone chat or tied to a
  workspace's agent session), containing ordered messages and streamed
  assistant responses, persisted locally. Its title is generated by
  truncating the user's first message (FR-012), and it has a live-computed
  `status` (`done` \| `requires_action` \| `failed` \| `in_progress`,
  FR-011) rather than a stored one. A conversation may instead be a
  subagent run — spawned by another conversation's agent loop rather than
  opened by the user, not shown in the main conversation list, but
  persisted like any other conversation.
- **Model**: A local inference model matched to a hardware tier; has a
  source, quantization, expected footprint, and capability tags (e.g.
  tool-calling, coding-focused).
- **Skill**: A filesystem-based capability pack (bundled default or
  user-added) with metadata the agent uses to decide contextual relevance.
- **MCP Server Connection**: A user-configured external tool server the agent
  can call into during a tool-use loop.
- **Generation Request**: A single unit of queued or in-progress model work
  (a chat send, or one turn of an agent's or subagent's tool-use loop),
  associated with a conversation and a queued/running/canceled state.
  Whether it is treated as prioritized depends on whether that associated
  conversation is the one currently being viewed — for a subagent's
  requests, "associated conversation" means the conversation that spawned
  it, not the subagent's own identity, so a subagent's priority always
  tracks its spawning conversation's focus state. Evaluated at the moment
  work is picked up rather than fixed when the request was created. Not
  persisted across app restarts.

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
- **SC-004**: A user can connect at least one MCP server and have its tools
  available to the agent without restarting the app.
- **SC-005**: No network request containing user conversation content or
  usage telemetry is made without an explicit, separate user opt-in (verified
  by inspection of outbound traffic during standard chat/agent use).
- **SC-006**: With two or more conversations/agent tasks active at once, the
  app's UI remains responsive throughout, no message is silently dropped,
  and a queued generation's status is visibly distinguishable from a
  running one at all times.
- **SC-007**: Whenever the conversation currently being viewed has no
  pending generation request, a queued background request (a different
  conversation, or an agent task not currently being viewed) is served
  without requiring any further user action — background progress is
  blocked only by the focused conversation's own active queue of requests,
  never by focus alone.
- **SC-008**: A subagent's intermediate tool calls and reasoning never
  appear in the spawning conversation's visible history — only its final
  result does, verified by inspecting the parent conversation's messages
  after a subagent-involving task completes.
- **SC-009**: A user can find a previously discussed topic by searching a
  keyword, with matching conversations ranked by relevance and shown with
  a highlighted excerpt of the match, and zero results ever drawn from a
  subagent-run conversation's content.
- **SC-010**: Across a representative sample of conversations ending in
  each of the three terminal outcomes (done, requires_action, failed), the
  displayed status matches the correct outcome in 100% of cases, and a
  conversation with an active or queued generation never displays a
  terminal status prematurely.
- **SC-011**: 100% of tested catastrophic-command patterns (recursive
  home/root deletion, whole-disk erase) are blocked before execution, with
  no way for a prompt to talk the agent around the block.

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
- doce v1.0 ships with no permission/approval system (FR-013): once agent
  mode is engaged, the agent may read, write, and execute anywhere on the
  local filesystem without user confirmation, not scoped to the opened
  workspace folder. This is an explicit v1.0 simplification (see
  constitution Principle V), accepted while local chat/agent use is the
  only external way to reach the agent. It MUST be revisited before v1.1
  WhatsApp bridging is designed, since a bridged, inbound-triggered channel
  changes the risk calculus for an unrestricted, unconfirmed agent.
- Generation queue state (FR-024–FR-028) is in-memory only and not
  persisted; if the app restarts mid-generation, that in-progress turn is
  lost and must be re-triggered by the user, consistent with typical chat
  application restart behavior. Resuming an interrupted multi-step agent
  task automatically across restarts is out of scope for v1.0.
- Background generation requests (FR-026) are served opportunistically, in
  the gaps when the focused conversation has no pending request of its
  own; there is deliberately no fairness/aging mechanism forcing background
  work through on a schedule. Sustained, gap-free continuous use of the
  focused conversation can delay background work indefinitely. This is an
  accepted simplification, not a defect: real conversational usage
  naturally includes pauses (reading a response, composing the next
  message) during which background work is serviced regardless of
  priority.
- Subagents (FR-015/FR-016) are capped at 30 turns by default, added after
  an adversarial review of this design flagged that subagents are, in
  effect, a second and *invisible* way to reach an already-unrestricted,
  unconfirmed agent (Principle V's no-permission-system trade-off was
  originally justified on "local chat/agent use is the only external way
  to reach the agent" — subagents don't add a new *external* entry point,
  but they do multiply how much unsupervised work can happen per external
  trigger). The turn cap directly bounds that multiplication; the
  one-level nesting limit (FR-016) separately bounds the *shape* of
  subagent proliferation (no recursive chains). Neither eliminates the
  underlying risk of the no-permission-system decision itself — that
  remains accepted per Principle V and its required v1.1 revisit — but
  together they prevent a single subagent invocation from compounding it
  without limit.
