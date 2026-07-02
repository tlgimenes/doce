# Quickstart: Validating Doce v1.0

Prerequisites: an Apple Silicon Mac (macOS 13+), the app built per
`src-tauri` (see Project Structure in `plan.md`), no prior Doce data
directory (for the zero-config scenarios) or an existing one (for the
persistence scenarios).

## 1. Zero-config first run (User Story 1, SC-001/SC-002/SC-003)

1. Remove any existing local app data directory, then launch the built app.
2. Observe: hardware detection happens automatically; a model download
   begins with visible progress; no model picker, API key field, or account
   form is shown.
3. Quit the app mid-download (simulate interruption), relaunch.
4. Observe: download resumes from prior progress rather than restarting.
5. Once installed, send a chat message.
6. Expected outcome: a streamed response appears with no additional setup
   step performed. `get_settings` shows no telemetry opt-in enabled.

## 2. Chat persistence (User Story 2)

1. Send several chat messages, including one containing a code block.
2. Expected outcome: code renders with formatting; response streamed
   token-by-token (observable via `assistant-token` events).
3. Quit and relaunch the app.
4. Expected outcome: the prior conversation and messages are still present
   (`list_conversations` / `create_conversation` continuity).

## 3. Agent mode on a workspace (User Story 3)

1. Call `open_workspace` on a small sample project folder.
2. Describe a small, concrete task (e.g. "add a comment to file X").
3. Expected outcome: `agent-activity` events show file diffs and/or shell
   output as the agent works; the change is applied inside the opened folder
   only.
4. Repeat with a model lacking native tool-calling (or force the
   grammar-constrained code path) and confirm the same task completes via
   GBNF-constrained tool calls (FR-010).

## 4. Permission gate (User Story 4, SC-004/SC-005)

1. While in agent mode, prompt a task that requires acting outside the
   opened workspace folder (e.g. reading a file elsewhere) or a
   not-yet-trusted shell command category.
2. Expected outcome: a `permission-prompt` event fires and the action is
   blocked until `respond_to_permission_prompt` is called; no action occurs
   silently.
3. Respond with `allow-always` for that action kind.
4. Trigger the same action kind again in the same workspace.
5. Expected outcome: no new prompt; `list_permission_grants` shows the
   persisted grant (`permission-grant-updated` fired once, not again).
6. Open a different workspace and trigger the same action kind.
7. Expected outcome: a new prompt fires — grants do not cross workspaces
   (FR-014).

## 5. MCP and skills (User Story 5)

1. Call `add_mcp_server` with a local test MCP server (stdio transport).
2. Start a task that would use a tool exposed only by that server.
3. Expected outcome: the tool is available to the agent's tool-use loop
   without restarting the app.
4. Place a test skill pack in the user skills directory.
5. Start a task matching that skill's declared purpose.
6. Expected outcome: `list_skills` shows it, and the agent's behavior
   reflects the skill being pulled into context without manual selection.

## 6. Privacy check (Principle II, SC-007)

1. During steps 1–5 above, monitor outbound network traffic from the app
   (e.g. via `lsof`/`nettop`/a local proxy).
2. Expected outcome: the only outbound traffic is the initial model
   download (and, if applicable, the model-registry refresh); no traffic
   carries conversation content or usage telemetry unless a telemetry
   opt-in setting was explicitly enabled first.

## References

- Functional requirements: [`spec.md`](./spec.md)
- IPC surface exercised above: [`contracts/tauri-ipc.md`](./contracts/tauri-ipc.md)
- Entities referenced above: [`data-model.md`](./data-model.md)
