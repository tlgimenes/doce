# Contract: Tool Call Widgets

This feature adds one new Tauri command and wires up one already-specified
event; the rest of its surface is the `messages` row shapes in
`data-model.md`, read via the existing `list_messages` command — no new
"fetch tool activity" command is needed.

## `answer_user_question` (new command)

| Input | Output | Notes |
|---|---|---|
| `{ questionId: string, answer: string[] }` | `{ ok: true }` on success, `Err(string)` otherwise | Resolves a pending `AskUserQuestion` tool call via `PendingQuestions::answer` (already implemented/unit-tested — this wires it up). Returns an error, not a silent no-op, if `questionId` is unknown (already answered, or never registered — FR-009's guard). On success, also updates the corresponding `tool_result` message row's `content` (setting `answer`), so the prompt shows as already-answered on reload, not just in the live session. |

This is exactly `001-doce-v1-core`'s `contracts/tauri-ipc.md`-specified
command (T061) — implemented here, not redefined.

## `ask-user-question` (event, already specified in `001`)

| Payload | When |
|---|---|
| `{ conversationId: string, toolCallId: string, header: string, question: string, options: { label: string, description?: string }[], multiSelect: boolean }` | The agent loop's dispatch hits `AskUserQuestion`, registers a pending question, and is now awaiting `answer_user_question` |

Implemented here (T058) — the frontend's `AskUserQuestionWidget` can
either wait for this event (matching `toolCallId` against the message it's
already rendering, for the live-in-session case) or simply render directly
from the persisted `tool_result` row's `content` (the reload/history case)
— both read the same shape.

## `agent-activity` (event, specified in `001`, deliberately NOT implemented by this feature)

`001`'s contract defines `{ conversationId, kind: "file-diff" | "shell-output"
| "subagent-status", detail: object }` for live, mid-turn updates. Per
`research.md` § 2, this feature does not emit it — every tool call other
than `AskUserQuestion` is rendered from its persisted `tool_result` row
once `send_agent_message` resolves and the frontend re-fetches
`list_messages`, not from a live event stream. Recorded here explicitly so
this isn't mistaken for an oversight: it's `001`'s original contract,
knowingly left unimplemented pending a future live-streaming pass, with
the data shapes in `data-model.md` already compatible with wiring it up
later.

## `list_messages` (existing command, unchanged signature, richer payload)

Returns `Message[]` exactly as today (`id`, `conversationId`, `role`,
`contentType`, `content`, `toolName`, `createdAt`, `durationMs`) — this
feature changes only what real `tool_call`/`tool_result` rows now exist to
return and what `content` holds for them (`data-model.md`), not the
command's shape.

## Failure handling

- A tool's underlying execution failing (e.g. `Edit`'s `old_string` not
  found, `Bash` non-zero exit) is not a *dispatch* failure — it's a normal,
  successfully-recorded `tool_result` whose `outcome.ok` is `false` (or
  `exitCode != 0` for `Bash`). The widget shows a failure state (FR-012);
  the loop continues normally, exactly as it does today (the model sees
  the same `model_text` it always has).
- A genuinely unparseable/corrupt `tool_result.content` (should not occur
  by construction — see `data-model.md`'s Validation rules) degrades to
  the fallback widget on the frontend rather than breaking the message
  list.
