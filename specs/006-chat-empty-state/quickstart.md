# Quickstart: Chat Empty State Composer

Validates the redesigned empty state against `spec.md`'s acceptance
scenarios. Requires the running app (`npm run tauri dev`), since this
exercises real Tauri commands (`open_workspace`, `create_conversation`,
`send_agent_message`) end-to-end, not mocked IPC.

## Automated validation

```bash
npx vitest run
```

Should cover, at minimum (see `tasks.md` for the exact breakdown):
- The composer renders instead of the old static placeholder, both when
  no conversation is selected and after clicking "+ New conversation."
- Clicking "+ New conversation" does not call `createConversation` (no
  conversation is created merely by clicking it).
- Submitting text with the Home target untouched resolves to the actual
  home directory and creates a workspace-scoped conversation.
- Changing the folder target before submitting scopes the new
  conversation to that folder instead.
- The folder picker lists `listWorkspaces()` results ordered
  most-recent-first, with "Home" pinned first, filterable by typing.
- Dismissing the picker (Escape / click-away) without picking anything
  leaves the previous target unchanged.
- `App.tsx` routes a selected conversation to the workspace view when its
  `workspaceId` is non-null, and to the existing plain view when it's
  null (regression guard for FR-012 — pre-existing conversations
  unaffected).

## Manual validation (in the running app)

1. **User Story 1**: launch with no conversation selected (or click
   "+ New conversation" if one is active). Confirm the composer appears,
   not plain text. Type a message without touching the folder selector,
   submit. Confirm: a new conversation appears in the sidebar, it's now
   active, it shows your message and a reply, and it behaves like an
   agent-mode conversation (tool access).
2. **User Story 2**: from the composer, click the folder selector.
   Confirm "Home" is shown as the current selection and previously
   opened folders appear below it, most-recently-used first. Type to
   filter the list. Pick a different folder, then submit a message —
   confirm the new conversation is scoped to that folder (check its
   title/sidebar entry reflects the folder, matching existing
   workspace-conversation display).
3. **User Story 3**: open the folder selector, choose to browse the
   filesystem, pick a folder you've never opened before, submit a
   message. Confirm it works, and confirm that folder now appears in the
   picker's recents next time you open it.
4. **Edge cases**: open the picker, change the selection, then dismiss
   without submitting — reopen the composer's picker and confirm the
   changed selection was remembered. Click "+ New conversation" several
   times without ever submitting a message, then check the sidebar's
   conversation list — confirm no empty "phantom" conversations were
   created.
5. **Regression check (FR-012)**: if any conversations existed before
   this feature (from earlier manual testing), select one from the
   sidebar and confirm it still renders and behaves exactly as it did
   before.
6. **System prompt check**: start a conversation scoped to a specific
   folder and ask the agent something like "what directory are you
   working in?" — confirm its answer reflects the selected folder (the
   system-prompt addition from `research.md` § 1).
