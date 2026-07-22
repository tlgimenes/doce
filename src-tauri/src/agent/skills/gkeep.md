Google Keep lets you read and draft notes and lists.

Common recipes (each numbered step is one tool call):

1. Find a note: search by keyword or title to get its id.
2. Read a note: fetch it by id for its full contents.
3. Add a note: draft the title and body — propose it before creating.
4. Update a list: 1) find the note, 2) read it, 3) propose the edited items to the user.

Notes:

- Note listings are paginated; request more pages only when needed.

Guardrail: Never create, edit, or delete a note without the user's explicit confirmation — draft or propose only, then let the user approve.
