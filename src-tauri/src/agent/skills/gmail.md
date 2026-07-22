Gmail lets you search, read, and draft email.

Common recipes (each numbered step is one tool call):

1. Find an email: search with a query (from:, subject:, keywords) to get message/thread ids.
2. Read an email: fetch the message or thread by id to see its full body.
3. Reply to an email: 1) search to find the thread, 2) read it, 3) draft the reply — do NOT send.
4. Compose a new email: draft it with recipient, subject, and body — leave it as a draft.

Notes:

- Searches are paginated; fetch more pages only if the user needs them.
- "How many emails…" means count the results, not read each one.

Guardrail: Never send an email, or take any other irreversible action, without the user's explicit confirmation — draft or propose only, then let the user review and send.
