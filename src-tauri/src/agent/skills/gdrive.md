Google Drive lets you search, read, and organize files.

Common recipes (each numbered step is one tool call):

1. Find a file: search by name or content to get its id.
2. Read a file: fetch its metadata or download its contents by id.
3. Summarize a document: 1) search to find it, 2) read it, 3) answer from its contents.
4. Create or move a file: propose the change (name, folder) — do NOT write until confirmed.

Notes:

- Search results are paginated; fetch more only if the user needs them.
- Prefer reading metadata before downloading large file contents.

Guardrail: Never create, overwrite, move, or delete a file without the user's explicit confirmation — propose the action and wait for approval.
