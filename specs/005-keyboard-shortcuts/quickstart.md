# Quickstart: Keyboard Shortcuts

Validates the three shortcuts against `spec.md`'s acceptance scenarios.
This is a frontend-only feature — no Tauri backend changes — so it can be
validated entirely via the frontend dev server plus targeted unit tests.

## Prerequisites

- `lib/shortcuts.ts`, `components/Dialog.tsx`, and
  `views/shortcuts/ShortcutsDialog.tsx` exist; `App.tsx` and
  `ConversationList.tsx` have been updated per `plan.md`'s Project
  Structure (produced by `/speckit-implement`).

## Automated validation

```bash
npx vitest run
```

Should cover, at minimum (see `tasks.md` for the exact task breakdown):
- Cmd+L focuses the chat input when a conversation is active, the agent
  input when in agent/workspace mode, and does nothing when Settings is
  open or no conversation exists.
- Cmd+N triggers the same conversation-creation path as clicking
  "+ New conversation" (asserted via the same mocked `commands.createConversation`
  the existing `ConversationList.test.tsx` already mocks).
- Cmd+K opens the shortcuts dialog; pressing it again, pressing `Escape`,
  or clicking the close control each close it.
- All three shortcuts still work while a text input has focus.
- Typing normally (including letters `l`, `n`, `k` without Cmd) is
  unaffected.

## Manual validation (in the running app)

1. Launch the app (`npm run tauri dev`) with at least one conversation
   existing.
2. **Cmd+L (User Story 1)**: click anywhere outside the input, press
   Cmd+L, confirm the message input is focused. Click into the input,
   press Cmd+L again, confirm focus is undisturbed. Switch to agent mode
   (open a folder), press Cmd+L, confirm the task input is focused
   instead.
3. **Cmd+N (User Story 2)**: from any view (chat, Settings, agent mode),
   press Cmd+N, confirm a new empty conversation is created and becomes
   active — compare against clicking "+ New conversation" directly to
   confirm identical behavior.
4. **Cmd+K (User Story 3)**: press Cmd+K from anywhere, confirm the
   dialog lists all three shortcuts with descriptions. Dismiss via
   Escape; reopen; dismiss via the close button; reopen; press Cmd+K
   again and confirm it closes rather than stacking.
5. **Edge cases**: with the shortcuts dialog open, press Cmd+L and Cmd+N
   and confirm neither acts on the conversation until the dialog is
   dismissed first.
6. **Regression check**: confirm Enter-to-send, normal typing, and
   copy/paste in both the chat and agent inputs all still work exactly as
   before.
