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
   output as the agent works, with no confirmation/approval prompt at any
   point (FR-013); the agent is not restricted to the opened folder.
4. Repeat with a model lacking native tool-calling (or force the
   grammar-constrained code path) and confirm the same task completes via
   GBNF-constrained tool calls (FR-014).

## 4. Subagent spawning (FR-015/FR-016, SC-008)

1. Describe an agent-mode task complex enough that the agent delegates a
   sub-task (or force this via a test prompt that explicitly asks for
   delegation).
2. Expected outcome: an `agent-activity` event with `kind: "subagent-status"`
   fires (`state: "running"`, then `"complete"`); no intermediate tool call
   or reasoning from the subagent ever reaches the frontend.
3. Inspect the spawning conversation's messages after completion.
4. Expected outcome: only the subagent's final result appears as a tool
   result — no intermediate subagent steps are present (SC-008).
5. Attempt to make that same subagent spawn a further subagent (test-only
   hook, since this isn't user-triggerable in normal use).
6. Expected outcome: rejected — nesting is capped at one level (FR-016).

## 5. Conversation title and status (User Story 7, FR-010/FR-011/FR-012, SC-010)

1. Create a new conversation and send a first message longer than the
   title truncation length.
2. Expected outcome: `list_conversations` shows a `title` truncated at a
   word boundary from that message — no extra model call involved.
3. Send a message and let it complete normally (no trailing question).
4. Expected outcome: that conversation's `status` is `done`.
5. Prompt the agent in a way that triggers `AskUserQuestion` (or ends its
   response in a real question, not a URL query string like `?ref=1`).
6. Expected outcome: an `ask-user-question` event fires (for the tool-call
   case) and/or `status` becomes `requires_action`; calling
   `answer_user_question` resolves it and the loop continues.
7. Force a tool execution failure (e.g. an invalid `Bash` command in a
   controlled test).
8. Expected outcome: that conversation's `status` becomes `failed`.
9. While a generation is actively running for a conversation, check its
   `status`.
10. Expected outcome: `in_progress` — it does not jump ahead to `done`/
    `requires_action`/`failed` before the turn actually finishes.

## 6. MCP and skills (User Story 4)

1. Call `add_mcp_server` with a local test MCP server (stdio transport).
2. Start a task that would use a tool exposed only by that server.
3. Expected outcome: the tool is available to the agent's tool-use loop
   without restarting the app.
4. Place a test skill pack in the user skills directory.
5. Start a task matching that skill's declared purpose.
6. Expected outcome: `list_skills` shows it, and the agent's behavior
   reflects the skill being pulled into context without manual selection.

## 7. Search (User Story 6, SC-009)

1. Have at least two conversations with distinct, identifiable topics
   (e.g. one mentions "quarterly budget," another mentions "hiking trails").
2. Call `search_conversations` with a keyword unique to one of them.
3. Expected outcome: that conversation is returned with a highlighted
   `snippet`; an unrelated conversation is not returned (or ranks lower).
4. Repeat with a keyword that only appears in a conversation's `title`
   (not its message content).
5. Expected outcome: the conversation is still found.
6. Trigger a subagent run (per step 4 above) whose internal messages
   contain a distinctive keyword, then search for that keyword.
7. Expected outcome: no result references that subagent conversation —
   search never surfaces subagent-only content (FR-030).

## 8. Privacy check (Principle II, SC-005)

1. During steps 1–7 above, monitor outbound network traffic from the app
   (e.g. via `lsof`/`nettop`/a local proxy).
2. Expected outcome: the only outbound traffic is the initial model
   download (and, if applicable, the model-registry refresh); no traffic
   carries conversation content or usage telemetry unless a telemetry
   opt-in setting was explicitly enabled first.

## References

- Functional requirements: [`spec.md`](./spec.md)
- IPC surface exercised above: [`contracts/tauri-ipc.md`](./contracts/tauri-ipc.md)
- Entities referenced above: [`data-model.md`](./data-model.md)
