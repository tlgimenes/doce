# Quickstart: Rich Chat Input

Validates the new Tiptap-based input against `spec.md`'s acceptance
scenarios. Requires the running app (`npm run tauri dev`) with a real,
installed local model, since US3 (skill context injection) can only be
proven by actually observing the agent's behavior change — an automated
test can assert the right text was sent to `generate()`, but "the
agent's response reflects that skill's guidance" (spec.md's
Independent Test for US3) needs a live run against the real model.

## Automated validation

```bash
npx vitest run          # frontend: RichInput, chip extensions, UserMessageContent
cd src-tauri && cargo test   # backend: expand_segments, migration, load_history
```

Should cover, at minimum (see `tasks.md` for the exact breakdown):
- `RichInput` renders identically (same classes, same behavior) across
  `EmptyState.tsx`, `Chat.tsx`, and `Workspace.tsx`'s test suites.
- Enter submits; Shift+Enter inserts a newline without submitting.
- A paste under the ~10-line/~500-char threshold inserts as plain text;
  a paste over it collapses into a `pastedText` chip with the correct
  `lineCount`; clicking the chip restores the original, editable text.
- An `attachment` chip renders a filename, shows a hover preview only
  when `isImage` is true, and never appears with its `data` exposed as
  plain visible text.
- Typing "/" opens the skill picker when `skillsEnabled`, and does
  nothing when it isn't (`Chat.tsx`'s plain-mode composer).
- `expand_segments(..., expand_skills: true)` inlines a skill's real
  `SKILL.md` content at the marker's position; `expand_skills: false`
  renders `/skill-name` instead — and errors (not a partial string) when
  a referenced skill can't be read.
- `expand_segments` never includes an `attachment` segment's `data` in
  its output, for either expansion mode.
- `load_history` expands a `content_type='rich_text'` row the same way
  send-time does, so a skill/paste from an earlier turn still reaches
  the model on a later turn in the same conversation.
- The `0003_rich_text_content_type` migration preserves existing rows'
  `rowid` (so `messages_fts` search results stay correctly linked) and
  the FTS5 sync triggers still fire on insert/update/delete afterward.

## Manual validation (live app, real model)

Prerequisites: `npm run tauri dev`; a model already installed and active
(Settings → Models); at least one skill installed under the app's skills
directory (`<app data dir>/skills/<skill-name>/SKILL.md`) for the US3
step below.

1. **US1 — one consistent input everywhere.** Open the new-conversation
   composer (`+ New conversation`), type a few lines using Shift+Enter
   between them, and confirm Enter (without Shift) sends. Repeat inside
   an existing plain conversation and inside a workspace/agent
   conversation — confirm identical appearance and behavior in all
   three (spec.md SC-001).

2. **US2 — paste collapses, expands, and reaches the model in full.**
   Copy a ~30-line block of text (e.g. a stack trace) and paste it into
   the input. Confirm it collapses into a "<pasted 30 lines>"-style
   chip rather than showing 30 raw lines. Click the chip — confirm it
   expands back to the original, fully editable text. Send a message
   containing the (re-collapsed) chip and confirm — via the agent's
   response — that it actually saw the full pasted content, not a
   placeholder (spec.md SC-002/SC-003).

3. **US3 — a skill marker actually changes agent behavior.** In an
   agent-mode conversation, type "/", confirm the picker lists your
   installed skill(s) by name and description, select one, and confirm
   a marker appears in the input. Send the message and confirm the
   agent's response reflects that skill's actual guidance — not just
   that a chip rendered (spec.md SC-004). Then: edit that skill's
   `SKILL.md` on disk, send a **new** message in the **same**
   conversation (without re-selecting the skill), and confirm the
   conversation's ongoing behavior — if the agent's context still
   includes that skill from history replay — reflects the **edited**
   content, matching research.md's resolve-at-use decision, not a
   stale snapshot from the original selection.

4. **US4 — image attachment, three ways, always the same chip.** Attach
   an image by (a) pasting from the clipboard, (b) dragging a file in,
   and (c) using the file-picker button. Confirm all three produce an
   identically-styled chip, hovering shows a preview, and sending the
   message completes normally (no error, no hang) even though the
   agent's response makes no reference to actually having seen the
   image's visual content (spec.md SC-005/SC-007).

5. **History round-trip.** After completing steps 2–4 in one
   conversation, reload the app (or switch away and back to the
   conversation) and confirm the same pasted-text chip, image chip, and
   skill marker all re-render in their original collapsed/chip form in
   the message history — not as a plain-text dump of raw JSON, and not
   flattened to ordinary text (spec.md SC-006).

6. **FR-011 regression check.** In a plain (non-agent) conversation,
   type "/" and confirm no skill picker appears.
