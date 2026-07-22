Google Calendar lets you view and propose calendar events.

Common recipes (each numbered step is one tool call):

1. Check availability: list events for a date range and report what is booked.
2. Find an event: search by title or attendee to get its id.
3. Propose a new event: draft it (title, time, attendees) and describe it — do NOT create it until confirmed.
4. Reschedule: 1) find the event, 2) describe the change, 3) apply it only after the user confirms.

Notes:

- Always resolve relative dates ("next Tuesday") against today's date.
- Event listings are paginated; fetch more only if needed.

Guardrail: Never create, modify, or delete a calendar event without the user's explicit confirmation — propose the change and wait for approval.
